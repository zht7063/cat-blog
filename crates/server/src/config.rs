#![expect(
    clippy::disallowed_methods,
    reason = "config 模块是唯一读环境变量和 .env 的地方（docs/conventions.md §3）"
)]

use std::env::{self, VarError};
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

/// 启动时从环境变量和 `.env` 读取的配置（设计文档 §4）。
#[derive(Debug)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub database_path: PathBuf,
    pub log_filter: String,
}

impl Config {
    /// 先加载当前目录的 `.env`（不存在时跳过），再读取环境变量。
    pub fn from_env() -> Result<Self, ConfigError> {
        if let Err(error) = dotenvy::dotenv()
            && !error.not_found()
        {
            return Err(ConfigError::DotEnv(error));
        }

        let bind_addr = read("BIND_ADDR")?.unwrap_or_else(|| "127.0.0.1:3000".to_owned());
        let bind_addr = bind_addr
            .parse()
            .map_err(|error: std::net::AddrParseError| ConfigError::Invalid {
                var: "BIND_ADDR",
                value: bind_addr.clone(),
                reason: error.to_string(),
            })?;

        Ok(Self {
            bind_addr,
            database_path: read("DATABASE_PATH")?
                .map_or_else(|| PathBuf::from("./data/cat-blog.db"), PathBuf::from),
            log_filter: read("RUST_LOG")?.unwrap_or_else(|| "info".to_owned()),
        })
    }
}

fn read(var: &'static str) -> Result<Option<String>, ConfigError> {
    match env::var(var) {
        Ok(value) => Ok(Some(value)),
        Err(VarError::NotPresent) => Ok(None),
        Err(VarError::NotUnicode(value)) => Err(ConfigError::Invalid {
            var,
            value: value.to_string_lossy().into_owned(),
            reason: "value is not valid UTF-8".to_owned(),
        }),
    }
}

#[derive(Debug)]
pub enum ConfigError {
    DotEnv(dotenvy::Error),
    Invalid {
        var: &'static str,
        value: String,
        reason: String,
    },
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DotEnv(error) => write!(f, ".env could not be loaded: {error}"),
            Self::Invalid { var, value, reason } => {
                write!(f, "{var}={value:?} is invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}
