//! 启动流程第 2–5 步（docs/v1.0-spec.md §4.1）：迁移、同步管理员、确保博客存在、
//! 删除过期会话。每一步都按库里现有的状态决定做什么，重复启动结果相同。

use std::error::Error;

use cat_blog_core::frontmatter::Kind;
use cat_blog_core::markdown::Renderer;
use cat_blog_core::save;
use sqlx::SqlitePool;

use crate::auth::password;
use crate::clock::Clock;
use crate::config::Config;
use crate::db::{self, BlogDb, sessions, users};

/// 第一次启动时的首页（D28）。
pub const DEFAULT_HOME: &str = include_str!("defaults/home.md");
/// 第一次启动时的示例草稿（D28）。
pub const SAMPLE_POST: &str = include_str!("defaults/hello.md");

pub type StartupError = Box<dyn Error + Send + Sync>;

/// 执行迁移并准备好数据，返回唯一的博客（D66）。
pub async fn prepare(
    pool: &SqlitePool,
    config: &Config,
    clock: &dyn Clock,
    renderer: &Renderer,
) -> Result<BlogDb, StartupError> {
    db::migrate(pool).await?;
    let admin = sync_admin(pool, config, clock).await?;
    let blog = match BlogDb::for_user(pool, admin).await? {
        Some(blog) => blog,
        None => create_blog(pool, admin, clock, renderer).await?,
    };
    let expired = sessions::delete_expired(pool, clock.now()).await?;
    if expired > 0 {
        tracing::info!(count = expired, "deleted expired sessions");
    }
    Ok(blog)
}

/// 让 `users` 里的管理员和 `.env` 一致（D14、D66）。用户名或密码变了时清空所有会话。
async fn sync_admin(
    pool: &SqlitePool,
    config: &Config,
    clock: &dyn Clock,
) -> Result<users::UserId, StartupError> {
    let username = config.admin_username.as_str();
    let password = config.admin_password.expose();
    let Some(user) = users::admin(pool).await? else {
        let hash = password::hash(password).await?;
        let id = users::create(pool, username, &hash, clock.now()).await?;
        tracing::info!(username, "created admin user");
        return Ok(id);
    };

    let renamed = user.username != username;
    let password_matches = password::verify(password, &user.password_hash).await?;
    if renamed || !password_matches {
        let hash = if password_matches {
            user.password_hash
        } else {
            password::hash(password).await?
        };
        users::replace_credentials(pool, user.id, username, &hash, clock.now()).await?;
        tracing::info!(
            username,
            renamed,
            password_changed = !password_matches,
            "admin credentials changed, cleared all sessions"
        );
    }
    Ok(user.id)
}

async fn create_blog(
    pool: &SqlitePool,
    admin: users::UserId,
    clock: &dyn Clock,
    renderer: &Renderer,
) -> Result<BlogDb, StartupError> {
    let now = clock.now();
    let home = save::prepare_home(renderer, DEFAULT_HOME)?;
    let sample = save::prepare_entry(renderer, SAMPLE_POST, Kind::Post, &home.timezone, None, now)?;
    let blog = BlogDb::create(pool, admin, &home, &[(Kind::Post, &sample)], now).await?;
    tracing::info!("created blog with default home page and sample draft");
    Ok(blog)
}
