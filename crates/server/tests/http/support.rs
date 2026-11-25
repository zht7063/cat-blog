//! HTTP 测试共用的站点：临时目录里的数据库、固定时钟，用 `oneshot` 直接调用路由，不开端口。

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
use jiff::Timestamp;
use sqlx::SqlitePool;
use tempfile::TempDir;
use tower::ServiceExt;

pub type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static RENDERER: LazyLock<Renderer> = LazyLock::new(Renderer::new);

/// 2026-11-24T00:00:00Z
pub const T0: Timestamp = Timestamp::constant(1_795_478_400, 0);

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
pub async fn site() -> TestResult<Site> {
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
