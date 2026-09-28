//! Directive text, rendered only from fixed templates (Design §9.3, Architecture F9).
//!
//! A track contributes *structure* — a command template, a glob, a failure signature — and
//! validated slot values. It never contributes prose that reaches the agent. Everything
//! interpolated is sanitised: no backticks, no angle brackets, no newlines, bounded length.

use crate::span_kind_label;
use adage_tracks::{Aspect, Exit, Origin, Route, SlotSource, Step, Trigger};

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

/// The wrapper around a directive's body, without a kind (as rendered before kinds existed; used
/// for previews). What the agent receives is [`wrap_directive`].
pub fn wrap(track: &str, version: &str, route: &str, aspect: Aspect, body: &str) -> String {
    wrap_kind(track, version, route, "", aspect, body)
}

/// A directive as the agent receives it.
pub fn wrap_directive(d: &crate::Directive) -> String {
    wrap_origin(&d.track, &d.version, &d.route, &d.kind, d.aspect, &d.text, d.origin.as_ref())
}

/// The last line of a learned route's wrapper.
pub const LEARNED_FOOTER: &str =
    "Learned by Adage from this repository's earlier agent sessions; not requested by the user.";

/// The last line of the wrapper: what the directive is based on. A learned route (or one whose
/// origin is unknown) says it was learned from sessions; an instinct says it is Adage's default
/// for repositories of its ecosystem, from the file that declares it — never that sessions showed
/// anything. Every interpolated value was validated with the track (a closed ecosystem list, a
/// repository-relative path) and is checked again here.
pub fn footer(origin: Option<&Origin>) -> String {
    match origin {
        Some(Origin::Instinct { ecosystem, declared, .. }) => {
            let eco = ecosystem
                .as_deref()
                .filter(|e| adage_tracks::ECOSYSTEMS.contains(e))
                .map(|e| format!("{e} repositories"))
                .unwrap_or_else(|| "repositories like this one".into());
            let from = declared
                .as_deref()
                .filter(|d| declared_ok(d))
                .map(|d| format!(" (from this repository's {d})"))
                .unwrap_or_default();
            format!("Adage default for {eco}{from}; not requested by the user.")
        }
        _ => LEARNED_FOOTER.to_string(),
    }
}

/// A declared file named in an instinct's text: a plain repository-relative path.
fn declared_ok(d: &str) -> bool {
    adage_tracks::advice::safe_glob(d) && !d.contains('*')
}

/// The wrapper around a directive's body: `<adage-signal track="name@version" route="id"
/// aspect="green" kind="work-items">`. `kind` (the route kind, [`adage_tracks::route_kind`]) comes
/// last and is left out when empty, so transcripts written before it existed parse as before.
/// Learning reads it to know which miners a directive could steer, and falls back to the route id
/// when it is missing (`adage_learn::import::steered_kinds`).
pub fn wrap_kind(track: &str, version: &str, route: &str, kind: &str, aspect: Aspect, body: &str) -> String {
    wrap_origin(track, version, route, kind, aspect, body, None)
}

/// [`wrap_kind`] for a route of a given origin: an instinct's wrapper carries `origin="instinct"`
/// after `kind`, and its own last line ([`footer`]).
pub fn wrap_origin(
    track: &str,
    version: &str,
    route: &str,
    kind: &str,
    aspect: Aspect,
    body: &str,
    origin: Option<&Origin>,
) -> String {
    let a = match aspect {
        Aspect::Green => "green",
        Aspect::Yellow => "yellow",
        Aspect::Red => "red",
    };
    let kind: String = kind.chars().filter(|c| c.is_ascii_lowercase() || *c == '-').take(32).collect();
    let kind = if kind.is_empty() { String::new() } else { format!(" kind=\"{kind}\"") };
    let attr = if matches!(origin, Some(Origin::Instinct { .. })) { " origin=\"instinct\"" } else { "" };
    format!(
        "<adage-signal track=\"{}@{}\" route=\"{}\" aspect=\"{a}\"{kind}{attr}>\n{}\n{}\n</adage-signal>",
        sanitise(track),
        sanitise(version),
        sanitise(route),
        body,
        footer(origin)
    )
}

/// The finish-line claim is only as strong as the evidence. That CI fails is said only when
/// sessions that skipped the step are known to have failed (`deviations_failed`, from outcomes);
/// otherwise the directive says what the sessions show: agents here run it.
pub const FINISH_CI_FAILS: &str = "CI fails if this is skipped.";
pub const FINISH_OBSERVED: &str = "Agents in this repository run it after such changes.";
/// An instinct's finish line claims no session evidence, only that the repository declares it
/// (`<file> declares this.` when the declaring file is known).
pub const FINISH_DECLARED: &str = "This repository's build declares this step.";

/// The evidence sentence of a finish-line directive.
fn finish_basis(r: &Route) -> String {
    if r.is_instinct() {
        return match r.origin() {
            Origin::Instinct { declared: Some(d), .. } if declared_ok(&d) => format!("{d} declares this."),
            _ => FINISH_DECLARED.to_string(),
        };
    }
    if r.evidence.deviations_failed > 0 { FINISH_CI_FAILS } else { FINISH_OBSERVED }.to_string()
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
            "In this repository, changes to files matching {} must be followed by running:\n  {}\n{}{}",
            sanitise(paths),
            sanitise(&bind(command, bound)),
            finish_basis(r),
            exit
        )),
        // After a failure: what to run instead (the instinct form of a dead end).
        (Step::Run { command }, Trigger::AfterFailure { .. }) => Some(format!(
            "That failed. In this repository, run this instead:\n  {}\n{}",
            sanitise(&bind(command, bound)),
            exit.trim_start()
        )),
        (Step::Run { command }, _) => {
            Some(format!("A known next step here is:\n  {}{}", sanitise(&bind(command, bound)), exit))
        }
        (Step::ReadBatch { slot, .. }, _) => {
            // Each path was validated individually; render each whole, never a truncated list.
            let files = bound.iter().find(|(k, _)| k == slot)?.1.clone();
            let shown: Vec<String> =
                files.split(", ").map(sanitise).filter(|f| !f.is_empty() && f.len() < MAX_FIELD).collect();
            if shown.is_empty() {
                return None;
            }
            Some(format!(
                "That result lists these files: {}.\nIf you need them, read them together in one turn rather than one at a time.{}",
                shown.join(", "),
                exit
            ))
        }
        (Step::Advise { template, slots }, _) => {
            // A fixed sentence; each slot value passed its contract when the track was read, and
            // is sanitised again (a value that changes is refused, not rendered).
            if slots.iter().any(|(_, v)| sanitise(v) != *v) {
                return None;
            }
            let mut text = adage_tracks::advice::text(template, slots)?;
            text.push_str(&exit);
            Some(text)
        }
        (Step::Avoid { avoid, instead, signature }, _) => {
            let mut text = format!(
                "In this repository, running {} fails (\"{}\"). Use this instead:\n  {}",
                sanitise(avoid),
                sanitise(signature),
                sanitise(instead)
            );
            // Design W2: the exit condition, on its own line after the command (as for a lookup).
            if !exit.is_empty() {
                text.push('\n');
                text.push_str(exit.trim_start());
            }
            Some(text)
        }
    }
}

pub fn stop_reason(paths: &str, command: &str) -> String {
    format!(
        "Adage finish-line check: this session changed files matching {}, and this repository requires running `{}` after such changes; it has not been run yet. Run it (and re-run the tests) before finishing. This reminder is from Adage, not from the user.",
        sanitise(paths),
        sanitise(command)
    )
}
