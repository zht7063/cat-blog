-- 表结构见 docs/v1.0-spec.md §6。时间都是 UTC 的 Unix 秒（D71），布尔值用 INTEGER 0/1。

CREATE TABLE users (
    id            INTEGER PRIMARY KEY,
    username      TEXT    NOT NULL UNIQUE,
    password_hash TEXT    NOT NULL,              -- argon2id，PHC 字符串（D17）
    created_at    INTEGER NOT NULL,
    updated_at    INTEGER NOT NULL
);

CREATE TABLE sessions (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash  BLOB    NOT NULL UNIQUE,         -- 会话 token 的 SHA-256（D16）
    csrf_token  TEXT    NOT NULL,                -- D47
    created_at  INTEGER NOT NULL,
    expires_at  INTEGER NOT NULL                 -- created_at + 30 天（D49）
);

CREATE TABLE blogs (
    id          INTEGER PRIMARY KEY,
    user_id     INTEGER NOT NULL REFERENCES users(id),
    home_source TEXT    NOT NULL,                -- 首页原文
    home_html   TEXT    NOT NULL,                -- 首页正文渲染结果（含 posts 占位）
    title       TEXT    NOT NULL,
    description TEXT    NOT NULL,
    lang        TEXT    NOT NULL,
    timezone    TEXT    NOT NULL,
    nav_html    TEXT    NOT NULL,                -- D65
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE posts (
    id           INTEGER PRIMARY KEY,
    blog_id      INTEGER NOT NULL REFERENCES blogs(id) ON DELETE CASCADE,
    kind         TEXT    NOT NULL CHECK (kind IN ('page', 'post')),
    slug         TEXT    NOT NULL,               -- 规范化后的值
    source       TEXT    NOT NULL,               -- 原文（含写回的 slug）
    html         TEXT    NOT NULL,               -- 渲染并清洗后的正文（含 posts 占位）
    title        TEXT    NOT NULL,
    description  TEXT    NOT NULL,
    tags         TEXT    NOT NULL DEFAULT '[]',  -- JSON 数组（D46）
    draft        INTEGER NOT NULL,
    published_at INTEGER,                        -- 可为空（§5.4）
    created_at   INTEGER NOT NULL,
    updated_at   INTEGER NOT NULL,
    UNIQUE (blog_id, slug)
);

CREATE INDEX posts_by_kind_date ON posts (blog_id, kind, published_at);
CREATE INDEX sessions_by_expiry ON sessions (expires_at);
