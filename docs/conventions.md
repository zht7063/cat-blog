# cat-blog 开发约定

> 状态：待审阅（2026-11-24）。
> 本文是写代码、提交和评审时要遵守的约定。人和 agent 都按本文执行。
> 标\"可否决\"的是先定下来的默认值，审阅者不同意时直接评论修改。

## 1. 文档优先级

1. [v1.0-spec.md](v1.0-spec.md)：v1.0 的设计，和 D1–D38 冲突时以它为准。
2. [v1.0-design.md](v1.0-design.md)：原始决定 D1–D38。
3. [v1.0-implementation.md](v1.0-implementation.md)：实施顺序和每个阶段的完成条件。
4. 本文：怎么写代码、怎么提交。

遇到设计没覆盖的问题，先改设计文档，再写代码。

## 2. 纠错层级

发现一类错误（自己犯的、评审指出的、agent 反复犯的）时，按下面的顺序找能拦住它的最高一层，**只在上一层做不到时才往下走**：

1. **架构**：让错误写不出来。例如 `core` 的 `Cargo.toml` 不依赖 `axum`、`sqlx`，`core` 里就不可能写出数据库代码。
2. **类型、lint、CI**：让错误编译不过或检查不过。错误信息要写明**该改用什么**（文件、类型或函数），见 §4.3。
3. **测试**：用测试锁住行为。
4. **文档**：只写需要判断力、机器检查不了的规则（比如本文 §7 的注释规则）。

新加一条检查时：

- 先证明它能拦住一个真实发生过的错误（或者手动造一个），再提交。
- 本地和 CI 跑同一条命令（§5.1）。
- 如果某种写法在代码里已经很多，检查先只拦新增的，不要求一次改完。

## 3. 架构边界

| 边界 | 规则 | 强制手段 |
|---|---|---|
| `core` 不依赖 Web 和数据库 | `crates/core` 不能直接或间接依赖 `axum`、`sqlx`、`tokio`、`tower`、`hyper` | `core/Cargo.toml` 不列这些依赖，代码里根本 `use` 不到；`scripts/check-arch.sh` 用 `cargo tree` 检查间接依赖（dev 依赖也算） |
| SQL 只在 `db` 模块 | 只有 `crates/server/src/db/` 能写 SQL | `clippy.toml` 禁用 sqlx 的查询函数（含 `raw_sql` 和 `Executor` 上直接执行字符串的方法），`db/mod.rs` 用到时在顶部用 `#![expect(...)]` 放行；`scripts/check-arch.sh` 检查 `db/` 以外的 sqlx 查询宏（原因见 §4.2） |
| 按 `blog_id` 过滤（D6） | 操作内容的查询只能通过绑定了 `blog_id` 的句柄调用，比如 `BlogDb { pool, blog_id }` 上的方法；不提供接收裸 `blog_id` 参数的公开查询函数 | 类型；评审 |
| 环境变量只在配置模块读 | 只有 `crates/server/src/config.rs` 读环境变量和 `.env`，其他代码从 `Config` 取值 | `clippy.toml` 禁用 `std::env::var` 等，`config.rs` 放行 |
| 当前时间从 `Clock` 取 | `server` 通过 `Clock` trait 取当前时间（生产用系统时钟，测试用可设定的时钟）；`core` 的函数把 `now` 作为参数，自己不取时间。这样定时发布（D12）、会话过期、登录限流都能在测试里控制时间 | `clippy.toml` 禁用 `SystemTime::now`、`Instant::now`、`jiff::Timestamp::now`、`jiff::ZonedDateTime::now`，`clock.rs` 放行 |
| 文件大小 | 单个 `.rs` 文件超过 500 行警告，超过 1000 行报错（可否决） | `scripts/check-arch.sh` |

## 4. Lint

### 4.1 `[workspace.lints]`

写在根 `Cargo.toml`，每个 crate 的 `Cargo.toml` 写 `[lints] workspace = true`。

```toml
[workspace.lints.rust]
unsafe_code = "forbid"

[workspace.lints.clippy]
unwrap_used = "deny"
expect_used = "deny"
dbg_macro = "deny"
todo = "deny"
print_stdout = "deny"
print_stderr = "deny"
allow_attributes = "deny"                  # 例外只能用 #[expect]，见 §4.4
allow_attributes_without_reason = "deny"
disallowed_methods = "deny"
disallowed_macros = "deny"
```

`unwrap_used`、`expect_used` 不是说不能处理\"不可能失败\"的情况，而是要求用 `?` 传播错误，或者在确实不可能失败的地方写 `#[expect(clippy::expect_used, reason = "...")]` 说明为什么不会失败。

pedantic 组先不开（可否决）。

### 4.2 `clippy.toml`

放在仓库根目录。`reason` 写明该改用什么，clippy 报错时会把它显示出来。

```toml
allow-unwrap-in-tests = true
allow-expect-in-tests = true
allow-print-in-tests = true

disallowed-methods = [
  { path = "std::time::SystemTime::now", reason = "用 Clock::now()（crates/server/src/clock.rs）；core 里把 now 作为参数传入" },
  { path = "std::time::Instant::now", reason = "用 Clock::now()（crates/server/src/clock.rs），测试里才能控制时间" },
  { path = "jiff::Timestamp::now", reason = "用 Clock::now()（crates/server/src/clock.rs）；core 里把 now 作为参数传入" },
  { path = "jiff::ZonedDateTime::now", reason = "用 Clock::now() 再按站点时区转换（crates/server/src/clock.rs）" },
  { path = "std::env::var", reason = "只在 crates/server/src/config.rs 读环境变量，其他地方从 Config 取值" },
  { path = "std::env::var_os", reason = "只在 crates/server/src/config.rs 读环境变量，其他地方从 Config 取值" },
  { path = "std::env::vars", reason = "只在 crates/server/src/config.rs 读环境变量，其他地方从 Config 取值" },
  { path = "dotenvy::dotenv", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::dotenv_override", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::dotenv_iter", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_path", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_path_override", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_path_iter", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_filename", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_filename_override", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_filename_iter", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_read", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_read_override", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::from_read_iter", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::var", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "dotenvy::vars", reason = "只在 crates/server/src/config.rs 加载 .env 和读环境变量" },
  { path = "sqlx::query", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::query_as", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::query_scalar", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::raw_sql", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::execute", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::execute_many", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::fetch", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::fetch_many", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::fetch_all", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::fetch_one", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::fetch_optional", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::prepare", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::prepare_with", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
  { path = "sqlx::Executor::describe", reason = "SQL 只写在 crates/server/src/db/，在那里加函数再调用" },
]
```

放行的写法，以 `db/mod.rs` 为例：

```rust
#![expect(clippy::disallowed_methods, reason = "db 模块是唯一写 SQL 的地方（docs/conventions.md §3）")]
```

阶段 0 实测（Rust 1.100、clippy 1.100、sqlx 0.9.1）的结果：

- `disallowed-macros` 能按路径拦住 `sqlx::query_scalar!` 这类宏，但报错挂在 crate 根上：模块、函数、语句上的 `#[expect]` 都放行不了，只有 lib.rs 的 `#![expect]` 可以，那样整个 crate 都放行了。所以 `clippy.toml` 不列 sqlx 的宏，改由 `scripts/check-arch.sh` 检查 `crates/server/src/db/` 以外的 `query…!` 宏调用和对它们的 `use`（含 `as` 别名），注释行不算。
- 同名的函数和宏互不影响：不列宏时，宏展开后的代码不会触发 `disallowed-methods` 里的 `sqlx::query` 等函数。
- `jiff` 1.0 把 `Zoned` 改名为 `ZonedDateTime`。路径写错时 clippy 只打一条警告（`does not refer to a reachable function`），`-D warnings` 不会把它变成错误，这条规则就悄悄失效了。所以 `scripts/check.sh` 在 clippy 输出里见到这类警告时报错。

### 4.3 检查的错误信息

所有自写的检查（脚本、hook、`clippy.toml` 的 `reason`）报错时都要回答三个问题，按这个格式输出：

```
Violation: crates/core 间接依赖了 tokio（经由 foo → bar）
Fix: 去掉 crates/core/Cargo.toml 里的 foo，或把用到它的代码移到 crates/server
See: docs/conventions.md §3
```

只写\"不允许\"而不写改法的检查不合格。

### 4.4 例外

- 一律用 `#[expect(lint, reason = "为什么这里可以例外")]`，不用 `#[allow]`（lint 会拒绝）。`expect` 在例外不再需要时会报警，提醒删掉。
- 加在出问题的那一行或那个最小的项上，不加在整个模块上。只有\"整个模块就是例外\"时（如 §3 的 `db`、`config`、`clock`）才用 `#![expect]`。
- 新增例外要在 PR 描述里列出，由评审人认可。不写过期日期（可否决）。

## 5. 检查命令与 CI

### 5.1 一条命令跑全套检查

```sh
scripts/check.sh
```

本地和 CI 跑的是同一个脚本。提交前跑一遍，全部通过再提交（可否决：选 `scripts/check.sh` 而不是 `just check`，是为了不多装一个工具）。

它依次执行：

| 步骤 | 命令 |
|---|---|
| 格式 | `cargo fmt --all --check` |
| lint | `cargo clippy --workspace --all-targets --locked -- -D warnings`；输出里有 `clippy.toml` 路径失效的警告时也报错（§4.2） |
| 测试 | `cargo test --workspace --locked` |
| sqlx 离线数据是否最新 | 用临时 SQLite 执行迁移后跑 `cargo sqlx prepare --workspace --check` |
| 依赖许可证与漏洞 | `cargo deny --locked check` |
| 架构检查 | `scripts/check-arch.sh`（`core` 的间接依赖、`db/` 以外的 sqlx 查询宏、文件大小） |

`.sqlx/` 过期时的改法：`cargo sqlx prepare --workspace`，把生成的文件一起提交。

### 5.2 `cargo deny`

`deny.toml` 的默认值（可否决）：

- **许可证**：只允许和 AGPL-3.0（D29）兼容的 `MIT`、`Apache-2.0`、`BSD-2-Clause`、`BSD-3-Clause`、`ISC`、`Zlib`、`Unicode-3.0`、`MPL-2.0`。新依赖用了别的许可证时，先确认兼容，再加进列表。
- **漏洞**：RustSec 公告中的漏洞一律报错。无人维护（unmaintained）的公告只对直接依赖报错。
- **来源**：只允许 crates.io，不允许 git 依赖。
- **重复版本**：只警告，不报错。
- **通配版本**：依赖不能写 `*` 版本。

### 5.3 CI

`.github/workflows/ci.yml` 在 push 和 PR 时运行 `scripts/check.sh`，并设置 `SQLX_OFFLINE=true`。工具安装（`sqlx-cli`、`cargo-deny`）写在 workflow 里，版本固定。

## 6. 提交与 PR

### 6.1 提交信息

用 [Conventional Commits](https://www.conventionalcommits.org/)：

```
type(scope): subject

正文（可选）
```

- **type**：`feat`、`fix`、`docs`、`refactor`、`perf`、`test`、`build`、`ci`、`chore`、`deps`、`revert`。
- **scope**：`core`、`server`、`deploy`、`docs`。改动跨多个范围或在仓库根目录时可以不写（可否决）。
- 破坏性改动在冒号前加 `!`，如 `feat(core)!: ...`。

以下是未定细节的默认值，都可否决：

| 细节 | 默认 |
|---|---|
| subject 的语言 | 英文 |
| subject 首字母 | 小写（和已有的 `docs: record v1.0 design decisions` 一致） |
| subject 语气 | 祈使句，如 `add`、`fix`，不写 `added` |
| subject 结尾 | 不加句号 |
| 首行长度 | 不超过 72 个字符 |
| 正文 | 可选；写的话和首行之间空一行，说明为什么改，每行不超过 100 个字符；中英文都可以 |

### 6.2 commit-msg hook

`.githooks/commit-msg` 检查上面的规则（语言和语气除外），clone 后执行一次：

```sh
git config core.hooksPath .githooks
```

hook 内容见附录 A。git 自动生成的 `Merge`、`Revert`、`fixup!`、`squash!` 提交不检查。

### 6.3 提交与 PR 的粒度

- 一个提交只做一件事，单独拿出来也能通过 `scripts/check.sh`。重构和行为改动分成两个提交。
- 在 `main` 上开短命分支，PR 尽量小。PR 标题和提交首行格式相同。
- PR 描述（可否决）：为什么改、改了什么、取舍、影响范围、**怎么验证的**（命令和结果，界面改动附截图）。

## 7. 代码风格

### 7.1 错误信息

描述**失败时的状态**，不描述期望：

- 写 `session not found`、`slug \"admin\" is reserved`，不写 `session must exist`、`slug must not be reserved`。
- 给作者看的错误（front matter、slug）用中文，带行号，说明怎么改。日志和内部错误用英文（可否决）。

### 7.2 注释

- **禁止**：为变通写法辩护的注释（\"这里这样写是因为……\"）。先试着把代码改到不需要解释；改不了，就把限制放进类型、函数名或测试里。
- **保留**：公共 API 的 `///` 文档注释；`// SAFETY:`（`unsafe_code` 已经 forbid，真要例外时必须写）。
- 其他注释照常写，但不要复述代码在做什么。

### 7.3 测试

- 测行为，不测实现：通过公开接口断言结果，不断言私有函数、调用顺序或 mock 交互。
- 预期值独立得出，不从被测代码里抄。
- 时间相关的测试用固定的 `Clock` 或传入的 `now`，不 sleep。

### 7.4 格式

用 rustfmt 默认配置，不加 `rustfmt.toml`（可否决）。

## 8. 落地状态

§4 的 `Cargo.toml` 和 `clippy.toml` 配置、`scripts/check.sh`、`scripts/check-arch.sh`、`deny.toml`、CI、`.githooks/commit-msg` 已在实施文档的**阶段 0** 建立。和本文最初草稿的差别见 §4.2 末尾的实测结果。

## 附录 A：`.githooks/commit-msg`

```bash
#!/usr/bin/env bash
# 校验提交信息格式，规则见 docs/conventions.md §6。
set -euo pipefail
export LC_ALL=C.UTF-8  # 按字符而不是字节计算长度

msg_file="$1"
header="$(sed -n '1p' "$msg_file")"

# git 自动生成的提交信息不检查。
case "$header" in
  Merge\ *|Revert\ \"*|fixup!\ *|squash!\ *|amend!\ *) exit 0 ;;
esac

fail() {
  printf 'Violation: %s\nFix: %s\nSee: docs/conventions.md §6\n' "$1" "$2" >&2
  exit 1
}

types='feat|fix|docs|refactor|perf|test|build|ci|chore|deps|revert'
scopes='core|server|deploy|docs'

if ! printf '%s' "$header" | grep -Eq "^($types)(\(($scopes)\))?!?: .+"; then
  fail "首行不符合 \"type(scope): subject\"：$header" \
       "type 取 $types；scope 可省略，写时取 $scopes"
fi

subject="${header#*: }"
case "$subject" in
  [A-Z]*) fail "subject 以大写字母开头：$subject" "改成小写开头，例如 \"add slug normalization\"" ;;
  *.|*。) fail "subject 以句号结尾：$subject" "去掉末尾的句号" ;;
esac

if [ "${#header}" -gt 72 ]; then
  fail "首行有 ${#header} 个字符，超过 72" "缩短 subject，细节写进正文"
fi

if [ -n "$(sed -n '2p' "$msg_file")" ]; then
  fail "首行和正文之间没有空行" "在首行后加一个空行"
fi

long_line="$(grep -v '^#' "$msg_file" | awk 'length > 100 { print; exit }')"
if [ -n "$long_line" ]; then
  fail "有一行超过 100 个字符：$long_line" "正文按 100 字符以内换行（链接可以单独成行，但也不能超过 100）"
fi
```

## 附录 B：`scripts/check.sh`

```bash
#!/usr/bin/env bash
# 本地和 CI 共用的全套检查，见 docs/conventions.md §5。
set -euo pipefail
cd "$(dirname "$0")/.."

step() { printf '\n==> %s\n' "$*"; }

export SQLX_OFFLINE=true
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

step "fmt"
cargo fmt --all --check

step "clippy"
cargo clippy --workspace --all-targets --locked -- -D warnings 2>&1 | tee "$tmp/clippy.log"
if grep -q 'does not refer to a reachable' "$tmp/clippy.log"; then
  printf 'Violation: clippy.toml 里有路径找不到对应的函数，这条规则没有生效（见上面的 warning）\n' >&2
  printf 'Fix: 按依赖当前版本的 API 改正 clippy.toml 里的路径\nSee: docs/conventions.md §4.2\n' >&2
  exit 1
fi

step "test"
cargo test --workspace --locked

step "sqlx prepare --check"
export DATABASE_URL="sqlite://$tmp/check.db"
cargo sqlx database setup --source migrations
SQLX_OFFLINE=false cargo sqlx prepare --workspace --check

step "cargo deny"
cargo deny --locked check

step "architecture"
scripts/check-arch.sh

printf '\nAll checks passed.\n'
```
