//! 渲染结果的 HTML 清洗（D5、D53、D54，docs/v1.0-spec.md §7.2）。

use std::borrow::Cow;

/// 允许嵌入的 iframe 地址前缀（D54）。
const IFRAME_SRC_PREFIXES: [&str; 3] = [
    "https://www.youtube.com/embed/",
    "https://www.youtube-nocookie.com/embed/",
    "https://player.bilibili.com/",
];

/// 按 §7.2 配置好的 ammonia 清洗器。构造一次，重复使用。
pub struct Sanitizer {
    builder: ammonia::Builder<'static>,
}

impl Default for Sanitizer {
    fn default() -> Self {
        Self::new()
    }
}

impl Sanitizer {
    pub fn new() -> Self {
        Self { builder: builder() }
    }

    /// 清洗 HTML。作者写的注释（包括伪造的 posts 占位）、脚本、事件属性、
    /// `javascript:` 和 `data:` 链接都会被删掉；地址不在白名单里的 iframe 和
    /// 不是 checkbox 的 input 整个删掉，保留的 iframe 里面的内容也删掉。
    pub fn clean(&self, html: &str) -> String {
        let cleaned = self.builder.clean(html).to_string();
        let settings = lol_html::RewriteStrSettings::new()
            .append_element_content_handler(lol_html::element!("iframe", |el| {
                if el.has_attribute("src") {
                    el.set_inner_content("", lol_html::html_content::ContentType::Text);
                } else {
                    el.remove();
                }
                Ok(())
            }))
            .append_element_content_handler(lol_html::element!(
                r#"input:not([type="checkbox"])"#,
                |el| {
                    el.remove();
                    Ok(())
                }
            ));
        #[expect(
            clippy::expect_used,
            reason = "处理函数从不返回错误，默认设置也不限内存，rewrite_str 不会失败"
        )]
        lol_html::rewrite_str(&cleaned, settings).expect("rewriting sanitized HTML failed")
    }
}

fn builder() -> ammonia::Builder<'static> {
    let mut b = ammonia::Builder::default();
    b.add_generic_attributes(["id", "class", "aria-hidden", "aria-label"])
        .add_tags(["details", "summary", "section", "input", "iframe"])
        .add_tag_attributes("input", ["type", "checked"])
        .set_tag_attribute_value("input", "disabled", "")
        .add_tag_attributes(
            "iframe",
            ["src", "width", "height", "allowfullscreen", "title"],
        )
        .attribute_filter(|element, attribute, value| match (element, attribute) {
            ("iframe", "src") => IFRAME_SRC_PREFIXES
                .iter()
                .any(|p| value.starts_with(p))
                .then_some(Cow::Borrowed(value)),
            ("input", "type") => value
                .eq_ignore_ascii_case("checkbox")
                .then_some(Cow::Borrowed("checkbox")),
            _ => Some(Cow::Borrowed(value)),
        });
    b
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clean(html: &str) -> String {
        Sanitizer::new().clean(html)
    }

    #[test]
    fn scripts_and_event_handlers_are_removed() {
        let out = clean(
            r#"<p onclick="alert(1)">hi</p><script>alert(2)</script><img src="x.png" onerror="alert(3)">"#,
        );
        assert_eq!(out, r#"<p>hi</p><img src="x.png">"#);
    }

    #[test]
    fn javascript_and_data_links_are_removed() {
        let out = clean(
            r#"<a href="javascript:alert(1)">a</a><a href="JaVaScRiPt:alert(1)">b</a><a href="data:text/html,<script>alert(1)</script>">c</a><img src="data:image/png;base64,AAAA">"#,
        );
        assert_eq!(
            out,
            r#"<a rel="noopener noreferrer">a</a><a rel="noopener noreferrer">b</a><a rel="noopener noreferrer">c</a><img>"#
        );
    }

    #[test]
    fn whitelisted_iframes_are_kept() {
        for src in [
            "https://www.youtube.com/embed/abc",
            "https://www.youtube-nocookie.com/embed/abc",
            "https://player.bilibili.com/player.html?bvid=BV1",
        ] {
            let html = format!(
                r#"<iframe src="{src}" width="560" height="315" allowfullscreen title="v" onload="x()"></iframe>"#
            );
            let out = clean(&html);
            assert!(out.starts_with("<iframe "), "{out}");
            assert!(
                out.contains(&format!(r#"src="{}""#, src.replace('&', "&amp;"))),
                "{out}"
            );
            assert!(
                out.contains(r#"width="560""#) && out.contains("allowfullscreen"),
                "{out}"
            );
            assert!(!out.contains("onload"), "{out}");
        }
    }

    #[test]
    fn other_iframes_are_removed_entirely() {
        for src in [
            "https://evil.example/embed/",
            "http://www.youtube.com/embed/abc",
            "https://www.youtube.com.evil.example/embed/abc",
            "https://player.bilibili.com.evil.example/x",
            "javascript:alert(1)",
        ] {
            let out = clean(&format!(r#"<p>a</p><iframe src="{src}"></iframe><p>b</p>"#));
            assert_eq!(out, "<p>a</p><p>b</p>", "{src}");
        }
        assert_eq!(clean("<iframe></iframe>x"), "x");
    }

    #[test]
    fn checkboxes_are_forced_disabled() {
        let checked = clean(r#"<input type="CheckBox" checked onclick="x()">"#);
        for attr in [r#"type="checkbox""#, r#"disabled="""#, r#"checked="""#] {
            assert!(checked.contains(attr), "{checked}");
        }
        assert!(!checked.contains("onclick"), "{checked}");
        assert_eq!(
            clean(r#"<input type="checkbox" disabled>"#)
                .matches("disabled")
                .count(),
            1
        );
    }

    #[test]
    fn non_checkbox_inputs_are_removed() {
        for html in [
            r#"<input type="text" name="q" value="x">"#,
            r#"<input type="password">"#,
            "<input>",
        ] {
            assert_eq!(clean(&format!("a{html}b")), "ab", "{html}");
        }
    }

    #[test]
    fn whitelisted_iframe_content_is_dropped() {
        let out = clean(
            r#"<iframe src="https://www.youtube.com/embed/x"><!--cat-blog:posts--><script>x()</script></iframe>"#,
        );
        assert_eq!(
            out,
            r#"<iframe src="https://www.youtube.com/embed/x"></iframe>"#
        );
    }

    #[test]
    fn author_comments_cannot_forge_posts_placeholder() {
        let out = clean("<p>a</p><!--cat-blog:posts limit=5--><p>b</p>");
        assert_eq!(out, "<p>a</p><p>b</p>");
    }

    #[test]
    fn details_and_heading_anchors_are_kept() {
        let html = r##"<details><summary>more</summary>body</details><h2 id="x" class="c"><a href="#x" class="anchor" aria-label="Link"></a>X</h2>"##;
        assert_eq!(
            clean(html),
            r##"<details><summary>more</summary>body</details><h2 id="x" class="c"><a href="#x" class="anchor" aria-label="Link" rel="noopener noreferrer"></a>X</h2>"##
        );
    }
}
