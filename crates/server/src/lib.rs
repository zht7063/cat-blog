//! cat-blog 服务端：配置、数据库、路由。可执行文件见 `main.rs`。

pub mod assets;
pub mod auth;
pub mod clock;
pub mod config;
pub mod db;
mod render;
mod routes;
pub mod startup;

use std::sync::Arc;

use axum::Router;
use axum::extract::{Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::Response;
use axum::routing::get;
use cat_blog_core::markdown::Renderer;
use ipnet::IpNet;
use sqlx::SqlitePool;
use tower_http::limit::RequestBodyLimitLayer;
use tower_http::trace::{DefaultOnResponse, TraceLayer};
use tracing::Level;

use crate::assets::Assets;
use crate::auth::limiter::LoginLimiter;
use crate::clock::Clock;
use crate::config::Config;
use crate::db::BlogDb;
use crate::routes::{admin, feed, public};

/// 请求体上限（D62），超出时返回 413。
pub const BODY_LIMIT: usize = 2 * 1024 * 1024;

/// 请求处理函数共用的状态。
#[derive(Clone)]
pub struct AppState {
    pool: SqlitePool,
    /// 当前请求属于哪个博客（D6）。v1.0 只有一个（D66），以后改为按域名查找。
    blog: BlogDb,
    /// 去掉了末尾 `/` 的 `BASE_URL`。
    base_url: Arc<str>,
    /// `BASE_URL` 的 origin（`scheme://host[:port]`），登录时和 `Origin` 头比较（D47）。
    origin: Arc<str>,
    /// 会话 cookie 是否带 `Secure`（D36）。
    secure_cookies: bool,
    trusted_proxies: Arc<[IpNet]>,
    limiter: Arc<LoginLimiter>,
    clock: Arc<dyn Clock>,
    renderer: Arc<Renderer>,
    assets: Arc<Assets>,
}

impl AppState {
    pub fn new(
        pool: SqlitePool,
        blog: BlogDb,
        config: &Config,
        clock: Arc<dyn Clock>,
        renderer: Arc<Renderer>,
    ) -> Self {
        Self {
            pool,
            blog,
            base_url: config.base_url.as_str().into(),
            origin: config.origin().into(),
            secure_cookies: config.is_https(),
            trusted_proxies: config.trusted_proxies.as_slice().into(),
            limiter: Arc::new(LoginLimiter::default()),
            clock,
            renderer,
            assets: Arc::new(Assets::new()),
        }
    }
}

/// 构建全部路由。
pub fn app(state: AppState) -> Router {
    let access_log = TraceLayer::new_for_http()
        .make_span_with(|request: &Request| {
            tracing::info_span!("request", method = %request.method(), path = %request.uri().path())
        })
        .on_response(DefaultOnResponse::new().level(Level::INFO));
    Router::new()
        .route("/", get(public::home))
        .route("/blog/", get(public::blog))
        .route("/blog", get(public::add_slash))
        .route("/admin", get(public::add_slash))
        .route("/feed.xml", get(feed::feed))
        .route("/sitemap.xml", get(feed::sitemap))
        .route("/robots.txt", get(feed::robots))
        .route("/static/{name}", get(public::asset))
        .route("/favicon.ico", get(public::favicon))
        .route("/healthz", get(healthz))
        .merge(admin::router(state.clone()))
        .fallback(get(public::entry))
        .with_state(state)
        .layer(RequestBodyLimitLayer::new(BODY_LIMIT))
        .layer(middleware::from_fn(security_headers))
        .layer(access_log)
}

/// D74：所有响应加 `nosniff` 和 `Referrer-Policy`，`/admin/` 下另加 `X-Frame-Options: DENY`。
async fn security_headers(request: Request, next: Next) -> Response {
    let path = request.uri().path();
    let admin = path == "/admin" || path.starts_with("/admin/");
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );
    if admin {
        headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    }
    response
}

async fn healthz(State(state): State<AppState>) -> (StatusCode, &'static str) {
    match db::ping(&state.pool).await {
        Ok(()) => (StatusCode::OK, "ok"),
        Err(error) => {
            tracing::error!(%error, "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
        }
    }
}
