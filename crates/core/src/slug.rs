//! slug 的规范化、生成和保留路径（docs/v1.0-spec.md §5.3，D32）。

use std::fmt;

use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

/// slug 的最大长度，按字符计。
pub const MAX_LEN: usize = 80;

/// 和程序路由重名、不能用作 slug 的路径，按 [`normalize`] 之后的形式比较。
const RESERVED: [&str; 8] = [
    "blog",
    "admin",
    "static",
    "feed.xml",
    "sitemap.xml",
    "robots.txt",
    "favicon.ico",
    "healthz",
];

/// 规范化之后的 slug：非空、不超过 [`MAX_LEN`] 个字符、不是保留路径。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Slug(String);

/// slug 不能使用的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugError {
    /// 规范化之后什么都不剩（比如全是标点）。
    Empty,
    /// 手写的 slug 规范化之后超过 [`MAX_LEN`] 个字符。
    TooLong { len: usize },
    /// 和保留路径重名。
    Reserved(String),
}

impl fmt::Display for SlugError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("slug 规范化后为空，请写一个包含字母或数字的 slug"),
            Self::TooLong { len } => {
                write!(f, "slug 有 {len} 个字符，超过 {MAX_LEN} 个，请缩短")
            }
            Self::Reserved(s) => write!(f, "slug \"{s}\" 和程序的保留路径重名，请换一个"),
        }
    }
}

impl std::error::Error for SlugError {}

impl Slug {
    /// 把作者手写的 slug 规范化，超过 [`MAX_LEN`] 时报错。
    pub fn parse(raw: &str) -> Result<Self, SlugError> {
        let s = normalize(raw);
        let len = s.chars().count();
        if len > MAX_LEN {
            return Err(SlugError::TooLong { len });
        }
        Self::checked(s)
    }

    /// 用标题生成 slug，超过 [`MAX_LEN`] 时截断，再去掉末尾的 `-`。
    pub fn from_title(title: &str) -> Result<Self, SlugError> {
        let s: String = normalize(title).chars().take(MAX_LEN).collect();
        Self::checked(s.trim_end_matches('-').to_owned())
    }

    fn checked(s: String) -> Result<Self, SlugError> {
        if s.is_empty() {
            return Err(SlugError::Empty);
        }
        if is_reserved(&s) {
            return Err(SlugError::Reserved(s));
        }
        Ok(Self(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// 站内地址 `/{slug}/`，slug 做了百分号编码（docs/v1.0-spec.md §8.2）。
    pub fn path(&self) -> String {
        format!("/{}/", percent_encode(&self.0))
    }
}

/// 请求路径里的一段（已百分号解码）按 D32 规范化成查库用的形式：NFC，ASCII 字母转小写。
/// 结果和库里的 slug 相同时就是同一篇内容。
pub fn lookup_key(segment: &str) -> String {
    segment.nfc().map(|c| c.to_ascii_lowercase()).collect()
}

/// 把 slug 或标签放进 URL 时的百分号编码：ASCII 字母、数字和 `-` 原样保留，其余按 UTF-8 字节编码。
pub fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b == b'-' {
            out.push(char::from(b));
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// [`percent_encode`] 的逆过程。`%` 后面不是两位十六进制数，或解码后不是 UTF-8 时返回 `None`。
pub fn percent_decode(s: &str) -> Option<String> {
    let mut bytes = Vec::with_capacity(s.len());
    let mut rest = s.as_bytes();
    while let Some((&b, tail)) = rest.split_first() {
        if b == b'%' {
            let hex = std::str::from_utf8(tail.get(..2)?).ok()?;
            bytes.push(u8::from_str_radix(hex, 16).ok()?);
            rest = &tail[2..];
        } else {
            bytes.push(b);
            rest = tail;
        }
    }
    String::from_utf8(bytes).ok()
}

impl fmt::Display for Slug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// D32 的规范化：NFC，保留任何语言的字母和数字（含紧跟其后的组合符号，如天城文的元音符号），
/// ASCII 字母转小写，其余字符替换为 `-` 并合并，去掉首尾的 `-`。
pub fn normalize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.nfc() {
        if c.is_alphanumeric() || (is_combining_mark(c) && !out.is_empty() && !out.ends_with('-')) {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    if out.ends_with('-') {
        out.pop();
    }
    out
}

fn is_reserved(normalized: &str) -> bool {
    RESERVED.iter().any(|r| normalize(r) == normalized)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parsed(raw: &str) -> Result<String, SlugError> {
        Slug::parse(raw).map(|s| s.as_str().to_owned())
    }

    fn from_title(title: &str) -> Result<String, SlugError> {
        Slug::from_title(title).map(|s| s.as_str().to_owned())
    }

    #[test]
    fn title_example_from_spec() {
        assert_eq!(from_title("我的 Rust 笔记！"), Ok("我的-rust-笔记".into()));
    }

    #[test]
    fn full_width_punctuation_becomes_separator() {
        assert_eq!(
            from_title("你好，世界。（测试）"),
            Ok("你好-世界-测试".into())
        );
        assert_eq!(from_title("Rust：入门＆实践"), Ok("rust-入门-实践".into()));
    }

    #[test]
    fn nfd_input_is_composed() {
        let nfd = "Cafe\u{301} Del Mar";
        assert_eq!(from_title(nfd), Ok("caf\u{e9}-del-mar".into()));
        assert_eq!(Slug::parse(nfd), Slug::parse("Caf\u{e9} del mar"));
    }

    #[test]
    fn combining_marks_stay_inside_words() {
        assert_eq!(from_title("नमस्ते दुनिया"), Ok("नमस्ते-दुनिया".into()));
        assert_eq!(from_title("ที่นี่ ไทย"), Ok("ที่นี่-ไทย".into()));
    }

    #[test]
    fn runs_of_punctuation_merge_into_one_dash() {
        assert_eq!(
            from_title("  hello -- __ world!!! "),
            Ok("hello-world".into())
        );
        assert_eq!(parsed("a...b///c"), Ok("a-b-c".into()));
    }

    #[test]
    fn only_punctuation_is_empty() {
        assert_eq!(normalize("！？…——"), "");
        assert_eq!(from_title("!!! ???"), Err(SlugError::Empty));
        assert_eq!(parsed("---"), Err(SlugError::Empty));
    }

    #[test]
    fn long_title_is_truncated_without_trailing_dash() {
        let title = format!("{} {}", "a".repeat(79), "bcd");
        let slug = from_title(&title);
        assert_eq!(slug, Ok("a".repeat(79)));

        let title = "字".repeat(100);
        assert_eq!(from_title(&title), Ok("字".repeat(80)));
    }

    #[test]
    fn long_handwritten_slug_is_rejected() {
        assert_eq!(parsed(&"x".repeat(80)), Ok("x".repeat(80)));
        assert_eq!(parsed(&"x".repeat(81)), Err(SlugError::TooLong { len: 81 }));
    }

    #[test]
    fn reserved_paths_are_rejected_in_any_case() {
        for raw in [
            "blog",
            "Admin",
            "ADMIN",
            "Feed.XML",
            "feed.xml",
            "healthz",
            "Static",
            "robots.txt",
        ] {
            assert!(
                matches!(Slug::parse(raw), Err(SlugError::Reserved(_))),
                "{raw} should be reserved"
            );
        }
        assert!(matches!(
            Slug::from_title("Admin"),
            Err(SlugError::Reserved(_))
        ));
        assert_eq!(parsed("admin-notes"), Ok("admin-notes".into()));
        assert_eq!(parsed("blogs"), Ok("blogs".into()));
    }

    #[test]
    fn path_percent_encodes_non_ascii() {
        let slug = Slug::parse("我的 Rust 笔记").unwrap();
        assert_eq!(slug.path(), "/%E6%88%91%E7%9A%84-rust-%E7%AC%94%E8%AE%B0/");
        assert_eq!(percent_encode("c++ & a/b"), "c%2B%2B%20%26%20a%2Fb");
    }

    #[test]
    fn percent_decode_reverses_encode() {
        for s in ["我的-rust-笔记", "c++ & a/b", "plain"] {
            assert_eq!(percent_decode(&percent_encode(s)).as_deref(), Some(s));
        }
        assert_eq!(percent_decode("%E6%88%91").as_deref(), Some("我"));
        assert_eq!(percent_decode("%E6%88"), None);
        assert_eq!(percent_decode("%zz"), None);
        assert_eq!(percent_decode("a%2"), None);
    }

    #[test]
    fn lookup_key_matches_stored_slug() {
        let stored = Slug::parse("Café Notes").unwrap();
        assert_eq!(lookup_key("Cafe\u{301}-NOTES"), stored.as_str());
        assert_eq!(lookup_key("我的-Rust-笔记"), "我的-rust-笔记");
        assert_ne!(lookup_key("café_notes"), stored.as_str());
    }
}
