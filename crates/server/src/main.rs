use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use cat_blog_core::markdown::Renderer;
use cat_blog_server::clock::{Clock, SystemClock};
use cat_blog_server::commands::{self, CommandError, backup, rerender};
use cat_blog_server::config::Config;
use cat_blog_server::db::{BlogDb, users};
use cat_blog_server::{AppState, app, db, startup};
use clap::{Parser, Subcommand};
use tracing_subscriber::EnvFilter;

/// 单人自部署的简洁博客。三个命令都从当前目录的 `.env` 和环境变量读配置。
#[derive(Parser)]
#[command(name = "cat-blog", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// 启动服务（不写命令时的默认值）
    Serve,
    /// 按当前规则和首页时区重新渲染所有内容，不改原文（升级或改时区后执行）
    Rerender,
    /// 把数据库导出为一份一致的副本，服务运行时也可以执行
    Backup {
        /// 副本的路径，文件必须不存在
        target: PathBuf,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    let cli = Cli::parse();

    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => return early_exit(&error.to_string()),
    };

    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::new(&config.log_filter))
        .init();

    let result = match cli.command.unwrap_or(Command::Serve) {
        Command::Serve => serve(&config).await,
        Command::Rerender => run_rerender(&config).await,
        Command::Backup { target } => run_backup(&config, &target).await,
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "cat-blog failed");
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

async fn serve(config: &Config) -> Result<(), CommandError> {
    if !config.is_https() {
        tracing::warn!(
            base_url = %config.base_url,
            "BASE_URL is not https://; session cookies will not be Secure, use HTTPS in production"
        );
    }
    let pool = db::connect(&config.database_path).await?;
    let clock = Arc::new(SystemClock);
    let renderer = Arc::new(Renderer::new());
    let blog = startup::prepare(&pool, config, clock.as_ref(), &renderer).await?;
    let state = AppState::new(pool.clone(), blog, config, clock, renderer);

    let listener = tokio::net::TcpListener::bind(config.bind_addr).await?;
    tracing::info!(addr = %listener.local_addr()?, "listening");
    let service = app(state).into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, service)
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    pool.close().await;
    tracing::info!("stopped");
    Ok(())
}

async fn run_rerender(config: &Config) -> Result<(), CommandError> {
    let pool = commands::open_database(&config.database_path).await?;
    let admin = users::admin(&pool)
        .await?
        .ok_or("database has no admin user; run `cat-blog serve` once to create it")?;
    let blog = BlogDb::for_user(&pool, admin.id)
        .await?
        .ok_or("database has no blog; run `cat-blog serve` once to create it")?;
    let outcome = rerender::run(&blog, &Renderer::new(), SystemClock.now()).await?;
    pool.close().await;
    for failure in &outcome.failed {
        tracing::error!(item = %failure.target, reason = %failure.reason, "skipped, left unchanged");
    }
    tracing::info!(
        rendered = outcome.rendered,
        saved_meanwhile = outcome.saved_meanwhile,
        failed = outcome.failed.len(),
        "rerender finished"
    );
    if outcome.failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} item(s) failed to rerender, see the errors above",
            outcome.failed.len()
        )
        .into())
    }
}

async fn run_backup(config: &Config, target: &Path) -> Result<(), CommandError> {
    let pool = commands::open_database(&config.database_path).await?;
    let result = backup::run(&pool, target).await;
    pool.close().await;
    result?;
    tracing::info!(path = %target.display(), "backup written and passed integrity_check");
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
