//! `decide` as a function of index, state, event, policy and checkout, one file per area. The
//! fixtures every area shares are here; each area's own fixtures are in its file.

mod algebra;
mod attachment;
mod bash_edits;
mod basics;
mod chains;
mod contexts;
mod dead_end;
mod finish_line;
mod intent;
mod interlocking;
mod noticed;
mod receipt;
mod refused;
mod subagent;
mod turn_step;
mod typed_slots;

use super::*;
use adage_core::spans::SpanKind;
use adage_core::{CommandTable, Effect};
use adage_tracks::*;
use serde_json::{json, Value};
use std::path::Path;

fn route(id: &str, trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, strength: Strength) -> Route {
    Route {
        id: id.into(),
        summary: String::new(),
        trigger,
        step,
        slots,
        effect,
        strength,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence { support: 5, ..Default::default() },
        authored: false,
    }
}

fn fixture() -> (tempfile::TempDir, Index, &'static CommandTable) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("acme")).unwrap();
    std::fs::write(dir.path().join("acme/billing.py"), "x").unwrap();
    let table = CommandTable::load();
    let lookup_key = command_key("./scripts/wi-lookup W-1 --json", table);
    let track = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/t".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![
            route(
                "lookup",
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
                vec![Slot {
                    name: "item".into(),
                    source: SlotSource::Prompt { span: SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                Effect::Observe,
                Strength::Fire,
            ),
            route(
                "read-linked",
                Trigger::AfterCommand { command: lookup_key },
                Step::ReadBatch { slot: "files".into(), max: 5 },
                vec![Slot {
                    name: "files".into(),
                    source: SlotSource::ResultField { field: "linked_files".into() },
                    contract: Contract::RepoPath,
                }],
                Effect::Observe,
                Strength::Prime,
            ),
            route(
                "gen",
                Trigger::AfterEdit { paths: "schema/*.json".into() },
                Step::Run { command: "make gen".into() },
                vec![],
                Effect::Regenerate,
                Strength::Fire,
            ),
            route(
                "dead-unittest",
                Trigger::SessionStart,
                Step::Avoid {
                    avoid: "python3 -m unittest".into(),
                    instead: "make test".into(),
                    signature: "ACME_ENV is not set".into(),
                },
                vec![],
                Effect::Verify,
                Strength::Inhibit,
            ),
        ],
    };
    (dir, Index::compile(&[track]), table)
}

fn bash(cmd: &str, out: &str) -> Call {
    Call {
        tool: "Bash".into(),
        input: json!({ "command": cmd }),
        output: out.into(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    }
}

const MODEL: &str = "fixture@000000000000";

/// A stand-in embedder: "test" prompts point one way, "docs" prompts another, the rest a third.
fn fake_embed(text: &str) -> Option<(String, Vec<f32>)> {
    let t = text.to_lowercase();
    let v = if t.contains("test") {
        vec![0.8, 0.2, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    } else if t.contains("docs") {
        vec![0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    } else if t.is_empty() {
        return None;
    } else {
        vec![0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0]
    };
    Some((MODEL.to_string(), adage_intent::unit(v)))
}

fn track(name: &str, provenance: &str, routes: Vec<Route>) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.into(),
                version: "1.0.0".into(),
                description: String::new(),
                provenance: provenance.into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

fn edit_call(dir: &tempfile::TempDir, f: &str) -> Call {
    Call {
        tool: "Edit".into(),
        input: json!({ "file_path": dir.path().join(f).to_string_lossy() }),
        output: String::new(),
        failed: false,
        by: By::Agent,
        tool_use_id: None,
        agent: None,
    }
}

fn intent_route(id: &str, family: &str, command: &str, strength: Strength) -> Route {
    let centroid = adage_intent::quantize(&adage_intent::unit(vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]));
    route(
        id,
        Trigger::Intent { family: family.into(), model: MODEL.into(), centroid, threshold: 0.8 },
        Step::Run { command: command.into() },
        vec![],
        Effect::Verify,
        strength,
    )
}

fn npm_test_dead_end() -> Route {
    route(
        "dead-end-npm",
        Trigger::SessionStart,
        Step::Avoid { avoid: "npm test".into(), instead: "make test".into(), signature: "ACME_ENV is not set".into() },
        vec![],
        Effect::Verify,
        Strength::Inhibit,
    )
}

/// An index of these routes with no interlocking applied (as an index compiled before it was).
fn unlocked(tracks: &[&Track]) -> Index {
    Index {
        routes: tracks
            .iter()
            .flat_map(|t| {
                t.routes.iter().map(|r| IndexedRoute {
                    repo: "acme".into(),
                    track: t.manifest.track.name.clone(),
                    version: "1.0.0".into(),
                    route: r.clone(),
                    exec: None,
                })
            })
            .collect(),
        ..Default::default()
    }
}

fn ctx_route(id: &str, items: Vec<CtxItem>, within: u8, step: Step, effect: Effect) -> Route {
    route(id, Trigger::Context { items, within }, step, vec![], effect, Strength::Prime)
}

fn act_item(k: &str) -> CtxItem {
    CtxItem::Act { keys: vec![k.into()] }
}

fn failed_bash(cmd: &str, out: &str) -> Call {
    Call { failed: true, ..bash(cmd, out) }
}

/// Decide every event on the full path and, as the hook does, on the fast path (an event it
/// skips is not decided). A skipped event's full decision must be empty, and the state later
/// decisions read, context symbols included, must be the same on both paths.
fn decide_both(ctx: &Ctx, full: &mut SessionState, fast_st: &mut SessionState, ev: &Event) -> Decision {
    let root = ctx.root.to_string_lossy().to_string();
    let t = ctx.index.triggers(&ctx.repo);
    let skip = fast::skip(&t, ev, &root, || Some(fast_st.clone()));
    let d = decide(ctx, full, ev);
    if skip {
        assert!(
            d.would_have.is_empty() && d.judgements.is_empty() && d.noticed.is_empty(),
            "skipped {ev:?}, decided {d:?}"
        );
    } else {
        let f = decide(ctx, fast_st, ev);
        assert_eq!(serde_json::to_value(&f).unwrap(), serde_json::to_value(&d).unwrap(), "{ev:?}");
    }
    let state =
        |s: &SessionState| json!([s.emitted, s.pending, s.obligations, s.stop_requests, s.ctx, s.cand_ctx, s.noticed]);
    assert_eq!(state(fast_st), state(full), "after {ev:?}");
    d
}

/// The lookup, its linked files and the finish line of [`fixture`], plus `extra`, in one track
/// `acme/t`.
fn predict_fixture(extra: Vec<Route>) -> (tempfile::TempDir, Index, &'static CommandTable) {
    let (dir, base, table) = fixture();
    let mut routes: Vec<Route> =
        base.routes.iter().filter(|r| r.route.id != "dead-unittest").map(|r| r.route.clone()).collect();
    routes.extend(extra);
    (dir, Index::compile(&[track("acme/t", "desire-path", routes)]), table)
}

fn predict_ctx<'a>(dir: &'a tempfile::TempDir, index: &'a Index, table: &'a CommandTable, grants: &[&str]) -> Ctx<'a> {
    Ctx {
        checkout: &crate::checkout::Disk,
        index,
        repo: "acme".into(),
        root: dir.path(),
        table,
        embed: None,
        params: Default::default(),
        policy: Policy {
            predict_grants: grants.iter().map(|r| ("acme/t".to_string(), r.to_string())).collect(),
            ..Default::default()
        },
    }
}

fn step(turn: &str, index: u32, ending: bool) -> Event {
    Event::TurnStep { turn_id: turn.into(), index, model: Some("claude-x".into()), agent: None, ending }
}

fn by_adage(call: Call, id: &str) -> Call {
    Call { by: By::Adage, tool_use_id: Some(id.into()), ..call }
}

fn pending(route: &str, command: &str, table: &CommandTable) -> Pending {
    Pending {
        route: route.into(),
        expect_key: command_key(command, table),
        expect_head: String::new(),
        batches_left: 3,
        command: command.into(),
    }
}

fn armed(route: &str, command: &str, table: &CommandTable) -> Obligation {
    Obligation {
        route: route.into(),
        expect_key: command_key(command, table),
        command: command.into(),
        paths: "schema/*.json".into(),
        ..Default::default()
    }
}
