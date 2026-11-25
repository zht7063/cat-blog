//! 新增迁移文件时让 `sqlx::migrate!` 重新编译（见 sqlx 的 `migrate!` 文档）。

fn main() {
    println!("cargo:rerun-if-changed=../../migrations");
}
