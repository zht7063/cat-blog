//! 路由处理函数共用的类型：错误、页面布局和 404 页（D61）。

pub mod admin;
pub mod feed;
pub mod public;

use askama::Template;
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{Html, IntoResponse, Response};
use cat_blog_core::i18n::{self, Texts};

use crate::AppState;
use crate::assets::Assets;
use crate::db::Blog;

/// 处理请求时的内部错误，记日志后返回 500，不把细节给访客。
#[derive(Debug)]
pub enum AppError {
    Db(sqlx::Error),
    Template(askama::Error),
    Internal(&'static str),
    Other(Box<dyn std::error::Error + Send + Sync>),
}

impl From<sqlx::Error> for AppError {
    fn from(error: sqlx::Error) -> Self {
        Self::Db(error)
    }
}

impl From<askama::Error> for AppError {
    fn from(error: askama::Error) -> Self {
        Self::Template(error)
    }
}

impl From<crate::auth::password::PasswordError> for AppError {
    fn from(error: crate::auth::password::PasswordError) -> Self {
        Self::Other(error.into())
    }
}

impl From<rand::rngs::SysError> for AppError {
    fn from(error: rand::rngs::SysError) -> Self {
        Self::Other(error.into())
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        match &self {
            Self::Db(error) => tracing::error!(%error, "database query failed"),
            Self::Template(error) => tracing::error!(%error, "template rendering failed"),
            Self::Internal(error) => tracing::error!(error, "request failed"),
            Self::Other(error) => tracing::error!(%error, "request failed"),
        }
        (StatusCode::INTERNAL_SERVER_ERROR, "internal server error").into_response()
    }
}

pub type AppResult = Result<Response, AppError>;

/// `base.html` 用到的站点信息。
pub struct Layout<'a> {
    pub lang: String,
    pub site_title: String,
    pub nav_html: String,
    /// `<title>` 的内容。
    pub title: String,
    pub description: String,
    pub texts: &'static Texts,
    pub assets: AssetUrls<'a>,
    /// 管理员查看不可见的内容或预览时为真：加 `noindex`（D68）。
    pub noindex: bool,
}

pub struct AssetUrls<'a> {
    pub css: &'a str,
    pub js: &'a str,
    pub favicon: &'a str,
}

impl<'a> Layout<'a> {
    /// `title` 为空时 `<title>` 只用站点标题，否则是“页面标题 - 站点标题”。
    pub fn new(blog: &Blog, assets: &'a Assets, title: &str, description: &str) -> Self {
        Self {
            lang: blog.lang.clone(),
            site_title: blog.title.clone(),
            nav_html: blog.nav_html.clone(),
            title: if title.is_empty() {
                blog.title.clone()
            } else {
                format!("{title} - {}", blog.title)
            },
            description: if description.is_empty() {
                blog.description.clone()
            } else {
                description.to_owned()
            },
            texts: i18n::texts(&blog.lang),
            assets: AssetUrls {
                css: &assets.css.url,
                js: &assets.js.url,
                favicon: &assets.favicon.url,
            },
            noindex: false,
        }
    }
}

/// 只给管理员看的页面（草稿、未到时间的内容、预览）不能被缓存（D68）。
pub fn no_store(mut response: Response) -> Response {
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}

#[derive(Template)]
#[template(path = "not_found.html")]
struct NotFound<'a> {
    layout: Layout<'a>,
}

/// 统一样式的 404 页，文字跟随首页 `lang`（D61）。
pub async fn not_found(state: &AppState) -> AppResult {
    let blog = state.blog.blog().await?;
    let texts = i18n::texts(&blog.lang);
    let page = NotFound {
        layout: Layout::new(&blog, &state.assets, texts.not_found_title, ""),
    };
    Ok((StatusCode::NOT_FOUND, Html(page.render()?)).into_response())
}

/// 带 `Content-Type` 的文本响应，用于 XML 和纯文本。
pub fn typed(content_type: &'static str, body: String) -> Response {
    ([(header::CONTENT_TYPE, content_type)], body).into_response()
}
