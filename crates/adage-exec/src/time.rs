//! UTC timestamps for the approval store, without a date crate.

use std::time::{SystemTime, UNIX_EPOCH};

/// `YYYY-MM-DDTHH:MM:SSZ` for seconds since the epoch.
pub fn rfc3339(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, (rem % 3600) / 60, rem % 60)
}

/// Seconds since the epoch for a `YYYY-MM-DDTHH:MM:SSZ` [`rfc3339`] wrote; `None` for any other
/// spelling.
pub fn parse(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() != 20 || b[4] != b'-' || b[7] != b'-' || b[10] != b'T' || b[13] != b':' || b[16] != b':' || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) || hh > 23 || mm > 59 || ss > 59 {
        return None;
    }
    // Howard Hinnant's days-from-civil.
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days * 86_400 + hh * 3600 + mm * 60 + ss).ok()
}

/// How long before `now` `then` was (both seconds since the epoch), as a card says it.
pub fn ago(now: u64, then: u64) -> String {
    match now.saturating_sub(then) {
        0..=59 => "just now".into(),
        s @ 60..=3599 => format!("{} min ago", s / 60),
        s @ 3600..=172_799 => format!("{} h ago", s / 3600),
        s => format!("{} days ago", s / 86_400),
    }
}

pub fn now() -> String {
    rfc3339(now_secs())
}

/// Seconds since the epoch now (0 on a clock before it).
pub(crate) fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    #[test]
    fn known_dates() {
        assert_eq!(super::rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(super::rfc3339(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(super::rfc3339(1_790_000_000), "2026-09-21T14:13:20Z");
    }

    #[test]
    fn parse_reads_what_rfc3339_writes_and_nothing_else() {
        for secs in [0, 951_782_400, 1_790_000_000, 4_102_444_799] {
            assert_eq!(super::parse(&super::rfc3339(secs)), Some(secs));
        }
        for bad in ["", "2026-09-21", "2026-09-21T14:13:20+00:00", "2026-13-01T00:00:00Z", "2026-09-21T24:00:00Z"] {
            assert_eq!(super::parse(bad), None, "{bad}");
        }
        assert_eq!(super::ago(1_000, 1_000), "just now");
        assert_eq!(super::ago(1_000 + 240, 1_000), "4 min ago");
        assert_eq!(super::ago(1_000 + 7_200, 1_000), "2 h ago");
        assert_eq!(super::ago(1_000 + 3 * 86_400, 1_000), "3 days ago");
    }
}
