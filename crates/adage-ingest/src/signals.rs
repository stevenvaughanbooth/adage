//! What every trace decoder shares, so a session means the same thing whatever harness recorded
//! it: prompts anchored to the steps they served ([`Prompts`]), Adage's own directives found in the
//! trace ([`Signals`]), a tool call flattened into canonical steps ([`call_steps`]), and whether the
//! agent ended by asking for help ([`asks_for_help`]).

use adage_core::keys::{self, template_key};
use adage_core::{repo, CanonicalAction, Effect, Normalized};
use adage_trace::session::{Directed, PromptObs, StepObs, FINISH_LINE_STOP, MAX_PROMPTS, MAX_PROMPT_BYTES};
use adage_trace::{wire, Call};
use once_cell::sync::Lazy;
use regex::Regex;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::Path;

/// Collects a session's prompts while a transcript is read, and anchors them to its steps
/// afterwards ([`Prompts::anchor`]). Every importer uses it, so `StepObs::prompt` and
/// `PromptObs::at_step` mean the same thing whatever the harness.
#[derive(Debug, Default)]
pub(crate) struct Prompts {
    texts: Vec<String>,
}

impl Prompts {
    pub(crate) fn push(&mut self, text: &str) {
        if self.texts.len() < MAX_PROMPTS {
            self.texts.push(adage_core::tokens::bounded(text, MAX_PROMPT_BYTES).to_string());
        }
    }

    /// The index of the prompt a call recorded now serves.
    pub(crate) fn current(&self) -> u16 {
        self.texts.len().saturating_sub(1).min(u16::MAX as usize) as u16
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }

    /// The prompts, each anchored at the first step that served it or a later prompt.
    pub(crate) fn anchor(self, steps: &[StepObs]) -> Vec<PromptObs> {
        self.texts
            .into_iter()
            .enumerate()
            .map(|(i, text)| {
                let at = steps.iter().position(|s| s.prompt as usize >= i).unwrap_or(steps.len());
                PromptObs { text, at_step: at as u32 }
            })
            .collect()
    }
}

/// Phrases of a final message that asks the person for help, in its last question.
const HELP_PHRASES: [&str; 14] = [
    "could you",
    "can you",
    "would you",
    "please provide",
    "please share",
    "please clarify",
    "please let me know",
    "do you have",
    "what would you like",
    "what should i",
    "should i",
    "which ",
    "tell me",
    "more details",
];

/// Whether a final reply asks the person for help: its last sentence that ends in `?` holds a
/// help phrase, and it is near the end. "Would you like me to also …?" after a report of done work
/// counts too; that is why this is only a proxy, used when no outcome was recorded.
pub fn asks_for_help(text: &str) -> bool {
    let t = text.trim_end();
    let tail: String = t.chars().rev().take(600).collect::<Vec<_>>().into_iter().rev().collect();
    let Some(q) = tail.rfind('?') else { return false };
    let start = tail[..q].rfind(['.', '!', '\n', '?']).map(|i| i + 1).unwrap_or(0);
    let question = tail[start..=q].to_ascii_lowercase();
    // Only when the question is the last thing said (a list of questions counts).
    let after = tail[q + 1..].trim();
    let trailing_ok = after.is_empty() || after.len() < 200;
    trailing_ok && HELP_PHRASES.iter().any(|p| question.contains(p))
}

/// One `<adage-reflex ...>` opening tag, and its `route` attribute. Text is read plain; a JSON
/// record (a Claude Code attachment line) is read through its string values.
static SIGNAL_TAG: Lazy<Regex> = Lazy::new(|| Regex::new(r"<adage-reflex\b([^>]*)>").unwrap());
static ROUTE_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\broute=\\?"([A-Za-z0-9_.-]+)"#).unwrap());
/// `exec="e1"`: Adage ran the directed route itself and delivered its output (Track C §3.4).
static EXEC_ATTR: Lazy<Regex> = Lazy::new(|| Regex::new(r#"\bexec=\\?""#).unwrap());

/// What a transcript shows Adage delivered: the directives, where each arrived, and what it
/// named. An importer sets [`Signals::at`] to the number of tool calls recorded so far before
/// each [`Signals::note`], and [`Signals::anchor`] maps those call counts to steps.
#[derive(Debug, Default)]
pub struct Signals {
    pub steered: bool,
    pub directed: BTreeSet<String>,
    /// Directives, each at the index of the first tool call after it.
    pub found: Vec<Directed>,
    at: u32,
}

/// The string values of a JSON record, or the text itself.
fn texts(text: &str) -> Vec<String> {
    fn walk(v: &Value, out: &mut Vec<String>) {
        match v {
            Value::String(s) if s.contains("adage") || s.contains("Adage") => out.push(s.clone()),
            Value::Array(xs) => xs.iter().for_each(|x| walk(x, out)),
            Value::Object(m) => m.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    match serde_json::from_str::<Value>(text) {
        Ok(v) if v.is_object() || v.is_array() => {
            let mut out = Vec::new();
            walk(&v, &mut out);
            out
        }
        _ => vec![text.to_string()],
    }
}

impl Signals {
    /// The number of tool calls the transcript has recorded so far.
    pub fn at(&mut self, calls: usize) {
        self.at = calls.min(u32::MAX as usize) as u32;
    }

    pub fn note(&mut self, text: &str) {
        if !(text.contains("<adage-reflex")
            || text.contains("<adage-data")
            || text.contains("<adage-receipt")
            || text.contains("Adage finish-line check"))
        {
            return;
        }
        for t in texts(text) {
            self.note_text(&t);
        }
    }

    fn push(&mut self, route: String, named: Option<wire::Named>, all: bool) {
        let named = named.unwrap_or_default();
        let all = all || named.is_empty();
        self.found.push(Directed {
            at_step: self.at,
            route,
            commands: named.commands,
            avoided: named.avoided,
            files: named.files,
            skills: named.skills,
            all,
        });
    }

    fn note_text(&mut self, text: &str) {
        if text.contains("<adage-reflex") {
            self.steered = true;
            let tags: Vec<(usize, usize, String)> = SIGNAL_TAG
                .captures_iter(text)
                .map(|c| {
                    let m = c.get(0).expect("match");
                    (m.start(), m.end(), c[1].to_string())
                })
                .collect();
            if tags.is_empty() {
                // A wrapper cut short: it could have named anything.
                self.push(String::new(), None, true);
            }
            for (_, end, attrs) in tags {
                let route = ROUTE_ATTR.captures(&attrs).map(|r| r[1].to_string()).unwrap_or_default();
                if !route.is_empty() {
                    self.directed.insert(route.clone());
                }
                let body =
                    text[end..].split("</adage-reflex>").next().filter(|_| text[end..].contains("</adage-reflex>"));
                let named = body.map(wire::named);
                self.push(route, named, EXEC_ATTR.is_match(&attrs) || body.is_none());
            }
        }
        if text.contains("<adage-data") && !SIGNAL_TAG.captures_iter(text).any(|t| EXEC_ATTR.is_match(&t[1])) {
            // Execution output with no whole E1 wrapper beside it: it could have steered anything.
            self.steered = true;
            self.push(String::new(), None, true);
        }
        if text.contains("<adage-receipt") {
            // A receipt for work Adage already ran (a PreToolUse note): the agent may skip or
            // change a run because of it, so nothing after it is the agent's own demonstration.
            self.steered = true;
            self.push(String::new(), None, true);
        }
        if text.contains("Adage finish-line check") {
            self.steered = true;
            self.directed.insert(FINISH_LINE_STOP.to_string());
            self.push(FINISH_LINE_STOP.to_string(), Some(wire::named(text)), false);
        }
    }

    /// The directives with `at_step` a step index: `first_step[k]` is the first step of tool call
    /// `k` (a directive after the last call is at `steps`).
    pub fn anchor(&mut self, first_step: &[u32], steps: usize) -> Vec<Directed> {
        let mut out = std::mem::take(&mut self.found);
        for d in &mut out {
            d.at_step = first_step.get(d.at_step as usize).copied().unwrap_or(steps as u32);
        }
        out
    }
}

/// The steps one tool call contributes: its canonical actions, flattened and canonicalised
/// against the repository root. A failed call's failure is attributed to its last sub-action only,
/// unless the command has an `||` ([`keys::sub_action_failed`]): `make gen && pytest` failing
/// does not make `make gen` a failed run. Every importer (Claude Code, OpenCode, Codex) builds its
/// steps here. An invocation with no normalized actions remains an opaque boundary: omitting
/// an `export`, assignment or empty shell call would invent adjacency between its neighbors.
pub fn call_steps(turn: u32, prompt: u16, call: &Call, actions: Vec<Normalized>, root: &Path) -> Vec<StepObs> {
    let mut flat = keys::flatten(&actions);
    if flat.is_empty() {
        flat.push(Normalized {
            action: CanonicalAction::Unknown { raw: call.tool.clone() },
            effect: Effect::Environment,
            mutating: false,
            confidence: 0.0,
            note: "native call has no normalized action; retained as an opaque boundary".into(),
        });
    }
    let count = flat.len();
    let or_chain = keys::has_or_chain(&call.input);
    flat.into_iter()
        .enumerate()
        .map(|(i, mut n)| {
            keys::canonicalise(&mut n, root);
            let rel_path = keys::action_path(&n.action).map(|p| repo::relativize(root, p));
            let mut call = call.clone();
            call.failed = keys::sub_action_failed(i, count, call.failed, or_chain);
            StepObs {
                turn,
                key: template_key(&n.action),
                norm: n,
                call,
                rel_path,
                prompt,
                project_pytest_root_cwd: false,
            }
        })
        .collect()
}

#[cfg(test)]
#[path = "signals_tests.rs"]
mod tests;
