//! 后台（docs/v1.0-spec.md §9）：路由、会话中间件、CSRF（D47）和后台布局。
//! 登录与退出见 `login.rs`，列表、编辑、删除见 `edit.rs`，预览见 `preview.rs`。

mod edit;
mod login;
mod preview;

use axum::Router;
use axum::body::{Body, to_bytes};
use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Redirect, Response};
use axum::routing::{get, post};

use super::AppError;
use crate::assets::Assets;
use crate::auth::session;
use crate::db::sessions::{self, TokenHash};
use crate::{AppState, BODY_LIMIT};

/// 当前请求的管理员会话，由 [`require_admin`] 放进请求的 extensions。
#[derive(Debug, Clone)]
pub struct AdminSession {
    pub token_hash: TokenHash,
    pub csrf: String,
}

/// 后台的全部路由。除登录页外都要先登录，POST 都校验 `_csrf`。
pub fn router(state: AppState) -> Router<AppState> {
    let protected = Router::new()
        .route("/admin/", get(edit::home).post(edit::save_home))
        .route("/admin/pages/", get(edit::pages))
        .route("/admin/posts/", get(edit::posts))
        .route(
            "/admin/pages/new",
            get(edit::new_page).post(edit::create_page),
        )
        .route(
            "/admin/posts/new",
            get(edit::new_post).post(edit::create_post),
        )
        .route("/admin/edit/{id}", get(edit::edit).post(edit::update))
        .route(
            "/admin/delete/{id}",
            get(edit::confirm_delete).post(edit::delete),
        )
        .route("/admin/preview", post(preview::preview))
        .route("/admin/logout", post(login::logout))
        .route("/admin/{*rest}", get(unknown))
        .route_layer(middleware::from_fn_with_state(state, require_admin));
    Router::new()
        .route("/admin/login", get(login::form).post(login::submit))
        .merge(protected)
}

/// 请求带着这个博客所有者的有效会话时返回它（D18、D49），否则返回 `None`。
pub async fn current_session(
    state: &AppState,
    headers: &HeaderMap,
) -> Result<Option<AdminSession>, sqlx::Error> {
    let Some(token) = session::token_from(headers) else {
        return Ok(None);
    };
    let token_hash = session::hash_token(token);
    let found = sessions::find(&state.pool, &token_hash, state.clock.now()).await?;
    Ok(found
        .filter(|s| s.user_id == state.blog.owner())
        .map(|s| AdminSession {
            token_hash,
            csrf: s.csrf_token,
        }))
}

/// 没登录时 303 到登录页（§9.1）；POST 的表单里没有 `_csrf` 或不匹配时返回 403（D47）。
async fn require_admin(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let Some(admin) = current_session(&state, request.headers()).await? else {
        return Ok(Redirect::to("/admin/login").into_response());
    };
    if request.method() == Method::POST {
        let (parts, body) = request.into_parts();
        let Ok(bytes) = to_bytes(body, BODY_LIMIT).await else {
            return Ok(StatusCode::PAYLOAD_TOO_LARGE.into_response());
        };
        let submitted = form_urlencoded::parse(&bytes)
            .find(|(name, _)| name == "_csrf")
            .map(|(_, value)| value);
        if !submitted.is_some_and(|v| session::csrf_matches(&admin.csrf, &v)) {
            return Ok(forbidden(
                "表单缺少 CSRF token 或已失效，请刷新页面后重新提交",
            ));
        }
        request = Request::from_parts(parts, Body::from(bytes));
    }
    request.extensions_mut().insert(admin);
    Ok(next.run(request).await)
}

/// `/admin/` 下没有的地址。放在登录检查之后，未登录时同样跳到登录页（§9.1）。
async fn unknown(State(state): State<AppState>) -> super::AppResult {
    super::not_found(&state).await
}

fn forbidden(message: &'static str) -> Response {
    (StatusCode::FORBIDDEN, message).into_response()
}

/// 后台页面共用的布局（`templates/admin/base.html`）。后台只有中文（D35）。
pub struct AdminLayout<'a> {
    pub title: String,
    /// 已登录时为当前会话的 CSRF token，用于导航里的“退出”表单；登录页为空。
    pub csrf: Option<String>,
    pub css: &'a str,
    pub theme_js: &'a str,
    pub favicon: &'a str,
    /// 编辑页才加载的 `editor.js`。
    pub editor_js: Option<&'a str>,
}

impl<'a> AdminLayout<'a> {
    pub fn new(assets: &'a Assets, title: impl Into<String>, csrf: Option<&str>) -> Self {
        Self {
            title: title.into(),
            csrf: csrf.map(str::to_owned),
            css: &assets.css.url,
            theme_js: &assets.js.url,
            favicon: &assets.favicon.url,
            editor_js: None,
        }
    }
}
