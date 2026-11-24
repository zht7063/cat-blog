//! cat-blog 的纯逻辑：front matter、slug、日期、Markdown 渲染与清洗。
//!
//! 本 crate 不依赖 Web 框架、数据库和异步运行时，也不读取当前时间；
//! 需要时间的函数把 `now` 作为参数（docs/conventions.md §3）。
