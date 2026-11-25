//! front matter 的解析、字段校验和 slug 写回（docs/v1.0-spec.md §5）。

use jiff::Timestamp;
use jiff::tz::TimeZone;
use serde::Deserialize;
use serde_yaml_ng::Value;

use crate::slug::{Slug, SlugError};
use crate::{ContentError, ContentErrorKind, date, tags};

/// 换行统一为 LF 的原文（D70）。解析、写回和保存都用它，行号因此和编辑框里一致。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source(String);

impl Source {
    pub fn new(raw: &str) -> Self {
        Self(raw.replace("\r\n", "\n").replace('\r', "\n"))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_string(self) -> String {
        self.0
    }
}

/// 内容的类型，由后台的新建入口决定（D9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Page,
    Post,
}

/// front matter 后面的 Markdown 正文。`first_line` 是正文第一行在原文中的行号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Body<'a> {
    pub text: &'a str,
    pub first_line: usize,
}

/// 首页 front matter 解析出的站点设置（D11）。
#[derive(Debug, Clone)]
pub struct Home<'a> {
    pub title: String,
    pub description: String,
    pub lang: String,
    pub timezone: TimeZone,
    pub nav: String,
    pub body: Body<'a>,
    pub(crate) nav_line: Option<usize>,
}

/// 页面或文章的 front matter。
#[derive(Debug, Clone)]
pub struct Entry<'a> {
    pub title: String,
    pub slug: Option<WrittenSlug>,
    pub date: Option<Timestamp>,
    pub draft: bool,
    pub tags: Vec<tags::Tag>,
    pub description: String,
    pub body: Body<'a>,
    title_line: Option<usize>,
}

/// 原文里手写的 slug，尚未规范化。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WrittenSlug {
    pub value: String,
    line: Option<usize>,
}

const HOME_FIELDS: &[&str] = &["title", "description", "lang", "timezone", "nav"];
const POST_FIELDS: &[&str] = &["title", "slug", "date", "draft", "tags", "description"];
const PAGE_FIELDS: &[&str] = &["title", "slug", "date", "draft", "description"];

const DEFAULT_TITLE: &str = "ฅ^•ﻌ•^ฅ";
const DEFAULT_LANG: &str = "zh-CN";
const DEFAULT_TIMEZONE: &str = "Asia/Singapore";
const DEFAULT_NAV: &str = "[Home](/) [Blog](/blog/)";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawHome {
    title: Option<String>,
    description: Option<String>,
    lang: Option<String>,
    timezone: Option<String>,
    nav: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEntry {
    title: Option<String>,
    slug: Option<Value>,
    date: Option<String>,
    draft: Option<bool>,
    tags: Option<Value>,
    description: Option<String>,
}

#[derive(Deserialize)]
struct SlugOnly {
    slug: Option<Value>,
}

/// 原文按行切开后的结构。行号从 1 开始：第 1 行是开头的 `---`，第 `close` 行是结束的 `---`。
struct Split<'a> {
    lines: Vec<&'a str>,
    close: usize,
    yaml: String,
    body: Body<'a>,
}

impl<'a> Split<'a> {
    fn parse(src: &'a Source) -> Result<Self, ContentError> {
        let text = src.as_str();
        let lines: Vec<&str> = text.split_inclusive('\n').collect();
        if lines.first().is_none_or(|l| !is_fence(l)) {
            return Err(ContentError::new(
                Some(1),
                ContentErrorKind::MissingFrontMatter,
            ));
        }
        let close = lines
            .iter()
            .skip(1)
            .position(|l| is_fence(l))
            .map(|i| i + 2)
            .ok_or(ContentError::new(
                Some(1),
                ContentErrorKind::UnclosedFrontMatter,
            ))?;
        let body_start: usize = lines[..close].iter().map(|l| l.len()).sum();
        Ok(Self {
            yaml: lines[1..close - 1].concat(),
            body: Body {
                text: &text[body_start..],
                first_line: close + 1,
            },
            lines,
            close,
        })
    }

    /// front matter 里顶层 `key:` 所在的行号。
    fn key_line(&self, key: &str) -> Option<usize> {
        (2..self.close).find(|&n| {
            self.lines[n - 1]
                .strip_prefix(key)
                .and_then(|rest| rest.trim_start_matches([' ', '\t']).strip_prefix(':'))
                .is_some_and(|v| v.is_empty() || v.starts_with([' ', '\t', '\n']))
        })
    }

    /// 第 `n` 行的值是否跨了多行：块标量（`|`、`>`），或下一行是缩进的续行。
    fn is_multiline(&self, n: usize) -> bool {
        let value = self.lines[n - 1]
            .split_once(':')
            .map_or("", |(_, v)| v.trim());
        let continued = n + 1 < self.close && {
            let next = self.lines[n];
            next.starts_with([' ', '\t'])
                && !next.trim().is_empty()
                && !next.trim().starts_with('#')
        };
        value.starts_with(['|', '>']) || continued
    }

    fn deserialize<T: for<'de> Deserialize<'de>>(&self) -> Result<T, ContentError> {
        let yaml = if self.yaml.trim().is_empty() {
            "{}"
        } else {
            &self.yaml
        };
        serde_yaml_ng::from_str(yaml).map_err(yaml_error)
    }

    fn error(&self, key: &str, kind: ContentErrorKind) -> ContentError {
        ContentError::new(self.key_line(key), kind)
    }
}

fn is_fence(line: &str) -> bool {
    line.trim_end() == "---"
}

/// 把 YAML 库的错误换成给作者看的错误，行号换算成原文的行号（YAML 从原文第 2 行开始）。
fn yaml_error(e: serde_yaml_ng::Error) -> ContentError {
    let line = e.location().map(|l| l.line() + 1);
    let full = e.to_string();
    let msg = full.split(" at line ").next().unwrap_or(&full);
    let kind = if let Some(rest) = msg.strip_prefix("unknown field `") {
        let field = rest.split('`').next().unwrap_or_default().to_owned();
        ContentErrorKind::UnknownField {
            field,
            allowed: &[],
        }
    } else if let Some((field, _)) = msg.split_once(": invalid type: ") {
        ContentErrorKind::InvalidType {
            field: field.to_owned(),
            expected: expected_type(field),
        }
    } else {
        ContentErrorKind::Yaml(msg.to_owned())
    };
    ContentError::new(line, kind)
}

fn expected_type(field: &str) -> &'static str {
    match field {
        "draft" => "true 或 false",
        "tags" => "字符串列表，比如 [rust, 生活]",
        _ => "字符串",
    }
}

fn with_allowed(mut e: ContentError, allowed: &'static [&'static str]) -> ContentError {
    if let ContentErrorKind::UnknownField { allowed: a, .. } = &mut e.kind {
        *a = allowed;
    }
    e
}

/// 字段值为 null 或空字符串时视同没写（§5.1）。
fn non_empty(s: Option<String>) -> Option<String> {
    s.filter(|s| !s.trim().is_empty())
}

fn is_blank(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => true,
        Some(Value::String(s)) => s.trim().is_empty(),
        Some(Value::Sequence(items)) => items.is_empty(),
        Some(_) => false,
    }
}

/// 解析首页原文。没写的字段取 §5.1 的默认值。
pub fn parse_home(src: &Source) -> Result<Home<'_>, ContentError> {
    let split = Split::parse(src)?;
    let raw: RawHome = split
        .deserialize()
        .map_err(|e| with_allowed(e, HOME_FIELDS))?;
    let timezone_name = non_empty(raw.timezone);
    let timezone_name = timezone_name.as_deref().unwrap_or(DEFAULT_TIMEZONE);
    let timezone = date::parse_timezone(timezone_name)
        .map_err(|e| split.error("timezone", ContentErrorKind::Date(e)))?;
    Ok(Home {
        title: non_empty(raw.title).unwrap_or_else(|| DEFAULT_TITLE.to_owned()),
        description: raw.description.unwrap_or_default(),
        lang: non_empty(raw.lang).unwrap_or_else(|| DEFAULT_LANG.to_owned()),
        timezone,
        nav: non_empty(raw.nav).unwrap_or_else(|| DEFAULT_NAV.to_owned()),
        body: split.body,
        nav_line: split.key_line("nav"),
    })
}

/// 解析页面或文章的原文。`date` 按站点时区 `tz` 解释。
pub fn parse_entry<'a>(
    src: &'a Source,
    kind: Kind,
    tz: &TimeZone,
) -> Result<Entry<'a>, ContentError> {
    let split = Split::parse(src)?;
    let allowed = match kind {
        Kind::Page => PAGE_FIELDS,
        Kind::Post => POST_FIELDS,
    };
    let raw: RawEntry = split.deserialize().map_err(|e| with_allowed(e, allowed))?;

    let title =
        non_empty(raw.title).ok_or_else(|| split.error("title", ContentErrorKind::MissingTitle))?;
    let date = non_empty(raw.date)
        .map(|d| date::parse_date(d.trim(), tz))
        .transpose()
        .map_err(|e| split.error("date", ContentErrorKind::Date(e)))?;
    let tags = match kind {
        Kind::Post => parse_tags(&split, raw.tags)?,
        Kind::Page if is_blank(raw.tags.as_ref()) => Vec::new(),
        Kind::Page => return Err(split.error("tags", ContentErrorKind::TagsOnPage)),
    };

    Ok(Entry {
        title,
        slug: written_slug(&split, raw.slug)?,
        date,
        draft: raw.draft.unwrap_or(false),
        tags,
        description: raw.description.unwrap_or_default(),
        body: split.body,
        title_line: split.key_line("title"),
    })
}

fn parse_tags(split: &Split<'_>, value: Option<Value>) -> Result<Vec<tags::Tag>, ContentError> {
    let invalid = || {
        split.error(
            "tags",
            ContentErrorKind::InvalidType {
                field: "tags".into(),
                expected: expected_type("tags"),
            },
        )
    };
    if is_blank(value.as_ref()) {
        return Ok(Vec::new());
    }
    let items = match value {
        Some(Value::String(s)) => {
            return Err(split.error("tags", ContentErrorKind::TagsNotList(s)));
        }
        Some(Value::Sequence(items)) => items,
        _ => return Err(invalid()),
    };
    let strings = items
        .into_iter()
        .map(|v| match v {
            Value::String(s) => Ok(s),
            Value::Number(n) => Ok(n.to_string()),
            _ => Err(invalid()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    tags::parse_list(strings.iter().map(String::as_str))
        .map_err(|e| split.error("tags", ContentErrorKind::Tag(e)))
}

fn written_slug(
    split: &Split<'_>,
    value: Option<Value>,
) -> Result<Option<WrittenSlug>, ContentError> {
    let line = split.key_line("slug");
    let value = match value {
        None | Some(Value::Null) => return Ok(None),
        Some(Value::String(s)) => s,
        Some(Value::Number(n)) => n.to_string(),
        Some(_) => {
            return Err(ContentError::new(
                line,
                ContentErrorKind::InvalidType {
                    field: "slug".into(),
                    expected: expected_type("slug"),
                },
            ));
        }
    };
    Ok((!value.trim().is_empty()).then_some(WrittenSlug { value, line }))
}

impl Entry<'_> {
    /// 按 §5.3 确定 slug：手写的规范化；没写时沿用库里的 `stored`；都没有时用标题生成。
    /// 和同一博客其他内容的冲突由调用方检查。
    pub fn resolve_slug(&self, stored: Option<&Slug>) -> Result<Slug, ContentError> {
        if let Some(w) = &self.slug {
            return Slug::parse(&w.value)
                .map_err(|e| ContentError::new(w.line, ContentErrorKind::Slug(e)));
        }
        if let Some(s) = stored {
            return Ok(s.clone());
        }
        Slug::from_title(&self.title).map_err(|e| {
            let kind = match e {
                SlugError::Empty => ContentErrorKind::TitleWithoutSlug,
                e => ContentErrorKind::Slug(e),
            };
            ContentError::new(self.title_line, kind)
        })
    }
}

/// 把最终的 `slug` 写回原文（D39，§5.3）。原文里的 slug 已经是同一个字符串时原样返回；
/// 否则只改一行：替换顶层单行的 `slug:`，或插在顶层单行的 `title:` 后面，
/// 都没有时插在结束的 `---` 前面。写回后再解析一遍，结果不一致就报错。
pub fn write_back_slug(src: &Source, slug: &Slug) -> Result<Source, ContentError> {
    let split = Split::parse(src)?;
    let current: SlugOnly = split.deserialize()?;
    if matches!(&current.slug, Some(Value::String(s)) if s == slug.as_str()) {
        return Ok(src.clone());
    }

    let slug_line = split.key_line("slug");
    let comment = slug_line.and_then(|n| trailing_comment(split.lines[n - 1]));
    let new_line = match comment {
        Some(c) => format!("slug: \"{slug}\" {c}\n"),
        None => format!("slug: \"{slug}\"\n"),
    };
    let mut lines: Vec<&str> = split.lines.clone();
    match slug_line {
        Some(n) if split.is_multiline(n) => {
            return Err(ContentError::new(Some(n), ContentErrorKind::MultilineSlug));
        }
        Some(n) => lines[n - 1] = &new_line,
        None => {
            let at = match split.key_line("title") {
                Some(n) if !split.is_multiline(n) => n,
                _ => split.close - 1,
            };
            lines.insert(at, &new_line);
        }
    }

    let written = Source(lines.concat());
    let check: Result<SlugOnly, _> = Split::parse(&written).and_then(|s| s.deserialize());
    match check {
        Ok(SlugOnly {
            slug: Some(Value::String(s)),
        }) if s == slug.as_str() => Ok(written),
        _ => Err(ContentError::new(
            slug_line.or_else(|| split.key_line("title")),
            ContentErrorKind::SlugWriteBack(slug.as_str().to_owned()),
        )),
    }
}

/// 单行 `key: value  # 注释` 末尾的注释（含 `#`）。引号里的 `#` 不算。
fn trailing_comment(line: &str) -> Option<&str> {
    let value = line.trim_end().split_once(':')?.1;
    let after_value = match value.trim_start().chars().next()? {
        '"' => {
            let v = value.trim_start();
            let mut escaped = false;
            let end = v.char_indices().skip(1).find(|&(_, c)| {
                let closes = c == '"' && !escaped;
                escaped = c == '\\' && !escaped;
                closes
            })?;
            &v[end.0 + 1..]
        }
        '\'' => {
            let v = value.trim_start();
            let mut rest = &v[1..];
            loop {
                let i = rest.find('\'')?;
                if rest[i + 1..].starts_with('\'') {
                    rest = &rest[i + 2..];
                } else {
                    break &rest[i + 1..];
                }
            }
        }
        _ => {
            let i = value.find(" #").or_else(|| value.find("\t#"))?;
            &value[i..]
        }
    };
    let comment = after_value.trim_start();
    comment.starts_with('#').then_some(comment)
}

#[cfg(test)]
mod tests;
