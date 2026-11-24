use std::process::ExitCode;

use cat_blog_server::config::Config;
use cat_blog_server::{app, db};
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

async fn serve(config: &Config) -> Result<(), Box<dyn std::error::Error>> {
    let pool = db::connect(&config.database_path).await?;
    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %config.bind_addr, "listening");
    axum::serve(listener, app(pool)).await?;
    Ok(())
}
