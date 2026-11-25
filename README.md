# cat-blog

单人自部署的简洁博客：一个可执行文件 `cat-blog`、一个 SQLite 文件。在 `/admin/` 用一个纯文本框编辑首页、页面和文章（YAML front matter 加 Markdown 正文）。HTTPS 交给前面的 Caddy 或 Nginx。

设计见 [docs/v1.0-spec.md](docs/v1.0-spec.md)，开发约定见 [docs/conventions.md](docs/conventions.md)。许可证为 AGPL-3.0。

## 配置

所有命令都从**当前目录**的 `.env` 和环境变量读配置，环境变量优先。从仓库根目录的 `.env.example` 复制一份（Docker 部署时复制到 `deploy/.env`），权限设为 600，因为管理员密码是明文。

| 变量 | 必需 | 默认值 | 说明 |
|---|---|---|---|
| `ADMIN_USERNAME` | 是 | — | 唯一管理员的用户名 |
| `ADMIN_PASSWORD` | 是 | — | 至少 12 个字符。改了以后重启，所有会话失效 |
| `BASE_URL` | 是 | — | 站点的公开地址，如 `https://blog.example.com`。`https://` 时 cookie 带 `Secure`。Docker 部署时 Caddy 也用它作为站点地址 |
| `DATABASE_PATH` | 否 | `./data/cat-blog.db` | SQLite 文件，目录不存在时自动创建 |
| `BIND_ADDR` | 否 | `127.0.0.1:3000` | 监听地址 |
| `TRUSTED_PROXIES` | 否 | `127.0.0.1/32,::1/128` | 可信反向代理的 IP 段，逗号分隔。登录限流按 `X-Forwarded-For` 里的真实客户端 IP 计数，只信任这里列出的代理 |
| `RUST_LOG` | 否 | `info` | 日志级别 |

## 构建

```sh
docker build -t cat-blog .          # Docker 镜像，下面的 docker compose 会自动构建
cargo build --release --locked      # 或直接构建可执行文件 target/release/cat-blog，Rust 版本见 rust-toolchain.toml
```

不发布预编译的二进制和镜像，部署者自己构建。

## 用 Docker 部署

需要 Docker 和 Docker Compose，域名已解析到这台机器，80、443 端口可以从外网访问。

```sh
git clone https://github.com/zht7063/cat-blog.git
cd cat-blog
cd deploy
cp ../.env.example .env && chmod 600 .env   # 填好 ADMIN_PASSWORD、BASE_URL
docker compose up -d --build
```

- `cat-blog` 容器不对外暴露端口，只有 Caddy 能访问它；Caddy 按 `BASE_URL` 自动申请 HTTPS 证书。Caddy 只拿到 `BASE_URL`，拿不到管理员密码。
- 下文的 `docker compose` 命令都在 `deploy/` 目录里执行。
- 数据在名为 `data` 的卷里（容器内的 `/data`）。
- compose 网络固定为 `172.28.0.0/16`，在 `TRUSTED_PROXIES=172.16.0.0/12` 的范围内。改网段时两处一起改，否则所有访客会按 Caddy 的地址一起被登录限流。

第一次启动后打开 `https://<你的域名>/admin/` 登录。首页有默认的欢迎文字，文章列表里有一篇示例草稿。

### 在别的机器上构建，拷到服务器上启动

在 2 GB 内存的服务器上编译 Rust 容易内存不足：在 4 核机器上构建时，内存占用最多增加约 1.3 GB。可以在另一台 x86_64 的机器上构建镜像，再拷到服务器上。运行时 cat-blog 空闲约 9 MB，有少量访问时约 30 MB，Caddy 约 15–30 MB，2 核 2 GB 的服务器够用。

在构建机上：

```sh
git clone https://github.com/zht7063/cat-blog.git
cd cat-blog
git checkout v1.0.0
docker build -t cat-blog:v1.0.0 -t cat-blog:latest .
docker save cat-blog:v1.0.0 cat-blog:latest | gzip > cat-blog-v1.0.0.tar.gz   # 约 35 MB
scp cat-blog-v1.0.0.tar.gz user@your-vps:
```

在服务器上：

```sh
docker load -i cat-blog-v1.0.0.tar.gz        # 载入 cat-blog:v1.0.0 和 cat-blog:latest
git clone --branch v1.0.0 https://github.com/zht7063/cat-blog.git   # 只用到 deploy/ 和 .env.example
cd cat-blog/deploy
cp ../.env.example .env && chmod 600 .env   # 填好 ADMIN_PASSWORD、BASE_URL
docker compose up -d                        # 不加 --build：直接用载入的 cat-blog:latest
curl -s https://<你的域名>/healthz          # 返回 ok
```

`docker-compose.yml` 里 cat-blog 服务的镜像名是 `cat-blog:latest`，本机已有这个镜像时 `docker compose up -d` 不构建；没有时先尝试拉取（提示 `pull access denied`，可以忽略）再从源码构建；`--build` 总是重新构建。镜像不发布到任何镜像仓库，只在自己的机器之间拷贝。

## 不用 Docker 部署

```sh
cargo build --release --locked
sudo install -m 755 target/release/cat-blog /usr/local/bin/
sudo useradd --system --home-dir /var/lib/cat-blog --create-home cat-blog
sudo install -o cat-blog -g cat-blog -m 600 .env /var/lib/cat-blog/.env
sudo cp deploy/cat-blog.service /etc/systemd/system/
sudo systemctl enable --now cat-blog
```

服务在 `/var/lib/cat-blog` 里运行，读这里的 `.env`，数据库在 `/var/lib/cat-blog/data/cat-blog.db`。前面用 Caddy 时，Caddyfile 写：

```
blog.example.com {
	reverse_proxy 127.0.0.1:3000
}
```

下面的命令在这种部署下都要以 `cat-blog` 用户在 `/var/lib/cat-blog` 里执行，比如 `cd /var/lib/cat-blog && sudo -u cat-blog cat-blog backup ...`。

## 备份与恢复

不要直接复制数据库文件：服务运行时，最近写入的数据可能还在 `-wal` 文件里。用 `backup` 命令，服务运行时也可以执行：

```sh
cat-blog backup /path/to/cat-blog-2026-11-24.db
# Docker：
docker compose exec cat-blog cat-blog backup /data/backups/cat-blog-2026-11-24.db
```

- 得到一份一致的单文件副本，不需要 `-wal`、`-shm` 文件。命令会检查副本的完整性，不通过时以非零状态码退出。
- 目标文件已存在时报错，不会覆盖。
- 副本里有密码哈希和会话哈希，权限设为 600。Docker 下副本在 `data` 卷里，用 `docker compose cp cat-blog:/data/backups/cat-blog-2026-11-24.db .` 取到宿主机。

恢复：

1. 停止服务：`docker compose stop cat-blog`，或 `sudo systemctl stop cat-blog`。
2. 用副本替换数据库文件，删除旧的 `cat-blog.db-wal`、`cat-blog.db-shm`，文件属主仍是 `cat-blog`：
   ```sh
   cd /var/lib/cat-blog/data
   sudo rm -f cat-blog.db-wal cat-blog.db-shm
   sudo install -o cat-blog -g cat-blog -m 600 /path/to/cat-blog-2026-11-24.db cat-blog.db
   ```
   Docker 下数据在卷里，借一个临时容器：
   ```sh
   docker compose run --rm --no-deps --user root \
     -v "$PWD/cat-blog-2026-11-24.db:/restore.db:ro" cat-blog \
     sh -c 'rm -f /data/cat-blog.db-wal /data/cat-blog.db-shm &&
            install -o cat-blog -g cat-blog -m 600 /restore.db /data/cat-blog.db'
   ```
3. 启动服务：`docker compose start cat-blog`，或 `sudo systemctl start cat-blog`。

## 升级

```sh
git pull
cd deploy && docker compose up -d --build     # 或重新 cargo build、安装后 systemctl restart cat-blog
```

在别的机器上构建的：按“在别的机器上构建，拷到服务器上启动”一节，用新版本的标签构建、导出、在服务器上 `docker load`，再在服务器的仓库里执行 `git fetch --tags && git checkout <新标签>`，然后在 `deploy/` 里执行 `docker compose up -d`（不加 `--build`），compose 发现 `cat-blog:latest` 变了会重建容器。

数据库迁移在服务启动时自动执行，所以 `rerender` 要等新版本的服务启动过一次再执行。正文是保存时渲染好存进库里的，升级后已有内容不会自动用上新的渲染规则（比如 Markdown 或清洗规则变了）。需要时执行：

```sh
cat-blog rerender
# Docker：
docker compose exec cat-blog cat-blog rerender
```

`rerender` 也用于改了首页的 `timezone` 之后：写了 `date` 的内容会按新时区重算发布时间。它不改原文和地址；某篇内容在新规则下出错时跳过它、打印原因，并以非零状态码退出，在后台改好那篇再保存即可。升级前建议先备份。

## 开发

```sh
git config core.hooksPath .githooks   # 启用提交信息检查
scripts/check.sh                      # 提交前的全套检查，CI 跑同一个脚本
```

需要 `sqlx-cli` 和 `cargo-deny`，版本见 `.github/workflows/ci.yml`。
