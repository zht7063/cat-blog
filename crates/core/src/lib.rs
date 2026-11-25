//! cat-blog 的纯逻辑：front matter、slug、日期、Markdown 渲染与清洗。
//!
//! 本 crate 不依赖 Web 框架、数据库和异步运行时，也不读取当前时间；
//! 需要时间的函数把 `now` 作为参数（docs/conventions.md §3）。

pub mod date;
pub mod directive;
pub mod frontmatter;
pub mod i18n;
pub mod markdown;
pub mod sanitize;
pub mod save;
pub mod slug;
pub mod tags;
pub mod visibility;

use std::fmt;

/// 作者写的原文有问题，保存失败。`line` 是原文中的行号（从 1 开始），定位不到时为空。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentError {
    pub line: Option<usize>,
    pub kind: ContentErrorKind,
}

/// [`ContentError`] 的种类。`Display` 给作者看，用中文并说明怎么改（docs/conventions.md §7.1）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContentErrorKind {
    /// 第一行不是 `---`。
    MissingFrontMatter,
    /// 找不到结束 front matter 的 `---`。
    UnclosedFrontMatter,
    /// YAML 语法错误等无法归类的问题，内容是 YAML 库的原始说明。
    Yaml(String),
    UnknownField {
        field: String,
        allowed: &'static [&'static str],
    },
    InvalidType {
        field: String,
        expected: &'static str,
    },
    MissingTitle,
    /// 标题规范化后为空，又没有手写 slug。
    TitleWithoutSlug,
    TagsOnPage,
    /// `tags` 写成了单个字符串，内容是那个字符串。
    TagsNotList(String),
    Tag(tags::TagError),
    Date(date::DateError),
    Slug(slug::SlugError),
    /// slug 写成了多行或其他复杂 YAML，无法只改一行写回。
    MultilineSlug,
    /// front matter 的写法特殊（比如带引号的键名），写回 slug 后再解析得不到这个 slug。
    SlugWriteBack(String),
    Directive(directive::DirectiveError),
    /// `nav` 里写了标题、列表等块级内容。
    NavNotInline,
}

impl ContentError {
    pub(crate) fn new(line: Option<usize>, kind: ContentErrorKind) -> Self {
        Self { line, kind }
    }
}

impl fmt::Display for ContentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(line) = self.line {
            write!(f, "第 {line} 行：")?;
        }
        write!(f, "{}", self.kind)
    }
}

impl fmt::Display for ContentErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingFrontMatter => {
                f.write_str("原文没有以 --- 开头，请在第一行写 ---，下面写 front matter")
            }
            Self::UnclosedFrontMatter => {
                f.write_str("front matter 没有结束，请在字段后面单独写一行 ---")
            }
            Self::Yaml(detail) => write!(
                f,
                "front matter 的 YAML 语法有误（{detail}），请检查这一行的缩进、冒号和引号"
            ),
            Self::UnknownField { field, allowed } => {
                write!(f, "未知字段 {field}，可用的字段：{}", allowed.join("、"))
            }
            Self::InvalidType { field, expected } => {
                write!(f, "字段 {field} 的类型不对，应为{expected}")
            }
            Self::MissingTitle => f.write_str("缺少 title，请填写标题"),
            Self::TitleWithoutSlug => {
                f.write_str("标题里没有字母或数字，无法生成 slug，请手写 slug")
            }
            Self::TagsOnPage => f.write_str("页面不能写 tags，只有文章可以；请删掉这一行"),
            Self::TagsNotList(s) => write!(f, "tags 要写成列表，请改成 tags: [{s}]"),
            Self::Tag(e) => e.fmt(f),
            Self::Date(e) => e.fmt(f),
            Self::Slug(e) => e.fmt(f),
            Self::MultilineSlug => {
                f.write_str("slug 写成了多行，请改成单行，比如 slug: \"my-post\"")
            }
            Self::SlugWriteBack(slug) => write!(
                f,
                "无法把 slug 自动写回 front matter，请在 title 下面手动加一行 slug: \"{slug}\""
            ),
            Self::Directive(e) => e.fmt(f),
            Self::NavNotInline => f.write_str(
                "nav 只能写链接和文字，不能写标题、列表或代码块，比如 nav: \"[Home](/) [Blog](/blog/)\"",
            ),
        }
    }
}

impl std::error::Error for ContentError {}
