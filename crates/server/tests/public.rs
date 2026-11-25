//! 阶段 3 的完成条件（docs/v1.0-implementation.md §4）：公开站点。临时目录里的数据库、
//! 固定时钟，用 `oneshot` 直接调用路由，不开端口。

use std::sync::{Arc, LazyLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Request, StatusCode, header};
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::markdown::Renderer;
use cat_blog_core::save;
use cat_blog_server::clock::{Clock, FixedClock};
use cat_blog_server::config::{AdminPassword, Config};
use cat_blog_server::db::{self, BlogDb};
use cat_blog_server::{AppState, app, startup};
use jiff::{Timestamp, ToSpan};
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::ServiceExt;

type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static RENDERER: LazyLock<Renderer> = LazyLock::new(Renderer::new);

/// 2026-11-24T00:00:00Z
const T0: Timestamp = Timestamp::constant(1_795_478_400, 0);

const BASE_URL: &str = "https://blog.example";

struct Site {
    _dir: TempDir,
    pool: SqlitePool,
    blog: BlogDb,
    clock: Arc<FixedClock>,
    app: Router,
}

struct Reply {
    status: StatusCode,
    headers: HeaderMap,
    body: String,
}

impl Reply {
    fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

/// 第一次启动后的站点：默认首页（`{{ posts limit:5 }}`）和示例草稿 `hello-cat-blog`。
async fn site() -> TestResult<Site> {
    let dir = tempfile::tempdir()?;
    let pool = db::connect(&dir.path().join("t.db")).await?;
    let config = Config {
        admin_username: "admin".to_owned(),
        admin_password: AdminPassword::new("correct horse battery"),
        base_url: BASE_URL.to_owned(),
        bind_addr: ([127, 0, 0, 1], 0).into(),
        database_path: "unused".into(),
        trusted_proxies: Vec::new(),
        log_filter: "info".to_owned(),
    };
    let clock = Arc::new(FixedClock::new(T0));
    let blog = startup::prepare(&pool, &config, clock.as_ref(), &RENDERER).await?;
    let app = app(AppState::new(
        pool.clone(),
        blog.clone(),
        &config,
        clock.clone(),
    ));
    Ok(Site {
        _dir: dir,
        pool,
        blog,
        clock,
        app,
    })
}

impl Site {
    /// 按保存流程第一次保存一篇内容，时间取时钟的当前时间。
    async fn add(&self, kind: Kind, raw: &str) -> TestResult<()> {
        let tz = self.blog.blog().await?.timezone;
        let entry = save::prepare_entry(&RENDERER, raw, kind, &tz, None, self.clock.now())?;
        self.blog
            .insert_post(kind, &entry, self.clock.now())
            .await?;
        Ok(())
    }

    async fn post(&self, title: &str, extra: &str) -> TestResult<()> {
        self.add(
            Kind::Post,
            &format!("---\ntitle: {title}\ndraft: false\n{extra}---\n\nbody of {title}\n"),
        )
        .await
    }

    async fn get(&self, uri: &str) -> TestResult<Reply> {
        self.send(Request::get(uri).body(Body::empty())?).await
    }

    async fn send(&self, request: Request<Body>) -> TestResult<Reply> {
        let response = self.app.clone().oneshot(request).await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = to_bytes(response.into_body(), usize::MAX).await?;
        Ok(Reply {
            status,
            headers,
            body: String::from_utf8(body.to_vec())?,
        })
    }
}

#[tokio::test]
async fn visible_post_is_served() {
    let site = site().await.unwrap();
    site.post("First Post", "tags: [Rust]\n").await.unwrap();

    let reply = site.get("/first-post/").await.unwrap();
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("<h1>First Post</h1>"), "{}", reply.body);
    assert!(
        reply.body.contains("<time>2026-11-24</time>"),
        "{}",
        reply.body
    );
    assert!(
        reply.body.contains(r#"href="/blog/?tag=rust""#),
        "{}",
        reply.body
    );
    assert!(reply.body.contains("body of First Post"), "{}", reply.body);
    assert!(
        reply.header(header::CONTENT_TYPE).starts_with("text/html"),
        "{:?}",
        reply.headers
    );
}

#[tokio::test]
async fn draft_returns_404() {
    let site = site().await.unwrap();

    for uri in ["/hello-cat-blog/", "/hello-cat-blog", "/Hello-Cat-Blog/"] {
        let reply = site.get(uri).await.unwrap();
        assert_eq!(reply.status, StatusCode::NOT_FOUND, "{uri}");
        assert!(reply.body.contains("找不到这个页面"), "{}", reply.body);
    }
}

#[tokio::test]
async fn future_post_returns_404_until_its_time() {
    let site = site().await.unwrap();
    site.post("Later", "date: 2026-12-01\n").await.unwrap();

    assert_eq!(
        site.get("/later/").await.unwrap().status,
        StatusCode::NOT_FOUND
    );
    assert!(!site.get("/blog/").await.unwrap().body.contains("Later"));
    assert!(!site.get("/feed.xml").await.unwrap().body.contains("Later"));
    assert!(
        !site
            .get("/sitemap.xml")
            .await
            .unwrap()
            .body
            .contains("later")
    );

    // 2026-12-01 00:00 Asia/Singapore
    site.clock.set(T0 + (7 * 24 - 8).hours());
    assert_eq!(site.get("/later/").await.unwrap().status, StatusCode::OK);
}

#[tokio::test]
async fn non_canonical_paths_redirect_permanently() {
    let site = site().await.unwrap();
    site.post("Foo", "").await.unwrap();

    for uri in ["/Foo", "/Foo/", "/foo", "/FOO/"] {
        let reply = site.get(uri).await.unwrap();
        assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY, "{uri}");
        assert_eq!(reply.header(header::LOCATION), "/foo/", "{uri}");
    }
    assert_eq!(site.get("/foo/").await.unwrap().status, StatusCode::OK);
    assert_eq!(
        site.get("/nope").await.unwrap().status,
        StatusCode::NOT_FOUND
    );

    for (uri, location) in [
        ("/blog", "/blog/"),
        ("/admin", "/admin/"),
        ("/blog?tag=Rust", "/blog/?tag=Rust"),
        ("/Foo?ref=x", "/foo/?ref=x"),
    ] {
        let reply = site.get(uri).await.unwrap();
        assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY, "{uri}");
        assert_eq!(reply.header(header::LOCATION), location, "{uri}");
    }
}

#[tokio::test]
async fn percent_encoded_chinese_slug_is_served() {
    let site = site().await.unwrap();
    site.post("我的 Rust 笔记！", "").await.unwrap();

    let encoded = "/%E6%88%91%E7%9A%84-rust-%E7%AC%94%E8%AE%B0/";
    let reply = site.get(encoded).await.unwrap();
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply.body.contains("<h1>我的 Rust 笔记！</h1>"),
        "{}",
        reply.body
    );

    let reply = site
        .get("/%E6%88%91%E7%9A%84-RUST-%E7%AC%94%E8%AE%B0")
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(reply.header(header::LOCATION), encoded);

    let blog = site.get("/blog/").await.unwrap();
    assert!(
        blog.body.contains(&format!(r#"href="{encoded}""#)),
        "{}",
        blog.body
    );
}

#[tokio::test]
async fn feed_lists_only_visible_posts_with_absolute_urls() {
    let site = site().await.unwrap();
    site.add(
        Kind::Post,
        "---\ntitle: 中文 Post\ndraft: false\n---\n\n[home](/) ![cat](cat.png) note[^1]\n\n[^1]: n\n",
    )
    .await
    .unwrap();
    site.post("Later", "date: 2026-12-01\n").await.unwrap();
    site.add(
        Kind::Page,
        "---\ntitle: About\ndraft: false\n---\n\nabout\n",
    )
    .await
    .unwrap();

    let reply = site.get("/feed.xml").await.unwrap();
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        reply
            .header(header::CONTENT_TYPE)
            .starts_with("application/rss+xml"),
        "{:?}",
        reply.headers
    );
    let feed = &reply.body;
    assert_eq!(feed.matches("<item>").count(), 1, "{feed}");
    assert!(
        !feed.contains("Later") && !feed.contains("Hello, cat-blog") && !feed.contains("About"),
        "{feed}"
    );
    let post = "https://blog.example/%E4%B8%AD%E6%96%87-post/";
    assert!(feed.contains(&format!("<link>{post}</link>")), "{feed}");
    assert!(
        feed.contains("<guid isPermaLink=\"false\">urn:cat-blog:"),
        "{feed}"
    );
    assert!(
        feed.contains("<pubDate>Tue, 24 Nov 2026 00:00:00 +0000</pubDate>"),
        "{feed}"
    );
    // description 是转义后的 HTML，里面的相对地址都改成了绝对地址
    assert!(
        feed.contains("href=&#34;https://blog.example/&#34;"),
        "{feed}"
    );
    assert!(feed.contains(&format!("{post}cat.png")), "{feed}");
    assert!(feed.contains(&format!("{post}#fn-1")), "{feed}");
    for attr in ["href=&#34;/", "src=&#34;cat", "href=&#34;#"] {
        assert!(!feed.contains(attr), "{attr} in {feed}");
    }
}

#[tokio::test]
async fn tag_filter_is_case_insensitive() {
    let site = site().await.unwrap();
    site.post("Tagged", "tags: [Rust, Web]\n").await.unwrap();
    site.post("Other", "tags: [life]\n").await.unwrap();

    for uri in [
        "/blog/?tag=rust",
        "/blog/?tag=RUST",
        "/blog/?tag=%20Rust%20",
    ] {
        let reply = site.get(uri).await.unwrap();
        assert_eq!(reply.status, StatusCode::OK, "{uri}");
        assert!(reply.body.contains(">Tagged</a>"), "{uri}: {}", reply.body);
        assert!(!reply.body.contains(">Other</a>"), "{uri}: {}", reply.body);
    }
    let reply = site.get("/blog/?tag=missing").await.unwrap();
    assert_eq!(reply.status, StatusCode::OK);
    assert!(
        !reply.body.contains("Tagged") && reply.body.contains("还没有文章"),
        "{}",
        reply.body
    );
}

#[tokio::test]
async fn blog_lists_visible_posts_newest_first_and_home_expands_directive() {
    let site = site().await.unwrap();
    site.post("Older", "").await.unwrap();
    site.clock.set(T0 + 1.hour());
    site.post("Newer", "").await.unwrap();
    site.add(
        Kind::Page,
        "---\ntitle: About\ndraft: false\n---\n\nabout\n",
    )
    .await
    .unwrap();

    let blog = site.get("/blog/").await.unwrap().body;
    let newer = blog.find(">Newer</a>").unwrap();
    let older = blog.find(">Older</a>").unwrap();
    assert!(newer < older, "{blog}");
    assert!(
        !blog.contains("About") && !blog.contains("Hello, cat-blog"),
        "{blog}"
    );

    let home = site.get("/").await.unwrap();
    assert_eq!(home.status, StatusCode::OK);
    assert!(home.body.contains("欢迎来到 cat-blog"), "{}", home.body);
    assert!(
        home.body.contains(r#"<a href="/newer/">Newer</a>"#),
        "{}",
        home.body
    );
    assert!(!home.body.contains("cat-blog:posts"), "{}", home.body);
    assert!(
        home.body.contains(r#"<html lang="zh-CN">"#),
        "{}",
        home.body
    );

    let about = site.get("/about/").await.unwrap();
    assert_eq!(about.status, StatusCode::OK);
    assert!(!about.body.contains("<time>"), "{}", about.body);
}

#[tokio::test]
async fn sitemap_and_robots_use_absolute_urls() {
    let site = site().await.unwrap();
    site.post("Visible", "").await.unwrap();

    let sitemap = site.get("/sitemap.xml").await.unwrap().body;
    for loc in [
        "https://blog.example/",
        "https://blog.example/blog/",
        "https://blog.example/visible/",
    ] {
        assert!(sitemap.contains(&format!("<loc>{loc}</loc>")), "{sitemap}");
    }
    assert!(!sitemap.contains("hello-cat-blog"), "{sitemap}");

    let robots = site.get("/robots.txt").await.unwrap().body;
    assert!(robots.contains("Disallow: /admin/"), "{robots}");
    assert!(
        robots.contains("Sitemap: https://blog.example/sitemap.xml"),
        "{robots}"
    );
}

#[tokio::test]
async fn static_assets_are_embedded_and_cacheable() {
    let site = site().await.unwrap();
    let home = site.get("/").await.unwrap().body;
    let css_url = home
        .split(r#"<link rel="stylesheet" href=""#)
        .nth(1)
        .and_then(|s| s.split('"').next())
        .unwrap();
    assert!(css_url.starts_with("/static/style.css?v="), "{css_url}");

    let css = site.get(css_url).await.unwrap();
    assert_eq!(css.status, StatusCode::OK);
    assert!(css.header(header::CACHE_CONTROL).contains("immutable"));
    assert_eq!(css.header(header::CONTENT_TYPE), "text/css; charset=utf-8");
    assert!(
        css.body
            .contains(r#":root[data-theme="dark"] pre.syntax-highlighting"#)
    );

    let favicon = site.get("/favicon.ico").await.unwrap();
    assert_eq!(favicon.status, StatusCode::OK);
    assert_eq!(favicon.header(header::CONTENT_TYPE), "image/svg+xml");
    assert_eq!(
        site.get("/static/missing.css").await.unwrap().status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn security_headers_are_set() {
    let site = site().await.unwrap();
    for uri in ["/", "/nope/", "/feed.xml", "/admin"] {
        let reply = site.get(uri).await.unwrap();
        assert_eq!(
            reply.header(header::X_CONTENT_TYPE_OPTIONS),
            "nosniff",
            "{uri}"
        );
        assert_eq!(
            reply.header(header::REFERRER_POLICY),
            "strict-origin-when-cross-origin",
            "{uri}"
        );
        let frame = reply.header(header::X_FRAME_OPTIONS);
        assert_eq!(frame, if uri == "/admin" { "DENY" } else { "" }, "{uri}");
    }
}

#[tokio::test]
async fn oversized_body_returns_413() {
    let site = site().await.unwrap();
    let request = Request::post("/")
        .header(header::CONTENT_LENGTH, cat_blog_server::BODY_LIMIT + 1)
        .body(Body::from(vec![b'x'; cat_blog_server::BODY_LIMIT + 1]))
        .unwrap();
    let reply = site.send(request).await.unwrap();
    assert_eq!(reply.status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn healthz_returns_ok_when_database_answers() {
    let site = site().await.unwrap();
    let reply = site.get("/healthz").await.unwrap();
    assert_eq!((reply.status, reply.body.as_str()), (StatusCode::OK, "ok"));
}

#[tokio::test]
async fn healthz_returns_503_when_database_is_unreachable() {
    let site = site().await.unwrap();
    site.pool.close().await;
    let reply = site.get("/healthz").await.unwrap();
    assert_eq!(reply.status, StatusCode::SERVICE_UNAVAILABLE);
}
