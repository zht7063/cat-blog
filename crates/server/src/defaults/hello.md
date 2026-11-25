---
title: Hello, cat-blog
draft: true # 改成 false 就发布了
# date: 2026-11-24 09:30  # 不写时以第一次发布的时间为准；写成未来的时间就是定时发布
tags: [cat-blog]
description: 第一篇示例文章
---

这是一篇示例草稿，演示 front matter 的写法。

- `title`：标题。第一次保存时会用它生成 `slug`，写回到上面；之后改标题不会改地址。
- `draft`：`true` 是草稿，只有登录后才能看到。
- `date`：发布时间，可以写 `2026-11-24`、`2026-11-24 09:30` 或带时区的 `2026-11-24T09:30:00+08:00`。
- `tags`：标签列表，读者可以在 `/blog/?tag=cat-blog` 按标签筛选。
- `description`：用于搜索引擎和 RSS 的简介。

正文用 Markdown 写：**粗体**、[链接](https://example.com)、表格、脚注[^1]、任务列表和代码块都可以。

```rust
fn main() {
    println!("ฅ^•ﻌ•^ฅ");
}
```

[^1]: 这是一个脚注。
