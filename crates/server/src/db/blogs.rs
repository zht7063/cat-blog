//! `blogs` 表和绑定了 `blog_id` 的句柄 [`BlogDb`]（D6、D18）。

use cat_blog_core::date;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::save::{PreparedEntry, PreparedHome};
use jiff::Timestamp;
use jiff::tz::TimeZone;
use sqlx::SqlitePool;

use super::posts::{self, WriteError};
use super::users::UserId;
use super::{decode_error, timestamp};

/// 一个博客的数据库句柄。内容的读写都通过它，查询总是带着它的 `blog_id`。
/// 只能从库里查出或新建，不能用随便一个 id 构造。
#[derive(Debug, Clone)]
pub struct BlogDb {
    pub(super) pool: SqlitePool,
    pub(super) id: i64,
}

/// `blogs` 的一行：首页原文、渲染结果和从首页 front matter 解析出的站点设置。
#[derive(Debug, Clone)]
pub struct Blog {
    pub home_source: String,
    pub home_html: String,
    pub title: String,
    pub description: String,
    pub lang: String,
    pub timezone: TimeZone,
    pub nav_html: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl BlogDb {
    /// 这个用户的博客。v1.0 每个用户只有一个（D66），有多个时取最早建的。
    pub async fn for_user(pool: &SqlitePool, user: UserId) -> Result<Option<Self>, sqlx::Error> {
        let id = sqlx::query_scalar!(
            r#"SELECT id AS "id!" FROM blogs WHERE user_id = ? ORDER BY id LIMIT 1"#,
            user.0
        )
        .fetch_optional(pool)
        .await?;
        Ok(id.map(|id| Self {
            pool: pool.clone(),
            id,
        }))
    }

    /// 在一个事务里新建博客和它的初始内容，半途失败时什么都不留下（第一次启动，D28）。
    pub async fn create(
        pool: &SqlitePool,
        user: UserId,
        home: &PreparedHome,
        entries: &[(Kind, &PreparedEntry)],
        now: Timestamp,
    ) -> Result<Self, WriteError> {
        let timezone = timezone_name(&home.timezone)?;
        let now_s = now.as_second();
        let mut tx = pool.begin().await?;
        let id = sqlx::query!(
            "INSERT INTO blogs (user_id, home_source, home_html, title, description, lang,
                                timezone, nav_html, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            user.0,
            home.source,
            home.html,
            home.title,
            home.description,
            home.lang,
            timezone,
            home.nav_html,
            now_s,
            now_s,
        )
        .execute(&mut *tx)
        .await?
        .last_insert_rowid();
        for (kind, entry) in entries {
            posts::insert(&mut *tx, id, *kind, entry, now).await?;
        }
        tx.commit().await?;
        Ok(Self {
            pool: pool.clone(),
            id,
        })
    }

    /// 博客的 id，只用于对外的稳定标识，比如 RSS 的 guid（D75）。
    pub fn id(&self) -> i64 {
        self.id
    }

    pub async fn blog(&self) -> Result<Blog, sqlx::Error> {
        let r = sqlx::query!(
            "SELECT home_source, home_html, title, description, lang, timezone, nav_html,
                    created_at, updated_at
             FROM blogs WHERE id = ?",
            self.id
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(Blog {
            home_source: r.home_source,
            home_html: r.home_html,
            title: r.title,
            description: r.description,
            lang: r.lang,
            timezone: date::parse_timezone(&r.timezone).map_err(decode_error)?,
            nav_html: r.nav_html,
            created_at: timestamp(r.created_at)?,
            updated_at: timestamp(r.updated_at)?,
        })
    }

    /// 保存首页（D11）。
    pub async fn update_home(
        &self,
        home: &PreparedHome,
        now: Timestamp,
    ) -> Result<(), sqlx::Error> {
        let timezone = timezone_name(&home.timezone)?;
        let now = now.as_second();
        sqlx::query!(
            "UPDATE blogs SET home_source = ?, home_html = ?, title = ?, description = ?,
                              lang = ?, timezone = ?, nav_html = ?, updated_at = ?
             WHERE id = ?",
            home.source,
            home.html,
            home.title,
            home.description,
            home.lang,
            timezone,
            home.nav_html,
            now,
            self.id,
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }
}

/// 时区存 IANA 名称；首页解析出的时区总是按名称查到的，这里没有名称说明是程序的错。
fn timezone_name(tz: &TimeZone) -> Result<&str, sqlx::Error> {
    tz.iana_name()
        .ok_or_else(|| sqlx::Error::Encode("time zone has no IANA name".into()))
}
