use super::*;

fn sg() -> TimeZone {
    date::parse_timezone("Asia/Singapore").unwrap()
}

fn post_err(src: &str) -> ContentError {
    parse_entry(&Source::new(src), Kind::Post, &sg()).unwrap_err()
}

fn page_err(src: &str) -> ContentError {
    parse_entry(&Source::new(src), Kind::Page, &sg()).unwrap_err()
}

/// 解析原文、确定 slug、写回，返回写回后的原文和 slug。
fn save(src: &str, stored: Option<&str>) -> Result<(String, String), ContentError> {
    let src = Source::new(src);
    let stored = stored.map(|s| Slug::parse(s).unwrap());
    let entry = parse_entry(&src, Kind::Post, &sg())?;
    let slug = entry.resolve_slug(stored.as_ref())?;
    let written = write_back_slug(&src, &slug)?;
    Ok((written.into_string(), slug.as_str().to_owned()))
}

#[test]
fn full_post_is_parsed() {
    let src = Source::new(
        "---\ntitle: 我的 Rust 笔记！\ndate: 2026-11-24\ndraft: true\ntags: [Rust, 生活, rust]\n\
         description: 简介\n---\n# 正文\n",
    );
    let e = parse_entry(&src, Kind::Post, &sg()).unwrap();
    assert_eq!(e.title, "我的 Rust 笔记！");
    assert_eq!(e.date, Some("2026-11-23T16:00:00Z".parse().unwrap()));
    assert!(e.draft);
    assert_eq!(
        e.tags.iter().map(tags::Tag::as_str).collect::<Vec<_>>(),
        ["rust", "生活"]
    );
    assert_eq!(e.description, "简介");
    assert_eq!(
        e.body,
        Body {
            text: "# 正文\n",
            first_line: 8
        }
    );
}

#[test]
fn null_and_empty_fields_count_as_absent() {
    let src = Source::new("---\ntitle: t\ndate:\ndraft:\ntags:\ndescription: ''\nslug: ''\n---\n");
    let e = parse_entry(&src, Kind::Post, &sg()).unwrap();
    assert_eq!(
        (e.date, e.draft, e.tags.len(), e.slug),
        (None, false, 0, None)
    );
}

#[test]
fn unknown_field_reports_its_line() {
    let err = post_err("---\ntitle: a\n# 注释\nauthor: me\n---\nbody\n");
    assert_eq!(err.line, Some(4));
    assert!(
        matches!(&err.kind, ContentErrorKind::UnknownField { field, allowed }
        if field == "author" && allowed.contains(&"tags"))
    );
    assert!(err.to_string().starts_with("第 4 行："), "{err}");
}

#[test]
fn page_does_not_list_tags_as_allowed() {
    let err = page_err("---\ntitle: a\nfoo: 1\n---\n");
    assert!(
        matches!(&err.kind, ContentErrorKind::UnknownField { allowed, .. }
        if !allowed.contains(&"tags"))
    );
}

#[test]
fn wrong_types_report_field_and_line() {
    let err = post_err("---\ntitle: a\ndraft: yes\n---\n");
    assert_eq!(err.line, Some(3));
    assert!(matches!(&err.kind, ContentErrorKind::InvalidType { field, .. } if field == "draft"));

    let err = post_err("---\ntitle: [a, b]\n---\n");
    assert_eq!(err.line, Some(2));
    assert!(matches!(&err.kind, ContentErrorKind::InvalidType { field, .. } if field == "title"));

    let err = post_err("---\ntitle: a\ntags: {x: 1}\n---\n");
    assert_eq!(err.line, Some(3));
    assert!(matches!(&err.kind, ContentErrorKind::InvalidType { field, .. } if field == "tags"));
}

#[test]
fn yaml_syntax_error_reports_source_line() {
    let err = post_err("---\ntitle: a\n- x\ndraft: true\n---\n");
    assert!(matches!(err.kind, ContentErrorKind::Yaml(_)), "{err:?}");
    assert_eq!(err.line, Some(3), "{err:?}");
    assert!(!err.to_string().contains(" at line "), "{err}");
}

#[test]
fn missing_front_matter_or_closing_fence() {
    assert_eq!(
        post_err("title: a\n").kind,
        ContentErrorKind::MissingFrontMatter
    );
    assert_eq!(post_err("").kind, ContentErrorKind::MissingFrontMatter);
    let err = post_err("---\ntitle: a\n\nbody without fence\n");
    assert_eq!(err.kind, ContentErrorKind::UnclosedFrontMatter);
}

#[test]
fn crlf_input_parses_with_same_lines() {
    let src = Source::new("---\r\ntitle: a\r\n\r\nfoo: 1\r\n---\r\nbody\r\n");
    assert_eq!(src.as_str(), "---\ntitle: a\n\nfoo: 1\n---\nbody\n");
    let err = parse_entry(&src, Kind::Post, &sg()).unwrap_err();
    assert_eq!(err.line, Some(4));

    let src = Source::new("---\r\ntitle: a\r\n---\r\nbody\r\n");
    let e = parse_entry(&src, Kind::Post, &sg()).unwrap();
    assert_eq!((e.title.as_str(), e.body.text), ("a", "body\n"));
}

#[test]
fn page_with_tags_is_rejected() {
    let err = page_err("---\ntitle: a\ntags: [rust]\n---\n");
    assert_eq!(
        (err.line, err.kind),
        (Some(3), ContentErrorKind::TagsOnPage)
    );
    let empty = Source::new("---\ntitle: a\ntags: []\n---\n");
    assert!(parse_entry(&empty, Kind::Page, &sg()).is_ok());
}

#[test]
fn tags_as_single_string_is_rejected() {
    let err = post_err("---\ntitle: a\ntags: rust\n---\n");
    assert_eq!(
        (err.line, err.kind),
        (Some(3), ContentErrorKind::TagsNotList("rust".into()))
    );
    assert!(
        post_err("---\ntitle: a\ntags: rust\n---\n")
            .to_string()
            .contains("[rust]")
    );
}

#[test]
fn missing_title_and_bad_date() {
    assert_eq!(
        post_err("---\ndraft: true\n---\n").kind,
        ContentErrorKind::MissingTitle
    );
    let err = post_err("---\ntitle: a\ndate: next week\n---\n");
    assert_eq!(err.line, Some(3));
    assert!(matches!(
        err.kind,
        ContentErrorKind::Date(date::DateError::InvalidDate(_))
    ));
}

#[test]
fn home_defaults_and_timezone_check() {
    let src = Source::new("---\n---\nhello\n");
    let home = parse_home(&src).unwrap();
    assert_eq!(home.title, "ฅ^•ﻌ•^ฅ");
    assert_eq!(
        (
            home.lang.as_str(),
            home.timezone.iana_name().unwrap_or_default()
        ),
        ("zh-CN", "Asia/Singapore")
    );
    assert_eq!(home.nav, "[Home](/) [Blog](/blog/)");
    assert_eq!(
        home.body,
        Body {
            text: "hello\n",
            first_line: 3
        }
    );

    let err = parse_home(&Source::new("---\ntitle: x\ntimezone: Mars/Base\n---\n")).unwrap_err();
    assert_eq!(err.line, Some(3));
    assert!(matches!(
        err.kind,
        ContentErrorKind::Date(date::DateError::InvalidTimeZone(_))
    ));
    let err = parse_home(&Source::new("---\ndraft: true\n---\n")).unwrap_err();
    assert!(matches!(&err.kind, ContentErrorKind::UnknownField { field, .. } if field == "draft"));
}

#[test]
fn slug_comes_from_written_value_then_stored_then_title() {
    assert_eq!(
        save("---\ntitle: T\nslug: My Post\n---\n", Some("old"))
            .unwrap()
            .1,
        "my-post"
    );
    assert_eq!(
        save("---\ntitle: New Title\n---\n", Some("old")).unwrap().1,
        "old"
    );
    assert_eq!(
        save("---\ntitle: 我的 Rust 笔记！\n---\n", None).unwrap().1,
        "我的-rust-笔记"
    );
    let err = save("---\ntitle: ！！！\n---\n", None).unwrap_err();
    assert_eq!(
        (err.line, err.kind),
        (Some(2), ContentErrorKind::TitleWithoutSlug)
    );
    let err = save("---\ntitle: a\nslug: Admin\n---\n", None).unwrap_err();
    assert_eq!(err.line, Some(3));
    assert!(matches!(
        err.kind,
        ContentErrorKind::Slug(SlugError::Reserved(_))
    ));
}

#[test]
fn write_back_inserts_after_title() {
    let (src, _) = save(
        "---\n# 注释\ndraft: true\ntitle: Hello World\ntags: []\n---\nbody\n",
        None,
    )
    .unwrap();
    assert_eq!(
        src,
        "---\n# 注释\ndraft: true\ntitle: Hello World\nslug: \"hello-world\"\ntags: []\n---\nbody\n"
    );
}

#[test]
fn write_back_before_closing_fence_when_title_is_multiline() {
    let (src, slug) = save("---\ntitle: >\n  Long\n  Title\n---\n", None).unwrap();
    assert_eq!(slug, "long-title");
    assert_eq!(
        src,
        "---\ntitle: >\n  Long\n  Title\nslug: \"long-title\"\n---\n"
    );
}

#[test]
fn write_back_replaces_only_the_slug_value() {
    let original = "---\n# 顶部注释\ntitle: T  # 标题\nslug:   'My  Post'   \n\ndraft: false\n---\n正文 slug: x\n";
    let (src, _) = save(original, None).unwrap();
    assert_eq!(
        src,
        "---\n# 顶部注释\ntitle: T  # 标题\nslug: \"my-post\"\n\ndraft: false\n---\n正文 slug: x\n"
    );
}

#[test]
fn write_back_leaves_already_normalized_slug_untouched() {
    let original = "---\ntitle: T\nslug: my-post # 手写\n---\n";
    assert_eq!(save(original, None).unwrap().0, original);
}

#[test]
fn numeric_slug_is_quoted() {
    let (src, slug) = save("---\ntitle: T\nslug: 2024\n---\n", None).unwrap();
    assert_eq!(
        (src.as_str(), slug.as_str()),
        ("---\ntitle: T\nslug: \"2024\"\n---\n", "2024")
    );
}

#[test]
fn multiline_slug_is_rejected() {
    for src in [
        "---\ntitle: T\nslug: |\n  my-post\n---\n",
        "---\ntitle: T\nslug: my\n  post\n---\n",
    ] {
        let err = save(src, None).unwrap_err();
        assert_eq!(
            (err.line, err.kind),
            (Some(3), ContentErrorKind::MultilineSlug),
            "{src}"
        );
    }
}

#[test]
fn write_back_is_idempotent() {
    for original in [
        "---\ntitle: Hello, World!\n---\nbody\n",
        "---\ntitle: T\nslug: 2024\n---\n",
        "---\ntitle: T\nslug: Ｃafé  Notes\ndraft: true\n---\n",
    ] {
        let (first, slug) = save(original, None).unwrap();
        let (second, again) = save(&first, None).unwrap();
        assert_eq!(
            (second.as_str(), again.as_str()),
            (first.as_str(), slug.as_str()),
            "{original}"
        );
        let first_src = Source::new(&first);
        let reparsed = parse_entry(&first_src, Kind::Post, &sg()).unwrap();
        assert_eq!(reparsed.slug.map(|w| w.value), Some(slug));
    }
}

#[test]
fn write_back_keeps_comment_on_slug_line() {
    let (src, _) = save("---\ntitle: T\nslug: My Post # 旧地址\n---\n", None).unwrap();
    assert_eq!(src, "---\ntitle: T\nslug: \"my-post\" # 旧地址\n---\n");
    let (src, _) = save("---\ntitle: T\nslug: 'it''s # x'  # 注释\n---\n", None).unwrap();
    assert_eq!(src, "---\ntitle: T\nslug: \"it-s-x\" # 注释\n---\n");
    let (src, _) = save("---\ntitle: T\nslug: \"a#b\"\n---\n", None).unwrap();
    assert_eq!(src, "---\ntitle: T\nslug: \"a-b\"\n---\n");
}

#[test]
fn unusual_yaml_that_cannot_be_written_back_names_the_slug() {
    let err = save("---\ntitle: T\n\"slug\": My Post\n---\n", None).unwrap_err();
    assert_eq!(err.kind, ContentErrorKind::SlugWriteBack("my-post".into()));
    assert!(err.to_string().contains("slug: \"my-post\""), "{err}");
}
