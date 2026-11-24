#!/usr/bin/env bash
# 架构检查，规则见 docs/conventions.md §3。
set -euo pipefail
cd "$(dirname "$0")/.."

failed=0

violation() {
  printf 'Violation: %s\nFix: %s\nSee: %s\n\n' "$1" "$2" "$3" >&2
  failed=1
}

# crates/core 不能直接或间接依赖 Web 框架、数据库和异步运行时，dev 依赖也算。
tree="$(cargo tree -p cat-blog-core --prefix depth --format '{p}')"
forbidden='axum sqlx tokio tower hyper'
while IFS='|' read -r crate path direct; do
  [ -n "$crate" ] || continue
  if [ -z "$path" ]; then
    how="直接依赖了 $crate"
  else
    how="间接依赖了 $crate（经由 $path）"
  fi
  violation "crates/core $how" \
    "去掉 crates/core/Cargo.toml 里的 $direct，或把用到它的代码移到 crates/server" \
    "docs/conventions.md §3"
done < <(
  printf '%s\n' "$tree" |
    awk -v forbidden="$forbidden" '
      BEGIN { n = split(forbidden, f, " "); for (i = 1; i <= n; i++) bad[f[i]] = 1 }
      {
        match($0, /^[0-9]+/)
        depth = substr($0, 1, RLENGTH) + 0
        split(substr($0, RLENGTH + 1), parts, " ")
        stack[depth] = parts[1]
        if (depth > 0 && (parts[1] in bad) && !(parts[1] in seen)) {
          seen[parts[1]] = 1
          path = ""
          for (i = 1; i < depth; i++) path = path (i > 1 ? " → " : "") stack[i]
          print parts[1] "|" path "|" stack[1]
        }
      }'
)

# SQL 只写在 crates/server/src/db/。sqlx 的查询宏在这里查，查询函数由 clippy.toml 查
# （docs/conventions.md §4.2）：宏调用、引入（含 `as` 别名）都算，注释行不算。
sql_macro='\b(query[a-z_]*|raw_sql)(\s*!\s*[([{]|\s+as\s)|use sqlx::.*\b(query[a-z_]*|raw_sql)\b'
while IFS= read -r hit; do
  violation "在 crates/server/src/db/ 以外用了 sqlx 查询：$hit" \
    "把这条 SQL 移到 crates/server/src/db/ 里的函数，在这里调用那个函数" \
    "docs/conventions.md §3"
done < <(
  grep -rnE --include='*.rs' "$sql_macro" crates |
    grep -vE '^crates/server/src/db/|^[^:]+:[0-9]+:\s*//' || true
)

# 单个 .rs 文件超过 500 行警告，超过 1000 行报错。
while IFS= read -r file; do
  lines="$(wc -l < "$file")"
  if [ "$lines" -gt 1000 ]; then
    violation "$file 有 $lines 行，超过 1000 行" \
      "按职责把它拆成几个模块，每个不超过 500 行" \
      "docs/conventions.md §3"
  elif [ "$lines" -gt 500 ]; then
    printf 'Warning: %s 有 %s 行，超过 500 行\nFix: 考虑按职责拆分模块\nSee: docs/conventions.md §3\n\n' \
      "$file" "$lines" >&2
  fi
done < <(find crates -name '*.rs' -not -path '*/target/*')

exit "$failed"
