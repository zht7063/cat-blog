//! 首页、页面和文章的列表、编辑、删除与预览（D26、D56、D76，docs/v1.0-spec.md §9.1、§9.2）。

use askama::Template;
use axum::Form;
use axum::extract::{Extension, Path, Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect};
use cat_blog_core::date;
use cat_blog_core::frontmatter::Kind;
use cat_blog_core::save;
use serde::Deserialize;

use super::{AdminLayout, AdminSession};
use crate::AppState;
use crate::db::{Post, PostId, WriteError};
use crate::routes::{AppResult, not_found};

/// 新建时预填的模板（§5.1）。页面没有 `tags` 一行。
const NEW_POST: &str = "---\ntitle:\ndraft: true\ntags: []\n---\n\n";
const NEW_PAGE: &str = "---\ntitle:\ndraft: true\n---\n\n";

/// 编辑页上的内容是什么，预览时据此选择解析规则。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Target {
    Home,
    Page,
    Post,
}

impl Target {
    fn of(kind: Kind) -> Self {
        match kind {
            Kind::Page => Self::Page,
            Kind::Post => Self::Post,
        }
    }

    /// 页面或文章对应的 [`Kind`]；首页没有。
    pub(super) fn kind(self) -> Option<Kind> {
        match self {
            Self::Home => None,
            Self::Page => Some(Kind::Page),
            Self::Post => Some(Kind::Post),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Home => "home",
            Self::Page => "page",
            Self::Post => "post",
        }
    }
}

fn label(kind: Kind) -> &'static str {
    match kind {
        Kind::Page => "页面",
        Kind::Post => "文章",
    }
}

fn list_href(kind: Kind) -> &'static str {
    match kind {
        Kind::Page => "/admin/pages/",
        Kind::Post => "/admin/posts/",
    }
}

fn new_href(kind: Kind) -> &'static str {
    match kind {
        Kind::Page => "/admin/pages/new",
        Kind::Post => "/admin/posts/new",
    }
}

// ---------- 列表 ----------

pub struct Row {
    title: String,
    edit_href: String,
    date: String,
    mark: Option<&'static str>,
}

#[derive(Template)]
#[template(path = "admin/list.html")]
struct ListPage<'a> {
    admin: AdminLayout<'a>,
    heading: &'static str,
    new_href: &'static str,
    rows: Vec<Row>,
}

pub async fn pages(state: State<AppState>, admin: Extension<AdminSession>) -> AppResult {
    list(state, admin, Kind::Page).await
}

pub async fn posts(state: State<AppState>, admin: Extension<AdminSession>) -> AppResult {
    list(state, admin, Kind::Post).await
}

/// 未发布的草稿在最前，其余按发布时间倒序（§9.1）；日期列在没发布过时显示 `—`（D67）。
async fn list(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    kind: Kind,
) -> AppResult {
    let blog = state.blog.blog().await?;
    let now = state.clock.now();
    let rows = state
        .blog
        .list(kind)
        .await?
        .into_iter()
        .map(|p| Row {
            edit_href: format!("/admin/edit/{}", p.id.0),
            date: p
                .published_at
                .map_or_else(|| "—".to_owned(), |t| date::format_date(t, &blog.timezone)),
            mark: if p.draft {
                Some("草稿")
            } else if p.published_at.is_some_and(|t| t > now) {
                Some("定时")
            } else {
                None
            },
            title: p.title,
        })
        .collect();
    let heading = match kind {
        Kind::Page => "Pages",
        Kind::Post => "Blog",
    };
    let page = ListPage {
        admin: AdminLayout::new(&state.assets, heading, Some(&admin.csrf)),
        heading,
        new_href: new_href(kind),
        rows,
    };
    Ok(Html(page.render()?).into_response())
}

// ---------- 编辑页 ----------

#[derive(Template)]
#[template(path = "admin/edit.html")]
struct EditPage<'a> {
    admin: AdminLayout<'a>,
    csrf: String,
    heading: String,
    action: String,
    target: &'static str,
    id: Option<i64>,
    source: String,
    error: Option<String>,
    saved: bool,
    timezone_changed: bool,
    delete_href: Option<String>,
}

/// 编辑页的内容；`error` 不为空时是保存失败，返回 422 并保留提交的原文（§9.2）。
struct Editor {
    heading: String,
    action: String,
    target: Target,
    id: Option<PostId>,
    source: String,
    error: Option<String>,
    flags: Flags,
}

/// 保存成功后跳回编辑页时带的参数（§9.2、D44）。
#[derive(Debug, Default, Deserialize)]
pub struct Flags {
    saved: Option<String>,
    timezone: Option<String>,
}

fn render_editor(state: &AppState, admin: &AdminSession, editor: Editor) -> AppResult {
    let status = if editor.error.is_some() {
        StatusCode::UNPROCESSABLE_ENTITY
    } else {
        StatusCode::OK
    };
    let mut layout = AdminLayout::new(&state.assets, editor.heading.clone(), Some(&admin.csrf));
    layout.editor_js = Some(&state.assets.editor.url);
    let page = EditPage {
        admin: layout,
        csrf: admin.csrf.clone(),
        heading: editor.heading,
        action: editor.action,
        target: editor.target.as_str(),
        id: editor.id.map(|id| id.0),
        source: editor.source,
        error: editor.error,
        saved: editor.flags.saved.is_some(),
        timezone_changed: editor.flags.timezone.is_some(),
        delete_href: editor.id.map(|id| format!("/admin/delete/{}", id.0)),
    };
    Ok((status, Html(page.render()?)).into_response())
}

#[derive(Deserialize)]
pub struct SourceForm {
    source: String,
}

/// 首页（D11），站点设置写在它的 front matter 里。
pub async fn home(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Query(flags): Query<Flags>,
) -> AppResult {
    let blog = state.blog.blog().await?;
    render_editor(&state, &admin, home_editor(blog.home_source, None, flags))
}

fn home_editor(source: String, error: Option<String>, flags: Flags) -> Editor {
    Editor {
        heading: "首页".to_owned(),
        action: "/admin/".to_owned(),
        target: Target::Home,
        id: None,
        source,
        error,
        flags,
    }
}

/// 保存首页。时区变了时提示执行 `cat-blog rerender`（D44）。
pub async fn save_home(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Form(form): Form<SourceForm>,
) -> AppResult {
    let home = match save::prepare_home(&state.renderer, &form.source) {
        Ok(home) => home,
        Err(error) => {
            let editor = home_editor(form.source, Some(error.to_string()), Flags::default());
            return render_editor(&state, &admin, editor);
        }
    };
    let old = state.blog.blog().await?;
    state.blog.update_home(&home, state.clock.now()).await?;
    let location = if home.timezone.iana_name() == old.timezone.iana_name() {
        "/admin/?saved=1"
    } else {
        "/admin/?saved=1&timezone=changed"
    };
    Ok(Redirect::to(location).into_response())
}

fn new_editor(kind: Kind, source: String, error: Option<String>) -> Editor {
    Editor {
        heading: format!("新建{}", label(kind)),
        action: new_href(kind).to_owned(),
        target: Target::of(kind),
        id: None,
        source,
        error,
        flags: Flags::default(),
    }
}

fn post_editor(post: &Post, source: String, error: Option<String>, flags: Flags) -> Editor {
    Editor {
        heading: format!("编辑{}", label(post.kind)),
        action: format!("/admin/edit/{}", post.id.0),
        target: Target::of(post.kind),
        id: Some(post.id),
        source,
        error,
        flags,
    }
}

pub async fn new_page(state: State<AppState>, admin: Extension<AdminSession>) -> AppResult {
    let editor = new_editor(Kind::Page, NEW_PAGE.to_owned(), None);
    render_editor(&state, &admin, editor)
}

pub async fn new_post(state: State<AppState>, admin: Extension<AdminSession>) -> AppResult {
    let editor = new_editor(Kind::Post, NEW_POST.to_owned(), None);
    render_editor(&state, &admin, editor)
}

pub async fn create_page(
    state: State<AppState>,
    admin: Extension<AdminSession>,
    form: Form<SourceForm>,
) -> AppResult {
    create(state, admin, form, Kind::Page).await
}

pub async fn create_post(
    state: State<AppState>,
    admin: Extension<AdminSession>,
    form: Form<SourceForm>,
) -> AppResult {
    create(state, admin, form, Kind::Post).await
}

/// 第一次保存：确定 slug 并写回原文（D39），成功后 303 到它的编辑页。
async fn create(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Form(form): Form<SourceForm>,
    kind: Kind,
) -> AppResult {
    let now = state.clock.now();
    let tz = state.blog.blog().await?.timezone;
    let entry = match save::prepare_entry(&state.renderer, &form.source, kind, &tz, None, now) {
        Ok(entry) => entry,
        Err(error) => {
            let editor = new_editor(kind, form.source, Some(error.to_string()));
            return render_editor(&state, &admin, editor);
        }
    };
    match state.blog.insert_post(kind, &entry, now).await {
        Ok(id) => Ok(saved(id)),
        Err(WriteError::SlugTaken) => {
            let error = entry.slug_taken().to_string();
            render_editor(&state, &admin, new_editor(kind, form.source, Some(error)))
        }
        Err(WriteError::Db(error)) => Err(error.into()),
    }
}

pub async fn edit(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Path(id): Path<i64>,
    Query(flags): Query<Flags>,
) -> AppResult {
    let Some(post) = state.blog.post(PostId(id)).await? else {
        return not_found(&state).await;
    };
    let source = post.source.clone();
    render_editor(&state, &admin, post_editor(&post, source, None, flags))
}

/// 保存已有的页面或文章。slug 沿用库里的，改标题不改地址（D39）。
pub async fn update(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Path(id): Path<i64>,
    Form(form): Form<SourceForm>,
) -> AppResult {
    let Some(post) = state.blog.post(PostId(id)).await? else {
        return not_found(&state).await;
    };
    let tz = state.blog.blog().await?.timezone;
    let now = state.clock.now();
    let prepared = save::prepare_entry(
        &state.renderer,
        &form.source,
        post.kind,
        &tz,
        Some(post.stored()),
        now,
    );
    let entry = match prepared {
        Ok(entry) => entry,
        Err(error) => {
            let error = Some(error.to_string());
            let editor = post_editor(&post, form.source, error, Flags::default());
            return render_editor(&state, &admin, editor);
        }
    };
    match state.blog.update_post(post.id, &entry, now).await {
        Ok(true) => Ok(saved(post.id)),
        Ok(false) => not_found(&state).await,
        Err(WriteError::SlugTaken) => {
            let error = entry.slug_taken().to_string();
            let editor = post_editor(&post, form.source, Some(error), Flags::default());
            render_editor(&state, &admin, editor)
        }
        Err(WriteError::Db(error)) => Err(error.into()),
    }
}

fn saved(id: PostId) -> axum::response::Response {
    Redirect::to(&format!("/admin/edit/{}?saved=1", id.0)).into_response()
}

// ---------- 删除 ----------

#[derive(Template)]
#[template(path = "admin/delete.html")]
struct DeletePage<'a> {
    admin: AdminLayout<'a>,
    csrf: String,
    label: &'static str,
    title: String,
    action: String,
    back: String,
}

/// 删除确认页（D76），不改数据。
pub async fn confirm_delete(
    State(state): State<AppState>,
    Extension(admin): Extension<AdminSession>,
    Path(id): Path<i64>,
) -> AppResult {
    let Some(post) = state.blog.post(PostId(id)).await? else {
        return not_found(&state).await;
    };
    let page = DeletePage {
        admin: AdminLayout::new(&state.assets, "删除确认", Some(&admin.csrf)),
        csrf: admin.csrf.clone(),
        label: label(post.kind),
        title: post.title,
        action: format!("/admin/delete/{id}"),
        back: format!("/admin/edit/{id}"),
    };
    Ok(Html(page.render()?).into_response())
}

/// 确认后从数据库删除，回到列表。
pub async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> AppResult {
    let Some(post) = state.blog.post(PostId(id)).await? else {
        return not_found(&state).await;
    };
    if !state.blog.delete_post(post.id).await? {
        return not_found(&state).await;
    }
    Ok(Redirect::to(list_href(post.kind)).into_response())
}
