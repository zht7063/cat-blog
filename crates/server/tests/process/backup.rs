//! 阶段 5 的完成条件（docs/v1.0-implementation.md §4）：服务正在通过后台保存文章时执行
//! `cat-blog backup`，副本通过 `integrity_check` 且能被新进程打开；目标文件已存在时报错；
//! 副本权限为 600。

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use std::net::SocketAddr;
use std::sync::mpsc;
use std::thread;

use cat_blog_core::frontmatter::Kind;
use cat_blog_server::db::{self, BlogDb, users};

use crate::support::{Serving, TestResult, command, db_path, http, run, write_env};

const BACKUP: &str = "backups/cat-blog-2026-11-25.db";

/// 登录后台，返回会话 cookie 和 CSRF token。
fn login(addr: SocketAddr) -> TestResult<(String, String)> {
    let reply = http(
        addr,
        "POST",
        "/admin/login",
        None,
        &[("username", "admin"), ("password", "correct horse battery")],
    )?;
    let cookie = reply.cookie.ok_or("login set no cookie")?;
    let page = http(addr, "GET", "/admin/posts/new", Some(&cookie), &[])?;
    let csrf = page
        .body
        .split(r#"name="_csrf" value=""#)
        .nth(1)
        .and_then(|rest| rest.split('"').next())
        .ok_or("new post page has no _csrf")?
        .to_owned();
    Ok((cookie, csrf))
}

/// 通过后台不停新建文章，直到 `stop` 为真，`written` 记录服务已经保存的篇数。
/// `ready` 在保存了第一批文章后发送一次。
fn keep_writing(
    addr: SocketAddr,
    stop: &AtomicBool,
    written: &AtomicUsize,
    ready: mpsc::Sender<()>,
) -> TestResult<()> {
    let (cookie, csrf) = login(addr)?;
    let body = "正文。".repeat(200);
    for i in 0.. {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let source = format!("---\ntitle: 并发写入 {i}\n---\n{body}\n");
        let reply = http(
            addr,
            "POST",
            "/admin/posts/new",
            Some(&cookie),
            &[("_csrf", &csrf), ("source", &source)],
        )?;
        if reply.status != 303 {
            return Err(format!("saving post {i} returned {}", reply.status).into());
        }
        if written.fetch_add(1, Ordering::SeqCst) + 1 == 20 {
            ready.send(())?;
        }
    }
    Ok(())
}

async fn post_count(path: &std::path::Path) -> TestResult<usize> {
    let pool = db::open(path).await?;
    let admin = users::admin(&pool).await?.ok_or("admin user is missing")?;
    let blog = BlogDb::for_user(&pool, admin.id)
        .await?
        .ok_or("blog is missing")?;
    let count = blog.list(Kind::Post).await?.len();
    pool.close().await;
    Ok(count)
}

#[tokio::test]
async fn backup_while_writing_is_consistent_private_and_never_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "correct horse battery").unwrap();
    let server = Serving::start(command(dir.path())).unwrap();

    let stop = Arc::new(AtomicBool::new(false));
    let written = Arc::new(AtomicUsize::new(0));
    let (ready, started) = mpsc::channel();
    let writer = {
        let (addr, stop, written) = (server.addr, stop.clone(), written.clone());
        thread::spawn(move || keep_writing(addr, &stop, &written, ready).map_err(|e| e.to_string()))
    };
    started.recv().unwrap();

    let before = written.load(Ordering::SeqCst);
    let mut cmd = command(dir.path());
    cmd.args(["backup", BACKUP]);
    let (status, output) = run(cmd).unwrap();
    let after = written.load(Ordering::SeqCst);
    stop.store(true, Ordering::SeqCst);
    writer.join().unwrap().unwrap();
    assert!(status.success(), "{output}");
    assert!(
        after > before,
        "no save overlapped the backup ({before} before, {after} after)"
    );

    let copy = dir.path().join(BACKUP);
    let mode = fs::metadata(&copy).unwrap().permissions().mode();
    assert_eq!(mode & 0o777, 0o600, "backup mode is {mode:o}");
    let files: Vec<_> = fs::read_dir(copy.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(
        files,
        ["cat-blog-2026-11-25.db"],
        "backup needs no -wal/-shm"
    );
    assert_eq!(db::integrity_check(&copy).await.unwrap(), ["ok"]);

    // 示例草稿加上备份开始前已提交的文章都在；备份期间提交的可能在也可能不在。
    let in_copy = post_count(&copy).await.unwrap();
    assert!(
        (1 + before..=1 + after).contains(&in_copy),
        "copy has {in_copy} posts, expected between {} and {}",
        1 + before,
        1 + after
    );

    let bytes = fs::read(&copy).unwrap();
    let mut again = command(dir.path());
    again.args(["backup", BACKUP]);
    let (status, output) = run(again).unwrap();
    assert!(!status.success(), "{output}");
    assert!(output.contains("already exists"), "{output}");
    assert_eq!(fs::read(&copy).unwrap(), bytes, "existing file was changed");

    let (status, output) = server.stop().unwrap();
    assert!(status.success(), "{output}");

    // 新进程用副本启动服务。
    let mut restored = command(dir.path());
    restored.env("DATABASE_PATH", BACKUP);
    let (status, output) = Serving::start(restored).unwrap().stop().unwrap();
    assert!(status.success(), "{output}");
    assert_eq!(post_count(&copy).await.unwrap(), in_copy);
}

#[test]
fn backup_without_a_database_fails_and_creates_nothing() {
    let dir = tempfile::tempdir().unwrap();
    write_env(dir.path(), "correct horse battery").unwrap();
    let mut cmd = command(dir.path());
    cmd.args(["backup", BACKUP]);
    let (status, output) = run(cmd).unwrap();
    assert!(!status.success(), "{output}");
    assert!(output.contains("does not exist"), "{output}");
    assert!(!db_path(dir.path()).exists());
    assert!(!dir.path().join(BACKUP).exists());
}
