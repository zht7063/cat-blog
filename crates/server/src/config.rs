#![expect(
    clippy::disallowed_methods,
    reason = "config 模块是唯一读环境变量和 .env 的地方（docs/conventions.md §3）"
)]

use std::env::{self, VarError};
use std::fmt;
use std::net::SocketAddr;
use std::path::PathBuf;

use axum::http::Uri;
use ipnet::IpNet;

/// `ADMIN_PASSWORD` 至少要有的字符数（D14）。
pub const MIN_PASSWORD_CHARS: usize = 12;

/// 启动时从环境变量和 `.env` 读取的配置（设计文档 §4、docs/v1.0-spec.md §10.1）。
#[derive(Debug, Clone)]
pub struct Config {
    pub admin_username: String,
    pub admin_password: AdminPassword,
    /// 不带末尾 `/`，如 `https://example.com`。
    pub base_url: String,
    pub bind_addr: SocketAddr,
    pub database_path: PathBuf,
    pub trusted_proxies: Vec<IpNet>,
    pub log_filter: String,
}

/// `.env` 里的明文管理员密码。`Debug` 不显示内容，免得被打进日志。
#[derive(Clone, PartialEq, Eq)]
pub struct AdminPassword(String);

impl AdminPassword {
    pub fn new(password: impl Into<String>) -> Self {
        Self(password.into())
    }

    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for AdminPassword {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("AdminPassword(..)")
    }
}

impl Config {
    /// 先加载当前目录的 `.env`（不存在时跳过），再读取环境变量。
    pub fn from_env() -> Result<Self, ConfigError> {
        if let Err(error) = dotenvy::dotenv()
            && !error.not_found()
        {
            return Err(ConfigError::DotEnv(error));
        }
        Self::from_lookup(read)
    }

    /// 按启动流程第 1 步（docs/v1.0-spec.md §4.1）校验配置。`lookup` 返回变量的值，
    /// 没设置时返回 `None`。设置成空字符串的变量视同没设置，只有 `TRUSTED_PROXIES`
    /// 例外：写成空表示不信任任何代理。
    pub fn from_lookup(
        lookup: impl Fn(&'static str) -> Result<Option<String>, ConfigError>,
    ) -> Result<Self, ConfigError> {
        let get = |var| Ok(lookup(var)?.filter(|v| !v.trim().is_empty()));
        let required = |var| get(var)?.ok_or(ConfigError::Missing { var });

        let admin_password = required("ADMIN_PASSWORD")?;
        let chars = admin_password.chars().count();
        if chars < MIN_PASSWORD_CHARS {
            return Err(ConfigError::PasswordTooShort { chars });
        }

        let bind_addr = get("BIND_ADDR")?.unwrap_or_else(|| "127.0.0.1:3000".to_owned());
        let bind_addr = bind_addr
            .parse()
            .map_err(|error: std::net::AddrParseError| ConfigError::Invalid {
                var: "BIND_ADDR",
                value: bind_addr.clone(),
                reason: error.to_string(),
            })?;

        let trusted_proxies =
            lookup("TRUSTED_PROXIES")?.unwrap_or_else(|| "127.0.0.1/32,::1/128".to_owned());

        Ok(Self {
            admin_username: required("ADMIN_USERNAME")?.trim().to_owned(),
            admin_password: AdminPassword(admin_password),
            base_url: parse_base_url(&required("BASE_URL")?)?,
            bind_addr,
            database_path: get("DATABASE_PATH")?
                .map_or_else(|| PathBuf::from("./data/cat-blog.db"), PathBuf::from),
            trusted_proxies: parse_proxies(&trusted_proxies)?,
            log_filter: get("RUST_LOG")?.unwrap_or_else(|| "info".to_owned()),
        })
    }

    /// `BASE_URL` 是 `https://` 时会话 cookie 带 `Secure`（D36）。
    pub fn is_https(&self) -> bool {
        self.base_url.starts_with("https://")
    }
}

fn parse_base_url(value: &str) -> Result<String, ConfigError> {
    let invalid = |reason: &str| ConfigError::Invalid {
        var: "BASE_URL",
        value: value.to_owned(),
        reason: reason.to_owned(),
    };
    let trimmed = value.trim().trim_end_matches('/');
    let uri: Uri = trimmed
        .parse()
        .map_err(|_| invalid("not a valid URL, write it like https://example.com"))?;
    if !matches!(uri.scheme_str(), Some("http" | "https")) {
        return Err(invalid("scheme is not http:// or https://"));
    }
    if uri.host().is_none_or(str::is_empty) {
        return Err(invalid(
            "host is missing, write it like https://example.com",
        ));
    }
    if uri.query().is_some() {
        return Err(invalid("contains a query string, remove the part after ?"));
    }
    Ok(trimmed.to_owned())
}

fn parse_proxies(value: &str) -> Result<Vec<IpNet>, ConfigError> {
    value
        .split(',')
        .map(str::trim)
        .filter(|item| !item.is_empty())
        .map(|item| {
            item.parse().map_err(|_| ConfigError::Invalid {
                var: "TRUSTED_PROXIES",
                value: value.to_owned(),
                reason: format!("{item:?} is not an IP range, write it like 10.0.0.0/8 or ::1/128"),
            })
        })
        .collect()
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
    Missing {
        var: &'static str,
    },
    /// 不带密码本身，错误信息会打到终端和日志里。
    PasswordTooShort {
        chars: usize,
    },
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
            Self::Missing { var } => write!(f, "{var} is not set; add it to .env"),
            Self::PasswordTooShort { chars } => write!(
                f,
                "ADMIN_PASSWORD has {chars} characters, fewer than {MIN_PASSWORD_CHARS}; \
                 set a longer password in .env"
            ),
            Self::Invalid { var, value, reason } => {
                write!(f, "{var}={value:?} is invalid: {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    const VALID: [(&str, &str); 3] = [
        ("ADMIN_USERNAME", "admin"),
        ("ADMIN_PASSWORD", "correct horse battery"),
        ("BASE_URL", "https://example.com/"),
    ];

    fn parse(overrides: &[(&'static str, &str)]) -> Result<Config, ConfigError> {
        let mut vars: HashMap<&str, String> =
            VALID.iter().map(|(k, v)| (*k, (*v).to_owned())).collect();
        for (k, v) in overrides {
            vars.insert(k, (*v).to_owned());
        }
        Config::from_lookup(|var| Ok(vars.get(var).cloned()))
    }

    #[test]
    fn defaults_and_trailing_slash() {
        let config = parse(&[]).unwrap();
        assert_eq!(config.base_url, "https://example.com");
        assert!(config.is_https());
        assert_eq!(config.bind_addr, "127.0.0.1:3000".parse().unwrap());
        assert_eq!(config.database_path, PathBuf::from("./data/cat-blog.db"));
        assert_eq!(
            config.trusted_proxies,
            vec![
                "127.0.0.1/32".parse::<IpNet>().unwrap(),
                "::1/128".parse().unwrap()
            ]
        );
    }

    #[test]
    fn password_length_counts_characters_not_bytes() {
        let err = parse(&[("ADMIN_PASSWORD", "猫猫猫猫猫猫猫猫猫猫猫")]).unwrap_err();
        assert!(
            matches!(err, ConfigError::PasswordTooShort { chars: 11 }),
            "{err}"
        );
        assert!(!err.to_string().contains('猫'));
        assert!(parse(&[("ADMIN_PASSWORD", "猫猫猫猫猫猫猫猫猫猫猫猫")]).is_ok());
    }

    #[test]
    fn empty_required_variable_is_missing() {
        let err = parse(&[("ADMIN_USERNAME", "  ")]).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::Missing {
                var: "ADMIN_USERNAME"
            }
        ));
    }

    #[test]
    fn base_url_must_be_http_or_https() {
        for bad in [
            "example.com",
            "ftp://example.com",
            "https://",
            "https://x.com/?a=1",
        ] {
            let err = parse(&[("BASE_URL", bad)]).unwrap_err();
            assert!(
                matches!(
                    err,
                    ConfigError::Invalid {
                        var: "BASE_URL",
                        ..
                    }
                ),
                "{bad}: {err}"
            );
        }
        assert!(
            !parse(&[("BASE_URL", "http://localhost:3000")])
                .unwrap()
                .is_https()
        );
    }

    #[test]
    fn trusted_proxies_must_all_parse() {
        let config = parse(&[("TRUSTED_PROXIES", " 172.16.0.0/12 , 10.0.0.0/8,")]).unwrap();
        assert_eq!(config.trusted_proxies.len(), 2);
        let none = parse(&[("TRUSTED_PROXIES", "")]).unwrap();
        assert!(none.trusted_proxies.is_empty());
        let err = parse(&[("TRUSTED_PROXIES", "172.16.0.0/12,10.0.0.1")]).unwrap_err();
        assert!(err.to_string().contains("\"10.0.0.1\""), "{err}");
    }
}
