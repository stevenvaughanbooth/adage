//! `$ADAGE_HOME/learn.log` as the conditioning ticker: what each run changed.
//!
//! Two shapes are read, and any field may be missing:
//!
//! * the live contract: `{"ts", "kind": "learn", "trigger": "auto"|"manual", "repo",
//!   "sessions_read", "sessions_used", "duration_ms", "index_generation", "changes": [{"route",
//!   "track", "change": "new"|"retired"|"aspect"|"evidence"|"draft"|"proposal"|"stale", "from", "to",
//!   "summary"}]}` and `{"ts", "kind": "learn_error", "repo", "class", "error"}`;
//! * the older per-run summary (no `kind`): `{"ts", "sessions_read", "sessions_used",
//!   "index_generation", "repos": {repo: {"sessions", "tracks": [{"name", "version", "change",
//!   "routes"}]}}}`.
//!
//! Anything else (a line that is not JSON, an unknown `kind`) is skipped. The stored words stay
//! (`learn`, `green`, `draft`); lines are told in the lexicon's (conditioning, fire, nascent).

use super::theme::{parse_aspect, Theme};
use adage_tracks::Aspect;
use regex::Regex;
use serde_json::Value;
use std::sync::OnceLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A run's summary line.
    Run,
    /// One change to a reflex.
    Change,
    Error,
}

/// One change to one reflex, in display words.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    pub route: String,
    pub track: String,
    /// What happened: `strength`, `new`, `evidence`, `nascent`, `dormant`, `extinguished`,
    /// `proposal`, `written`.
    pub what: &'static str,
    /// The word before and after (`prime` → `fire`, `firm` → `dormant`).
    pub from: Option<String>,
    pub to: Option<String>,
    /// The evidence, as counts: `24/25`, `9/9 · lb 0.70`, `6/8 → 7/8`.
    pub numbers: String,
}

/// A run's summary.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Run {
    pub auto: bool,
    pub read: Option<u64>,
    pub used: Option<u64>,
    pub duration_ms: Option<u64>,
    pub changes: Option<usize>,
    pub generation: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub ts: u64,
    pub tone: Tone,
    /// The whole line as text (job results, filters).
    pub text: String,
    /// The same, without the repository or track (for narrow panes).
    pub short: String,
    /// The strength a change leaves the reflex at, for colour.
    pub aspect: Option<Aspect>,
    pub repo: Option<String>,
    pub change: Option<Change>,
    pub run: Option<Run>,
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v[k].as_str().filter(|x| !x.is_empty())
}

fn short_gen(g: &str) -> &str {
    &g[..g.len().min(8)]
}

/// Seconds or milliseconds: `0.73 s`, `640 ms`.
pub fn duration(ms: u64) -> String {
    if ms >= 1000 {
        format!("{:.1} s", ms as f64 / 1000.0)
    } else {
        format!("{:.2} s", ms as f64 / 1000.0)
    }
}

/// The ticker lines for one log line (a run's summary first, then one per change).
pub fn parse(line: &str) -> Vec<Tick> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    let ts = v["ts"].as_u64().unwrap_or(0);
    let repo = s(&v, "repo").map(String::from);
    match v["kind"].as_str() {
        Some("learn_error") => {
            let class = s(&v, "class").map(|c| format!(" ({c})")).unwrap_or_default();
            let err: String = s(&v, "error").unwrap_or("unknown error").chars().take(200).collect();
            let on = repo.as_deref().map(|r| format!(" for {r}")).unwrap_or_default();
            vec![Tick {
                ts,
                tone: Tone::Error,
                text: format!("conditioning failed{on}{class}: {err}"),
                short: format!("conditioning failed{class}: {err}"),
                aspect: None,
                repo,
                change: None,
                run: None,
            }]
        }
        Some("learn") | None if v.is_object() => run(&v, ts, repo),
        _ => vec![],
    }
}

fn run(v: &Value, ts: u64, repo: Option<String>) -> Vec<Tick> {
    let auto = s(v, "trigger") == Some("auto");
    let head = if auto { "auto-condition" } else { "condition" };
    let long_head = match &repo {
        Some(r) => format!("{head} {r}"),
        None => head.to_string(),
    };
    let mut info = Run {
        auto,
        read: v["sessions_read"].as_u64(),
        used: v["sessions_used"].as_u64(),
        duration_ms: v["duration_ms"].as_u64(),
        changes: None,
        generation: s(v, "index_generation").map(|g| short_gen(g).to_string()),
    };
    let mut parts = Vec::new();
    if let (Some(read), Some(used)) = (info.read, info.used) {
        parts.push(format!("{used}/{read} traces used"));
    }
    if let Some(ms) = info.duration_ms {
        parts.push(duration(ms));
    }
    let changes: Vec<&Value> = v["changes"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    // The older summary lists tracks per repository instead of changes (newer lines keep it too,
    // beside `changes`, which then says it better).
    let mut tracks = Vec::new();
    let legacy = if v["changes"].is_array() { None } else { v["repos"].as_object() };
    for (r, rr) in legacy.into_iter().flatten() {
        for t in rr["tracks"].as_array().into_iter().flatten() {
            let name = t["name"].as_str().unwrap_or("?").to_string();
            let change = t["change"].as_str().unwrap_or("written");
            let n = t["routes"].as_u64().map(|n| format!("{n} reflex{}", if n == 1 { "" } else { "es" }));
            tracks.push((r.clone(), name, t["version"].as_str().unwrap_or("?").to_string(), change.to_string(), n));
        }
    }
    if v["changes"].is_array() {
        info.changes = Some(changes.len());
        parts.push(match changes.len() {
            0 => "no reflex changed".to_string(),
            1 => "1 change".to_string(),
            n => format!("{n} changes"),
        });
    } else if !tracks.is_empty() {
        parts.push(format!("{} track{}", tracks.len(), if tracks.len() == 1 { "" } else { "s" }));
    }
    if let Some(g) = &info.generation {
        parts.push(format!("gen {g}"));
    }
    let tail = if parts.is_empty() { String::new() } else { format!(": {}", parts.join(", ")) };
    let mut out = vec![Tick {
        ts,
        tone: Tone::Run,
        text: format!("{long_head}{tail}"),
        short: format!("{head}{tail}"),
        aspect: None,
        repo: repo.clone(),
        change: None,
        run: Some(info),
    }];
    for c in changes {
        out.push(change(c, ts, repo.clone()));
    }
    for (r, name, version, change, n) in tracks {
        let numbers = n.unwrap_or_default();
        let text = format!(
            "wrote {name} @ {version} ({change}{})",
            if numbers.is_empty() { String::new() } else { format!(", {numbers}") }
        );
        out.push(Tick {
            ts,
            tone: Tone::Change,
            short: text.clone(),
            text,
            aspect: None,
            repo: Some(r),
            change: Some(Change {
                route: format!("{name}@{version}"),
                track: name,
                what: "written",
                from: None,
                to: Some(change),
                numbers,
            }),
            run: None,
        });
    }
    out
}

/// The counts in a summary: `(9 of 9 sessions, lower bound 0.70)` → `9/9 · lb 0.70`;
/// `evidence 6/8 → 7/8 sessions` → `6/8 → 7/8`.
pub fn numbers(summary: &str, evidence: bool) -> String {
    static OF: OnceLock<Regex> = OnceLock::new();
    static SLASH: OnceLock<Regex> = OnceLock::new();
    static LB: OnceLock<Regex> = OnceLock::new();
    let of = OF.get_or_init(|| Regex::new(r"(\d+) of (\d+)").expect("regex"));
    let slash = SLASH.get_or_init(|| Regex::new(r"\d+/\d+").expect("regex"));
    let lb = LB.get_or_init(|| Regex::new(r"lower bound ([0-9.]+)").expect("regex"));
    if evidence {
        let all: Vec<&str> = slash.find_iter(summary).map(|m| m.as_str()).collect();
        return match all.as_slice() {
            [a, b, ..] => format!("{a} → {b}"),
            [a] => a.to_string(),
            [] => String::new(),
        };
    }
    let scope = summary.rfind('(').map_or(summary, |i| &summary[i..]);
    let count = of
        .captures(scope)
        .map(|c| format!("{}/{}", &c[1], &c[2]))
        .or_else(|| slash.find(scope).map(|m| m.as_str().to_string()));
    let bound = lb.captures(scope).map(|c| format!("lb {}", &c[1]));
    [count, bound].into_iter().flatten().collect::<Vec<_>>().join(" · ")
}

fn strength_of(x: Option<&str>) -> Option<String> {
    x.map(|w| parse_aspect(w).map_or_else(|| w.to_string(), |a| Theme::strength_word(a).to_string()))
}

fn change(c: &Value, ts: u64, repo: Option<String>) -> Tick {
    let route = s(c, "route").unwrap_or("a reflex").to_string();
    let track = s(c, "track").unwrap_or("").to_string();
    let (from, to) = (s(c, "from"), s(c, "to"));
    let kind = s(c, "change").unwrap_or("");
    let summary = s(c, "summary").unwrap_or("");
    let (what, from_w, to_w) = match kind {
        "aspect" => ("strength", strength_of(from), strength_of(to)),
        "new" => ("new", None, strength_of(to)),
        "evidence" => ("evidence", None, None),
        "draft" => ("nascent", None, Some("nascent".to_string())),
        "stale" => ("dormant", Some("firm".to_string()), Some("dormant".to_string())),
        "retired" => ("extinguished", None, Some("extinguished".to_string())),
        "proposal" => ("proposal", None, None),
        _ => ("changed", strength_of(from), strength_of(to)),
    };
    let mut nums = numbers(summary, kind == "evidence");
    if nums.is_empty() && kind == "proposal" && !summary.is_empty() {
        nums = summary.to_string();
    }
    let transition = match (what, &from_w, &to_w) {
        ("new", _, Some(t)) => format!("new · {t}"),
        ("evidence", _, _) => "evidence".to_string(),
        ("proposal", _, _) => "change proposed".to_string(),
        (_, Some(f), Some(t)) => format!("{f} → {t}"),
        (_, None, Some(t)) => format!("→ {t}"),
        _ => what.to_string(),
    };
    let nums_s = if nums.is_empty() { String::new() } else { format!(" ({nums})") };
    let long_track = if track.is_empty() { String::new() } else { format!("{track}: ") };
    let short_track = if track.is_empty() { String::new() } else { format!("{}: ", super::words::short_track(&track)) };
    Tick {
        ts,
        tone: Tone::Change,
        text: format!("{long_track}{route} {transition}{nums_s}"),
        short: format!("{short_track}{route} {transition}{nums_s}"),
        aspect: to.and_then(parse_aspect),
        repo,
        change: Some(Change { route, track, what, from: from_w, to: to_w, numbers: nums }),
        run: None,
    }
}
