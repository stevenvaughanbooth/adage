//! Directive text, rendered only from fixed templates (Design §9.3, Architecture F9).
//!
//! A track contributes *structure* — a command template, a glob, a failure signature — and
//! validated slot values. It never contributes prose that reaches the agent. Everything
//! interpolated is sanitised: no backticks, no angle brackets, no newlines, bounded length.

use crate::span_kind_label;
use adage_tracks::{Aspect, Exit, Route, SlotSource, Step, Trigger};

const MAX_FIELD: usize = 200;

pub fn sanitise(s: &str) -> String {
    s.chars()
        .filter(|c| {
            !matches!(c, '`' | '<' | '>' | '"' | '\\' | '\u{85}' | '\u{2028}' | '\u{2029}')
                && !c.is_control()
                && !adage_tracks::io::invisible(*c)
        })
        .take(MAX_FIELD)
        .collect::<String>()
        .trim()
        .to_string()
}

pub fn bind(command: &str, bound: &[(String, String)]) -> String {
    let mut s = command.to_string();
    for (k, v) in bound {
        s = s.replace(&format!("{{{k}}}"), v);
    }
    s
}

fn exit_clause(exits: &[Exit]) -> String {
    let parts: Vec<&str> = exits
        .iter()
        .map(|e| match e {
            Exit::IntentChanges => "the user is asking for something else",
            Exit::ExploratoryChange => "this is an exploratory or draft change",
        })
        .collect();
    if parts.is_empty() {
        String::new()
    } else {
        format!(" Ignore this if {}.", parts.join(" or "))
    }
}

pub fn wrap(track: &str, version: &str, route: &str, aspect: Aspect, body: &str) -> String {
    let a = match aspect {
        Aspect::Green => "green",
        Aspect::Yellow => "yellow",
        Aspect::Red => "red",
    };
    format!(
        "<adage-signal track=\"{}@{}\" route=\"{}\" aspect=\"{a}\">\n{}\nLearned by Adage from this repository's earlier agent sessions; not requested by the user.\n</adage-signal>",
        sanitise(track),
        sanitise(version),
        sanitise(route),
        body
    )
}

/// The body text for a route with its slots bound, or `None` if it cannot be rendered safely.
pub fn route_text(r: &Route, bound: &[(String, String)]) -> Option<String> {
    let exit = exit_clause(&r.exit);
    match (&r.step, &r.trigger) {
        (Step::Run { command }, Trigger::Prompt { span }) => {
            let slot = r.slots.first()?;
            let kind = match slot.source {
                SlotSource::Prompt { span } => span,
                _ => *span,
            };
            Some(format!(
                "In this repository, {} details are looked up with:\n  {}\n{}",
                span_kind_label(kind),
                sanitise(&bind(command, bound)),
                exit.trim_start()
            ))
        }
        // The family's label and phrasings are for people; the agent sees only this template.
        (Step::Run { command }, Trigger::Intent { .. }) => Some(format!(
            "In this repository, requests like this one usually start by running:\n  {}\n{}",
            sanitise(&bind(command, bound)),
            exit.trim_start()
        )),
        (Step::Run { command }, Trigger::AfterEdit { paths }) => Some(format!(
            "In this repository, changes to files matching {} must be followed by running:\n  {}\nCI fails if this is skipped.{}",
            sanitise(paths),
            sanitise(command),
            exit
        )),
        (Step::Run { command }, _) => Some(format!("A known next step here is:\n  {}{}", sanitise(&bind(command, bound)), exit)),
        (Step::ReadBatch { slot, .. }, _) => {
            // Each path was validated individually; render each whole, never a truncated list.
            let files = bound.iter().find(|(k, _)| k == slot)?.1.clone();
            let shown: Vec<String> = files.split(", ").map(sanitise).filter(|f| !f.is_empty() && f.len() < MAX_FIELD).collect();
            if shown.is_empty() {
                return None;
            }
            Some(format!(
                "That result lists these files: {}.\nIf you need them, read them together in one turn rather than one at a time.{}",
                shown.join(", "),
                exit
            ))
        }
        (Step::Avoid { avoid, instead, signature }, _) => Some(format!(
            "In this repository, running {} fails (\"{}\"). Use this instead:\n  {}",
            sanitise(avoid),
            sanitise(signature),
            sanitise(instead)
        )),
    }
}

pub fn stop_reason(paths: &str, command: &str) -> String {
    format!(
        "Adage finish-line check: this session changed files matching {}, and this repository requires running `{}` after such changes; it has not been run yet. Run it (and re-run the tests) before finishing. This reminder is from Adage, not from the user.",
        sanitise(paths),
        sanitise(command)
    )
}
