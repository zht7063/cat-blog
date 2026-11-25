//! `posts` 表：页面和文章（D9），都通过 [`BlogDb`] 读写。

use std::fmt;

use cat_blog_core::frontmatter::Kind;
use cat_blog_core::save::{PreparedEntry, Stored};
use cat_blog_core::slug::Slug;
use cat_blog_core::tags::Tag;
use cat_blog_core::visibility;
use jiff::Timestamp;
use sqlx::SqliteExecutor;

use super::blogs::BlogDb;
use super::{decode_error, timestamp};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PostId(pub i64);

/// `posts` 的一行。
#[derive(Debug, Clone)]
pub struct Post {
    pub id: PostId,
    pub kind: Kind,
    pub slug: Slug,
    pub source: String,
    pub html: String,
    pub title: String,
    pub description: String,
    pub tags: Vec<Tag>,
    pub draft: bool,
    pub published_at: Option<Timestamp>,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
}

impl Post {
    /// 公开站点能否看到这篇内容（§5.4）。
    pub fn is_visible(&self, now: Timestamp) -> bool {
        visibility::is_visible(self.draft, self.published_at, now)
    }

    /// 再次保存时要沿用的 slug 和发布时间（§5.3、§5.4）。
    pub fn stored(&self) -> Stored<'_> {
        Stored {
            slug: &self.slug,
            published_at: self.published_at,
        }
    }
}

/// 列表、sitemap 和 `{{ posts }}` 用的一行，不带原文和正文。
#[derive(Debug, Clone)]
pub struct PostSummary {
    pub id: PostId,
    pub slug: Slug,
    pub title: String,
    pub tags: Vec<Tag>,
    pub draft: bool,
    pub published_at: Option<Timestamp>,
    pub updated_at: Timestamp,
}

impl PostSummary {
    /// 公开站点能否看到这篇内容（§5.4）。
    pub fn is_visible(&self, now: Timestamp) -> bool {
        visibility::is_visible(self.draft, self.published_at, now)
    }
}

/// 写入页面或文章失败。
#[derive(Debug)]
pub enum WriteError {
    /// 同一博客里已经有内容用了这个 slug（D10）。
    SlugTaken,
    Db(sqlx::Error),
}

impl From<sqlx::Error> for WriteError {
    fn from(error: sqlx::Error) -> Self {
        match &error {
            sqlx::Error::Database(e)
                if e.is_unique_violation() && e.message().contains("posts.blog_id, posts.slug") =>
            {
                Self::SlugTaken
            }
            _ => Self::Db(error),
        }
    }
}

impl fmt::Display for WriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SlugTaken => f.write_str("slug is already used by another page or post"),
            Self::Db(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for WriteError {}

struct PostRow {
    id: i64,
    kind: String,
    slug: String,
    source: String,
    html: String,
    title: String,
    description: String,
    tags: String,
    draft: i64,
    published_at: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl TryFrom<PostRow> for Post {
    type Error = sqlx::Error;

    fn try_from(r: PostRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: PostId(r.id),
            kind: [Kind::Page, Kind::Post]
                .into_iter()
                .find(|k| kind_str(*k) == r.kind)
                .ok_or_else(|| decode_error(format!("unknown post kind {:?}", r.kind)))?,
            slug: Slug::parse(&r.slug).map_err(decode_error)?,
            source: r.source,
            html: r.html,
            title: r.title,
            description: r.description,
            tags: parse_tags(&r.tags)?,
            draft: r.draft != 0,
            published_at: r.published_at.map(timestamp).transpose()?,
            created_at: timestamp(r.created_at)?,
            updated_at: timestamp(r.updated_at)?,
        })
    }
}

struct SummaryRow {
    id: i64,
    slug: String,
    title: String,
    tags: String,
    draft: i64,
    published_at: Option<i64>,
    updated_at: i64,
}

impl TryFrom<SummaryRow> for PostSummary {
    type Error = sqlx::Error;

    fn try_from(r: SummaryRow) -> Result<Self, sqlx::Error> {
        Ok(Self {
            id: PostId(r.id),
            slug: Slug::parse(&r.slug).map_err(decode_error)?,
            title: r.title,
            tags: parse_tags(&r.tags)?,
            draft: r.draft != 0,
            published_at: r.published_at.map(timestamp).transpose()?,
            updated_at: timestamp(r.updated_at)?,
        })
    }
}

fn parse_tags(json: &str) -> Result<Vec<Tag>, sqlx::Error> {
    let tags: Vec<String> = serde_json::from_str(json).map_err(decode_error)?;
    tags.iter()
        .map(|t| Tag::parse(t))
        .collect::<Result<_, _>>()
        .map_err(decode_error)
}

/// `posts.kind` 列的取值（docs/v1.0-spec.md §6）。
fn kind_str(kind: Kind) -> &'static str {
    match kind {
        Kind::Page => "page",
        Kind::Post => "post",
    }
}

fn tags_json(tags: &[Tag]) -> Result<String, sqlx::Error> {
    let tags: Vec<&str> = tags.iter().map(Tag::as_str).collect();
    serde_json::to_string(&tags).map_err(|e| sqlx::Error::Encode(e.into()))
}

pub(super) async fn insert<'e>(
    executor: impl SqliteExecutor<'e>,
    blog_id: i64,
    kind: Kind,
    entry: &PreparedEntry,
    now: Timestamp,
) -> Result<PostId, WriteError> {
    let kind = kind_str(kind);
    let slug = entry.slug.as_str();
    let tags = tags_json(&entry.tags)?;
    let published_at = entry.published_at.map(Timestamp::as_second);
    let now = now.as_second();
    let id = sqlx::query!(
        "INSERT INTO posts (blog_id, kind, slug, source, html, title, description, tags, draft,
                            published_at, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        blog_id,
        kind,
        slug,
        entry.source,
        entry.html,
        entry.title,
        entry.description,
        tags,
        entry.draft,
        published_at,
        now,
        now,
    )
    .execute(executor)
    .await?
    .last_insert_rowid();
    Ok(PostId(id))
}

impl BlogDb {
    /// 第一次保存页面或文章。
    pub async fn insert_post(
        &self,
        kind: Kind,
        entry: &PreparedEntry,
        now: Timestamp,
    ) -> Result<PostId, WriteError> {
        insert(&self.pool, self.id, kind, entry, now).await
    }

    pub async fn post(&self, id: PostId) -> Result<Option<Post>, sqlx::Error> {
        sqlx::query_as!(
            PostRow,
            r#"SELECT id AS "id!", kind, slug, source, html, title, description, tags, draft,
                      published_at, created_at, updated_at
               FROM posts WHERE blog_id = ? AND id = ?"#,
            self.id,
            id.0,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(Post::try_from)
        .transpose()
    }

    pub async fn post_by_slug(&self, slug: &Slug) -> Result<Option<Post>, sqlx::Error> {
        let slug = slug.as_str();
        sqlx::query_as!(
            PostRow,
            r#"SELECT id AS "id!", kind, slug, source, html, title, description, tags, draft,
                      published_at, created_at, updated_at
               FROM posts WHERE blog_id = ? AND slug = ?"#,
            self.id,
            slug,
        )
        .fetch_optional(&self.pool)
        .await?
        .map(Post::try_from)
        .transpose()
    }

    /// 后台列表（docs/v1.0-spec.md §9.1）：未发布的草稿在最前，其余按 `published_at` 倒序。
    pub async fn list(&self, kind: Kind) -> Result<Vec<Post>, sqlx::Error> {
        let kind = kind_str(kind);
        sqlx::query_as!(
            PostRow,
            r#"SELECT id AS "id!", kind, slug, source, html, title, description, tags, draft,
                      published_at, created_at, updated_at
               FROM posts WHERE blog_id = ? AND kind = ?
               ORDER BY published_at IS NULL DESC, published_at DESC, id DESC"#,
            self.id,
            kind,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(Post::try_from)
        .collect()
    }

    /// 某一类内容的摘要，按 `published_at` 倒序，没有发布时间的排在最后。给了 `tag` 时只要带这个
    /// 标签的（D46）。不按可见性过滤，调用方用 [`PostSummary::is_visible`] 判断。
    pub async fn summaries(
        &self,
        kind: Kind,
        tag: Option<&Tag>,
    ) -> Result<Vec<PostSummary>, sqlx::Error> {
        let kind = kind_str(kind);
        let tag = tag.map(Tag::as_str);
        sqlx::query_as!(
            SummaryRow,
            r#"SELECT id AS "id!", slug, title, tags, draft, published_at, updated_at
               FROM posts
               WHERE blog_id = ? AND kind = ?
                 AND (? IS NULL OR EXISTS (SELECT 1 FROM json_each(posts.tags) WHERE value = ?))
               ORDER BY published_at IS NULL, published_at DESC, id DESC"#,
            self.id,
            kind,
            tag,
            tag,
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(PostSummary::try_from)
        .collect()
    }

    /// 保存已有的页面或文章。这篇内容不在这个博客里时返回 `false`。
    pub async fn update_post(
        &self,
        id: PostId,
        entry: &PreparedEntry,
        now: Timestamp,
    ) -> Result<bool, WriteError> {
        let slug = entry.slug.as_str();
        let tags = tags_json(&entry.tags)?;
        let published_at = entry.published_at.map(Timestamp::as_second);
        let now = now.as_second();
        let result = sqlx::query!(
            "UPDATE posts SET slug = ?, source = ?, html = ?, title = ?, description = ?, tags = ?,
                              draft = ?, published_at = ?, updated_at = ?
             WHERE blog_id = ? AND id = ?",
            slug,
            entry.source,
            entry.html,
            entry.title,
            entry.description,
            tags,
            entry.draft,
            published_at,
            now,
            self.id,
            id.0,
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// 删除页面或文章（D76 确认之后）。这篇内容不在这个博客里时返回 `false`。
    pub async fn delete_post(&self, id: PostId) -> Result<bool, sqlx::Error> {
        let result = sqlx::query!(
            "DELETE FROM posts WHERE blog_id = ? AND id = ?",
            self.id,
            id.0
        )
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }
}
