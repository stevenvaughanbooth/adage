//! Hook activity as feed items: one per spool record (one hook call), with what Adage did.
//!
//! A record is `{ts, event, kind, arm, repo, generation, decision, harness?}` (adage-hook's
//! `record.rs`, `docs/spool.md`); records written by older versions may lack `kind`, `repo` or
//! `harness`, which are then inferred or left blank. A PreToolUse record (`kind: "pretool"`) has no decision: it says
//! what Adage did to the command instead (`pretool: {rewrite|advise, interpreter}`).
//!
//! Older spellings are read too: arm `holdout` (now `control`), directive `aspect` (now
//! `strength`), judgements `derailed` and `followed_failed` (now `ignored` and `misfired`).

use super::theme::parse_strength;
use adage_trace::{EventKind, Harness};
use adage_tracks::Strength;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq)]
pub enum Act {
    /// A directive delivered to the agent: the reflex fired. `detail` is the command or advice
    /// it gave, when one line says it; `instinct` when the reflex ships with Adage.
    Directed {
        route: String,
        strength: Strength,
        detail: String,
        instinct: bool,
    },
    /// Control arm: what would have been said, withheld.
    Withheld {
        route: String,
        strength: Strength,
    },
    /// Treated: the agent took the directed step, or left it (ignored).
    Followed(String),
    Ignored(String),
    /// Treated: the agent ran the directed command, and it failed.
    Misfired(String),
    /// A finish-line obligation was met.
    Met(String),
    /// Control arm: the agent found the reflex's step unaided, or did not.
    Unaided(String),
    NotReached(String),
    /// A stop-hook request to continue (finish line), or one withheld in the control arm.
    StopRequest,
    StopWithheld,
    /// Before a tool ran: Adage rewrote the command (`rewrite`, e.g. a test run pointed at the
    /// project's environment) or added advice (`advise`); in the control arm, withheld.
    Pretool {
        rewrite: bool,
        kind: String,
        interpreter: String,
        withheld: bool,
    },
    /// A candidate this event's context completed (plan D2): a sequence learning counted in
    /// `support` earlier sessions where a reflex needs `s_min`, or a nascent reflex waiting for
    /// `review`. Recorded and shown alike in both arms; never said to the agent.
    Noticed {
        candidate: String,
        support: u32,
        s_min: u32,
        review: bool,
    },
    /// A call answered by a receipt (the Claude Code mod, plan C3): the command did not run, and
    /// Adage's own earlier run of `command` was its result. The receipt card's data.
    Answered {
        operation: String,
        command: String,
        summary: String,
        tests: Vec<String>,
        more: u64,
        ran_at: String,
        ago: String,
    },
    /// An approved action was interrupted, and Adage pauses actions in this worktree until a
    /// person inspects it and acknowledges `operation` (ux-design §9.9). `None` when the journal
    /// itself could not be read.
    Recovery {
        operation: Option<String>,
    },
    /// A judgement of a kind this version does not know: `(kind, reflex)`, shown as it is.
    Other(String, String),
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
    pub control: bool,
    pub acts: Vec<Act>,
    /// Calls observed in this event (tool batches).
    pub observed: usize,
    /// The repository has no reflexes yet: the session is seen, and nothing is decided.
    pub unrouted: bool,
    /// Each directive delivered (or withheld), exactly as the agent saw (or would have seen) it:
    /// `(reflex, text)`.
    pub texts: Vec<(String, String)>,
    /// The harness's ids for the calls this record is about, where its payload carried them: the
    /// one call of a `pretool` record, or a tool batch's calls in order.
    pub tool_use_ids: Vec<String>,
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
            Act::Followed(r)
            | Act::Ignored(r)
            | Act::Misfired(r)
            | Act::Met(r)
            | Act::Unaided(r)
            | Act::NotReached(r)
            | Act::Other(_, r) => Some(r.as_str()),
            Act::Noticed { candidate, .. } => Some(candidate.as_str()),
            Act::StopRequest
            | Act::StopWithheld
            | Act::Pretool { .. }
            | Act::Answered { .. }
            | Act::Recovery { .. } => None,
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
            match a {
                Act::Directed { detail, .. } => {
                    s.push(' ');
                    s.push_str(detail);
                }
                Act::Pretool { kind, interpreter, .. } => {
                    s.push_str(&format!(" {kind} {interpreter}"));
                }
                Act::Answered { command, .. } => {
                    s.push_str(&format!(" receipt {command}"));
                }
                Act::Recovery { .. } => s.push_str(" recovery"),
                _ => {}
            }
        }
        if self.control {
            s.push_str(" control");
        }
        s
    }
}

/// The harness's short name for narrow panes.
pub fn harness_short(h: &str) -> &str {
    adage_trace::Harness::parse(h).map_or(h, |h| h.short())
}

/// The harness's name as a row shows it.
pub fn harness_name(h: &str) -> &str {
    adage_trace::Harness::parse(h).map_or(h, |h| h.hook_name())
}

/// How a row names an event. Exhaustive, so a new [`EventKind`] does not compile until it has a
/// label; which native names mean it is the one event table's ([`Harness::events`]).
const fn kind_label(kind: EventKind) -> &'static str {
    match kind {
        EventKind::SessionStart => "start",
        EventKind::Prompt => "prompt",
        EventKind::ToolBatch | EventKind::ToolUse => "tools",
        EventKind::Failure => "failure",
        EventKind::Stop => "stop",
        EventKind::SessionEnd => "end",
        EventKind::TurnStep => "step",
        // The asks the hook answers itself. Their records carry a kind no event decides
        // (`pretool`, `skill_prompt`, `press_issued`: adage-hook's `RecordKind`), which a row
        // shows as written; these name the ask for a record that has no kind.
        EventKind::PreToolUse => "pretool",
        EventKind::Press => "press",
        EventKind::Standing => "standing",
        EventKind::SkillPrompt => "skill_prompt",
    }
}

/// Every event in the table, with its native name, whichever harness sends it.
fn table() -> impl Iterator<Item = (&'static str, EventKind)> {
    Harness::ALL.into_iter().flat_map(|h| h.events().iter().copied())
}

/// The event a record is about, as a row shows it. A record's `kind` is the runtime event's name
/// for a decided event (`adage_runtime::Event::name`, [`EventKind::decided`]); a record written
/// before records carried one is read by what its native name means in the table. A kind no event
/// decides (`pretool`, `stop_gate`) or a name the table lacks shows as written.
fn event_label(kind: Option<&str>, native: &str) -> String {
    let event = match kind {
        Some(k) => table().map(|(_, e)| e).find(|e| e.decided() == Some(k)),
        None => table().find(|(name, _)| *name == native).map(|(_, e)| e),
    };
    match event {
        Some(e) => kind_label(e).to_string(),
        None => kind.unwrap_or(native).to_string(),
    }
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

/// A procedure card's one line: how many files it lists, and from how many traces
/// (`adage_tracks::card::text`).
fn card_detail(text: &str) -> String {
    let mut files = 0;
    let mut in_files = false;
    for l in text.lines() {
        if l.starts_with("Files such a change touched") {
            in_files = true;
        } else if !l.starts_with(' ') && !l.trim().is_empty() && !l.trim_start().starts_with('<') {
            in_files = false;
        } else if in_files && l.trim_start().starts_with("- ") {
            files += 1;
        }
    }
    let traces = text
        .split("learned from ")
        .nth(1)
        .and_then(|r| r.split_whitespace().next())
        .and_then(|n| n.parse::<u32>().ok());
    let mut out = "card".to_string();
    if files > 0 {
        out.push_str(&format!(" · {files} file{}", if files == 1 { "" } else { "s" }));
    }
    if let Some(n) = traces {
        out.push_str(&format!(" · {n} trace{}", if n == 1 { "" } else { "s" }));
    }
    out
}

struct Said {
    route: String,
    strength: Strength,
    card: bool,
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
            let strength = x["strength"]
                .as_str()
                .or_else(|| x["aspect"].as_str())
                .and_then(parse_strength)
                .unwrap_or(Strength::Prime);
            let text = x["text"].as_str().unwrap_or("").to_string();
            let track = x["track"].as_str().unwrap_or("");
            let instinct = x["origin"].is_object() || track.starts_with("instincts/");
            let card = x["kind"] == "card";
            Some(Said { route, strength, card, text, instinct })
        })
        .collect()
}

pub fn from_record(session: &str, rec: &Value) -> FeedItem {
    let d = &rec["decision"];
    let control = rec["arm"] == "control" || rec["arm"] == "holdout";
    let mut acts = Vec::new();
    let mut texts = Vec::new();
    if let Some(p) = rec["pretool"].as_object() {
        let (rewrite, kind) = match (p.get("rewrite").and_then(Value::as_str), p.get("advise").and_then(Value::as_str))
        {
            (Some(k), _) => (true, k),
            (None, Some(k)) => (false, k),
            (None, None) => (false, "?"),
        };
        acts.push(Act::Pretool {
            rewrite,
            kind: kind.to_string(),
            interpreter: p.get("interpreter").and_then(Value::as_str).unwrap_or("").to_string(),
            withheld: control || rec["withheld"] == true,
        });
    }
    // A call answered by a receipt (treated only: in control the answer is withheld, and the call
    // ran).
    if rec["answered"] == "receipt" && rec["decision"].is_object() {
        let c = &rec["card"];
        let text = |v: &Value| v.as_str().unwrap_or("").to_string();
        acts.push(Act::Answered {
            operation: text(&c["operation"]),
            command: text(&rec["command"]),
            summary: text(&c["summary"]),
            tests: c["tests"].as_array().into_iter().flatten().filter_map(|t| t.as_str().map(String::from)).collect(),
            more: c["more"].as_u64().unwrap_or(0),
            ran_at: text(&c["ran_at"]),
            ago: text(&c["ago"]),
        });
    }
    if rec["recovery_required"] == true {
        // The interrupted operation, when the journal named one (`exec` lists the event's actions).
        let operation = rec["exec"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|x| x["outcome"] == "unknown")
            .and_then(|x| x["operation_id"].as_str())
            .map(String::from);
        acts.push(Act::Recovery { operation });
    }
    let event = event_label(rec["kind"].as_str(), rec["event"].as_str().unwrap_or("?"));
    for s in directives(d, "directives") {
        let detail = match (s.card, event.as_str()) {
            (true, "prompt") => card_detail(&s.text),
            // The grey zone: a prompt just under the card's threshold gets it at the first edit
            // of one of its files instead.
            (true, _) => format!("{} · at the first edit", card_detail(&s.text)),
            (false, _) => detail_of(&s.text),
        };
        acts.push(Act::Directed { route: s.route.clone(), strength: s.strength, detail, instinct: s.instinct });
        texts.push((s.route, s.text));
    }
    // Would-have directives are recorded in both arms; in the treated arm they duplicate what was
    // delivered, so only a control trace's are shown (as withheld).
    if control {
        for s in directives(d, "would_have") {
            acts.push(Act::Withheld { route: s.route.clone(), strength: s.strength });
            texts.push((s.route, s.text));
        }
    }
    for j in d["judgements"].as_array().into_iter().flatten() {
        let Some(route) = j["route"].as_str().map(String::from) else { continue };
        acts.push(match (j["kind"].as_str(), control) {
            (Some("followed"), false) => Act::Followed(route),
            (Some("ignored" | "derailed"), false) => Act::Ignored(route),
            (Some("misfired" | "followed_failed"), _) => Act::Misfired(route),
            (Some("followed"), true) => Act::Unaided(route),
            (Some("ignored" | "derailed"), true) => Act::NotReached(route),
            (Some("obligation_met"), _) => Act::Met(route),
            (Some(k), _) => Act::Other(k.replace('_', " "), route),
            (None, _) => continue,
        });
    }
    // A step Adage made in the agent's place (`turn_step`), or would have (control, or past the
    // session's cap).
    if let Some(route) = d["predicted"]["route"].as_str() {
        acts.push(Act::Other("chose".into(), route.to_string()));
    } else if let Some(route) = d["would_predict"]["route"].as_str() {
        acts.push(Act::Other("would choose".into(), route.to_string()));
    }
    // Candidates noticed (both arms alike).
    for n in d["noticed"].as_array().into_iter().flatten() {
        let Some(candidate) = n["candidate"].as_str() else { continue };
        let count = |k: &str| n[k].as_u64().and_then(|x| u32::try_from(x).ok()).unwrap_or(0);
        acts.push(Act::Noticed {
            candidate: candidate.to_string(),
            support: count("support"),
            s_min: count("s_min"),
            review: n["review"] == true,
        });
    }
    if d["continue_reason"].is_string() {
        acts.push(Act::StopRequest);
    } else if control && d["would_continue"].is_string() {
        acts.push(Act::StopWithheld);
    }
    FeedItem {
        seq: 0,
        ts: rec["ts"].as_u64().unwrap_or(0),
        session: session.to_string(),
        harness: rec["harness"].as_str().unwrap_or("claude").to_string(),
        event,
        repo: rec["repo"].as_str().unwrap_or("").to_string(),
        control,
        acts,
        // A seen-only record (no reflexes here yet) counts its calls; a decision lists them.
        observed: d["observed"].as_array().map_or_else(|| rec["calls"].as_u64().unwrap_or(0) as usize, |a| a.len()),
        unrouted: rec["unrouted"] == true,
        texts,
        tool_use_ids: match rec["tool_use_id"].as_str() {
            Some(id) => vec![id.to_string()],
            None => rec["tool_use_ids"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|x| x.as_str().map(String::from))
                .collect(),
        },
    }
}

/// The last `n` characters of a session id: the random part of a UUID (v4 or v7), unlike the
/// prefix, which sessions started together share.
pub fn short_session(s: &str, n: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    chars[chars.len().saturating_sub(n)..].iter().collect()
}

#[cfg(test)]
mod tests {
    use super::{event_label, kind_label, table};

    /// Every event in the one table has a label, and a record's `kind` and its native name agree
    /// on it: a kind-less record reads as the same row as one that carries its decided kind.
    #[test]
    fn every_event_in_the_table_has_a_label() {
        for (native, kind) in table() {
            let label = kind_label(kind);
            assert!(!label.is_empty(), "{native}");
            assert_eq!(event_label(None, native), label, "{native}");
            if let Some(decided) = kind.decided() {
                assert_eq!(event_label(Some(decided), native), label, "{native} as {decided}");
            }
        }
        assert_eq!(event_label(Some("pretool"), "PreToolUse"), "pretool");
        assert_eq!(event_label(Some("stop_gate"), "Stop"), "stop_gate");
        assert_eq!(event_label(None, "Unheard"), "Unheard");
    }
}
