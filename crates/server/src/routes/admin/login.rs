//! 登录与退出（D15、D16、D47、D48，docs/v1.0-spec.md §9.3）。

use std::net::SocketAddr;

use askama::Template;
use axum::Form;
use axum::extract::rejection::FormRejection;
use axum::extract::{ConnectInfo, Extension, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::response::{Html, IntoResponse, Redirect, Response};
use serde::Deserialize;

use super::{AdminLayout, AdminSession, current_session, forbidden};
use crate::AppState;
use crate::auth::client_ip::client_ip;
use crate::auth::{password, session};
use crate::db::sessions::{self, NewSession};
use crate::db::users;
use crate::routes::{AppError, AppResult};

#[derive(Template)]
#[template(path = "admin/login.html")]
struct LoginPage<'a> {
    admin: AdminLayout<'a>,
    username: String,
    error: Option<&'static str>,
}

fn login_page(
    state: &AppState,
    status: StatusCode,
    username: String,
    error: Option<&'static str>,
) -> AppResult {
    let page = LoginPage {
        admin: AdminLayout::new(&state.assets, "登录", None),
        username,
        error,
    };
    Ok((status, Html(page.render()?)).into_response())
}

/// 已登录时直接进后台。
pub async fn form(State(state): State<AppState>, headers: HeaderMap) -> AppResult {
    if current_session(&state, &headers).await?.is_some() {
        return Ok(Redirect::to("/admin/").into_response());
    }
    login_page(&state, StatusCode::OK, String::new(), None)
}

#[derive(Deserialize)]
pub struct Credentials {
    username: String,
    password: String,
}

pub async fn submit(
    State(state): State<AppState>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    form: Result<Form<Credentials>, FormRejection>,
) -> AppResult {
    if !same_origin(&headers, &state.origin) {
        return Ok(forbidden(
            "登录请求的来源（Origin 或 Referer）和站点地址 BASE_URL 不一致，请从本站的登录页提交",
        ));
    }
    let Form(form) = match form {
        Ok(form) => form,
        Err(rejection) => return Ok(rejection.into_response()),
    };
    let ip = client_ip(peer.ip(), &headers, &state.trusted_proxies);
    let now = state.clock.now();
    if !state.limiter.begin_attempt(ip, now) {
        return login_page(
            &state,
            StatusCode::TOO_MANY_REQUESTS,
            form.username,
            Some("登录失败的次数太多，请 15 分钟后再试"),
        );
    }
    let user = users::admin(&state.pool)
        .await?
        .ok_or(AppError::Internal("admin user is missing"))?;
    // 用户名不对时也校验一次密码，响应时间不暴露用户名是否正确（§9.3）。
    let password_ok = password::verify(&form.password, &user.password_hash).await?;
    if !password_ok || form.username.trim() != user.username {
        return login_page(
            &state,
            StatusCode::UNPROCESSABLE_ENTITY,
            form.username,
            Some("用户名或密码错误"),
        );
    }
    state.limiter.succeeded(ip);

    let token = session::random_token()?;
    let csrf = session::random_token()?;
    sessions::create(
        &state.pool,
        &NewSession {
            user_id: user.id,
            token_hash: &session::hash_token(&token),
            csrf_token: &csrf,
            created_at: now,
            expires_at: now + session::SESSION_TTL,
        },
    )
    .await?;
    Ok(see_other(
        "/admin/",
        session::set_cookie(&token, state.secure_cookies),
    ))
}

/// 删除当前会话（D16）并让浏览器删掉 cookie。
pub async fn logout(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
) -> AppResult {
    sessions::delete(&state.pool, &admin.token_hash).await?;
    Ok(see_other(
        "/admin/login",
        session::clear_cookie(state.secure_cookies),
    ))
}

fn see_other(location: &'static str, cookie: String) -> Response {
    (
        StatusCode::SEE_OTHER,
        [
            (header::LOCATION, location.to_owned()),
            (header::SET_COOKIE, cookie),
        ],
    )
        .into_response()
}

/// `Origin` 必须等于站点的 origin；没有 `Origin` 时 `Referer` 必须是站内地址；两者都没有时拒绝（D47）。
fn same_origin(headers: &HeaderMap, origin: &str) -> bool {
    let value = |name| headers.get(name).and_then(|v| v.to_str().ok());
    if let Some(sent) = value(header::ORIGIN) {
        return sent.eq_ignore_ascii_case(origin);
    }
    let Some(referer) = value(header::REFERER) else {
        return false;
    };
    match (referer.get(..origin.len()), referer.get(origin.len()..)) {
        (Some(head), Some(rest)) => {
            head.eq_ignore_ascii_case(origin) && (rest.is_empty() || rest.starts_with('/'))
        }
        _ => false,
    }
}
