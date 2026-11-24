//! front matter 的 `date`、发布时间和日期显示（D43、D45，docs/v1.0-spec.md §5.4）。

use std::fmt;

use jiff::Timestamp;
use jiff::civil::{Date, DateTime};
use jiff::tz::TimeZone;

/// 日期或时区写错了。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DateError {
    InvalidDate(String),
    InvalidTimeZone(String),
}

impl fmt::Display for DateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDate(s) => write!(
                f,
                "date \"{s}\" 无法识别，请写成 2026-11-24、2026-11-24 09:30 \
                 或 2026-11-24T09:30:00+08:00"
            ),
            Self::InvalidTimeZone(s) => write!(
                f,
                "timezone \"{s}\" 不是 IANA 时区名，请写成 Asia/Singapore 这样的形式"
            ),
        }
    }
}

impl std::error::Error for DateError {}

/// 按 IANA 名称查找时区，用程序自带的时区数据库，不依赖系统 tzdata。
pub fn parse_timezone(name: &str) -> Result<TimeZone, DateError> {
    TimeZone::get(name).map_err(|_| DateError::InvalidTimeZone(name.to_owned()))
}

/// 解析 D43 的三种写法。前两种按站点时区 `tz` 解释，只写日期时取当天 00:00。
/// 夏令时拨快时不存在的本地时间顺延到拨快之后，重复的本地时间取较早的一次。
pub fn parse_date(s: &str, tz: &TimeZone) -> Result<Timestamp, DateError> {
    let invalid = || DateError::InvalidDate(s.to_owned());
    let local = if let Ok(date) = Date::strptime("%Y-%m-%d", s) {
        date.to_datetime(jiff::civil::Time::midnight())
    } else if let Ok(dt) = DateTime::strptime("%Y-%m-%d %H:%M", s) {
        dt
    } else {
        return s.parse::<Timestamp>().map_err(|_| invalid());
    };
    tz.to_ambiguous_timestamp(local)
        .compatible()
        .map_err(|_| invalid())
}

/// 按站点时区显示为 `2026-11-24`（D34）。
pub fn format_date(ts: Timestamp, tz: &TimeZone) -> String {
    ts.to_zoned_datetime(tz.clone())
        .strftime("%Y-%m-%d")
        .to_string()
}

/// 保存时计算 `published_at`（§5.4）：写了 `date` 时以它为准；否则沿用库里已有的值
/// （撤回后再发布也一样，D45）；都没有且不是草稿时取 `now`；其余情况为空。
pub fn published_at(
    date: Option<Timestamp>,
    stored: Option<Timestamp>,
    draft: bool,
    now: Timestamp,
) -> Option<Timestamp> {
    date.or(stored).or((!draft).then_some(now))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tz(name: &str) -> TimeZone {
        parse_timezone(name).unwrap()
    }

    fn ts(rfc3339: &str) -> Timestamp {
        rfc3339.parse().unwrap()
    }

    #[test]
    fn three_accepted_forms() {
        let sg = tz("Asia/Singapore");
        assert_eq!(
            parse_date("2026-11-24", &sg),
            Ok(ts("2026-11-23T16:00:00Z"))
        );
        assert_eq!(
            parse_date("2026-11-24 09:30", &sg),
            Ok(ts("2026-11-24T01:30:00Z"))
        );
        assert_eq!(
            parse_date("2026-11-24T09:30:00+09:00", &sg),
            Ok(ts("2026-11-24T00:30:00Z"))
        );
        assert_eq!(
            parse_date("2026-11-24T09:30:00Z", &sg),
            Ok(ts("2026-11-24T09:30:00Z"))
        );
        assert_eq!(
            parse_date("2026-11-24t09:30:00.5+08:00", &sg),
            Ok(ts("2026-11-24T01:30:00.5Z"))
        );
    }

    #[test]
    fn malformed_dates_are_rejected() {
        let sg = tz("Asia/Singapore");
        for bad in [
            "2026-13-01",
            "2026-02-30",
            "24/11/2026",
            "2026-11-24 9",
            "tomorrow",
            "",
        ] {
            assert_eq!(
                parse_date(bad, &sg),
                Err(DateError::InvalidDate(bad.into())),
                "{bad}"
            );
        }
    }

    #[test]
    fn date_only_is_local_midnight_in_site_timezone() {
        let ny = tz("America/New_York");
        assert_eq!(
            parse_date("2026-07-01", &ny),
            Ok(ts("2026-07-01T04:00:00Z"))
        );
        assert_eq!(
            parse_date("2026-12-01", &ny),
            Ok(ts("2026-12-01T05:00:00Z"))
        );
    }

    #[test]
    fn daylight_saving_boundaries_in_new_york() {
        let ny = tz("America/New_York");
        assert_eq!(
            parse_date("2026-03-08", &ny),
            Ok(ts("2026-03-08T05:00:00Z"))
        );
        assert_eq!(
            parse_date("2026-03-08 01:59", &ny),
            Ok(ts("2026-03-08T06:59:00Z"))
        );
        assert_eq!(
            parse_date("2026-03-08 02:30", &ny),
            Ok(ts("2026-03-08T07:30:00Z"))
        );
        assert_eq!(
            parse_date("2026-03-08 03:00", &ny),
            Ok(ts("2026-03-08T07:00:00Z"))
        );
        assert_eq!(
            parse_date("2026-11-01 01:30", &ny),
            Ok(ts("2026-11-01T05:30:00Z"))
        );
        assert_eq!(
            parse_date("2026-11-01 02:00", &ny),
            Ok(ts("2026-11-01T07:00:00Z"))
        );
    }

    #[test]
    fn unknown_timezone_is_rejected() {
        assert!(parse_timezone("Asia/Singapore").is_ok());
        for bad in ["Mars/Olympus", "", "asia singapore"] {
            assert_eq!(
                parse_timezone(bad).err(),
                Some(DateError::InvalidTimeZone(bad.into()))
            );
        }
    }

    #[test]
    fn display_date_uses_site_timezone() {
        let instant = ts("2026-11-23T17:00:00Z");
        assert_eq!(format_date(instant, &tz("Asia/Singapore")), "2026-11-24");
        assert_eq!(format_date(instant, &tz("America/New_York")), "2026-11-23");
    }

    #[test]
    fn first_publish_takes_now() {
        let now = ts("2026-11-24T00:00:00Z");
        assert_eq!(published_at(None, None, false, now), Some(now));
        assert_eq!(published_at(None, None, true, now), None);
    }

    #[test]
    fn republishing_after_unpublish_keeps_first_time() {
        let first = ts("2026-01-01T00:00:00Z");
        let now = ts("2026-11-24T00:00:00Z");
        assert_eq!(published_at(None, Some(first), true, now), Some(first));
        assert_eq!(published_at(None, Some(first), false, now), Some(first));
    }

    #[test]
    fn explicit_date_wins() {
        let date = ts("2027-01-01T00:00:00Z");
        let stored = ts("2026-01-01T00:00:00Z");
        let now = ts("2026-11-24T00:00:00Z");
        assert_eq!(
            published_at(Some(date), Some(stored), false, now),
            Some(date)
        );
        assert_eq!(published_at(Some(date), None, true, now), Some(date));
    }
}
