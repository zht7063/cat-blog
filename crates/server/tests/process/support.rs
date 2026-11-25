//! 运行真实 `cat-blog` 进程的辅助函数：临时目录里的 `.env`、命令和启停。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread;

pub const DB: &str = "data/nested/cat-blog.db";

pub type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

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

/// 在后台运行的 `cat-blog serve`。drop 时发 SIGKILL，测试失败时也不会留下进程。
pub struct Serving {
    child: Child,
    output: thread::JoinHandle<String>,
    /// 服务实际监听的地址（`.env` 里写的是端口 0）。
    pub addr: SocketAddr,
}

impl Serving {
    /// 用 `cmd` 启动服务，等到它开始监听才返回。
    pub fn start(mut cmd: Command) -> TestResult<Self> {
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().ok_or("stdout is not piped")?;
        let mut lines = BufReader::new(stdout).lines();
        let mut output = String::new();
        let addr = loop {
            let Some(line) = lines.next() else {
                let _ = child.kill();
                let mut stderr = String::new();
                if let Some(mut pipe) = child.stderr.take() {
                    pipe.read_to_string(&mut stderr)?;
                }
                return Err(format!("serve exited before listening:\n{output}{stderr}").into());
            };
            let line = line?;
            output.push_str(&line);
            output.push('\n');
            if line.contains("listening") {
                let addr = line
                    .rsplit("addr=")
                    .next()
                    .ok_or("listening line has no addr")?;
                break addr.trim().parse()?;
            }
        };
        let output = thread::spawn(move || {
            for line in lines.map_while(Result::ok) {
                output.push_str(&line);
                output.push('\n');
            }
            output
        });
        Ok(Self {
            child,
            output,
            addr,
        })
    }

    /// 发 SIGTERM，等它退出，返回退出状态和标准输出。
    pub fn stop(mut self) -> TestResult<(ExitStatus, String)> {
        let killed = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()?;
        if !killed.success() {
            return Err("kill -TERM failed".into());
        }
        let status = self.child.wait()?;
        let output = std::mem::replace(&mut self.output, thread::spawn(String::new))
            .join()
            .map_err(|_| "output reader panicked")?;
        Ok((status, output))
    }
}

impl Drop for Serving {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// 运行一个会自己结束的命令（`backup`、`rerender`），返回退出状态和全部输出。
pub fn run(mut cmd: Command) -> TestResult<(ExitStatus, String)> {
    let out = cmd.output()?;
    let mut text = String::from_utf8(out.stdout)?;
    text.push_str(&String::from_utf8(out.stderr)?);
    Ok((out.status, text))
}

/// 一个 HTTP 响应：状态码、`Set-Cookie` 的 `name=value` 部分和正文。
pub struct Response {
    pub status: u16,
    pub cookie: Option<String>,
    pub body: String,
}

/// 向 `addr` 发一个 HTTP/1.1 请求。`body` 不为空时作为表单提交。`Origin` 和 `.env` 里的
/// `BASE_URL` 一致，登录时的 Origin 校验（D47）能通过。
pub fn http(
    addr: SocketAddr,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    form: &[(&str, &str)],
) -> TestResult<Response> {
    let body = form_urlencoded::Serializer::new(String::new())
        .extend_pairs(form)
        .finish();
    let cookie = cookie.map_or_else(String::new, |c| format!("Cookie: {c}\r\n"));
    let mut stream = TcpStream::connect(addr)?;
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: localhost:3000\r\nOrigin: http://localhost:3000\r\n\
         {cookie}Content-Type: application/x-www-form-urlencoded\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{body}",
        body.len()
    )?;
    let mut raw = String::new();
    stream.read_to_string(&mut raw)?;
    let (head, body) = raw
        .split_once("\r\n\r\n")
        .ok_or("response has no header end")?;
    let status = head
        .split(' ')
        .nth(1)
        .ok_or("response has no status")?
        .parse()?;
    let cookie = head.lines().find_map(|line| {
        let value = line.strip_prefix("set-cookie: ")?;
        Some(value.split(';').next()?.to_owned())
    });
    Ok(Response {
        status,
        cookie,
        body: body.to_owned(),
    })
}
