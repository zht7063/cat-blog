//! 唯一写 SQL 的地方（docs/conventions.md §3）。操作内容的查询都在 [`BlogDb`] 上，
//! 它绑定了一个 `blog_id`（D6）。

mod blogs;
mod posts;
pub mod sessions;
pub mod users;

use std::path::Path;
use std::time::Duration;

use jiff::Timestamp;
use sqlx::error::BoxDynError;
use sqlx::migrate::{MigrateError, Migrator};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqliteSynchronous};
use sqlx::{Connection, SqliteConnection, SqlitePool};

pub use blogs::{Blog, BlogDb};
pub use posts::{Post, PostId, PostSummary, WriteError};

static MIGRATOR: Migrator = sqlx::migrate!("../../migrations");

/// 打开 SQLite 数据库（D72），文件和所在目录不存在时创建。
pub async fn connect(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    if let Some(dir) = path.parent() {
        tokio::fs::create_dir_all(dir).await?;
    }
    let options = options(path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal);
    SqlitePool::connect_with(options).await
}

/// 打开 [`connect`] 建好的数据库，文件不存在时报错，不新建。`rerender`、`backup` 用它，
/// 可以和运行中的服务同时打开同一个库。
pub async fn open(path: &Path) -> Result<SqlitePool, sqlx::Error> {
    SqlitePool::connect_with(options(path)).await
}

fn options(path: &Path) -> SqliteConnectOptions {
    SqliteConnectOptions::new()
        .filename(path)
        .synchronous(SqliteSynchronous::Normal)
        .foreign_keys(true)
        .busy_timeout(Duration::from_secs(5))
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

/// 把数据库导出为 `target` 处的一份一致的单文件副本（D40）。服务运行时也可以执行。
/// `target` 必须不存在或是空文件。
pub async fn vacuum_into(pool: &SqlitePool, target: &str) -> Result<(), sqlx::Error> {
    sqlx::query!("VACUUM INTO ?", target).execute(pool).await?;
    Ok(())
}

/// 以只读方式打开 `path` 处的数据库，执行 `PRAGMA integrity_check`，返回它的结果行。
/// 数据库完好时只有一行 `ok`。
pub async fn integrity_check(path: &Path) -> Result<Vec<String>, sqlx::Error> {
    let options = SqliteConnectOptions::new().filename(path).read_only(true);
    let mut conn = SqliteConnection::connect_with(&options).await?;
    let rows = sqlx::query_scalar!(r#"PRAGMA integrity_check"#)
        .fetch_all(&mut conn)
        .await?;
    conn.close().await?;
    Ok(rows.into_iter().map(Option::unwrap_or_default).collect())
}

/// 库里存的 Unix 秒（D71）转回时间。
fn timestamp(seconds: i64) -> Result<Timestamp, sqlx::Error> {
    Timestamp::from_second(seconds).map_err(decode_error)
}

fn decode_error(error: impl Into<BoxDynError>) -> sqlx::Error {
    sqlx::Error::Decode(error.into())
}
