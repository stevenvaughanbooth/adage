//! Small text helpers every module shares: words, relative times, clock times, token estimates.

/// The lowercase alphanumeric words of `s`, in order, with `#412`-style numbers and `MKT-1182`-style
/// keys kept whole (`#412` as `#412`, `mkt-1182` as one word).
pub fn words(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let flush = |cur: &mut String, out: &mut Vec<String>| {
        if !cur.is_empty() {
            out.push(std::mem::take(cur));
        }
    };
    for c in s.chars() {
        let keep = c.is_alphanumeric() || c == '#' && cur.is_empty() || c == '-' && !cur.is_empty();
        if keep {
            cur.extend(c.to_lowercase());
        } else {
            flush(&mut cur, &mut out);
        }
    }
    flush(&mut cur, &mut out);
    out.into_iter().map(|w| w.trim_end_matches('-').to_string()).filter(|w| !w.is_empty()).collect()
}

/// Whether `needle`'s words occur consecutively in `hay`'s words.
pub fn contains_phrase(hay: &[String], needle: &[String]) -> bool {
    !needle.is_empty() && hay.windows(needle.len()).any(|w| w == needle)
}

/// Words that carry no reference on their own.
pub const STOP: &[&str] = &[
    "a",
    "an",
    "the",
    "of",
    "to",
    "in",
    "on",
    "for",
    "from",
    "and",
    "or",
    "with",
    "at",
    "by",
    "is",
    "it",
    "that",
    "this",
    "those",
    "these",
    "we",
    "i",
    "you",
    "me",
    "my",
    "our",
    "us",
    "up",
    "out",
    "about",
    "into",
    "as",
    "be",
    "was",
    "were",
    "are",
    "do",
    "did",
    "does",
    "can",
    "could",
    "please",
    "let",
    "lets",
    "just",
    "now",
    "then",
    "what",
    "which",
    "who",
    "how",
    "when",
    "where",
    "there",
    "here",
    "earlier",
    "today",
    "yesterday",
    "morning",
    "afternoon",
    "meeting",
    "sync",
    "call",
    "notes",
    "file",
    "pr",
    "ticket",
    "doc",
    "document",
    "transcript",
];

/// `words(s)` less the stop words and words shorter than three characters (keys and numbers kept).
pub fn content_words(s: &str) -> Vec<String> {
    words(s)
        .into_iter()
        .filter(|w| w.starts_with('#') || w.contains('-') || (w.len() >= 3 && !STOP.contains(&w.as_str())))
        .collect()
}

/// "just now", "12 min ago", "2 h ago", "3 days ago".
pub fn ago(now: u64, ts: u64) -> String {
    let d = now.saturating_sub(ts) / 1000;
    match d {
        0..=59 => "just now".into(),
        60..=5_399 => format!("{} min ago", d / 60),
        5_400..=172_799 => format!("{} h ago", (d + 1_800) / 3_600),
        _ => format!("{} days ago", d / 86_400),
    }
}

/// "in 4 min", "in 2 h".
pub fn until(now: u64, ts: u64) -> String {
    let d = ts.saturating_sub(now) / 1000;
    match d {
        0..=59 => "now".into(),
        60..=5_399 => format!("in {} min", d / 60),
        _ => format!("in {} h", (d + 1_800) / 3_600),
    }
}

/// `HH:MM` in the person's local time (`tz_offset_min` from UTC).
pub fn hhmm(ts: u64, tz_offset_min: i32) -> String {
    let local = (ts / 1000) as i64 + i64::from(tz_offset_min) * 60;
    let day = local.rem_euclid(86_400);
    format!("{:02}:{:02}", day / 3_600, (day % 3_600) / 60)
}

/// The day number (days since the epoch) of `ts` in local time: two timestamps on one day share it.
pub fn local_day(ts: u64, tz_offset_min: i32) -> i64 {
    ((ts / 1000) as i64 + i64::from(tz_offset_min) * 60).div_euclid(86_400)
}

/// An estimate of a text's tokens: characters / 4, rounded up.
pub fn tokens(s: &str) -> u32 {
    u32::try_from(s.chars().count().div_ceil(4)).unwrap_or(u32::MAX)
}

/// The opening words of the line that says who a text is from; [`fit`] keeps it.
pub const FROM_ADAGE: &str = "This note is from Adage, not from the user.";

/// `s` cut to at most `budget` tokens, on a line boundary, never cutting the closing tag of an
/// `<adage-world>` block or the line that says it is from Adage: the last whole body lines are
/// dropped until it fits.
pub fn fit(s: &str, budget: u32) -> String {
    if tokens(s) <= budget {
        return s.to_string();
    }
    let lines: Vec<&str> = s.lines().collect();
    let mut tail: Vec<&str> = Vec::new();
    let mut body: &[&str] = &lines;
    if let Some(l) = body.last().filter(|l| l.starts_with("</")) {
        tail.insert(0, l);
        body = &body[..body.len() - 1];
    }
    if let Some(l) = body.last().filter(|l| l.starts_with(FROM_ADAGE)) {
        tail.insert(0, l);
        body = &body[..body.len() - 1];
    }
    let mut kept: Vec<&str> = body.to_vec();
    while kept.len() > 1 {
        let text = join(&kept, &tail);
        if tokens(&text) <= budget {
            return text;
        }
        kept.pop();
    }
    join(&kept, &tail)
}

fn join(lines: &[&str], tail: &[&str]) -> String {
    lines.iter().chain(tail.iter()).copied().collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_keep_keys_and_numbers_whole() {
        assert_eq!(
            words("Fix MKT-1182 and review #412, then the Q4-plan."),
            ["fix", "mkt-1182", "and", "review", "#412", "then", "the", "q4-plan"]
        );
        assert_eq!(content_words("file the follow-ups from the marketing sync"), ["follow-ups", "marketing"]);
    }

    #[test]
    fn phrases_match_in_order() {
        let hay = words("notes from the marketing sync today");
        assert!(contains_phrase(&hay, &words("marketing sync")));
        assert!(!contains_phrase(&hay, &words("sync marketing")));
        assert!(!contains_phrase(&hay, &[]));
    }

    #[test]
    fn times_read_as_a_person_says_them() {
        let now = 1_000_000_000;
        assert_eq!(ago(now, now - 30_000), "just now");
        assert_eq!(ago(now, now - 12 * 60_000), "12 min ago");
        assert_eq!(ago(now, now - 2 * 3_600_000), "2 h ago");
        assert_eq!(ago(now, now - 3 * 86_400_000), "3 days ago");
        assert_eq!(until(now, now + 4 * 60_000), "in 4 min");
        assert_eq!(hhmm(0, 0), "00:00");
        assert_eq!(hhmm(13 * 3_600_000, -120), "11:00");
        assert_eq!(local_day(23 * 3_600_000, 120), 1, "an hour past midnight local time is the next day");
    }

    #[test]
    fn fit_drops_whole_lines_and_keeps_the_closing_tag() {
        let s =
            "<adage-world kind=\"snapshot\">\nline one is here\nline two is here\nline three is here\n</adage-world>";
        let fitted = fit(s, 16);
        assert!(fitted.ends_with("</adage-world>"), "{fitted}");
        assert!(tokens(&fitted) <= 16, "{fitted}");
        assert!(!fitted.contains("line three"));
        assert!(fitted.contains("line one"));
        assert_eq!(fit("short", 10), "short");
    }
}
