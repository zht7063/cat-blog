//! 运行真实 `cat-blog` 进程的辅助函数：临时目录里的 `.env`、命令和启停。

use std::fs;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};

pub const DB: &str = "data/nested/cat-blog.db";

pub type TestResult<T> = Result<T, Box<dyn std::error::Error>>;

pub fn write_env(dir: &Path, password: &str) -> std::io::Result<()> {
    fs::write(
        dir.join(".env"),
        format!(
            "ADMIN_USERNAME=admin\nADMIN_PASSWORD=\"{password}\"\nBASE_URL=http://localhost:3000\n\
             BIND_ADDR=127.0.0.1:0\nDATABASE_PATH={DB}\n"
        ),
    )
}

pub fn command(dir: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_cat-blog"));
    cmd.current_dir(dir)
        .env_clear()
        .env("NO_COLOR", "1")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

/// 用 `cmd` 启动服务，等它开始监听后发 SIGTERM，返回退出状态和全部输出。
pub fn start_and_stop(mut cmd: Command) -> TestResult<(ExitStatus, String)> {
    let mut child = cmd.spawn()?;
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

pub fn db_path(dir: &Path) -> PathBuf {
    dir.join(DB)
}
