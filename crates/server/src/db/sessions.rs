//! `sessions` 表。库里只存 token 的 SHA-256（D16）；过期时间固定，不顺延（D49）。

use jiff::Timestamp;
use sqlx::SqlitePool;

use super::timestamp;
use super::users::UserId;

/// 会话 token 的 SHA-256。
pub type TokenHash = [u8; 32];

#[derive(Debug, Clone)]
pub struct NewSession<'a> {
    pub user_id: UserId,
    pub token_hash: &'a TokenHash,
    pub csrf_token: &'a str,
    pub created_at: Timestamp,
    pub expires_at: Timestamp,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub user_id: UserId,
    pub csrf_token: String,
    pub expires_at: Timestamp,
}

pub async fn create(pool: &SqlitePool, session: &NewSession<'_>) -> Result<(), sqlx::Error> {
    let token_hash = session.token_hash.as_slice();
    let created_at = session.created_at.as_second();
    let expires_at = session.expires_at.as_second();
    sqlx::query!(
        "INSERT INTO sessions (user_id, token_hash, csrf_token, created_at, expires_at)
         VALUES (?, ?, ?, ?, ?)",
        session.user_id.0,
        token_hash,
        session.csrf_token,
        created_at,
        expires_at,
    )
    .execute(pool)
    .await?;
    Ok(())
}

/// 按 token 哈希查会话；已过期（`expires_at <= now`）的当作不存在。
pub async fn find(
    pool: &SqlitePool,
    token_hash: &TokenHash,
    now: Timestamp,
) -> Result<Option<Session>, sqlx::Error> {
    let token_hash = token_hash.as_slice();
    let now = now.as_second();
    let row = sqlx::query!(
        "SELECT user_id, csrf_token, expires_at FROM sessions
         WHERE token_hash = ? AND expires_at > ?",
        token_hash,
        now,
    )
    .fetch_optional(pool)
    .await?;
    row.map(|r| {
        Ok(Session {
            user_id: UserId(r.user_id),
            csrf_token: r.csrf_token,
            expires_at: timestamp(r.expires_at)?,
        })
    })
    .transpose()
}

/// 退出登录时删除当前会话。
pub async fn delete(pool: &SqlitePool, token_hash: &TokenHash) -> Result<(), sqlx::Error> {
    let token_hash = token_hash.as_slice();
    sqlx::query!("DELETE FROM sessions WHERE token_hash = ?", token_hash)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除所有已过期的会话，返回删除的条数。
pub async fn delete_expired(pool: &SqlitePool, now: Timestamp) -> Result<u64, sqlx::Error> {
    let now = now.as_second();
    let result = sqlx::query!("DELETE FROM sessions WHERE expires_at <= ?", now)
        .execute(pool)
        .await?;
    Ok(result.rows_affected())
}
