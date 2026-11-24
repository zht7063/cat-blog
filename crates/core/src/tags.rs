//! 文章标签的规范化（D46，docs/v1.0-spec.md §5.2）。

use std::fmt;

use unicode_normalization::UnicodeNormalization;

/// 单个标签的最大长度，按字符计。
pub const MAX_LEN: usize = 40;

/// 规范化之后的标签：去掉首尾空白、NFC、小写，非空且不超过 [`MAX_LEN`] 个字符。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Tag(String);

/// 标签不能使用的原因。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TagError {
    Empty,
    TooLong { tag: String, len: usize },
}

impl fmt::Display for TagError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("有一个标签是空的，请删掉它"),
            Self::TooLong { tag, len } => {
                write!(
                    f,
                    "标签 \"{tag}\" 有 {len} 个字符，超过 {MAX_LEN} 个，请缩短"
                )
            }
        }
    }
}

impl std::error::Error for TagError {}

impl Tag {
    pub fn parse(raw: &str) -> Result<Self, TagError> {
        let tag = normalize(raw);
        let len = tag.chars().count();
        if len == 0 {
            return Err(TagError::Empty);
        }
        if len > MAX_LEN {
            return Err(TagError::TooLong { tag, len });
        }
        Ok(Self(tag))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// D46 的规范化，也用于 `/blog/?tag=x` 的参数：去掉首尾空白，NFC，转小写。
pub fn normalize(raw: &str) -> String {
    raw.trim().nfc().collect::<String>().to_lowercase()
}

/// 逐个规范化，去掉重复的标签，保留第一次出现的顺序。
pub fn parse_list<'a>(raw: impl IntoIterator<Item = &'a str>) -> Result<Vec<Tag>, TagError> {
    let mut tags: Vec<Tag> = Vec::new();
    for r in raw {
        let tag = Tag::parse(r)?;
        if !tags.contains(&tag) {
            tags.push(tag);
        }
    }
    Ok(tags)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(tags: &[Tag]) -> Vec<&str> {
        tags.iter().map(Tag::as_str).collect()
    }

    #[test]
    fn tags_are_trimmed_lowercased_composed_and_deduplicated() {
        let tags = parse_list([" Rust ", "rust", "Caf\u{65}\u{301}", "caf\u{e9}", "生活"]);
        assert_eq!(
            tags.as_deref().map(strings),
            Ok(vec!["rust", "caf\u{e9}", "生活"])
        );
    }

    #[test]
    fn empty_and_too_long_tags_are_rejected() {
        assert_eq!(parse_list(["rust", "  "]), Err(TagError::Empty));
        assert!(Tag::parse(&"标".repeat(40)).is_ok());
        assert!(matches!(
            Tag::parse(&"标".repeat(41)),
            Err(TagError::TooLong { len: 41, .. })
        ));
    }

    #[test]
    fn query_parameter_normalizes_like_stored_tags() {
        assert_eq!(
            Tag::parse("Rust").map(|t| t.as_str().to_owned()),
            Ok("rust".to_owned())
        );
        assert_eq!(normalize(" RUST "), "rust");
    }
}
