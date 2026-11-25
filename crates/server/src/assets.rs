//! 嵌入二进制的静态资源（D60）。地址带内容哈希参数，可以长期缓存（docs/v1.0-spec.md §8.1）。

use sha2::{Digest, Sha256};

const STYLE: &str = include_str!("../../../static/style.css");
const THEME_JS: &str = include_str!("../../../static/theme.js");
const FAVICON: &str = include_str!("../../../static/favicon.svg");

/// 一个静态文件。
#[derive(Debug)]
pub struct Asset {
    pub name: &'static str,
    pub content_type: &'static str,
    pub body: String,
    /// `/static/{name}?v={内容哈希}`。
    pub url: String,
}

impl Asset {
    fn new(name: &'static str, content_type: &'static str, body: String) -> Self {
        let digest = Sha256::digest(body.as_bytes());
        let hash: String = digest.iter().take(8).map(|b| format!("{b:02x}")).collect();
        let url = format!("/static/{name}?v={hash}");
        Self {
            name,
            content_type,
            body,
            url,
        }
    }
}

/// 全部静态资源。代码高亮的 CSS（D51）在启动时拼在 style.css 前面，style.css 可以覆盖它。
#[derive(Debug)]
pub struct Assets {
    pub css: Asset,
    pub js: Asset,
    pub favicon: Asset,
}

impl Default for Assets {
    fn default() -> Self {
        Self::new()
    }
}

impl Assets {
    pub fn new() -> Self {
        let css = format!("{}\n{STYLE}", cat_blog_core::markdown::highlight_css());
        Self {
            css: Asset::new("style.css", "text/css; charset=utf-8", css),
            js: Asset::new(
                "theme.js",
                "text/javascript; charset=utf-8",
                THEME_JS.to_owned(),
            ),
            favicon: Asset::new("favicon.svg", "image/svg+xml", FAVICON.to_owned()),
        }
    }

    pub fn get(&self, name: &str) -> Option<&Asset> {
        [&self.css, &self.js, &self.favicon]
            .into_iter()
            .find(|a| a.name == name)
    }
}
