//! `cat-blog backup <目标文件>`（D40，docs/v1.0-spec.md §10.3）。

use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io;
#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

use sqlx::SqlitePool;

use crate::db;

/// 用 `VACUUM INTO` 把数据库导出到 `target`，再对副本执行 `PRAGMA integrity_check`。
/// 副本里有密码哈希和会话哈希，所以先以权限 600 新建空的目标文件，再往里写；
/// 目标文件已存在时报错，不覆盖。目标目录不存在时创建。失败时删掉写了一半的副本，
/// 只有完整性检查不通过的副本留在原处，方便检查。
pub async fn run(pool: &SqlitePool, target: &Path) -> Result<(), BackupError> {
    let target_str = target
        .to_str()
        .ok_or_else(|| BackupError::NotUtf8(target.to_owned()))?;
    create_private_file(target)?;
    let result = write_and_check(pool, target, target_str).await;
    if let Err(error) = &result
        && !matches!(error, BackupError::Corrupt { .. })
        && let Err(remove) = fs::remove_file(target)
    {
        tracing::warn!(error = %remove, path = %target.display(), "incomplete backup could not be removed");
    }
    result
}

async fn write_and_check(
    pool: &SqlitePool,
    target: &Path,
    target_str: &str,
) -> Result<(), BackupError> {
    db::vacuum_into(pool, target_str)
        .await
        .map_err(BackupError::Vacuum)?;
    let report = db::integrity_check(target)
        .await
        .map_err(BackupError::Check)?;
    if report != ["ok"] {
        return Err(BackupError::Corrupt {
            path: target.to_owned(),
            report,
        });
    }
    File::open(target)
        .and_then(|file| file.sync_all())
        .map_err(|error| BackupError::Io(target.to_owned(), error))
}

fn create_private_file(target: &Path) -> Result<(), BackupError> {
    let io_error = |error| BackupError::Io(target.to_owned(), error);
    if let Some(dir) = target.parent().filter(|dir| !dir.as_os_str().is_empty()) {
        fs::create_dir_all(dir).map_err(io_error)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    match options.open(target) {
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
            Err(BackupError::Exists(target.to_owned()))
        }
        Err(error) => Err(io_error(error)),
    }
}

#[derive(Debug)]
pub enum BackupError {
    Exists(PathBuf),
    NotUtf8(PathBuf),
    Io(PathBuf, io::Error),
    Vacuum(sqlx::Error),
    Check(sqlx::Error),
    /// 副本留在原处，方便检查。
    Corrupt {
        path: PathBuf,
        report: Vec<String>,
    },
}

impl fmt::Display for BackupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exists(path) => write!(
                f,
                "{} already exists and backup does not overwrite files; choose a new file name",
                path.display()
            ),
            Self::NotUtf8(path) => write!(
                f,
                "{} is not valid UTF-8; choose a target path without such characters",
                path.display()
            ),
            Self::Io(path, error) => write!(f, "{} could not be written: {error}", path.display()),
            Self::Vacuum(error) => write!(
                f,
                "VACUUM INTO failed: {error}; check free disk space and that the target directory is writable"
            ),
            Self::Check(error) => write!(
                f,
                "backup could not be opened for PRAGMA integrity_check: {error}"
            ),
            Self::Corrupt { path, report } => write!(
                f,
                "backup {} failed PRAGMA integrity_check: {}; the source database may be damaged, \
                 run PRAGMA integrity_check on it before relying on this copy",
                path.display(),
                report.join("; ")
            ),
        }
    }
}

impl std::error::Error for BackupError {}
