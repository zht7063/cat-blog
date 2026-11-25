//! 阶段 4 的完成条件（docs/v1.0-implementation.md §4）：后台的编辑、预览和删除。

use axum::http::{StatusCode, header};
use cat_blog_core::frontmatter::Kind;
use cat_blog_server::db::PostId;

use crate::support::{CLIENT, PASSWORD, T0, login, site, textarea, try_login};

const NOINDEX: &str = r#"<meta name="robots" content="noindex">"#;

#[tokio::test]
async fn full_flow_login_publish_draft_logout() {
    let site = site().await.unwrap();

    let reply = try_login(&site, CLIENT, PASSWORD).await;
    let cookie = reply.header(header::SET_COOKIE);
    for attr in [
        "Path=/",
        "HttpOnly",
        "SameSite=Lax",
        "Secure",
        "Max-Age=2592000",
    ] {
        assert!(cookie.contains(attr), "{attr} in {cookie}");
    }
    let admin = login(&site).await.unwrap();

    let source = "---\ntitle: 我的 Rust 笔记！\ndraft: false\ntags: [rust]\n---\n\nhello cat\n";
    let id = admin.create_post(&site, source).await.unwrap();
    let edit = admin
        .get(&site, &format!("/admin/edit/{id}?saved=1"))
        .await
        .unwrap();
    assert!(edit.body.contains("已保存"), "{}", edit.body);
    let written = textarea(&edit.body);
    assert_eq!(
        written,
        "---\ntitle: 我的 Rust 笔记！\nslug: \"我的-rust-笔记\"\ndraft: false\ntags: [rust]\n---\n\nhello cat\n"
    );

    let url = "/%E6%88%91%E7%9A%84-rust-%E7%AC%94%E8%AE%B0/";
    let public = site.get(url).await.unwrap();
    assert_eq!(public.status, StatusCode::OK);
    assert!(public.body.contains("hello cat"), "{}", public.body);
    assert!(!public.body.contains(NOINDEX), "{}", public.body);
    assert!(
        site.get("/feed.xml")
            .await
            .unwrap()
            .body
            .contains("hello cat")
    );

    let draft = written.replace("draft: false", "draft: true");
    let reply = admin
        .post(&site, &format!("/admin/edit/{id}"), &[("source", &draft)])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
    assert_eq!(site.get(url).await.unwrap().status, StatusCode::NOT_FOUND);
    assert!(!site.get("/blog/").await.unwrap().body.contains("我的 Rust"));

    // 管理员仍能看到草稿，但不被收录、不被缓存（D12、D68）。
    let preview = admin.get(&site, url).await.unwrap();
    assert_eq!(preview.status, StatusCode::OK);
    assert!(preview.body.contains("hello cat") && preview.body.contains(NOINDEX));
    assert_eq!(preview.header(header::CACHE_CONTROL), "no-store");

    let reply = admin.post(&site, "/admin/logout", &[]).await.unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header(header::LOCATION), "/admin/login");
    assert!(reply.header(header::SET_COOKIE).contains("Max-Age=0"));

    let after = admin.get(&site, "/admin/").await.unwrap();
    assert_eq!(after.status, StatusCode::SEE_OTHER);
    assert_eq!(after.header(header::LOCATION), "/admin/login");
    assert_eq!(
        admin.get(&site, url).await.unwrap().status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn renaming_title_keeps_the_url() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    let id = admin
        .create_post(
            &site,
            "---\ntitle: First Title\ndraft: false\n---\n\nbody\n",
        )
        .await
        .unwrap();

    let source = site.blog.post(PostId(id)).await.unwrap().unwrap().source;
    let renamed = source.replace("title: First Title", "title: Second Title");
    let reply = admin
        .post(&site, &format!("/admin/edit/{id}"), &[("source", &renamed)])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);

    let page = site.get("/first-title/").await.unwrap();
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("<h1>Second Title</h1>"), "{}", page.body);
    assert_eq!(
        site.get("/second-title/").await.unwrap().status,
        StatusCode::NOT_FOUND
    );

    // 作者删掉了 slug 那一行再改标题：沿用库里的 slug，并重新写回（§5.3）。
    let without_slug = "---\ntitle: Third Title\ndraft: false\n---\n\nbody\n";
    let reply = admin
        .post(
            &site,
            &format!("/admin/edit/{id}"),
            &[("source", without_slug)],
        )
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER, "{}", reply.body);
    let post = site.blog.post(PostId(id)).await.unwrap().unwrap();
    assert_eq!(post.slug.as_str(), "first-title");
    assert!(
        post.source.contains("slug: \"first-title\""),
        "{}",
        post.source
    );
    assert_eq!(post.published_at, Some(T0));
}

#[tokio::test]
async fn conflicting_slug_returns_422_and_keeps_the_source() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    admin
        .create_post(&site, "---\ntitle: Taken\ndraft: false\n---\n\nfirst\n")
        .await
        .unwrap();

    // 浏览器提交的换行是 CRLF，编辑框要原样还回去。
    let clash = "---\r\ntitle: Other\r\nslug: TAKEN\r\n---\r\n\r\n<b>second</b> & more\r\n";
    let reply = admin
        .post(&site, "/admin/posts/new", &[("source", clash)])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        reply
            .body
            .contains("第 3 行：slug &#34;taken&#34; 已被其他页面或文章使用"),
        "{}",
        reply.body
    );
    assert_eq!(textarea(&reply.body), clash);
    assert_eq!(site.blog.list(Kind::Post).await.unwrap().len(), 2);

    // 已有文章改成冲突的 slug：同样 422，库里的原文不变。
    let id = admin
        .create_post(&site, "---\ntitle: Second\ndraft: false\n---\n")
        .await
        .unwrap();
    let before = site.blog.post(PostId(id)).await.unwrap().unwrap().source;
    let edited = before.replace("slug: \"second\"", "slug: taken");
    let reply = admin
        .post(&site, &format!("/admin/edit/{id}"), &[("source", &edited)])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(textarea(&reply.body), edited);
    let after = site.blog.post(PostId(id)).await.unwrap().unwrap().source;
    assert_eq!(after, before);
}

#[tokio::test]
async fn save_errors_show_the_line_and_keep_the_source() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    let before = site.blog.blog().await.unwrap().home_source;

    let bad = "---\ntitle: 猫\nbogus: 1\n---\n";
    let reply = admin
        .post(&site, "/admin/", &[("source", bad)])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(
        reply.body.contains("第 3 行：未知字段 bogus"),
        "{}",
        reply.body
    );
    assert!(reply.body.contains("data-unsaved"), "{}", reply.body);
    assert_eq!(textarea(&reply.body), bad);
    assert_eq!(site.blog.blog().await.unwrap().home_source, before);

    // 改了时区：保存成功，并提示执行 rerender（D44）。
    let home = "---\ntitle: 猫\ntimezone: America/New_York\n---\n\nhi\n";
    let reply = admin
        .post(&site, "/admin/", &[("source", home)])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    let page = admin
        .get(&site, reply.header(header::LOCATION))
        .await
        .unwrap();
    assert!(page.body.contains("已保存") && page.body.contains("cat-blog rerender"));
    assert_eq!(site.blog.blog().await.unwrap().title, "猫");
    let reply = admin
        .post(&site, "/admin/", &[("source", home)])
        .await
        .unwrap();
    assert_eq!(reply.header(header::LOCATION), "/admin/?saved=1");
}

#[tokio::test]
async fn preview_renders_unsaved_source_without_writing() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();

    let source = "---\ntitle: Preview Me\ndraft: true\ntags: [cat]\n---\n\nnot saved yet\n";
    let reply = admin
        .post(
            &site,
            "/admin/preview",
            &[("target", "post"), ("source", source)],
        )
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("<h1>Preview Me</h1>") && reply.body.contains("not saved yet"));
    assert!(reply.body.contains(NOINDEX), "{}", reply.body);
    assert_eq!(reply.header(header::CACHE_CONTROL), "no-store");
    assert_eq!(site.blog.list(Kind::Post).await.unwrap().len(), 1);

    let reply = admin
        .post(
            &site,
            "/admin/preview",
            &[
                ("target", "home"),
                ("source", "---\ntitle: 新标题\n---\n\n预览首页\n"),
            ],
        )
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::OK);
    assert!(reply.body.contains("新标题") && reply.body.contains("预览首页"));
    assert_ne!(site.blog.blog().await.unwrap().title, "新标题");

    let reply = admin
        .post(
            &site,
            "/admin/preview",
            &[
                ("target", "page"),
                ("source", "---\ntitle: A\ntags: [x]\n---\n"),
            ],
        )
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert!(reply.body.contains("页面不能写 tags"), "{}", reply.body);
}

#[tokio::test]
async fn admin_sees_future_posts_unindexed_and_uncached() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    admin
        .create_post(&site, "---\ntitle: Later\ndate: 2027-01-01\n---\n\nsoon\n")
        .await
        .unwrap();
    assert_eq!(
        site.get("/later/").await.unwrap().status,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        site.get("/Later").await.unwrap().status,
        StatusCode::NOT_FOUND
    );

    let page = admin.get(&site, "/later/").await.unwrap();
    assert_eq!(page.status, StatusCode::OK);
    assert!(page.body.contains("soon") && page.body.contains(NOINDEX));
    assert_eq!(page.header(header::CACHE_CONTROL), "no-store");

    // 不能让浏览器永久缓存只有管理员看得到的地址的跳转（D68）。
    let redirect = admin.get(&site, "/Later").await.unwrap();
    assert_eq!(redirect.status, StatusCode::MOVED_PERMANENTLY);
    assert_eq!(redirect.header(header::CACHE_CONTROL), "no-store");
    let public = site.post("Visible", "").await;
    public.unwrap();
    let redirect = admin.get(&site, "/Visible").await.unwrap();
    assert_eq!(redirect.header(header::CACHE_CONTROL), "");
}

#[tokio::test]
async fn delete_asks_for_confirmation_first() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    let id = admin
        .create_post(&site, "---\ntitle: Doomed\ndraft: false\n---\n")
        .await
        .unwrap();

    let confirm = admin
        .get(&site, &format!("/admin/delete/{id}"))
        .await
        .unwrap();
    assert_eq!(confirm.status, StatusCode::OK);
    assert!(confirm.body.contains("《Doomed》"), "{}", confirm.body);
    assert!(site.blog.post(PostId(id)).await.unwrap().is_some());

    let reply = admin
        .post(&site, &format!("/admin/delete/{id}"), &[])
        .await
        .unwrap();
    assert_eq!(reply.status, StatusCode::SEE_OTHER);
    assert_eq!(reply.header(header::LOCATION), "/admin/posts/");
    assert!(site.blog.post(PostId(id)).await.unwrap().is_none());
    assert_eq!(
        site.get("/doomed/").await.unwrap().status,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn lists_put_unpublished_drafts_first_and_mark_them() {
    let site = site().await.unwrap();
    let admin = login(&site).await.unwrap();
    admin
        .create_post(&site, "---\ntitle: Published\ndraft: false\n---\n")
        .await
        .unwrap();
    admin
        .create_post(&site, "---\ntitle: Scheduled\ndate: 2027-01-01\n---\n")
        .await
        .unwrap();

    let list = admin.get(&site, "/admin/posts/").await.unwrap().body;
    let rows: Vec<&str> = list.split("<tr>").skip(2).collect();
    assert_eq!(rows.len(), 3, "{list}");
    assert!(
        rows[0].contains("Hello, cat-blog") && rows[0].contains("—") && rows[0].contains("草稿")
    );
    assert!(
        rows[1].contains("Scheduled") && rows[1].contains("2027-01-01") && rows[1].contains("定时")
    );
    assert!(
        rows[2].contains("Published")
            && rows[2].contains("2026-11-24")
            && !rows[2].contains("badge")
    );

    let pages = admin.get(&site, "/admin/pages/").await.unwrap().body;
    assert!(pages.contains(r#"href="/admin/pages/new""#), "{pages}");
}
