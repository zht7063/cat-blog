//! HTTP 测试共用的站点：临时目录里的数据库、固定时钟，用 `oneshot` 直接调用路由，不开端口。

use std::net::SocketAddr;
use std::sync::{Arc, LazyLock};

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::connect_info::MockConnectInfo;
use axum::http::{HeaderMap, Request, StatusCode, header};
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::markdown::Renderer;
use cat_blog_core::save;
use cat_blog_server::clock::{Clock, FixedClock};
use cat_blog_server::config::{AdminPassword, Config};
use cat_blog_server::db::{self, BlogDb};
use cat_blog_server::{AppState, app, startup};
use jiff::Timestamp;
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::ServiceExt;

pub type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static RENDERER: LazyLock<Arc<Renderer>> = LazyLock::new(|| Arc::new(Renderer::new()));

/// 2026-11-24T00:00:00Z
pub const T0: Timestamp = Timestamp::constant(1_795_478_400, 0);

/// 管理员的用户名和密码。
pub const USERNAME: &str = "admin";
pub const PASSWORD: &str = "correct horse battery";

pub const BASE_URL: &str = "https://blog.example";

pub struct Site {
    _dir: TempDir,
    pub pool: SqlitePool,
    pub blog: BlogDb,
    pub clock: Arc<FixedClock>,
    app: Router,
}

pub struct Reply {
    pub status: StatusCode,
    pub headers: HeaderMap,
    pub body: String,
}

impl Reply {
    pub fn header(&self, name: header::HeaderName) -> &str {
        self.headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
    }
}

/// 第一次启动后的站点：默认首页（`{{ posts limit:5 }}`）和示例草稿 `hello-cat-blog`。
/// 直连地址是可信代理 127.0.0.1，客户端 IP 由请求的 `X-Forwarded-For` 决定（D38）。
pub async fn site() -> TestResult<Site> {
    let dir = tempfile::tempdir()?;
    let pool = db::connect(&dir.path().join("t.db")).await?;
    let config = Config {
        admin_username: USERNAME.to_owned(),
        admin_password: AdminPassword::new(PASSWORD),
        base_url: BASE_URL.to_owned(),
        bind_addr: ([127, 0, 0, 1], 0).into(),
        database_path: "unused".into(),
        trusted_proxies: vec!["127.0.0.1/32".parse()?],
        log_filter: "info".to_owned(),
    };
    let clock = Arc::new(FixedClock::new(T0));
    let blog = startup::prepare(&pool, &config, clock.as_ref(), &RENDERER).await?;
    let app = app(AppState::new(
        pool.clone(),
        blog.clone(),
        &config,
        clock.clone(),
        RENDERER.clone(),
    ))
    .layer(MockConnectInfo(SocketAddr::from(([127, 0, 0, 1], 40000))));
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
    pub async fn add(&self, kind: Kind, raw: &str) -> TestResult<()> {
        let tz = self.blog.blog().await?.timezone;
        let entry = save::prepare_entry(&RENDERER, raw, kind, &tz, None, self.clock.now())?;
        self.blog
            .insert_post(kind, &entry, self.clock.now())
            .await?;
        Ok(())
    }

    pub async fn post(&self, title: &str, extra: &str) -> TestResult<()> {
        self.add(
            Kind::Post,
            &format!("---\ntitle: {title}\ndraft: false\n{extra}---\n\nbody of {title}\n"),
        )
        .await
    }

    pub async fn get(&self, uri: &str) -> TestResult<Reply> {
        self.send(Request::get(uri).body(Body::empty())?).await
    }

    pub async fn send(&self, request: Request<Body>) -> TestResult<Reply> {
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

/// 测试里管理员的客户端 IP，经可信代理 127.0.0.1 转发。
pub const CLIENT: &str = "198.51.100.7";

pub fn encode(fields: &[(&str, &str)]) -> String {
    form_urlencoded::Serializer::new(String::new())
        .extend_pairs(fields)
        .finish()
}

/// 从 `ip` 提交登录表单。`origin`、`referer` 为空时不带对应的请求头。
pub fn login_request(
    ip: &str,
    origin: Option<&str>,
    referer: Option<&str>,
    password: &str,
) -> TestResult<Request<Body>> {
    let mut request = Request::post("/admin/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header("x-forwarded-for", ip);
    if let Some(origin) = origin {
        request = request.header(header::ORIGIN, origin);
    }
    if let Some(referer) = referer {
        request = request.header(header::REFERER, referer);
    }
    Ok(request.body(Body::from(encode(&[
        ("username", USERNAME),
        ("password", password),
    ])))?)
}

pub async fn try_login(site: &Site, ip: &str, password: &str) -> Reply {
    let request = login_request(ip, Some(BASE_URL), None, password);
    let reply = match request {
        Ok(request) => site.send(request).await,
        Err(error) => Err(error),
    };
    reply.unwrap_or_else(|error| panic!("login request failed: {error}"))
}

/// 登录后的浏览器：带着会话 cookie，从后台页面上读到 CSRF token。
pub struct Admin {
    pub cookie: String,
    pub csrf: String,
}

pub async fn login(site: &Site) -> TestResult<Admin> {
    let reply = try_login(site, CLIENT, PASSWORD).await;
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
    assert_eq!(reply.header(header::LOCATION), "/admin/");
    let set_cookie = reply.header(header::SET_COOKIE);
    let cookie = set_cookie.split(';').next().unwrap_or_default().to_owned();
    let mut admin = Admin {
        cookie,
        csrf: String::new(),
    };
    let page = admin.get(site, "/admin/").await?;
    admin.csrf = between(&page.body, r#"name="_csrf" value=""#, "\"").to_owned();
    assert!(!admin.csrf.is_empty(), "{}", page.body);
    Ok(admin)
}

impl Admin {
    pub async fn get(&self, site: &Site, uri: &str) -> TestResult<Reply> {
        let request = Request::get(uri)
            .header(header::COOKIE, &self.cookie)
            .body(Body::empty())?;
        site.send(request).await
    }

    /// 提交后台表单，自动带上 `_csrf`。
    pub async fn post(&self, site: &Site, uri: &str, fields: &[(&str, &str)]) -> TestResult<Reply> {
        let mut all = vec![("_csrf", self.csrf.as_str())];
        all.extend_from_slice(fields);
        self.post_raw(site, uri, &encode(&all)).await
    }

    pub async fn post_raw(&self, site: &Site, uri: &str, body: &str) -> TestResult<Reply> {
        let request = Request::post(uri)
            .header(header::COOKIE, &self.cookie)
            .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(Body::from(body.to_owned()))?;
        site.send(request).await
    }

    /// 新建一篇文章，返回它的 id。
    pub async fn create_post(&self, site: &Site, source: &str) -> TestResult<i64> {
        let reply = self
            .post(site, "/admin/posts/new", &[("source", source)])
            .await?;
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
        edit_id(reply.header(header::LOCATION))
    }
}

pub fn between<'a>(s: &'a str, start: &str, end: &str) -> &'a str {
    s.split_once(start)
        .and_then(|(_, rest)| rest.split_once(end))
        .map_or("", |(inner, _)| inner)
}

/// `/admin/edit/{id}?saved=1` 里的 id。
pub fn edit_id(location: &str) -> TestResult<i64> {
    let id = location
        .strip_prefix("/admin/edit/")
        .and_then(|rest| rest.strip_suffix("?saved=1"))
        .ok_or_else(|| format!("not an edit page after saving: {location}"))?;
    Ok(id.parse()?)
}

/// 编辑页文本框里的原文，反转义后的。
pub fn textarea(body: &str) -> String {
    let inner = between(body, "<textarea", "</textarea>");
    let text = inner.split_once(">\n").map_or("", |(_, t)| t);
    [
        ("&#34;", "\""),
        ("&#39;", "'"),
        ("&#60;", "<"),
        ("&#62;", ">"),
    ]
    .into_iter()
    .fold(text.to_owned(), |t, (entity, c)| t.replace(entity, c))
    .replace("&#38;", "&")
}
