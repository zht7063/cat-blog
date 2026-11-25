//! `cat-blog rerender`（D33、D44，docs/v1.0-spec.md §7.1）：按当前的规则和首页的时区，
//! 对首页和所有页面、文章重跑保存流水线，不改原文、slug 和 `updated_at`。

use std::fmt;

use cat_blog_core::frontmatter::Kind;
use cat_blog_core::markdown::Renderer;
use cat_blog_core::save;
use cat_blog_core::slug::Slug;
use jiff::Timestamp;

use crate::db::{BlogDb, PostId};

/// 一次 `rerender` 的结果。`failed` 不为空时命令以非零状态码退出。
#[derive(Debug, Default)]
pub struct Outcome {
    pub rendered: usize,
    /// 处理期间在后台保存过的内容。保存时已经按当前规则渲染，这里不再写。
    pub saved_meanwhile: usize,
    pub failed: Vec<Failure>,
}

/// 新规则下处理失败、被跳过的内容。库里的这一行保持原样。
#[derive(Debug)]
pub struct Failure {
    pub target: Target,
    pub reason: String,
}

#[derive(Debug)]
pub enum Target {
    Home,
    Entry { id: PostId, slug: Slug },
}

impl fmt::Display for Target {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Home => f.write_str("home page"),
            Self::Entry { id, slug } => {
                write!(f, "/{}/ (edit at /admin/edit/{})", slug.as_str(), id.0)
            }
        }
    }
}

/// 先处理首页，再用首页（可能刚改过）的时区处理页面和文章，这样写了 `date` 的内容按新时区
/// 重算 `published_at`（D44）。
pub async fn run(
    blog: &BlogDb,
    renderer: &Renderer,
    now: Timestamp,
) -> Result<Outcome, sqlx::Error> {
    let mut outcome = Outcome::default();

    let home_source = blog.blog().await?.home_source;
    match save::prepare_home(renderer, &home_source) {
        Ok(home) => outcome.count(blog.rerender_home(&home).await?),
        Err(error) => outcome.fail(Target::Home, error.to_string()),
    }
    let timezone = blog.blog().await?.timezone;

    for kind in [Kind::Page, Kind::Post] {
        for post in blog.list(kind).await? {
            let target = || Target::Entry {
                id: post.id,
                slug: post.slug.clone(),
            };
            match save::prepare_entry(
                renderer,
                &post.source,
                kind,
                &timezone,
                Some(post.stored()),
                now,
            ) {
                Err(error) => outcome.fail(target(), error.to_string()),
                Ok(entry) if entry.slug != post.slug => outcome.fail(
                    target(),
                    format!(
                        "slug 在新规则下变成了 \"{}\"，rerender 不改原文和地址；\
                         请在后台打开这篇内容确认 slug 后保存",
                        entry.slug.as_str()
                    ),
                ),
                Ok(entry) => outcome.count(blog.rerender_post(post.id, &entry).await?),
            }
        }
    }
    Ok(outcome)
}

impl Outcome {
    fn count(&mut self, written: bool) {
        if written {
            self.rendered += 1;
        } else {
            self.saved_meanwhile += 1;
        }
    }

    fn fail(&mut self, target: Target, reason: String) {
        self.failed.push(Failure { target, reason });
    }
}
