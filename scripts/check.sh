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
