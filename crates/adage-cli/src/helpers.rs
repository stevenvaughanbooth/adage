//! Small helpers every command module shares: reading a flag's value off the argument list, and
//! the few words several surfaces put on times and counts. One copy each, so two surfaces never
//! drift apart on how they say the same thing.

/// The value after `name` (`--repo <path>`), if the flag is given with one.
pub fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// Every value given after `name`, for a flag that may repeat (`--outputs a --outputs b`).
pub fn flags(args: &[String], name: &str) -> Vec<String> {
    args.windows(2).filter(|w| w[0] == name).map(|w| w[1].clone()).collect()
}

/// How long before `now` `ts` was (both ms since the epoch), in a card's words: `just now`,
/// `4 min ago`, `2 h ago`, `3 days ago`.
pub fn ago(now: u64, ts: u64) -> String {
    let s = now.saturating_sub(ts) / 1000;
    match s {
        0..=59 => "just now".into(),
        60..=3599 => format!("{} min ago", s / 60),
        3600..=172_799 => format!("{} h ago", s / 3600),
        _ => format!("{} days ago", s / 86_400),
    }
}

/// [`ago`] in a check's terse form, where several share a line: `12s ago`, `4m ago`, `2h ago`,
/// `3d ago`.
pub fn ago_short(now: u64, ts: u64) -> String {
    let s = now.saturating_sub(ts) / 1000;
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=172_799 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// `1 change`, `3 changes`.
pub fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{n} {}", if n == 1 { one } else { many })
}
