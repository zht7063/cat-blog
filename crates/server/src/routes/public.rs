//! 公开页面：首页、`/blog/`、`/{slug}/`、静态资源（docs/v1.0-spec.md §8.1、§8.2）。

use askama::Template;
use axum::extract::{Path, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, Uri, header};
use axum::response::{Html, IntoResponse, Response};
use cat_blog_core::date;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::i18n;
use cat_blog_core::slug::{self, Slug};
use cat_blog_core::tags::{self, Tag};
use jiff::Timestamp;
use serde::Deserialize;

use super::{AppResult, Layout, admin, no_store, not_found};
use crate::AppState;
use crate::db::Blog;
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
    render_home(&state, &blog, false).await
}

/// 按 `blog` 的首页和站点设置渲染首页。`private` 为真（预览）时加 `noindex` 和 `no-store`（D68）。
pub(super) async fn render_home(state: &AppState, blog: &Blog, private: bool) -> AppResult {
    let texts = i18n::texts(&blog.lang);
    let body = expand_posts(
        &state.blog,
        &blog.home_html,
        &blog.timezone,
        texts,
        state.clock.now(),
    )
    .await?;
    let mut layout = Layout::new(blog, &state.assets, "", "");
    layout.noindex = private;
    let page = HomePage { layout, body };
    Ok(private_if(private, Html(page.render()?).into_response()))
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

/// 其余的 GET 请求：按 `/{slug}/` 找页面或文章（§8.2）。不可见的内容只有已登录的管理员能看到
/// （D12、D68）。
pub async fn entry(State(state): State<AppState>, uri: Uri, headers: HeaderMap) -> AppResult {
    let Some(target) = resolve(uri.path()) else {
        return not_found(&state).await;
    };
    let Some(post) = state.blog.post_by_slug(&target.slug).await? else {
        return not_found(&state).await;
    };
    let visible = post.is_visible(state.clock.now());
    if !visible && admin::current_session(&state, &headers).await?.is_none() {
        return not_found(&state).await;
    }
    if !target.canonical {
        let redirect = moved_permanently(&post.slug.path(), &uri);
        return Ok(private_if(!visible, redirect));
    }
    let blog = state.blog.blog().await?;
    let view = EntryView {
        kind: post.kind,
        title: &post.title,
        description: &post.description,
        published_at: post.published_at,
        tags: &post.tags,
        html: &post.html,
    };
    render_entry(&state, &blog, view, !visible).await
}

/// 页面或文章页要显示的内容，来自库里的一行或预览时的原文。
pub(super) struct EntryView<'a> {
    pub kind: Kind,
    pub title: &'a str,
    pub description: &'a str,
    pub published_at: Option<Timestamp>,
    pub tags: &'a [Tag],
    pub html: &'a str,
}

/// 页面：标题和正文；文章：标题、日期、标签和正文（§8.1）。`private` 为真时加 `noindex` 和
/// `no-store`（D68）。
pub(super) async fn render_entry(
    state: &AppState,
    blog: &Blog,
    view: EntryView<'_>,
    private: bool,
) -> AppResult {
    let texts = i18n::texts(&blog.lang);
    let body = expand_posts(
        &state.blog,
        view.html,
        &blog.timezone,
        texts,
        state.clock.now(),
    )
    .await?;
    let (date, tags) = match view.kind {
        Kind::Page => (None, Vec::new()),
        Kind::Post => (
            view.published_at
                .map(|t| date::format_date(t, &blog.timezone)),
            view.tags
                .iter()
                .map(|t| TagLink {
                    name: t.as_str().to_owned(),
                    href: format!("/blog/?tag={}", slug::percent_encode(t.as_str())),
                })
                .collect(),
        ),
    };
    let mut layout = Layout::new(blog, &state.assets, view.title, view.description);
    layout.noindex = private;
    let page = EntryPage {
        layout,
        title: view.title.to_owned(),
        date,
        tags,
        body,
    };
    Ok(private_if(private, Html(page.render()?).into_response()))
}

fn private_if(private: bool, response: Response) -> Response {
    if private {
        no_store(response)
    } else {
        response
    }
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
