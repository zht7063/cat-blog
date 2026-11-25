//! 公开页面里程序自带的文字（D35、D65）：首页 `lang` 以 `en` 开头时用英文，其他情况用中文。

/// 一种语言的全部文字。
#[derive(Debug)]
pub struct Texts {
    /// `/blog/` 的标题。
    pub blog: &'static str,
    /// `/blog/?tag=x` 的标题，后面接标签。
    pub tagged: &'static str,
    /// 文章列表为空时显示。
    pub no_posts: &'static str,
    /// 主题切换按钮的说明。
    pub toggle_theme: &'static str,
    pub not_found_title: &'static str,
    pub not_found_body: &'static str,
    pub back_home: &'static str,
}

const ZH: Texts = Texts {
    blog: "文章",
    tagged: "标签：",
    no_posts: "还没有文章。",
    toggle_theme: "切换亮色和暗色主题",
    not_found_title: "找不到这个页面",
    not_found_body: "这个地址没有内容，可能已经删除或换了地址。",
    back_home: "回到首页",
};

const EN: Texts = Texts {
    blog: "Blog",
    tagged: "Tagged: ",
    no_posts: "No posts yet.",
    toggle_theme: "Switch between light and dark theme",
    not_found_title: "Page not found",
    not_found_body: "There is nothing at this address. It may have been deleted or moved.",
    back_home: "Back to the home page",
};

/// 按首页的 `lang` 选文字。
pub fn texts(lang: &str) -> &'static Texts {
    if lang.get(..2).is_some_and(|p| p.eq_ignore_ascii_case("en")) {
        &EN
    } else {
        &ZH
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn english_only_for_en_languages() {
        assert_eq!(texts("en").blog, "Blog");
        assert_eq!(texts("en-US").blog, "Blog");
        assert_eq!(texts("EN-gb").blog, "Blog");
        assert_eq!(texts("zh-CN").blog, "文章");
        assert_eq!(texts("ja").blog, "文章");
        assert_eq!(texts("e").blog, "文章");
    }
}
