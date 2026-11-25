//! 请求时的渲染：展开 `{{ posts }}` 占位（D33、D55），以及各页面共用的文章列表。

use askama::Template;
use cat_blog_core::date;
use cat_blog_core::directive::{self, PostsDirective};
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::i18n::Texts;
use jiff::Timestamp;
use jiff::tz::TimeZone;

use crate::db::{BlogDb, PostSummary};
use crate::routes::AppError;

/// 文章列表的一行：日期和标题（D37）。
pub struct ListItem {
    pub date: String,
    pub title: String,
    pub href: String,
}

#[derive(Template)]
#[template(path = "post_list.html")]
struct PostList<'a> {
    items: &'a [ListItem],
    empty: &'a str,
}

/// 可见文章的列表行，调用方已按 `published_at` 倒序排好。
pub fn list_items<'a>(
    posts: impl IntoIterator<Item = &'a PostSummary>,
    tz: &TimeZone,
) -> Vec<ListItem> {
    posts
        .into_iter()
        .filter_map(|p| {
            Some(ListItem {
                date: date::format_date(p.published_at?, tz),
                title: p.title.clone(),
                href: p.slug.path(),
            })
        })
        .collect()
}

/// 文章列表的 HTML，`/blog/` 和 `{{ posts }}` 共用。
pub fn post_list(items: &[ListItem], texts: &Texts) -> Result<String, askama::Error> {
    PostList {
        items,
        empty: texts.no_posts,
    }
    .render()
}

/// 把正文里的 `{{ posts }}` 占位换成可见文章的列表。没有占位时不查库。
pub async fn expand_posts(
    blog: &BlogDb,
    html: &str,
    tz: &TimeZone,
    texts: &Texts,
    now: Timestamp,
) -> Result<String, AppError> {
    if !directive::has_markers(html) {
        return Ok(html.to_owned());
    }
    let posts: Vec<PostSummary> = blog
        .summaries(Kind::Post, None)
        .await?
        .into_iter()
        .filter(|p| p.is_visible(now))
        .collect();
    let mut error = None;
    let expanded = directive::expand_markers(html, |d: &PostsDirective| {
        let selected = posts
            .iter()
            .filter(|p| d.tag.as_ref().is_none_or(|t| p.tags.contains(t)))
            .take(d.limit.map_or(usize::MAX, |n| n.get() as usize));
        match post_list(&list_items(selected, tz), texts) {
            Ok(html) => html,
            Err(e) => {
                error.get_or_insert(e);
                String::new()
            }
        }
    });
    match error {
        Some(e) => Err(e.into()),
        None => Ok(expanded),
    }
}
