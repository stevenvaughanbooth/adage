//! Hook activity as feed items: one per spool record (one hook call), with what Adage did.
//!
//! A record is `{ts, event, kind, arm, repo, generation, decision, harness?}` (`hook.rs`);
//! records written by older versions may lack `kind`, `repo` or `harness`, which are then
//! inferred or left blank.

use super::theme::parse_strength;
use adage_tracks::Strength;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// A directive delivered to the agent.
    Directed {
        route: String,
        strength: Strength,
    },
    /// Control: what would have been said, withheld.
    Withheld {
        route: String,
        strength: Strength,
    },
    /// Treated: the agent took the directed step, or left it.
    Followed(String),
    Ignored(String),
    /// A finish-line obligation was met.
    Met(String),
    /// Control: the agent found the route's step unaided, or did not.
    Unaided(String),
    NotReached(String),
    /// A stop-hook request to continue (finish line), or one withheld in the control.
    StopRequest,
    StopWithheld,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FeedItem {
    pub ts: u64,
    pub session: String,
    pub harness: String,
    pub event: String,
    pub repo: String,
    pub control: bool,
    pub acts: Vec<Act>,
    /// Calls observed in this event (tool batches).
    pub observed: usize,
}

impl FeedItem {
    /// Nothing to say and nothing judged.
    pub fn quiet(&self) -> bool {
        self.acts.is_empty()
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

fn directives(d: &Value, key: &str) -> Vec<(String, Strength)> {
    d[key]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|x| {
            let route = x["route"].as_str()?.to_string();
            let strength = x["strength"].as_str().and_then(parse_strength).unwrap_or(Strength::Prime);
            Some((route, strength))
        })
        .collect()
}

pub fn from_record(session: &str, rec: &Value) -> FeedItem {
    let d = &rec["decision"];
    let control = rec["arm"] == "control";
    let mut acts = Vec::new();
    for (route, strength) in directives(d, "directives") {
        acts.push(Act::Directed { route, strength });
    }
    // Would-have directives are recorded in both arms; in the treated arm they duplicate what was
    // delivered, so only a control's are shown (as withheld).
    if control {
        for (route, strength) in directives(d, "would_have") {
            acts.push(Act::Withheld { route, strength });
        }
    }
    for j in d["judgements"].as_array().into_iter().flatten() {
        let Some(route) = j["route"].as_str().map(String::from) else { continue };
        acts.push(match (j["kind"].as_str(), control) {
            (Some("followed"), false) => Act::Followed(route),
            (Some("ignored"), false) => Act::Ignored(route),
            (Some("followed"), true) => Act::Unaided(route),
            (Some("ignored"), true) => Act::NotReached(route),
            (Some("obligation_met"), _) => Act::Met(route),
            _ => continue,
        });
    }
    if d["continue_reason"].is_string() {
        acts.push(Act::StopRequest);
    } else if control && d["would_continue"].is_string() {
        acts.push(Act::StopWithheld);
    }
    FeedItem {
        ts: rec["ts"].as_u64().unwrap_or(0),
        session: session.to_string(),
        harness: rec["harness"].as_str().unwrap_or("claude").to_string(),
        event: event_label(rec["kind"].as_str(), rec["event"].as_str().unwrap_or("?")),
        repo: rec["repo"].as_str().unwrap_or("").to_string(),
        control,
        acts,
        observed: d["observed"].as_array().map_or(0, |a| a.len()),
    }
}

/// The last `n` characters of a session id: the random part of a UUID (v4 or v7), unlike the
/// prefix, which sessions started together share.
pub fn short_session(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}
