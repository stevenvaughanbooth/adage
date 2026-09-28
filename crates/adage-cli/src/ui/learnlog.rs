//! `$ADAGE_HOME/learn.log` as ticker lines: what each learning run changed.
//!
//! Two shapes are read, and any field may be missing:
//!
//! * the live-learning contract: `{"ts", "kind": "condition", "trigger": "auto"|"manual", "repo",
//!   "sessions_read", "sessions_used", "duration_ms", "index_generation", "changes": [{"route",
//!   "track", "change": "new"|"extinguished"|"strength"|"evidence"|"nascent"|"proposal"|"dormant", "from", "to",
//!   "summary"}]}` and `{"ts", "kind": "condition_error", "repo", "class", "error"}`;
//! * the older per-run summary (no `kind`): `{"ts", "sessions_read", "sessions_used",
//!   "index_generation", "repos": {repo: {"sessions", "tracks": [{"name", "version", "change",
//!   "routes"}]}}}`.
//!
//! Anything else (a line that is not JSON, an unknown `kind`) is skipped.

use super::theme::parse_strength;
use adage_tracks::Strength;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    /// A run's summary line.
    Run,
    /// One change to a route.
    Change,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    pub ts: u64,
    pub tone: Tone,
    pub text: String,
    /// The same, without the repository or track (for narrow panes).
    pub short: String,
    /// The strength a change leaves the route at, for colour.
    pub strength: Option<Strength>,
    pub repo: Option<String>,
}

fn s<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v[k].as_str().filter(|x| !x.is_empty())
}

fn short_gen(g: &str) -> &str {
    &g[..g.len().min(8)]
}

/// The ticker lines for one log line (a run's summary first, then one per change).
pub fn parse(line: &str) -> Vec<Tick> {
    let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![] };
    let ts = v["ts"].as_u64().unwrap_or(0);
    let repo = s(&v, "repo").map(String::from);
    match v["kind"].as_str() {
        Some("condition_error") => {
            let class = s(&v, "class").map(|c| format!(" ({c})")).unwrap_or_default();
            let err: String = s(&v, "error").unwrap_or("unknown error").chars().take(200).collect();
            let on = repo.as_deref().map(|r| format!(" for {r}")).unwrap_or_default();
            let short = format!("conditioning failed{class}: {err}");
            vec![Tick {
                ts,
                tone: Tone::Error,
                text: format!("conditioning failed{on}{class}: {err}"),
                short,
                strength: None,
                repo,
            }]
        }
        Some("condition") | None if v.is_object() => run(&v, ts, repo),
        _ => vec![],
    }
}

fn run(v: &Value, ts: u64, repo: Option<String>) -> Vec<Tick> {
    let head = match s(v, "trigger") {
        Some("auto") => "auto-learn".to_string(),
        Some(t) => format!("condition ({t})"),
        None => "condition".to_string(),
    };
    let long_head = match &repo {
        Some(r) => format!("{head} {r}"),
        None => head.clone(),
    };
    let mut parts = Vec::new();
    if let (Some(read), Some(used)) = (v["sessions_read"].as_u64(), v["sessions_used"].as_u64()) {
        parts.push(format!("{used}/{read} sessions used"));
    }
    if let Some(ms) = v["duration_ms"].as_u64() {
        parts.push(if ms >= 1000 { format!("{:.1}s", ms as f64 / 1000.0) } else { format!("{ms}ms") });
    }
    let changes: Vec<&Value> = v["changes"].as_array().map(|a| a.iter().collect()).unwrap_or_default();
    // The older summary lists tracks per repository instead of changes (newer lines keep it too,
    // beside `changes`, which then says it better).
    let mut tracks = Vec::new();
    let legacy = if v["changes"].is_array() { None } else { v["repos"].as_object() };
    for (r, rr) in legacy.into_iter().flatten() {
        for t in rr["tracks"].as_array().into_iter().flatten() {
            let name = t["name"].as_str().unwrap_or("?");
            let change = t["change"].as_str().unwrap_or("written");
            let routes = t["routes"].as_u64().map(|n| format!(", {n} routes")).unwrap_or_default();
            tracks.push((r.clone(), format!("{name} @ {} ({change}{routes})", t["version"].as_str().unwrap_or("?"))));
        }
    }
    if v["changes"].is_array() {
        parts.push(match changes.len() {
            0 => "no route changed".to_string(),
            1 => "1 change".to_string(),
            n => format!("{n} changes"),
        });
    } else if !tracks.is_empty() {
        parts.push(format!("{} track(s)", tracks.len()));
    }
    if let Some(g) = s(v, "index_generation") {
        parts.push(format!("gen {}", short_gen(g)));
    }
    let tail = if parts.is_empty() { String::new() } else { format!(": {}", parts.join(", ")) };
    let mut out = vec![Tick {
        ts,
        tone: Tone::Run,
        text: format!("{long_head}{tail}"),
        short: format!("{head}{tail}"),
        strength: None,
        repo: repo.clone(),
    }];
    for c in changes {
        out.push(change(c, ts, repo.clone()));
    }
    for (r, t) in tracks {
        let text = format!("wrote {t}");
        out.push(Tick { ts, tone: Tone::Change, short: text.clone(), text, strength: None, repo: Some(r) });
    }
    out
}

fn change(c: &Value, ts: u64, repo: Option<String>) -> Tick {
    let route = s(c, "route").unwrap_or("a route");
    let track = s(c, "track").map(|t| format!("{t}: ")).unwrap_or_default();
    let short_track = s(c, "track").map(|t| format!("{}: ", t.rsplit('/').next().unwrap_or(t))).unwrap_or_default();
    let (from, to) = (s(c, "from"), s(c, "to"));
    let arrow = match (from, to) {
        (Some(f), Some(t)) => format!(" {f}→{t}"),
        (None, Some(t)) => format!(" {t}"),
        _ => String::new(),
    };
    // Summaries are fixed templates that restate the change and end with the evidence in
    // parentheses ("finish line route prime → fire (9 of 9 sessions, lower bound 0.70)"): show
    // that evidence after the arrow, or the whole summary when there is no arrow.
    let summary = match s(c, "summary") {
        // "fire finish line route evidence 6/8 → 7/8 sessions": the counts say it.
        Some(x) if s(c, "change") == Some("evidence") && x.contains("evidence ") => {
            format!(" {}", &x[x.find("evidence ").unwrap_or(0) + "evidence ".len()..])
        }
        Some(x) if x.ends_with(')') && x.contains('(') && !arrow.is_empty() => {
            let inner = &x[x.rfind('(').unwrap_or(0)..];
            format!(" {inner}")
        }
        Some(x) if arrow.is_empty() => format!(": {x}"),
        Some(x) if !x.contains('→') => format!(" ({x})"),
        _ => String::new(),
    };
    let verb = match s(c, "change") {
        Some("new") => "new reflex",
        Some("extinguished") => "extinguished",
        Some("strength") => "conditioned",
        Some("evidence") => "evidence",
        Some("nascent") => "nascent",
        Some("proposal") => "proposed",
        Some("dormant") => "held dormant",
        _ => "changed",
    };
    Tick {
        ts,
        tone: Tone::Change,
        text: format!("{verb} {track}{route}{arrow}{summary}"),
        short: format!("{verb} {short_track}{route}{arrow}{summary}"),
        strength: to.and_then(parse_strength),
        repo,
    }
}
