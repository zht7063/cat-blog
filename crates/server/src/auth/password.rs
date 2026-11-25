//! argon2id 密码哈希（D17）。哈希很耗 CPU，放到阻塞线程池里做，不占住异步运行时。

use argon2::Argon2;
use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use tokio::task;

/// 用随机盐哈希密码，返回 PHC 字符串。
pub async fn hash(password: &str) -> Result<String, PasswordError> {
    let password = password.to_owned();
    task::spawn_blocking(move || {
        Argon2::default()
            .hash_password(password.as_bytes())
            .map(|h| h.to_string())
            .map_err(|e| PasswordError(e.to_string()))
    })
    .await
    .map_err(|e| PasswordError(e.to_string()))?
}

/// 密码和 PHC 字符串是否匹配。PHC 字符串格式不对时也算不匹配。
pub async fn verify(password: &str, phc: &str) -> Result<bool, PasswordError> {
    let (password, phc) = (password.to_owned(), phc.to_owned());
    task::spawn_blocking(move || {
        PasswordHash::new(&phc).is_ok_and(|hash| {
            Argon2::default()
                .verify_password(password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|e| PasswordError(e.to_string()))
}

#[derive(Debug)]
pub struct PasswordError(String);

impl std::fmt::Display for PasswordError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "password could not be hashed or verified: {}", self.0)
    }
}

impl std::error::Error for PasswordError {}
