//! 唯一写 SQL 的地方（docs/conventions.md §3）。操作内容的查询都在 [`BlogDb`] 上，
//! 它绑定了一个 `blog_id`（D6）。

mod blogs;
mod posts;
pub mod sessions;
pub mod users;

use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;
use sqlx::SqlitePool;
use sqlx::error::BoxDynError;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};

pub use blogs::{Blog, BlogDb};
pub use posts::{Post, PostId, PostSummary, WriteError};

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// 打开 SQLite 数据库（D72），文件和所在目录不存在时创建。
pub async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5));
    SqlitePool::connect_with(options).await
}

/// 执行 `migrations/` 里还没执行过的迁移（D2）。
pub async fn migrate(pool: &SqlitePool) -> Result<(), MigrateError> {
    MIGRATOR.run(pool).await
}

/// 健康检查用的 `SELECT 1`（D64）。
pub async fn ping(pool: &SqlitePool) -> Result<(), sqlx::Error> {
    sqlx::query_scalar!("SELECT 1").fetch_one(pool).await?;
    Ok(())
}

/// 库里存的 Unix 秒（D71）转回时间。
fn timestamp(seconds: i64) -> Result<Timestamp, sqlx::Error> {
    Timestamp::from_second(seconds).map_err(decode_error)
}

fn decode_error(error: impl Into<BoxDynError>) -> sqlx::Error {
    sqlx::Error::Decode(error.into())
}
