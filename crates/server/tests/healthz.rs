use std::error::Error;

use axum::body::{Body, to_bytes};
use axum::http::{Request, StatusCode};
use sqlx::SqlitePool;
use tower::ServiceExt;

async fn get_healthz(pool: SqlitePool) -> Result<(StatusCode, String), Box<dyn Error>> {
    let request = Request::get("/healthz").body(Body::empty())?;
    let response = cat_blog_server::app(pool).oneshot(request).await?;
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await?;
    Ok((status, String::from_utf8(body.to_vec())?))
}

#[tokio::test]
async fn healthz_returns_ok_when_database_answers() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();

    assert_eq!(
        get_healthz(pool).await.unwrap(),
        (StatusCode::OK, "ok".to_owned())
    );
}

#[tokio::test]
async fn healthz_returns_503_when_database_is_unreachable() {
    let pool = SqlitePool::connect("sqlite::memory:").await.unwrap();
    pool.close().await;

    let (status, _) = get_healthz(pool).await.unwrap();
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
}
