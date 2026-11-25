//! `{{ posts }}` 指令的语法和占位注释（D55，docs/v1.0-spec.md §7.3）。

use std::fmt;
use std::fmt::Write as _;
use std::num::NonZeroU32;

use crate::slug::percent_decode;
use crate::tags::{Tag, TagError};

/// 一条 `{{ posts ... }}`：列出可见文章，可按标签筛选、限制条数。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PostsDirective {
    pub limit: Option<NonZeroU32>,
    pub tag: Option<Tag>,
}

/// 指令的参数写错了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DirectiveError {
    /// 参数不是 `名字:值` 的形式。
    Malformed(String),
    UnknownParam(String),
    DuplicateParam(String),
    InvalidLimit(String),
    Tag(TagError),
}

impl fmt::Display for DirectiveError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Malformed(p) => write!(
                f,
                "{{{{ posts }}}} 的参数 \"{p}\" 写错了，请写成 limit:5 或 tag:rust 这样，冒号前后不加空格"
            ),
            Self::UnknownParam(p) => {
                write!(f, "{{{{ posts }}}} 没有参数 {p}，只能用 limit 和 tag")
            }
            Self::DuplicateParam(p) => write!(f, "{{{{ posts }}}} 的参数 {p} 写了两次，请删掉一个"),
            Self::InvalidLimit(v) => {
                write!(
                    f,
                    "{{{{ posts }}}} 的 limit \"{v}\" 不是正整数，请写成 limit:5 这样"
                )
            }
            Self::Tag(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for DirectiveError {}

const MARKER_PREFIX: &str = "<!--cat-blog:posts";
const MARKER_SUFFIX: &str = "-->";

/// 解析一个段落的全部文字。不是 `{{ posts ... }}` 时返回 `None`，按普通文字处理。
pub fn parse(paragraph: &str) -> Option<Result<PostsDirective, DirectiveError>> {
    let inner = paragraph.trim().strip_prefix("{{")?.strip_suffix("}}")?;
    let mut words = inner.split_whitespace();
    if words.next() != Some("posts") {
        return None;
    }
    Some(parse_params(words))
}

fn parse_params<'a>(
    params: impl Iterator<Item = &'a str>,
) -> Result<PostsDirective, DirectiveError> {
    let mut d = PostsDirective::default();
    for param in params {
        let (key, value) = param
            .split_once(':')
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .ok_or_else(|| DirectiveError::Malformed(param.to_owned()))?;
        match key {
            "limit" if d.limit.is_some() => return Err(DirectiveError::DuplicateParam(key.into())),
            "tag" if d.tag.is_some() => return Err(DirectiveError::DuplicateParam(key.into())),
            "limit" => {
                let n = Some(value)
                    .filter(|v| v.bytes().all(|b| b.is_ascii_digit()))
                    .and_then(|v| v.parse::<NonZeroU32>().ok())
                    .ok_or_else(|| DirectiveError::InvalidLimit(value.to_owned()))?;
                d.limit = Some(n);
            }
            "tag" => d.tag = Some(Tag::parse(value).map_err(DirectiveError::Tag)?),
            _ => return Err(DirectiveError::UnknownParam(key.to_owned())),
        }
    }
    Ok(d)
}

impl PostsDirective {
    /// 存进 `html` 列的占位注释，如 `<!--cat-blog:posts limit=5 tag=rust-->`。
    /// 标签按字节做百分号编码，所以注释不会被标签里的 `-->` 提前结束。
    pub fn to_marker(&self) -> String {
        let mut m = MARKER_PREFIX.to_owned();
        if let Some(limit) = self.limit {
            let _ = write!(m, " limit={limit}");
        }
        if let Some(tag) = &self.tag {
            m.push_str(" tag=");
            for b in tag.as_str().bytes() {
                if b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.' {
                    m.push(char::from(b));
                } else {
                    let _ = write!(m, "%{b:02X}");
                }
            }
        }
        m.push_str(MARKER_SUFFIX);
        m
    }

    fn from_marker_params(params: &str) -> Option<Self> {
        let mut d = Self::default();
        for param in params.split_whitespace() {
            match param.split_once('=')? {
                ("limit", v) => d.limit = Some(v.parse().ok()?),
                ("tag", v) => d.tag = Some(Tag::parse(&percent_decode(v)?).ok()?),
                _ => return None,
            }
        }
        Some(d)
    }
}

/// `html` 里有没有占位注释。没有时请求时不用查文章列表。
pub fn has_markers(html: &str) -> bool {
    html.contains(MARKER_PREFIX)
}

/// 把 `html` 里的占位注释换成 `expand` 的结果（请求时展开文章列表；RSS 里换成空字符串）。
pub fn expand_markers(html: &str, mut expand: impl FnMut(&PostsDirective) -> String) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    while let Some(start) = rest.find(MARKER_PREFIX) {
        out.push_str(&rest[..start]);
        let after = &rest[start + MARKER_PREFIX.len()..];
        let Some(end) = after.find(MARKER_SUFFIX) else {
            break;
        };
        let marker_len = MARKER_PREFIX.len() + end + MARKER_SUFFIX.len();
        match PostsDirective::from_marker_params(&after[..end]) {
            Some(d) => out.push_str(&expand(&d)),
            None => out.push_str(&rest[start..start + marker_len]),
        }
        rest = &rest[start + marker_len..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn limit(n: u32) -> Option<NonZeroU32> {
        NonZeroU32::new(n)
    }

    fn tag(s: &str) -> Option<Tag> {
        Tag::parse(s).ok()
    }

    #[test]
    fn bare_directive_has_no_params() {
        assert_eq!(parse("{{ posts }}"), Some(Ok(PostsDirective::default())));
        assert_eq!(parse("{{posts}}"), Some(Ok(PostsDirective::default())));
    }

    #[test]
    fn params_in_any_order() {
        let expected = PostsDirective {
            limit: limit(5),
            tag: tag("rust"),
        };
        assert_eq!(
            parse("{{ posts limit:5 tag:rust }}"),
            Some(Ok(expected.clone()))
        );
        assert_eq!(parse("{{ posts tag:Rust limit:5 }}"), Some(Ok(expected)));
    }

    #[test]
    fn bad_params_are_errors() {
        assert_eq!(
            parse("{{ posts sort:date }}"),
            Some(Err(DirectiveError::UnknownParam("sort".into())))
        );
        assert_eq!(
            parse("{{ posts limit:0 }}"),
            Some(Err(DirectiveError::InvalidLimit("0".into())))
        );
        assert_eq!(
            parse("{{ posts limit:-1 }}"),
            Some(Err(DirectiveError::InvalidLimit("-1".into())))
        );
        assert_eq!(
            parse("{{ posts limit:+5 }}"),
            Some(Err(DirectiveError::InvalidLimit("+5".into())))
        );
        assert_eq!(
            parse("{{ posts limit: 5 }}"),
            Some(Err(DirectiveError::Malformed("limit:".into())))
        );
        assert_eq!(
            parse("{{ posts tag:a tag:b }}"),
            Some(Err(DirectiveError::DuplicateParam("tag".into())))
        );
    }

    #[test]
    fn other_text_is_not_a_directive() {
        assert_eq!(parse("{{ post }}"), None);
        assert_eq!(parse("see {{ posts }}"), None);
        assert_eq!(parse("{{ posts }} and more"), None);
    }

    #[test]
    fn marker_round_trips_through_expansion() {
        let d = PostsDirective {
            limit: limit(3),
            tag: tag("c++ & -->"),
        };
        let marker = d.to_marker();
        assert_eq!(marker.matches("-->").count(), 1, "{marker}");
        assert!(marker.ends_with("-->"), "{marker}");

        let html = format!("<p>a</p>\n{marker}\n<p>b</p>");
        let mut seen = Vec::new();
        let out = expand_markers(&html, |d| {
            seen.push(d.clone());
            "<ul></ul>".into()
        });
        assert_eq!(out, "<p>a</p>\n<ul></ul>\n<p>b</p>");
        assert_eq!(seen, vec![d]);
    }

    #[test]
    fn expansion_handles_bare_and_chinese_markers() {
        let bare = PostsDirective::default().to_marker();
        assert_eq!(bare, "<!--cat-blog:posts-->");
        let zh = PostsDirective {
            limit: None,
            tag: tag("生活"),
        }
        .to_marker();
        let out = expand_markers(&format!("{bare}{zh}"), |d| {
            format!("[{}]", d.tag.as_ref().map_or("all", Tag::as_str))
        });
        assert_eq!(out, "[all][生活]");
    }
}
