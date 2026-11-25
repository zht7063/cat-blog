//! `/feed.xml`、`/sitemap.xml`、`/robots.txt`（D20，docs/v1.0-spec.md §8.1、§8.3）。

use askama::Template;
use axum::extract::State;
use cat_blog_core::directive;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::sanitize::Sanitizer;
use jiff::Timestamp;

use super::{AppError, AppResult, typed};
use crate::AppState;

/// RSS 里最多放几篇文章（D20）。
const FEED_LEN: usize = 20;

struct FeedItem {
    title: String,
    link: String,
    guid: String,
    pub_date: String,
    html: String,
}

#[derive(Template)]
#[template(path = "feed.xml")]
struct Feed {
    title: String,
    link: String,
    self_link: String,
    description: String,
    lang: String,
    items: Vec<FeedItem>,
}

/// RSS 2.0：最近 20 篇可见文章的全文，地址都是绝对地址（D57），guid 不随 slug 变（D75）。
pub async fn feed(State(state): State<AppState>) -> AppResult {
    let blog = state.blog.blog().await?;
    let now = state.clock.now();
    let base = &state.base_url;
    let mut items = Vec::new();
    for post in state
        .blog
        .list(Kind::Post)
        .await?
        .into_iter()
        .filter(|p| p.is_visible(now))
        .take(FEED_LEN)
    {
        let link = format!("{base}{}", post.slug.path());
        let sanitizer =
            Sanitizer::with_base(&link).ok_or(AppError::Internal("post URL is not absolute"))?;
        let html = sanitizer.clean(&directive::expand_markers(&post.html, |_| String::new()));
        items.push(FeedItem {
            guid: format!("urn:cat-blog:{}:{}", state.blog.id(), post.id.0),
            pub_date: rfc2822(post.published_at.unwrap_or(post.updated_at)),
            title: post.title,
            link,
            html,
        });
    }
    let feed = Feed {
        title: blog.title,
        link: format!("{base}/"),
        self_link: format!("{base}/feed.xml"),
        description: blog.description,
        lang: blog.lang,
        items,
    };
    Ok(typed("application/rss+xml; charset=utf-8", feed.render()?))
}

fn rfc2822(t: Timestamp) -> String {
    t.strftime("%a, %d %b %Y %H:%M:%S +0000").to_string()
}

struct SitemapUrl {
    loc: String,
    lastmod: String,
}

#[derive(Template)]
#[template(path = "sitemap.xml")]
struct Sitemap {
    urls: Vec<SitemapUrl>,
}

/// 首页、`/blog/`、所有可见的页面和文章；`lastmod` 取 `updated_at`（§8.1）。
pub async fn sitemap(State(state): State<AppState>) -> AppResult {
    let blog = state.blog.blog().await?;
    let now = state.clock.now();
    let base = &state.base_url;
    let pages = state.blog.summaries(Kind::Page, None).await?;
    let posts = state.blog.summaries(Kind::Post, None).await?;
    let pages: Vec<_> = pages.iter().filter(|p| p.is_visible(now)).collect();
    let posts: Vec<_> = posts.iter().filter(|p| p.is_visible(now)).collect();
    let blog_updated = posts
        .iter()
        .map(|p| p.updated_at)
        .max()
        .unwrap_or(blog.updated_at);

    let mut urls = vec![
        SitemapUrl {
            loc: format!("{base}/"),
            lastmod: blog.updated_at.to_string(),
        },
        SitemapUrl {
            loc: format!("{base}/blog/"),
            lastmod: blog_updated.to_string(),
        },
    ];
    urls.extend(pages.iter().chain(&posts).map(|p| SitemapUrl {
        loc: format!("{base}{}", p.slug.path()),
        lastmod: p.updated_at.to_string(),
    }));
    Ok(typed(
        "application/xml; charset=utf-8",
        Sitemap { urls }.render()?,
    ))
}

pub async fn robots(State(state): State<AppState>) -> AppResult {
    Ok(typed(
        "text/plain; charset=utf-8",
        format!(
            "User-agent: *\nDisallow: /admin/\n\nSitemap: {}/sitemap.xml\n",
            state.base_url
        ),
    ))
}
