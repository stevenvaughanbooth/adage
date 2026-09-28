//! Hook activity as feed items: one per spool record (one hook call), with what Adage did.
//!
//! A record is `{ts, event, kind, arm, repo, generation, decision, harness?}` (`hook.rs`);
//! records written by older versions may lack `kind`, `repo` or `harness`, which are then
//! inferred or left blank.

use super::theme::parse_aspect;
use adage_tracks::Aspect;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// A directive delivered to the agent: the reflex fired. `detail` is the command or advice
    /// it gave, when one line says it; `instinct` when the reflex ships with Adage.
    Directed {
        route: String,
        aspect: Aspect,
        detail: String,
        instinct: bool,
    },
    /// Control arm: what would have been said, withheld.
    Withheld {
        route: String,
        aspect: Aspect,
    },
    /// Treated: the agent took the directed step, or left it (ignored).
    Followed(String),
    Derailed(String),
    /// A finish-line obligation was met.
    Met(String),
    /// Control arm: the agent found the reflex's step unaided, or did not.
    Unaided(String),
    NotReached(String),
    /// A stop-hook request to continue (finish line), or one withheld in the control arm.
    StopRequest,
    StopWithheld,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedItem {
    /// Arrival order, assigned by the live view (0 until then).
    pub seq: u64,
    pub ts: u64,
    pub session: String,
    pub harness: String,
    pub event: String,
    pub repo: String,
    /// The control arm: reflexes withheld on purpose.
    pub holdout: bool,
    pub acts: Vec<Act>,
    /// Calls observed in this event (tool batches).
    pub observed: usize,
    /// Each directive delivered (or withheld), exactly as the agent saw (or would have seen) it:
    /// `(reflex, text)`.
    pub texts: Vec<(String, String)>,
}

impl FeedItem {
    /// Nothing to say and nothing judged.
    pub fn quiet(&self) -> bool {
        self.acts.is_empty()
    }

    /// Whether a reflex fired here (a directive was delivered).
    pub fn fired(&self) -> bool {
        self.acts.iter().any(|a| matches!(a, Act::Directed { .. }))
    }

    /// Every reflex this event names.
    pub fn reflexes(&self) -> impl Iterator<Item = &str> {
        self.acts.iter().filter_map(|a| match a {
            Act::Directed { route, .. } | Act::Withheld { route, .. } => Some(route.as_str()),
            Act::Followed(r) | Act::Derailed(r) | Act::Met(r) | Act::Unaided(r) | Act::NotReached(r) => {
                Some(r.as_str())
            }
            Act::StopRequest | Act::StopWithheld => None,
        })
    }

    /// Plain text for filtering: the trace, harness, event, every reflex named and what it said.
    pub fn haystack(&self) -> String {
        let mut s = format!("{} {} {}", self.session, self.harness, self.event);
        for r in self.reflexes() {
            s.push(' ');
            s.push_str(r);
        }
        for a in &self.acts {
            if let Act::Directed { detail, .. } = a {
                s.push(' ');
                s.push_str(detail);
            }
        }
        if self.holdout {
            s.push_str(" control");
        }
        s
    }
}

/// The harness's short name for narrow panes.
pub fn harness_short(h: &str) -> &str {
    match h {
        "claude" | "claude-code" => "cc",
        "opencode" => "oc",
        "codex" => "cx",
        other => other,
    }
}

/// The harness's name as a row shows it.
pub fn harness_name(h: &str) -> &str {
    match h {
        "claude-code" => "claude",
        other => other,
    }
}

/// The normalised event name (`adage_runtime::Event::name`) as shown.
fn event_label(kind: Option<&str>, native: &str) -> String {
    let k = kind.unwrap_or(match native {
        "UserPromptSubmit" | "chat.message" => "prompt",
        "PostToolBatch" | "PostToolUse" | "tool.execute.after" => "batch",
        "PostToolUseFailure" => "failure",
        "Stop" | "session.idle" => "stop",
        "SessionStart" | "session.created" => "session_start",
        "SessionEnd" => "session_end",
        other => other,
    });
    match k {
        "batch" => "tools",
        "session_start" => "start",
        "session_end" => "end",
        other => other,
    }
    .to_string()
}

/// The one line of a directive's body that says what to do: the indented command of a run
/// step, else its first sentence. Empty when there is no text.
pub fn detail_of(text: &str) -> String {
    let body: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('<')).collect();
    let indented = body.iter().find(|l| l.starts_with("  ")).map(|l| {
        let l = l.trim();
        l.split(". Ignore this").next().unwrap_or(l).trim_end_matches('.').trim().to_string()
    });
    indented
        .or_else(|| {
            body.first().map(|l| l.split(". ").next().unwrap_or(l).trim().trim_end_matches(['.', ':']).to_string())
        })
        .unwrap_or_default()
}

struct Said {
    route: String,
    aspect: Aspect,
    text: String,
    instinct: bool,
}

fn directives(d: &Value, key: &str) -> Vec<Said> {
    d[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let route = x["route"].as_str()?.to_string();
            let aspect = x["aspect"].as_str().and_then(parse_aspect).unwrap_or(Aspect::Yellow);
            let text = x["text"].as_str().unwrap_or("").to_string();
            let track = x["track"].as_str().unwrap_or("");
            let instinct = x["origin"].is_object() || track.starts_with("instincts/");
            Some(Said { route, aspect, text, instinct })
        })
        .collect()
}

pub fn from_record(session: &str, rec: &Value) -> FeedItem {
    let d = &rec["decision"];
    let holdout = rec["arm"] == "holdout";
    let mut acts = Vec::new();
    let mut texts = Vec::new();
    for s in directives(d, "directives") {
        let detail = detail_of(&s.text);
        acts.push(Act::Directed { route: s.route.clone(), aspect: s.aspect, detail, instinct: s.instinct });
        texts.push((s.route, s.text));
    }
    // Would-have directives are recorded in both arms; in the treated arm they duplicate what was
    // delivered, so only a control trace's are shown (as withheld).
    if holdout {
        for s in directives(d, "would_have") {
            acts.push(Act::Withheld { route: s.route.clone(), aspect: s.aspect });
            texts.push((s.route, s.text));
        }
    }
    for j in d["judgements"].as_array().into_iter().flatten() {
        let Some(route) = j["route"].as_str().map(String::from) else { continue };
        acts.push(match (j["kind"].as_str(), holdout) {
            (Some("followed"), false) => Act::Followed(route),
            (Some("derailed"), false) => Act::Derailed(route),
            (Some("followed"), true) => Act::Unaided(route),
            (Some("derailed"), true) => Act::NotReached(route),
            (Some("obligation_met"), _) => Act::Met(route),
            _ => continue,
        });
    }
    if d["continue_reason"].is_string() {
        acts.push(Act::StopRequest);
    } else if holdout && d["would_continue"].is_string() {
        acts.push(Act::StopWithheld);
    }
    FeedItem {
        seq: 0,
        ts: rec["ts"].as_u64().unwrap_or(0),
        session: session.to_string(),
        harness: rec["harness"].as_str().unwrap_or("claude").to_string(),
        event: event_label(rec["kind"].as_str(), rec["event"].as_str().unwrap_or("?")),
        repo: rec["repo"].as_str().unwrap_or("").to_string(),
        holdout,
        acts,
        observed: d["observed"].as_array().map_or(0, |a| a.len()),
        texts,
    }
}

/// The last `n` characters of a session id: the random part of a UUID (v4 or v7), unlike the
/// prefix, which sessions started together share.
pub fn short_session(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}
