//! 阶段 4 的完成条件（docs/v1.0-implementation.md §4）：登录、会话、CSRF 和限流。

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use cat_blog_core::frontmatter::Kind;
use jiff::{SignedDuration, ToSpan};

use crate::support::{
    BASE_URL, CLIENT, PASSWORD, T0, encode, login, login_request, site, try_login,
};

#[tokio::test]
async fn post_without_valid_csrf_returns_403() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    let source = encode(&[("source", "---\ntitle: Sneaky\ndraft: false\n---\n")]);

    let missing = admin
        .post_raw(&site, "/admin/posts/new", &source)
        .await
        .unwrap();
    assert_eq!(missing.status, StatusCode::FORBIDDEN);
    let wrong = admin
        .post_raw(
            &site,
            "/admin/posts/new",
            &format!("_csrf=x{}&{source}", admin.csrf),
        )
        .await
        .unwrap();
    assert_eq!(wrong.status, StatusCode::FORBIDDEN);
    let logout = admin.post_raw(&site, "/admin/logout", "").await.unwrap();
    assert_eq!(logout.status, StatusCode::FORBIDDEN);

    assert!(
        !site
            .blog
            .list(Kind::Post)
            .await
            .unwrap()
            .iter()
            .any(|p| p.title == "Sneaky")
    );
    assert_eq!(
        admin.get(&site, "/admin/").await.unwrap().status,
        StatusCode::OK,
        "the session survives a rejected logout"
    );
}

#[tokio::test]
async fn admin_pages_require_login() {
    let site = site().await.unwrap();
    for uri in [
        "/admin/",
        "/admin/posts/",
        "/admin/pages/new",
        "/admin/edit/1",
        "/admin/nope",
    ] {
        let reply = site.get(uri).await.unwrap();
        assert_eq!(reply.status, StatusCode::SEE_OTHER, "{uri}");
        assert_eq!(reply.header(header::LOCATION), "/admin/login", "{uri}");
    }
    let request = Request::post("/admin/posts/new")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(Body::from("source=x"))
        .unwrap();
    let reply = site.send(request).await.unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER);

    let login_page = site.get("/admin/login").await.unwrap();
    assert_eq!(login_page.status, StatusCode::OK);
    assert_eq!(login_page.header(header::X_FRAME_OPTIONS), "DENY");

    let admin = login(&site).await.unwrap();
    assert_eq!(
        admin.get(&site, "/admin/nope").await.unwrap().status,
        StatusCode::NOT_FOUND
    );
    let reply = admin.get(&site, "/admin/login").await.unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header(header::LOCATION), "/admin/");
}

#[tokio::test]
async fn login_from_another_origin_returns_403() {
    let site = site().await.unwrap();
    for (origin, referer) in [
        (Some("https://evil.example"), None),
        (Some("null"), Some("https://blog.example/admin/login")),
        (None, Some("https://evil.example/admin/login")),
        (None, Some("https://blog.example.evil/admin/login")),
        (None, None),
    ] {
        let reply = site
            .send(login_request(CLIENT, origin, referer, PASSWORD).unwrap())
            .await
            .unwrap();
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{origin:?} {referer:?}"
        );
        assert!(reply.header(header::SET_COOKIE).is_empty());
    }

    // 来源检查在解析表单之前：字段缺失的跨站请求也是 403。
    let request = Request::post("/admin/login")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .header(header::ORIGIN, "https://evil.example")
        .body(Body::from("username=admin"))
        .unwrap();
    assert_eq!(
        site.send(request).await.unwrap().status,
        StatusCode::FORBIDDEN
    );

    for (origin, referer) in [
        (Some(BASE_URL), None),
        (None, Some("https://blog.example/admin/login")),
    ] {
        let reply = site
            .send(login_request(CLIENT, origin, referer, PASSWORD).unwrap())
            .await
            .unwrap();
        assert_eq!(
            reply.status,
            StatusCode::SEE_OTHER,
            "{origin:?} {referer:?}"
        );
    }
}

#[tokio::test]
async fn sixth_login_from_one_ip_within_15_minutes_returns_429() {
    let site = site().await.unwrap();
    for i in 0..5 {
        site.clock.set(T0 + (i * 2).minutes());
        let reply = try_login(&site, CLIENT, "wrong password!").await;
        assert_eq!(
            reply.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "attempt {i}"
        );
        assert!(reply.body.contains("用户名或密码错误"), "{}", reply.body);
    }

    // 窗口内第 6 次，即使密码正确也拒绝（D48）。
    site.clock.set(T0 + 14.minutes());
    let blocked = try_login(&site, CLIENT, PASSWORD).await;
    assert_eq!(blocked.status, StatusCode::TOO_MANY_REQUESTS);
    assert!(blocked.header(header::SET_COOKIE).is_empty());

    // 按真实客户端 IP 计数，不是按代理的地址（D38）。
    assert_eq!(
        try_login(&site, "203.0.113.9", PASSWORD).await.status,
        StatusCode::SEE_OTHER
    );

    // 最早的失败滑出窗口后，失败次数降到 5 次以下，又可以登录。
    site.clock.set(T0 + 15.minutes());
    assert_eq!(
        try_login(&site, CLIENT, PASSWORD).await.status,
        StatusCode::SEE_OTHER
    );
    // 登录成功清空了记录：再失败 4 次也不会被限流。
    for _ in 0..4 {
        try_login(&site, CLIENT, "wrong password!").await;
    }
    assert_eq!(
        try_login(&site, CLIENT, PASSWORD).await.status,
        StatusCode::SEE_OTHER
    );
}

#[tokio::test]
async fn session_expires_30_days_after_login() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();

    site.clock
        .set(T0 + SignedDuration::from_hours(30 * 24) - 1.second());
    assert_eq!(
        admin.get(&site, "/admin/").await.unwrap().status,
        StatusCode::OK
    );

    site.clock.set(T0 + SignedDuration::from_hours(30 * 24));
    let reply = admin.get(&site, "/admin/").await.unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header(header::LOCATION), "/admin/login");
}
