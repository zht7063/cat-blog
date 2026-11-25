//! 阶段 2 的完成条件（docs/v1.0-implementation.md §4）：真的启动 `cat-blog` 进程，
//! 用 `.env` 配置，发 SIGTERM 让它退出，再打开它留下的数据库检查。

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

use cat_blog_core::frontmatter::Kind;
use cat_blog_server::auth::password;
use cat_blog_server::db::{self, BlogDb, sessions, users};
use jiff::{Timestamp, ToSpan};

const DB: &str = "data/nested/cat-blog.db";

type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

fn write_env(dir: &Path, password: &str) -> std::io::Result<()> {
    fs::write(
        dir.join(".env"),
        format!(
            "ADMIN_USERNAME=admin\nADMIN_PASSWORD=\"{password}\"\nBASE_URL=http://localhost:3000\n\
             BIND_ADDR=127.0.0.1:0\nDATABASE_PATH={DB}\n"
        ),
    )
}

fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cat-blog"));
    cmd.current_dir(dir)
        .env_clear()
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// 启动服务，等它开始监听后发 SIGTERM，返回退出状态和全部输出。
fn start_and_stop(dir: &Path) -> TestResult<(ExitStatus, String)> {
    let mut child = command(dir).spawn()?;
    let stdout = child.stdout.take().ok_or("stdout is not piped")?;
    let mut output = String::new();
    for line in BufReader::new(stdout).lines() {
        let line = line?;
        if line.contains("listening") {
            let killed = Command::new("kill")
                .args(["-TERM", &child.id().to_string()])
                .status()?;
            if !killed.success() {
                return Err("kill -TERM failed".into());
            }
        }
        output.push_str(&line);
        output.push('\n');
    }
    let status = child.wait()?;
    child
        .stderr
        .take()
        .ok_or("stderr is not piped")?
        .read_to_string(&mut output)?;
    Ok((status, output))
}

fn db_path(dir: &Path) -> PathBuf {
    dir.join(DB)
}

#[tokio::test]
async fn first_start_then_password_change_clears_sessions() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "correct horse battery").unwrap();

    let (status, output) = start_and_stop(dir.path()).unwrap();
    assert!(status.success(), "{output}");
    assert!(output.contains("stopped"), "{output}");

    let pool = db::connect(&db_path(dir.path())).await.unwrap();
    assert_eq!(users::count(&pool).await.unwrap(), 1);
    let admin = users::admin(&pool).await.unwrap().unwrap();
    assert_eq!(admin.username, "admin");
    let blog = BlogDb::for_user(&pool, admin.id).await.unwrap().unwrap();
    let home = blog.blog().await.unwrap();
    assert!(home.home_source.contains("{{ posts limit:5 }}"));
    assert!(home.home_html.contains("<!--cat-blog:posts limit=5-->"));
    assert_eq!(home.title, "ฅ^•ﻌ•^ฅ");
    assert_eq!(home.timezone.iana_name(), Some("Asia/Singapore"));
    assert!(blog.list(Kind::Page).await.unwrap().is_empty());
    let posts = blog.list(Kind::Post).await.unwrap();
    assert_eq!(posts.len(), 1);
    let sample = &posts[0];
    assert_eq!(sample.title, "Hello, cat-blog");
    assert_eq!(sample.slug.as_str(), "hello-cat-blog");
    assert!(
        sample
            .source
            .contains("title: Hello, cat-blog\nslug: \"hello-cat-blog\"\n")
    );
    assert!(sample.draft);
    assert_eq!(sample.published_at, None);
    assert_eq!(sample.tags.len(), 1);

    let now: Timestamp = "2100-01-01T00:00:00Z".parse().unwrap();
    let token = [7u8; 32];
    sessions::create(
        &pool,
        &sessions::NewSession {
            user_id: admin.id,
            token_hash: &token,
            csrf_token: "csrf",
            created_at: now,
            expires_at: now + 720.hours(),
        },
    )
    .await
    .unwrap();
    pool.close().await;

    let (status, output) = start_and_stop(dir.path()).unwrap();
    assert!(status.success(), "{output}");
    let pool = db::connect(&db_path(dir.path())).await.unwrap();
    assert!(
        sessions::find(&pool, &token, now).await.unwrap().is_some(),
        "a restart without changes cleared the sessions"
    );
    pool.close().await;

    write_env(dir.path(), "a brand new password").unwrap();
    let (status, output) = start_and_stop(dir.path()).unwrap();
    assert!(status.success(), "{output}");
    let pool = db::connect(&db_path(dir.path())).await.unwrap();
    assert!(sessions::find(&pool, &token, now).await.unwrap().is_none());
    let admin = users::admin(&pool).await.unwrap().unwrap();
    assert!(
        password::verify("a brand new password", &admin.password_hash)
            .await
            .unwrap()
    );
    assert_eq!(users::count(&pool).await.unwrap(), 1);
    let blog = BlogDb::for_user(&pool, admin.id).await.unwrap().unwrap();
    assert_eq!(blog.list(Kind::Post).await.unwrap().len(), 1);
}

#[test]
fn short_password_exits_nonzero_before_touching_the_database() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "猫猫猫猫猫猫猫猫猫猫猫").unwrap();

    let (status, output) = start_and_stop(dir.path()).unwrap();
    assert!(!status.success(), "{output}");
    assert!(
        output.contains("ADMIN_PASSWORD has 11 characters"),
        "{output}"
    );
    assert!(!output.contains('猫'), "password leaked: {output}");
    assert!(!db_path(dir.path()).exists());
}
