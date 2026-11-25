//! 保存时的处理（docs/v1.0-spec.md §5、§7.1）：解析 front matter、确定并写回 slug、
//! 计算发布时间、渲染正文。结果直接对应数据库的列，不读数据库，也不取当前时间。

use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::frontmatter::{self, Kind, Source};
use crate::markdown::Renderer;
use crate::slug::Slug;
use crate::tags::Tag;
use crate::{ContentError, date};

/// 保存首页后要写进 `blogs` 的内容。
#[derive(Debug, Clone)]
pub struct PreparedHome {
    /// 换行统一为 LF 的原文（D70）。
    pub source: String,
    pub html: String,
    pub title: String,
    pub description: String,
    pub lang: String,
    pub timezone: TimeZone,
    pub nav_html: String,
}

/// 库里已有的这篇内容的状态，第一次保存时为空。
#[derive(Debug, Clone, Copy)]
pub struct Stored<'a> {
    pub slug: &'a Slug,
    pub published_at: Option<Timestamp>,
}

/// 保存页面或文章后要写进 `posts` 的内容。
#[derive(Debug, Clone)]
pub struct PreparedEntry {
    /// 换行统一为 LF、写回了 slug 的原文（D39、D70）。
    pub source: String,
    pub html: String,
    pub title: String,
    pub description: String,
    pub slug: Slug,
    pub tags: Vec<Tag>,
    pub draft: bool,
    pub published_at: Option<Timestamp>,
}

/// 处理首页原文。
pub fn prepare_home(renderer: &Renderer, raw: &str) -> Result<PreparedHome, ContentError> {
    let src = Source::new(raw);
    let home = frontmatter::parse_home(&src)?;
    let html = renderer.render(home.body)?;
    let nav_html = renderer.render_nav(&home)?;
    let (title, description, lang, timezone) =
        (home.title, home.description, home.lang, home.timezone);
    Ok(PreparedHome {
        source: src.into_string(),
        html,
        title,
        description,
        lang,
        timezone,
        nav_html,
    })
}

/// 处理页面或文章的原文。`tz` 是站点时区，`now` 用于第一次发布（§5.4）。
/// 和同一博客其他内容的 slug 冲突由写库时的唯一约束检查。
pub fn prepare_entry(
    renderer: &Renderer,
    raw: &str,
    kind: Kind,
    tz: &TimeZone,
    stored: Option<Stored<'_>>,
    now: Timestamp,
) -> Result<PreparedEntry, ContentError> {
    let src = Source::new(raw);
    let entry = frontmatter::parse_entry(&src, kind, tz)?;
    let slug = entry.resolve_slug(stored.map(|s| s.slug))?;
    let html = renderer.render(entry.body)?;
    let published_at = date::published_at(
        entry.date,
        stored.and_then(|s| s.published_at),
        entry.draft,
        now,
    );
    let source = frontmatter::write_back_slug(&src, &slug)?.into_string();
    Ok(PreparedEntry {
        source,
        html,
        title: entry.title,
        description: entry.description,
        slug,
        tags: entry.tags,
        draft: entry.draft,
        published_at,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;

    static RENDERER: LazyLock<Renderer> = LazyLock::new(Renderer::new);

    fn sg() -> TimeZone {
        date::parse_timezone("Asia/Singapore").unwrap()
    }

    #[test]
    fn renaming_after_first_save_keeps_slug() {
        let now: Timestamp = "2026-11-24T00:00:00Z".parse().unwrap();
        let first = prepare_entry(
            &RENDERER,
            "---\r\ntitle: 第一篇\r\n---\r\n正文\r\n",
            Kind::Post,
            &sg(),
            None,
            now,
        )
        .unwrap();
        assert_eq!(first.slug.as_str(), "第一篇");
        assert_eq!(
            first.source,
            "---\ntitle: 第一篇\nslug: \"第一篇\"\n---\n正文\n"
        );
        assert_eq!(first.published_at, Some(now));

        let renamed = first.source.replace("title: 第一篇", "title: 改过的标题");
        let later: Timestamp = "2026-11-25T00:00:00Z".parse().unwrap();
        let second = prepare_entry(
            &RENDERER,
            &renamed,
            Kind::Post,
            &sg(),
            Some(Stored {
                slug: &first.slug,
                published_at: first.published_at,
            }),
            later,
        )
        .unwrap();
        assert_eq!(second.slug.as_str(), "第一篇");
        assert_eq!(second.source, renamed);
        assert_eq!(second.published_at, Some(now));
    }

    #[test]
    fn home_fields_and_nav_are_rendered() {
        let home = prepare_home(
            &RENDERER,
            "---\ntitle: 猫\ntimezone: America/New_York\nnav: \"[关于](/about/)\"\n---\n{{ posts limit:3 }}\n",
        )
        .unwrap();
        assert_eq!(home.title, "猫");
        assert_eq!(home.timezone.iana_name(), Some("America/New_York"));
        assert_eq!(
            home.nav_html,
            r#"<a href="/about/" rel="noopener noreferrer">关于</a>"#
        );
        assert_eq!(home.html.trim(), "<!--cat-blog:posts limit=3-->");
    }
}
