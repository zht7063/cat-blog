//! 公开站点能否看到一篇内容（D12，docs/v1.0-spec.md §5.4）。

use jiff::Timestamp;

/// 不是草稿、已有发布时间、且发布时间不晚于 `now` 时可见。
pub fn is_visible(draft: bool, published_at: Option<Timestamp>, now: Timestamp) -> bool {
    !draft && published_at.is_some_and(|t| t <= now)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ts(s: &str) -> Timestamp {
        s.parse().unwrap()
    }

    #[test]
    fn published_in_the_past_is_visible() {
        let now = ts("2026-11-24T00:00:00Z");
        assert!(is_visible(false, Some(ts("2026-01-01T00:00:00Z")), now));
    }

    #[test]
    fn draft_is_hidden_even_when_published_before() {
        let now = ts("2026-11-24T00:00:00Z");
        assert!(!is_visible(true, Some(ts("2026-01-01T00:00:00Z")), now));
    }

    #[test]
    fn future_publish_time_is_hidden() {
        let now = ts("2026-11-24T00:00:00Z");
        assert!(!is_visible(false, Some(ts("2026-11-24T00:00:01Z")), now));
    }

    #[test]
    fn missing_publish_time_is_hidden() {
        assert!(!is_visible(false, None, ts("2026-11-24T00:00:00Z")));
    }

    #[test]
    fn publish_time_equal_to_now_is_visible() {
        let now = ts("2026-11-24T00:00:00Z");
        assert!(is_visible(false, Some(now), now));
    }
}
