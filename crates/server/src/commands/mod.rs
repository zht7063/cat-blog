//! `serve` 以外的命令（D73）：[`rerender`] 和 [`backup`]。它们和 `serve` 读同一套配置，
//! 服务运行时也可以执行。

pub mod backup;
pub mod rerender;

use std::path::Path;

use sqlx::SqlitePool;

use crate::db;

pub type CommandError = Box<dyn std::error::Error + Send + Sync>;

/// 打开 `serve` 建好的数据库。文件不存在时报错，不新建。
pub async fn open_database(path: &Path) -> Result<SqlitePool, CommandError> {
    if !tokio::fs::try_exists(path).await? {
        return Err(format!(
            "database {} does not exist; check DATABASE_PATH, or run `cat-blog serve` once to create it",
            path.display()
        )
        .into());
    }
    Ok(db::open(path).await?)
}
