//! The routes of each kind the suites keep writing, as [`RouteBuilder`]s a test adjusts: a
//! prompt lookup, a finish line, a known dead end, a context route, an ordered sequence and an
//! intent route. [`every_kind`] is one of each, valid together in one track.

use super::build::{route, RouteBuilder};
use crate::{Contract, CtxItem, Slot, SlotSource, Step, Strength, Trigger};
use adage_core::spans::SpanKind;
use adage_core::Effect;

/// The work-item pattern every lookup fixture binds (`W-2042`).
pub const WORK_ITEM: &str = r"^W-\d{4}$";

/// A slot named `name`, bound from a work-item span of the prompt and held to [`WORK_ITEM`].
pub fn ticket_slot(name: &str) -> Slot {
    Slot {
        name: name.into(),
        source: SlotSource::Prompt { span: SpanKind::Ticket },
        contract: Contract::Pattern { regex: WORK_ITEM.into() },
    }
}

/// A prompt naming a work item runs `command` (an observation). Each `{slot}` in the command is a
/// [`ticket_slot`] of that name, so `scripts/wi-lookup {item} --json` binds `item`.
pub fn lookup(id: &str, command: &str) -> RouteBuilder {
    let mut b = route(id, Trigger::Prompt { span: SpanKind::Ticket }, Step::Run { command: command.into() })
        .effect(Effect::Observe);
    for name in placeholders(command) {
        b = b.slot(ticket_slot(name));
    }
    b
}

/// After an edit to a file matching `paths`, run `command` before the turn ends (a
/// regeneration).
pub fn finish_line(id: &str, paths: &str, command: &str) -> RouteBuilder {
    route(id, Trigger::AfterEdit { paths: paths.into() }, Step::Run { command: command.into() })
        .effect(Effect::Regenerate)
}

/// From the session's start: `avoid` fails here with `signature`; `instead` works (inhibit).
pub fn dead_end(id: &str, avoid: &str, instead: &str, signature: &str) -> RouteBuilder {
    let step = Step::Avoid { avoid: avoid.into(), instead: instead.into(), signature: signature.into() };
    route(id, Trigger::SessionStart, step).strength(Strength::Inhibit)
}

/// After `items` within `within` steps, run `command` (a verification).
pub fn context(id: &str, items: Vec<CtxItem>, within: u8, command: &str) -> RouteBuilder {
    route(id, Trigger::Context { items, within }, Step::Run { command: command.into() })
}

/// A context item: a call whose canonical key is `key` (`exec make gen`).
pub fn act(key: &str) -> CtxItem {
    CtxItem::Act { keys: vec![key.into()] }
}

/// A context item: an edit of a file matching `glob`.
pub fn edit(glob: &str) -> CtxItem {
    CtxItem::Edit { glob: glob.into() }
}

/// After an edit to a file matching `paths`, run `commands` in order.
pub fn sequence(id: &str, paths: &str, commands: &[&str]) -> RouteBuilder {
    let step = Step::Sequence { commands: commands.iter().map(|c| c.to_string()).collect() };
    route(id, Trigger::AfterEdit { paths: paths.into() }, step).effect(Effect::Regenerate)
}

/// A prompt whose embedding under `model` is within `threshold` of `centroid` runs `command`;
/// its family is its id.
pub fn intent(id: &str, model: &str, centroid: Vec<i16>, threshold: f32, command: &str) -> RouteBuilder {
    let trigger = Trigger::Intent { family: id.into(), model: model.into(), centroid, threshold };
    route(id, trigger, Step::Run { command: command.into() })
}

/// One route of each kind above, with ids and globs that do not collide: what a learned home
/// looks like to a test that only needs "some of everything".
pub fn every_kind() -> Vec<RouteBuilder> {
    vec![
        lookup("lookup-work-item", "python3 scripts/wi-lookup {item}"),
        finish_line("finish-line-gen", "schema/*.json", "make gen"),
        dead_end("dead-end-pytest", "python3 -m pytest", "make test", "No module named pytest"),
        context("context-check", vec![edit("registry/*.csv"), act("exec make gen")], 8, "make check"),
        sequence("seq-gen-test", "proto/*.proto", &["make gen", "make test"]),
        // A unit vector, in units of 1e-4 (validation refuses any other centroid).
        intent("intent-tests", "fixture@000000000000", vec![3536; 8], 0.8, "make test"),
    ]
}

/// The `{name}` placeholders of a command, in order.
fn placeholders(command: &str) -> Vec<&str> {
    let mut out = vec![];
    let mut rest = command;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}') else { break };
        out.push(&rest[open + 1..open + close]);
        rest = &rest[open + close + 1..];
    }
    out
}
