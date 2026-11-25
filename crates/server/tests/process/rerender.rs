//! `cat-blog rerender`（D33、D44，docs/v1.0-spec.md §7.1）：改时区后按新时区重算写了 `date`
//! 的内容，重新渲染正文，不改原文和 `updated_at`；新规则下失败的内容被跳过并以非零状态码退出。

use cat_blog_core::date;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::markdown::Renderer;
use cat_blog_core::save;
use cat_blog_server::db::{self, BlogDb, Post, PostId, users};
use jiff::Timestamp;

use crate::support::{TestResult, command, db_path, run, start_and_stop, write_env};

async fn find(blog: &BlogDb, id: PostId) -> TestResult<Post> {
    Ok(blog.post(id).await?.ok_or("post is missing")?)
}

#[tokio::test]
async fn rerender_applies_the_new_timezone_and_skips_broken_entries() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "correct horse battery").unwrap();
    let (status, output) = start_and_stop(command(dir.path())).unwrap();
    assert!(status.success(), "{output}");

    let pool = db::connect(&db_path(dir.path())).await.unwrap();
    let admin = users::admin(&pool).await.unwrap().unwrap();
    let blog = BlogDb::for_user(&pool, admin.id).await.unwrap().unwrap();
    let renderer = Renderer::new();
    let singapore = date::parse_timezone("Asia/Singapore").unwrap();
    let t0: Timestamp = "2026-11-20T00:00:00Z".parse().unwrap();
    let prepare =
        |raw: &str| save::prepare_entry(&renderer, raw, Kind::Post, &singapore, None, t0).unwrap();

    let mut dated = prepare("---\ntitle: Dated\ndate: 2026-11-24 09:30\n---\n**粗体**\n");
    assert_eq!(
        dated.published_at,
        Some("2026-11-24T01:30:00Z".parse().unwrap())
    );
    dated.html = "stale".to_owned();
    let dated_id = blog.insert_post(Kind::Post, &dated, t0).await.unwrap();
    let undated = prepare("---\ntitle: Undated\n---\n正文\n");
    let undated_id = blog.insert_post(Kind::Post, &undated, t0).await.unwrap();
    let mut broken = prepare("---\ntitle: Broken\n---\n正文\n");
    broken.source = "---\ntitle: Broken\nunknown: 1\n---\n正文\n".to_owned();
    broken.html = "kept".to_owned();
    let broken_id = blog.insert_post(Kind::Post, &broken, t0).await.unwrap();

    // 在后台改了时区，但没有执行 rerender。
    let old_home = blog.blog().await.unwrap();
    let home_source = old_home
        .home_source
        .replace("timezone: Asia/Singapore", "timezone: America/New_York");
    let home = save::prepare_home(&renderer, &home_source).unwrap();
    assert_eq!(home.timezone.iana_name(), Some("America/New_York"));
    blog.update_home(&home, t0).await.unwrap();
    let before = find(&blog, dated_id).await.unwrap();
    pool.close().await;

    let mut cmd = command(dir.path());
    cmd.arg("rerender");
    let (status, output) = run(cmd).unwrap();
    assert!(!status.success(), "{output}");
    assert!(
        output.contains(&format!("/broken/ (edit at /admin/edit/{})", broken_id.0)),
        "{output}"
    );
    // 首页、示例草稿和两篇正常的文章。
    assert!(output.contains("rendered=4"), "{output}");

    let pool = db::open(&db_path(dir.path())).await.unwrap();
    let blog = BlogDb::for_user(&pool, admin.id).await.unwrap().unwrap();
    let after = find(&blog, dated_id).await.unwrap();
    assert_eq!(
        after.published_at,
        Some("2026-11-24T14:30:00Z".parse().unwrap()),
        "09:30 in New York is 14:30 UTC"
    );
    assert_eq!(after.html.trim(), "<p><strong>粗体</strong></p>");
    assert_eq!(after.source, before.source);
    assert_eq!(after.updated_at, before.updated_at);
    assert_eq!(
        find(&blog, undated_id).await.unwrap().published_at,
        Some(t0),
        "first publication time stays (D45)"
    );
    let skipped = find(&blog, broken_id).await.unwrap();
    assert_eq!((skipped.source, skipped.html), (broken.source, broken.html));

    assert!(blog.delete_post(broken_id).await.unwrap());
    pool.close().await;
    let mut cmd = command(dir.path());
    cmd.arg("rerender");
    let (status, output) = run(cmd).unwrap();
    assert!(status.success(), "{output}");
}
