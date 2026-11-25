//! `users` 表。v1.0 只有一个管理员（D13、D66）。

use jiff::Timestamp;
use sqlx::SqlitePool;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserId(pub(super) i64);

#[derive(Debug, Clone)]
pub struct User {
    pub id: UserId,
    pub username: String,
    /// argon2id 的 PHC 字符串（D17）。
    pub password_hash: String,
}

/// 唯一的管理员；表还是空的时返回 `None`。
pub async fn admin(pool: &SqlitePool) -> Result<Option<User>, sqlx::Error> {
    let row = sqlx::query!(
        r#"SELECT id AS "id!", username, password_hash FROM users ORDER BY id LIMIT 1"#
    )
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| User {
        id: UserId(r.id),
        username: r.username,
        password_hash: r.password_hash,
    }))
}

pub async fn count(pool: &SqlitePool) -> Result<i64, sqlx::Error> {
    sqlx::query_scalar!("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await
}

pub async fn create(
    pool: &SqlitePool,
    username: &str,
    password_hash: &str,
    now: Timestamp,
) -> Result<UserId, sqlx::Error> {
    let now = now.as_second();
    let id = sqlx::query!(
        "INSERT INTO users (username, password_hash, created_at, updated_at) VALUES (?, ?, ?, ?)",
        username,
        password_hash,
        now,
        now,
    )
    .execute(pool)
    .await?
    .last_insert_rowid();
    Ok(UserId(id))
}

/// 改用户名和密码哈希，并在同一个事务里清空所有会话（D14、§4.1 第 3 步）。
pub async fn replace_credentials(
    pool: &SqlitePool,
    id: UserId,
    username: &str,
    password_hash: &str,
    now: Timestamp,
) -> Result<(), sqlx::Error> {
    let now = now.as_second();
    let mut tx = pool.begin().await?;
    sqlx::query!(
        "UPDATE users SET username = ?, password_hash = ?, updated_at = ? WHERE id = ?",
        username,
        password_hash,
        now,
        id.0,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!("DELETE FROM sessions")
        .execute(&mut *tx)
        .await?;
    tx.commit().await
}
