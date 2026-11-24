# cat-blog

单人自部署的简洁博客程序（Rust、axum、SQLite），以后扩展为多用户托管服务。

`CLAUDE.md` 是指向本文件的软链接，只改本文件，不要把软链接换成普通文件。

## 先读

- [docs/conventions.md](docs/conventions.md)：开发约定。写代码、提交、评审都按它执行。
- [docs/v1.0-spec.md](docs/v1.0-spec.md)：v1.0 设计，和 [docs/v1.0-design.md](docs/v1.0-design.md)（D1–D38）冲突时以它为准。
- [docs/v1.0-implementation.md](docs/v1.0-implementation.md)：实施阶段和每个阶段的完成条件。

设计没覆盖的问题，先改设计文档，再写代码。

## 常用命令

```sh
scripts/check.sh                       # 全套检查，提交前必须通过（CI 跑同一个脚本）
cargo test --workspace                 # 只跑测试
cargo sqlx prepare --workspace         # 改了 SQL 后重新生成 .sqlx/ 并提交
git config core.hooksPath .githooks    # clone 后执行一次，启用 commit-msg 检查
```

## 规则与强制手段

| 规则 | 强制手段 | 详见 |
|---|---|---|
| `crates/core` 不依赖 axum、sqlx、tokio | `core/Cargo.toml` 不列这些依赖；`scripts/check-arch.sh` | conventions §3 |
| SQL 只写在 `crates/server/src/db/` | `clippy.toml` 禁用 sqlx 查询函数；`scripts/check-arch.sh` 检查查询宏 | §3、§4.2 |
| 内容查询都按 `blog_id` 过滤 | 只通过绑定了 `blog_id` 的句柄查询 | §3 |
| 当前时间从 `Clock` 取，`core` 把 `now` 作参数 | `clippy.toml` 禁用 `SystemTime::now` 等 | §3、§4.2 |
| 环境变量只在 `config.rs` 读 | `clippy.toml` 禁用 `std::env::var` 等 | §3、§4.2 |
| 不用 `unwrap`、`expect`、`dbg!`、`todo!`、`println!`、`unsafe` | `[workspace.lints]` | §4.1 |
| 例外用 `#[expect(..., reason = "...")]`，不用 `#[allow]` | `allow_attributes` lint | §4.4 |
| 依赖许可证兼容 AGPL-3.0，无已知漏洞 | `cargo deny` | §5.2 |
| `.sqlx/` 与查询同步 | `cargo sqlx prepare --check` | §5.1 |
| 提交信息用 Conventional Commits，scope 为 `core`/`server`/`deploy`/`docs` | `.githooks/commit-msg` | §6 |
| 错误信息描述失败状态；检查报错写明改用什么 | 评审 | §4.3、§7.1 |
| 不写为变通写法辩护的注释；保留公共 API 的 `///` 和 `// SAFETY:` | 评审 | §7.2 |

发现新的一类错误时，按纠错层级（架构 > 类型/lint/CI > 测试 > 文档）把它拦在能做到的最高一层，见 conventions §2。

## 开发时使用 pstack

用户已在 Amp 里安装了 pstack 插件。**开发时要用起来**：遇到下表的场景，先加载对应的 skill，再动手。本仓库不复制 pstack 的内容。

| 场景 | 用哪个 skill |
|---|---|
| 不确定该用哪个 skill 或 playbook | `poteto-help` |
| 做功能、修 bug、重构、做多阶段计划 | `poteto-mode`，按任务选它的 playbook（`feature`、`bug-fix`、`refactoring`、`multi-phase-plan`） |
| 提 PR、发布 | `poteto-mode` 的 `opening-a-pr`、`shipping` playbook；PR 描述和提交信息用 `technical-writing` |
| 被纠正，或同一个错误又犯了 | `correct`：按纠错层级把它变成写不出来或检查得到的错误（conventions §2）；`principle-encode-lessons-in-structure` |
| 写代码前设计类型和模块 | `architect`；`principle-type-system-discipline` |
| 弄清代码怎么工作、为什么这样设计 | `how`、`why` |
| 调试 | `principle-fix-root-causes` |
| 写或改测试 | `principle-test-behavior-not-implementation` |
| 清理注释 | `no-comments`，但以 conventions §7.2 为准：保留公共 API 的 `///` 和 `// SAFETY:` |
| 合并前评估影响面、做对抗评审 | `blast-radius`、`interrogate` |
| 宣布完成之前 | `principle-prove-it-works` |
| 长时间无人值守的工作 | `figure-it-out`、`show-me-your-work` |

pstack 的建议和本仓库的约定冲突时，以 [docs/conventions.md](docs/conventions.md) 为准。
