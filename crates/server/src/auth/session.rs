//! 会话 token、会话 cookie 和 CSRF token（D15、D16、D19、D36、D47、D49）。

use axum::http::{HeaderMap, header};
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use jiff::SignedDuration;
use rand::TryRng;
use rand::rngs::{SysError, SysRng};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::db::sessions::TokenHash;

/// 会话 cookie 的名字（docs/v1.0-spec.md §9.3）。
pub const COOKIE_NAME: &str = "cat_blog_session";

/// 会话从登录起固定有效 30 天，不随使用顺延（D49）。
pub const SESSION_TTL: SignedDuration = SignedDuration::from_hours(30 * 24);

/// 32 字节系统随机数的 base64url 编码（D19）。会话 token 和 CSRF token 都用它生成。
pub fn random_token() -> Result<String, SysError> {
    let mut bytes = [0u8; 32];
    SysRng.try_fill_bytes(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

/// 库里存的是 token 的 SHA-256（D16）。
pub fn hash_token(token: &str) -> TokenHash {
    Sha256::digest(token.as_bytes()).into()
}

/// 请求 `Cookie` 头里的会话 token。
pub fn token_from(headers: &HeaderMap) -> Option<&str> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == COOKIE_NAME && !value.is_empty()).then_some(value)
        })
}

/// 登录成功后下发的 cookie。`secure` 为真（`BASE_URL` 是 https）时带 `Secure`（D36）。
pub fn set_cookie(token: &str, secure: bool) -> String {
    cookie(token, SESSION_TTL.as_secs(), secure)
}

/// 退出登录时让浏览器删掉会话 cookie。
pub fn clear_cookie(secure: bool) -> String {
    cookie("", 0, secure)
}

fn cookie(value: &str, max_age: i64, secure: bool) -> String {
    let secure = if secure { "; Secure" } else { "" };
    format!("{COOKIE_NAME}={value}; Path=/; Max-Age={max_age}; HttpOnly; SameSite=Lax{secure}")
}

/// 表单提交的 `_csrf` 和会话里的是否相同，用常量时间比较（D47）。
pub fn csrf_matches(expected: &str, submitted: &str) -> bool {
    expected.as_bytes().ct_eq(submitted.as_bytes()).into()
}

#[cfg(test)]
mod tests {
    use axum::http::HeaderValue;

    use super::*;

    #[test]
    fn token_is_read_from_any_cookie_header() {
        let mut headers = HeaderMap::new();
        assert_eq!(token_from(&headers), None);
        headers.append(header::COOKIE, HeaderValue::from_static("theme=dark"));
        headers.append(
            header::COOKIE,
            HeaderValue::from_static("a=1; cat_blog_session=abc; b=2"),
        );
        assert_eq!(token_from(&headers), Some("abc"));
    }

    #[test]
    fn tokens_are_32_random_bytes() {
        let (a, b) = (random_token().unwrap(), random_token().unwrap());
        assert_ne!(a, b);
        assert_eq!(URL_SAFE_NO_PAD.decode(&a).unwrap().len(), 32);
    }
}
