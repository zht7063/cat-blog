//! 保存时的渲染流水线（docs/v1.0-spec.md §7.1）：comrak 解析，`{{ posts }}` 换成占位，
//! syntect 高亮（CSS class），ammonia 清洗，最后把占位换成程序生成的注释。

use comrak::nodes::{AstNode, NodeHtmlBlock, NodeValue};
use comrak::plugins::syntect::{SyntectAdapter, SyntectAdapterBuilder};
use comrak::{Arena, Options, options};

use crate::directive::{self, PostsDirective};
use crate::frontmatter::Body;
use crate::sanitize::Sanitizer;
use crate::{ContentError, ContentErrorKind};

/// Markdown 渲染器。加载 syntect 的语法集较慢，启动时构造一次，重复使用。
pub struct Renderer {
    highlighter: SyntectAdapter,
    sanitizer: Sanitizer,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

fn comrak_options() -> Options<'static> {
    let mut o = Options::default();
    o.extension.table = true;
    o.extension.footnotes = true;
    o.extension.tasklist = true;
    o.extension.strikethrough = true;
    o.extension.autolink = true;
    o.extension.header_id_prefix = Some(String::new());
    o.render.r#unsafe = true;
    o
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            highlighter: SyntectAdapterBuilder::new().css().build(),
            sanitizer: Sanitizer::new(),
        }
    }

    /// 渲染并清洗正文。单独成段的 `{{ posts ... }}` 变成占位注释（D55）；
    /// 参数写错时返回带原文行号的错误。
    pub fn render(&self, body: Body<'_>) -> Result<String, ContentError> {
        let options = comrak_options();
        let mut plugins = options::Plugins::default();
        plugins.render.codefence_syntax_highlighter = Some(&self.highlighter);

        let mut salt = 0u32;
        loop {
            let token = placeholder_prefix(salt);
            salt += 1;
            if body.text.contains(&token) {
                continue;
            }
            let arena = Arena::new();
            let root = comrak::parse_document(&arena, body.text, &options);
            let directives = replace_directives(root, &token, body.first_line)?;

            let mut html = String::new();
            #[expect(
                clippy::expect_used,
                reason = "写入 String 不会失败；syntect 适配器高亮出错时自己回退为原文"
            )]
            comrak::format_html_with_plugins(root, &options, &mut html, &plugins)
                .expect("formatting into a String failed");
            let cleaned = self.sanitizer.clean(&html);
            if cleaned.matches(&token).count() != directives.len() {
                continue;
            }
            return Ok(directives.iter().enumerate().fold(cleaned, |html, (i, d)| {
                html.replace(&placeholder_html(&token, i), &d.to_marker())
            }));
        }
    }
}

fn placeholder_prefix(salt: u32) -> String {
    format!("CATBLOGPOSTS{salt}Z")
}

fn placeholder_html(prefix: &str, index: usize) -> String {
    format!("<p>{prefix}{index}Z</p>")
}

/// 把正文顶层单独成段的 `{{ posts ... }}` 换成占位段落，按出现顺序返回解析出的指令。
/// 引用、列表和脚注里的段落不算。
fn replace_directives<'a>(
    root: &'a AstNode<'a>,
    prefix: &str,
    first_line: usize,
) -> Result<Vec<PostsDirective>, ContentError> {
    let paragraphs: Vec<&AstNode<'_>> = root
        .children()
        .filter(|n| matches!(n.data().value, NodeValue::Paragraph))
        .collect();
    let mut found = Vec::new();
    for p in paragraphs {
        let Some(text) = plain_text(p) else {
            continue;
        };
        let Some(parsed) = directive::parse(&text) else {
            continue;
        };
        let line = first_line + p.data().sourcepos.start.line - 1;
        let d =
            parsed.map_err(|e| ContentError::new(Some(line), ContentErrorKind::Directive(e)))?;
        let block = NodeHtmlBlock::new(placeholder_html(prefix, found.len()) + "\n");
        p.data_mut().value = NodeValue::HtmlBlock(block);
        while let Some(child) = p.first_child() {
            child.detach();
        }
        found.push(d);
    }
    Ok(found)
}

/// 段落只由纯文字和软换行组成时返回它的文字；含代码、链接、强调等时返回 `None`。
fn plain_text<'a>(paragraph: &'a AstNode<'a>) -> Option<String> {
    let mut text = String::new();
    for child in paragraph.children() {
        match &child.data().value {
            NodeValue::Text(t) => text.push_str(t),
            NodeValue::SoftBreak => text.push(' '),
            _ => return None,
        }
    }
    Some(text)
}

#[cfg(test)]
mod tests {
    use std::sync::LazyLock;

    use super::*;

    static RENDERER: LazyLock<Renderer> = LazyLock::new(Renderer::new);

    fn render(md: &str) -> Result<String, ContentError> {
        RENDERER.render(Body {
            text: md,
            first_line: 1,
        })
    }

    fn html(md: &str) -> String {
        render(md).unwrap()
    }

    #[test]
    fn table_survives_sanitizing() {
        let out = html("| a | b |\n|---|---|\n| 1 | 2 |\n");
        assert!(
            out.contains("<table>") && out.contains("<th>a</th>") && out.contains("<td>2</td>"),
            "{out}"
        );
    }

    #[test]
    fn footnotes_survive_sanitizing() {
        let out = html("text[^1]\n\n[^1]: note\n");
        assert!(out.contains(r##"<a href="#fn-1" id="fnref-1""##), "{out}");
        assert!(out.contains(r#"<section class="footnotes">"#), "{out}");
        assert!(out.contains(r#"<li id="fn-1">"#), "{out}");
        assert!(out.contains(r##"href="#fnref-1""##), "{out}");
    }

    #[test]
    fn task_list_checkboxes_survive_disabled() {
        let out = html("- [x] done\n- [ ] todo\n");
        assert_eq!(out.matches("<input ").count(), 2, "{out}");
        assert_eq!(out.matches(r#"type="checkbox""#).count(), 2, "{out}");
        assert_eq!(out.matches(r#"disabled="""#).count(), 2, "{out}");
        assert_eq!(out.matches(r#"checked="""#).count(), 1, "{out}");
    }

    #[test]
    fn heading_anchors_survive() {
        let out = html("## Hello 世界\n");
        assert!(out.contains(r#"<h2 id="hello-世界">"#), "{out}");
        assert!(
            out.contains(r##"<a href="#hello-世界""##) && out.contains(r#"class="anchor""#),
            "{out}"
        );
    }

    #[test]
    fn highlight_classes_survive() {
        let out = html("```rust\nfn main() {}\n```\n");
        assert!(
            out.contains(r#"<pre class="syntax-highlighting"><code class="language-rust">"#),
            "{out}"
        );
        assert!(
            out.contains(r#"<span class="storage type function rust">fn</span>"#),
            "{out}"
        );
        assert!(!out.contains("style="), "{out}");
    }

    #[test]
    fn details_survive() {
        let out = html("<details>\n<summary>More</summary>\n\nhidden *text*\n\n</details>\n");
        assert!(
            out.contains("<details>") && out.contains("<summary>More</summary>"),
            "{out}"
        );
        assert!(out.contains("<em>text</em>"), "{out}");
    }

    #[test]
    fn raw_scripts_are_removed() {
        let out =
            html("<script>alert(1)</script>\n\n<a href=\"javascript:x()\" onclick=\"y()\">a</a>\n");
        assert!(
            !out.contains("script") && !out.contains("javascript") && !out.contains("onclick"),
            "{out}"
        );
    }

    #[test]
    fn standalone_directive_becomes_marker() {
        let out = html("intro\n\n{{ posts tag:Rust limit:5 }}\n\noutro\n");
        assert_eq!(
            out,
            "<p>intro</p>\n<!--cat-blog:posts limit=5 tag=rust-->\n<p>outro</p>\n"
        );
    }

    #[test]
    fn directive_inline_or_in_code_is_left_as_text() {
        let out = html(
            "see {{ posts }} here\n\n`{{ posts }}`\n\n```\n{{ posts }}\n```\n\n    {{ posts }}\n\n\
             > {{ posts }}\n\n- {{ posts }}\n",
        );
        assert!(!out.contains("cat-blog:posts"), "{out}");
        assert_eq!(out.matches("{{ posts }}").count(), 6, "{out}");
    }

    #[test]
    fn directive_error_reports_source_line() {
        let body = Body {
            text: "intro\n\n{{ posts sort:date }}\n",
            first_line: 5,
        };
        let err = RENDERER.render(body).unwrap_err();
        assert_eq!(err.line, Some(7));
        assert!(matches!(
            err.kind,
            ContentErrorKind::Directive(directive::DirectiveError::UnknownParam(_))
        ));
    }

    #[test]
    fn author_cannot_forge_marker_or_placeholder() {
        let out = html(
            "<!--cat-blog:posts limit=5-->\n\nCATBLOGPOSTS0Z0Z\n\n&#67;ATBLOGPOSTS1Z0Z\n\n{{ posts }}\n",
        );
        assert_eq!(out.matches("<!--cat-blog:posts").count(), 1, "{out}");
        assert!(out.ends_with("<!--cat-blog:posts-->\n"), "{out}");
        assert!(
            out.contains("<p>CATBLOGPOSTS0Z0Z</p>") && out.contains("<p>CATBLOGPOSTS1Z0Z</p>"),
            "{out}"
        );

        let out = html(
            "<iframe src=\"https://www.youtube.com/embed/x\"><!--cat-blog:posts limit=99--></iframe>\n\n\
             <iframe src=\"https://www.youtube.com/embed/y\" />\n<!--cat-blog:posts-->\n",
        );
        assert!(!out.contains("cat-blog:posts"), "{out}");
    }
}
