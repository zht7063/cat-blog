//! 公开页面：首页、`/blog/`、`/{slug}/`、静态资源（docs/v1.0-spec.md §8.1、§8.2）。

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use cat_blog_core::date;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::i18n;
use cat_blog_core::slug::{self, Slug};
use cat_blog_core::tags::{self, Tag};
use serde::Deserialize;

use super::{AppResult, Layout, not_found};
use crate::AppState;
use crate::render::{expand_posts, list_items, post_list};

#[derive(Template)]
#[template(path = "home.html")]
struct HomePage<'a> {
    layout: Layout<'a>,
    body: String,
}

#[derive(Template)]
#[template(path = "blog.html")]
struct BlogPage<'a> {
    layout: Layout<'a>,
    heading: String,
    list: String,
}

pub struct TagLink {
    pub name: String,
    pub href: String,
}

#[derive(Template)]
#[template(path = "entry.html")]
struct EntryPage<'a> {
    layout: Layout<'a>,
    title: String,
    date: Option<String>,
    tags: Vec<TagLink>,
    body: String,
}

/// 首页正文，不另显示 h1（§8.1）。
pub async fn home(State(state): State<AppState>) -> AppResult {
    let blog = state.blog.blog().await?;
    let texts = i18n::texts(&blog.lang);
    let body = expand_posts(
        &state.blog,
        &blog.home_html,
        &blog.timezone,
        texts,
        state.clock.now(),
    )
    .await?;
    let page = HomePage {
        layout: Layout::new(&blog, &state.assets, "", ""),
        body,
    };
    Ok(Html(page.render()?).into_response())
}

#[derive(Deserialize)]
pub struct BlogQuery {
    tag: Option<String>,
}

/// `/blog/` 和 `/blog/?tag=x`：可见文章按发布时间倒序，不分页（D37）。标签按 D46 规范化后匹配，
/// 没有这个标签（或标签写法不合法）时返回空列表。
pub async fn blog(State(state): State<AppState>, Query(query): Query<BlogQuery>) -> AppResult {
    let blog = state.blog.blog().await?;
    let texts = i18n::texts(&blog.lang);
    let now = state.clock.now();
    let (heading, posts) = match query.tag.as_deref() {
        None => (
            texts.blog.to_owned(),
            state.blog.summaries(Kind::Post, None).await?,
        ),
        Some(raw) => {
            let heading = format!("{}{}", texts.tagged, tags::normalize(raw));
            let posts = match Tag::parse(raw) {
                Ok(tag) => state.blog.summaries(Kind::Post, Some(&tag)).await?,
                Err(_) => Vec::new(),
            };
            (heading, posts)
        }
    };
    let items = list_items(posts.iter().filter(|p| p.is_visible(now)), &blog.timezone);
    let page = BlogPage {
        layout: Layout::new(&blog, &state.assets, &heading, ""),
        list: post_list(&items, texts)?,
        heading,
    };
    Ok(Html(page.render()?).into_response())
}

/// 其余的 GET 请求：按 `/{slug}/` 找页面或文章（§8.2）。
pub async fn entry(State(state): State<AppState>, uri: Uri) -> AppResult {
    let Some(target) = resolve(uri.path()) else {
        return not_found(&state).await;
    };
    let now = state.clock.now();
    let post = match state.blog.post_by_slug(&target.slug).await? {
        Some(post) if post.is_visible(now) => post,
        _ => return not_found(&state).await,
    };
    if !target.canonical {
        return Ok(moved_permanently(&post.slug.path(), &uri));
    }

    let blog = state.blog.blog().await?;
    let texts = i18n::texts(&blog.lang);
    let body = expand_posts(&state.blog, &post.html, &blog.timezone, texts, now).await?;
    let (date, tags) = match post.kind {
        Kind::Page => (None, Vec::new()),
        Kind::Post => (
            post.published_at
                .map(|t| date::format_date(t, &blog.timezone)),
            post.tags
                .iter()
                .map(|t| TagLink {
                    name: t.as_str().to_owned(),
                    href: format!("/blog/?tag={}", slug::percent_encode(t.as_str())),
                })
                .collect(),
        ),
    };
    let page = EntryPage {
        layout: Layout::new(&blog, &state.assets, &post.title, &post.description),
        title: post.title,
        date,
        tags,
        body,
    };
    Ok(Html(page.render()?).into_response())
}

/// 请求路径对应的 slug，以及请求的写法是否已经是规范地址。
struct Target {
    slug: Slug,
    canonical: bool,
}

/// 百分号解码后做 NFC 和小写规范化（D32）。结果和解码后的原文不同（大小写、Unicode 规范形式），
/// 或缺少尾斜杠时不是规范地址。规范化之外还要改写的（比如空格、标点）不算同一个 slug。
fn resolve(path: &str) -> Option<Target> {
    let raw = path.strip_prefix('/')?;
    let (segment, has_slash) = match raw.strip_suffix('/') {
        Some(s) => (s, true),
        None => (raw, false),
    };
    if segment.is_empty() || segment.contains('/') {
        return None;
    }
    let decoded = slug::percent_decode(segment)?;
    let key = slug::lookup_key(&decoded);
    let slug = Slug::parse(&key).ok().filter(|s| s.as_str() == key)?;
    Some(Target {
        canonical: has_slash && decoded == key,
        slug,
    })
}

/// `/blog` 和 `/admin` 跳到带尾斜杠的地址（§8.2）。
pub async fn add_slash(uri: Uri) -> Response {
    moved_permanently(&format!("{}/", uri.path()), &uri)
}

/// 301 跳转（D58），保留原请求的查询参数。
fn moved_permanently(path: &str, uri: &Uri) -> Response {
    let location = match uri.query() {
        Some(query) => format!("{path}?{query}"),
        None => path.to_owned(),
    };
    (
        StatusCode::MOVED_PERMANENTLY,
        [(header::LOCATION, location)],
    )
        .into_response()
}

/// 嵌入的静态资源，地址带内容哈希，所以可以缓存一年。
pub async fn asset(State(state): State<AppState>, Path(name): Path<String>) -> AppResult {
    match state.assets.get(&name) {
        Some(asset) => Ok(cached(asset.content_type, asset.body.clone())),
        None => not_found(&state).await,
    }
}

/// `/favicon.ico` 返回 SVG（D61）。地址不带哈希，只缓存一天。
pub async fn favicon(State(state): State<AppState>) -> Response {
    let mut response = cached(
        state.assets.favicon.content_type,
        state.assets.favicon.body.clone(),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=86400"),
    );
    response
}

fn cached(content_type: &'static str, body: String) -> Response {
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "public, max-age=31536000, immutable"),
        ],
        body,
    )
        .into_response()
}
