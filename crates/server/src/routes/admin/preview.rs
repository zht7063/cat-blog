//! 预览（D56、D68）：把编辑框里未保存的原文渲染成公开页面的样子。

use askama::Template;
use axum::Form;
use axum::extract::{Extension, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse};
use cat_blog_core::save;
use serde::Deserialize;

use super::edit::Target;
use super::{AdminLayout, AdminSession};
use crate::AppState;
use crate::db::{Blog, Post, PostId};
use crate::routes::public::{EntryView, render_entry, render_home};
use crate::routes::{AppResult, no_store};

#[derive(Deserialize)]
pub struct PreviewForm {
    target: Target,
    id: Option<i64>,
    source: String,
}

#[derive(Template)]
#[template(path = "admin/preview_error.html")]
struct PreviewError<'a> {
    admin: AdminLayout<'a>,
    error: String,
}

/// 把编辑框里未保存的原文渲染成公开页面的样子（D56）：不写库、不写回 slug，
/// 加 `noindex` 和 `no-store`（D68）。原文有错时显示错误，返回 422。
pub async fn preview(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Form(form): Form<PreviewForm>,
) -> AppResult {
    let blog = state.blog.blog().await?;
    let result = match form.target.kind() {
        None => match save::prepare_home(&state.renderer, &form.source) {
            Ok(home) => {
                let preview = Blog {
                    home_html: home.html,
                    title: home.title,
                    description: home.description,
                    lang: home.lang,
                    timezone: home.timezone,
                    nav_html: home.nav_html,
                    ..blog
                };
                return render_home(&state, &preview, true).await;
            }
            Err(error) => error.to_string(),
        },
        Some(kind) => {
            let stored = match form.id {
                Some(id) => state.blog.post(PostId(id)).await?,
                None => None,
            };
            let stored = stored.as_ref().filter(|p| p.kind == kind);
            let prepared = save::prepare_entry(
                &state.renderer,
                &form.source,
                kind,
                &blog.timezone,
                stored.map(Post::stored),
                state.clock.now(),
            );
            match prepared {
                Ok(entry) => {
                    let view = EntryView {
                        kind,
                        title: &entry.title,
                        description: &entry.description,
                        published_at: entry.published_at,
                        tags: &entry.tags,
                        html: &entry.html,
                    };
                    return render_entry(&state, &blog, view, true).await;
                }
                Err(error) => error.to_string(),
            }
        }
    };
    let page = PreviewError {
        admin: AdminLayout::new(&state.assets, "预览失败", Some(&admin.csrf)),
        error: result,
    };
    Ok(no_store(
        (StatusCode::UNPROCESSABLE_ENTITY, Html(page.render()?)).into_response(),
    ))
}
