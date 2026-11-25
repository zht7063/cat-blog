//! 启动流程的细节和 `db` 模块的行为，用临时目录里的 SQLite 和固定时钟。

use std::sync::LazyLock;

use cat_blog_core::date;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::markdown::Renderer;
use cat_blog_core::save::{self, PreparedEntry};
use cat_blog_server::clock::FixedClock;
use cat_blog_server::config::{AdminPassword, Config};
use cat_blog_server::db::{self, BlogDb, WriteError, sessions, users};
use cat_blog_server::startup;
use jiff::{Timestamp, ToSpan};
use sqlx::SqlitePool;
use tempfile::TempDir;

static RENDERER: LazyLock<Renderer> = LazyLock::new(Renderer::new);

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

/// 2026-11-24T00:00:00Z
const T0: Timestamp = Timestamp::constant(1_795_478_400, 0);

fn config(username: &str, password: &str) -> Config {
    Config {
        admin_username: username.to_owned(),
        admin_password: AdminPassword::new(password),
        base_url: "http://localhost:3000".to_owned(),
        bind_addr: ([127, 0, 0, 1], 0).into(),
        database_path: "unused".into(),
        trusted_proxies: Vec::new(),
        log_filter: "info".to_owned(),
    }
}

async fn pool() -> TestResult<(TempDir, SqlitePool)> {
    let dir = tempfile::tempdir()?;
    let pool = db::connect(&dir.path().join("t.db")).await?;
    Ok((dir, pool))
}

async fn add_session(pool: &SqlitePool, token: &[u8; 32], created_at: Timestamp) -> TestResult<()> {
    let admin = users::admin(pool).await?.ok_or("admin user is missing")?;
    sessions::create(
        pool,
        &sessions::NewSession {
            user_id: admin.id,
            token_hash: token,
            csrf_token: "csrf",
            created_at,
            expires_at: created_at + 720.hours(),
        },
    )
    .await?;
    Ok(())
}

fn entry(raw: &str) -> TestResult<PreparedEntry> {
    let tz = date::parse_timezone("Asia/Singapore")?;
    Ok(save::prepare_entry(
        &RENDERER,
        raw,
        Kind::Post,
        &tz,
        None,
        T0,
    )?)
}

#[tokio::test]
async fn renaming_admin_updates_the_same_user_and_clears_sessions() {
    let (_dir, pool) = pool().await.unwrap();
    let clock = FixedClock::new(T0);
    let cfg = config("admin", "correct horse battery");
    let blog = startup::prepare(&pool, &cfg, &clock, &RENDERER)
        .await
        .unwrap();
    let before = users::admin(&pool).await.unwrap().unwrap();
    add_session(&pool, &[1; 32], T0).await.unwrap();

    let renamed = config("cat", "correct horse battery");
    startup::prepare(&pool, &renamed, &clock, &RENDERER)
        .await
        .unwrap();

    let after = users::admin(&pool).await.unwrap().unwrap();
    assert_eq!(users::count(&pool).await.unwrap(), 1);
    assert_eq!(after.id, before.id);
    assert_eq!(after.username, "cat");
    assert_eq!(after.password_hash, before.password_hash);
    assert!(sessions::find(&pool, &[1; 32], T0).await.unwrap().is_none());
    let same_blog = BlogDb::for_user(&pool, after.id).await.unwrap().unwrap();
    assert_eq!(
        same_blog.list(Kind::Post).await.unwrap().len(),
        blog.list(Kind::Post).await.unwrap().len()
    );
}

#[tokio::test]
async fn startup_deletes_expired_sessions_only() {
    let (_dir, pool) = pool().await.unwrap();
    let clock = FixedClock::new(T0);
    let cfg = config("admin", "correct horse battery");
    startup::prepare(&pool, &cfg, &clock, &RENDERER)
        .await
        .unwrap();
    add_session(&pool, &[1; 32], T0).await.unwrap();
    add_session(&pool, &[2; 32], T0 + 24.hours()).await.unwrap();

    clock.set(T0 + 720.hours());
    startup::prepare(&pool, &cfg, &clock, &RENDERER)
        .await
        .unwrap();

    let earlier = T0 + 1.hour();
    assert!(
        sessions::find(&pool, &[1; 32], earlier)
            .await
            .unwrap()
            .is_none(),
        "session expiring exactly at now is deleted"
    );
    assert!(
        sessions::find(&pool, &[2; 32], earlier)
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn session_is_not_found_once_expires_at_is_reached() {
    let (_dir, pool) = pool().await.unwrap();
    let clock = FixedClock::new(T0);
    startup::prepare(
        &pool,
        &config("admin", "correct horse battery"),
        &clock,
        &RENDERER,
    )
    .await
    .unwrap();
    add_session(&pool, &[1; 32], T0).await.unwrap();

    let expiry = T0 + 720.hours();
    assert!(
        sessions::find(&pool, &[1; 32], expiry - 1.second())
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        sessions::find(&pool, &[1; 32], expiry)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn posts_are_scoped_to_their_blog_and_slugs_are_unique_per_blog() {
    let (_dir, pool) = pool().await.unwrap();
    let clock = FixedClock::new(T0);
    let mine = startup::prepare(
        &pool,
        &config("admin", "correct horse battery"),
        &clock,
        &RENDERER,
    )
    .await
    .unwrap();
    let other_user = users::create(&pool, "other", "x", T0).await.unwrap();
    let home = save::prepare_home(&RENDERER, startup::DEFAULT_HOME).unwrap();
    let theirs = BlogDb::create(&pool, other_user, &home, &[], T0)
        .await
        .unwrap();

    let post = entry("---\ntitle: 同名\n---\n正文\n").unwrap();
    let id = mine.insert_post(Kind::Post, &post, T0).await.unwrap();
    assert!(matches!(
        mine.insert_post(Kind::Page, &post, T0).await,
        Err(WriteError::SlugTaken)
    ));
    let their_id = theirs.insert_post(Kind::Post, &post, T0).await.unwrap();

    assert!(theirs.post(id).await.unwrap().is_none());
    assert!(!theirs.delete_post(id).await.unwrap());
    assert!(!theirs.update_post(id, &post, T0).await.unwrap());
    assert_eq!(
        theirs.post_by_slug(&post.slug).await.unwrap().unwrap().id,
        their_id
    );
    assert_eq!(mine.post_by_slug(&post.slug).await.unwrap().unwrap().id, id);
    assert!(mine.post(id).await.unwrap().is_some());
}

#[tokio::test]
async fn update_and_delete_round_trip() {
    let (_dir, pool) = pool().await.unwrap();
    let clock = FixedClock::new(T0);
    let blog = startup::prepare(
        &pool,
        &config("admin", "correct horse battery"),
        &clock,
        &RENDERER,
    )
    .await
    .unwrap();
    let first = entry("---\ntitle: A\ntags: [Rust, 猫]\n---\n").unwrap();
    let id = blog.insert_post(Kind::Post, &first, T0).await.unwrap();
    let taken = entry("---\ntitle: B\n---\n").unwrap();
    blog.insert_post(Kind::Post, &taken, T0).await.unwrap();

    let renamed = entry("---\ntitle: A\nslug: b\n---\n").unwrap();
    assert!(matches!(
        blog.update_post(id, &renamed, T0).await,
        Err(WriteError::SlugTaken)
    ));
    let stored = blog.post(id).await.unwrap().unwrap();
    assert_eq!(stored.slug.as_str(), "a");
    let tags: Vec<&str> = stored.tags.iter().map(|t| t.as_str()).collect();
    assert_eq!(tags, ["rust", "猫"]);
    assert_eq!(stored.published_at, Some(T0));

    let later = T0 + 24.hours();
    let moved = entry("---\ntitle: A\nslug: c\ndraft: true\n---\n").unwrap();
    assert!(blog.update_post(id, &moved, later).await.unwrap());
    let stored = blog.post(id).await.unwrap().unwrap();
    assert_eq!((stored.slug.as_str(), stored.draft), ("c", true));
    assert_eq!((stored.created_at, stored.updated_at), (T0, later));

    assert!(blog.delete_post(id).await.unwrap());
    assert!(blog.post(id).await.unwrap().is_none());
    assert!(!blog.delete_post(id).await.unwrap());
}

#[tokio::test]
async fn admin_list_puts_unpublished_drafts_first_then_newest() {
    let (_dir, pool) = pool().await.unwrap();
    let clock = FixedClock::new(T0);
    let blog = startup::prepare(
        &pool,
        &config("admin", "correct horse battery"),
        &clock,
        &RENDERER,
    )
    .await
    .unwrap();
    for raw in [
        "---\ntitle: old\ndate: 2026-01-01\n---\n",
        "---\ntitle: new\ndate: 2026-06-01\n---\n",
    ] {
        blog.insert_post(Kind::Post, &entry(raw).unwrap(), T0)
            .await
            .unwrap();
    }
    let titles: Vec<String> = blog
        .list(Kind::Post)
        .await
        .unwrap()
        .into_iter()
        .map(|p| p.title)
        .collect();
    assert_eq!(titles, ["Hello, cat-blog", "new", "old"]);
}
