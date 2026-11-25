use std::process::ExitCode;
use std::sync::Arc;

use cat_blog_core::markdown::Renderer;
use cat_blog_server::clock::SystemClock;
use cat_blog_server::config::Config;
use cat_blog_server::{AppState, app, db, startup};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => {}
        [cmd] if cmd == "serve" => {}
        _ => {
            return early_exit(&format!(
                "unknown command {args:?}; usage: cat-blog [serve]"
            ));
        }
    }

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => return early_exit(&error.to_string()),
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_filter))
        .init();

    match serve(&config).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "cat-blog stopped");
            ExitCode::FAILURE
        }
    }
}

#[expect(
    clippy::print_stderr,
    reason = "配置读完之前日志还没有初始化，只能直接写 stderr"
)]
fn early_exit(message: &str) -> ExitCode {
    eprintln!("cat-blog: {message}");
    ExitCode::FAILURE
}

async fn serve(config: &Config) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if !config.is_https() {
        tracing::warn!(
            base_url = %config.base_url,
            "BASE_URL is not https://; session cookies will not be Secure, use HTTPS in production"
        );
    }
    let pool = db::connect(&config.database_path).await?;
    let clock = Arc::new(SystemClock);
    let blog = startup::prepare(&pool, config, clock.as_ref(), &Renderer::new()).await?;
    let state = AppState::new(pool.clone(), blog, config, clock);

    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    pool.close().await;
    tracing::info!("stopped");
    Ok(())
}

/// SIGTERM 或 Ctrl+C 到来时返回。之后 axum 不再接收新连接，等进行中的请求结束（§4.1 第 6 步）。
async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!(%error, "Ctrl+C handler could not be installed");
            std::future::pending::<()>().await;
        }
    };
    #[cfg(unix)]
    let terminate = async {
        use tokio::signal::unix::{SignalKind, signal};
        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                sigterm.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "SIGTERM handler could not be installed");
                std::future::pending::<()>().await;
            }
        }
    };
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => {}
        () = terminate => {}
    }
    tracing::info!("shutting down, waiting for in-flight requests");
}
