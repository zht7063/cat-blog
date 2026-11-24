//! cat-blog 服务端：配置、数据库、路由。可执行文件见 `main.rs`。

pub mod clock;
pub mod config;
pub mod db;

use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::get;
use sqlx::SqlitePool;

/// 构建全部路由。
pub fn app(pool: SqlitePool) -> Router {
    Router::new()
        .route("/healthz", get(healthz))
        .with_state(pool)
}

async fn healthz(State(pool): State<SqlitePool>) -> (StatusCode, &'static str) {
    match db::ping(&pool).await {
        Ok(()) => (StatusCode::OK, "ok"),
        Err(error) => {
            tracing::error!(%error, "health check failed");
            (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
        }
    }
}
