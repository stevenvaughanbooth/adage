//! UI tests: the tailers under concurrent appends and truncation, the parsers, the keys, and
//! frames rendered with `TestBackend` from fixture data, compared with insta snapshots in
//! `ui/snapshots/` (`cargo insta review` to inspect a change; `INSTA_UPDATE=always` to accept).
//!
//! Frames are snapshotted twice where colour matters: as plain text (what `NO_COLOR` shows) and
//! styled, each run of styled cells tagged with its colour and effects (`[green b]● fire[/]`).

use super::app::{self, AppState, Cmd, Modal, Tab};
use super::feed::{self, Act};
use super::health::{Fix, Health, Light};
use super::keys;
use super::learnlog::{self, Tone};
use super::palette::{self, Palette};
use super::routes::{Arm, LiveCounts, RouteRow, Routes};
use super::tabs;
use super::tail::{LineTail, SpoolTail};
use super::theme::{Lamp, Theme};
use super::view::{self, Ctx, Live};
use super::watch::{self, buffer_text, Sources};
use super::wizard::{self, Wizard};
use crate::doctor::{Check, Status};
use adage_store::{spool, Paths};
use adage_tracks::Strength;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::{Color, Modifier};
use ratatui::Terminal;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// 2026-09-23 12:04:00 UTC.
const T0: u64 = 1_790_165_040_000;
const NONE: KeyModifiers = KeyModifiers::NONE;

fn rec(k: u64, arm: &str, event: &str, kind: &str, decision: Value) -> Value {
    json!({ "ts": T0 + k * 1000, "event": event, "kind": kind, "arm": arm, "repo": "harbor-dispatch", "generation": "8216e29f00", "decision": decision })
}

fn directive(route: &str, strength: &str, text: &str) -> Value {
    json!({ "route": route, "track": "harbor-dispatch/incidents", "version": "0.3.0", "strength": strength, "text": text })
}

const LOOKUP: &str = "Incidents here are looked up with a script. A known next step here is:\n  bin/fetch-incident INC-2045. Ignore this if the user is asking for something else.";
const DEAD_END: &str = "npm test fails in this repository: HARBOR_ENV is not set. Instead run:\n  npm run check";

fn decision(directives: Vec<Value>, would: Vec<Value>, judgements: Vec<Value>, cont: Option<&str>) -> Value {
    json!({
        "directives": directives, "would_have": would, "judgements": judgements,
        "continue_reason": cont, "would_continue": cont, "observed": ["exec ls"],
    })
}

/// A data directory with two treated traces and a control trace, and a learn.log.
fn fixture(dir: &Path) -> Paths {
    let paths = Paths::at(dir);
    let s = |id: &str| paths.spool(id);
    let a = "7c1e04d2-11aa-4a31-9f0e-00000000a11c";
    let b = "3f9b62e8-22bb-4c7d-8b21-000000000b0b";
    let c = "91d5aa70-33cc-47e2-a5c3-00000000c4a1";
    let lookup = || directive("lookup-incident", "fire", LOOKUP);
    spool::append(
        &s(a),
        &rec(0, "treated", "UserPromptSubmit", "prompt", decision(vec![lookup()], vec![lookup()], vec![], None)),
    )
    .unwrap();
    spool::append(
        &s(c),
        &rec(1, "control", "UserPromptSubmit", "prompt", decision(vec![], vec![lookup()], vec![], None)),
    )
    .unwrap();
    spool::append(
        &s(a),
        &rec(
            2,
            "treated",
            "PostToolBatch",
            "batch",
            decision(
                vec![directive("read-linked-files", "prime", "That result lists these files: read them together.")],
                vec![],
                vec![json!({ "kind": "followed", "route": "lookup-incident" })],
                None,
            ),
        ),
    )
    .unwrap();
    spool::append(&s(b), &rec(3, "treated", "PostToolBatch", "batch", decision(vec![], vec![], vec![], None))).unwrap();
    spool::append(
        &s(b),
        &rec(
            4,
            "treated",
            "PostToolUseFailure",
            "failure",
            decision(vec![directive("dead-end-npm-test", "inhibit", DEAD_END)], vec![], vec![], None),
        ),
    )
    .unwrap();
    spool::append(
        &s(b),
        &rec(
            5,
            "treated",
            "PostToolBatch",
            "batch",
            decision(vec![], vec![], vec![json!({ "kind": "ignored", "route": "lookup-incident" })], None),
        ),
    )
    .unwrap();
    spool::append(
        &s(c),
        &rec(
            6,
            "control",
            "PostToolBatch",
            "batch",
            decision(vec![], vec![], vec![json!({ "kind": "followed", "route": "lookup-incident" })], None),
        ),
    )
    .unwrap();
    spool::append(
        &s(b),
        &rec(7, "treated", "Stop", "stop", decision(vec![], vec![], vec![], Some("Adage finish-line check: …"))),
    )
    .unwrap();
    spool::append(
        &s(a),
        &rec(
            8,
            "treated",
            "PostToolBatch",
            "batch",
            decision(
                vec![],
                vec![],
                vec![json!({ "kind": "obligation_met", "route": "codegen-after-registry" })],
                None,
            ),
        ),
    )
    .unwrap();
    // A record from another repository, written by an older version (`aspect`, not `strength`).
    let mut other = rec(
        9,
        "treated",
        "UserPromptSubmit",
        "prompt",
        decision(vec![json!({ "route": "lookup", "aspect": "green", "text": "" })], vec![], vec![], None),
    );
    other["repo"] = json!("acme-billing");
    other["harness"] = json!("codex");
    spool::append(&s("019a-codex-session-0001"), &other).unwrap();
    let log = [
        json!({ "ts": T0 - 3_600_000, "sessions_read": 12, "sessions_used": 12, "index_generation": "1a2b3c4d5e", "repos": { "harbor-dispatch": { "sessions": 12, "tracks": [{ "name": "harbor-dispatch/incidents", "version": "0.2.0", "change": "new", "routes": 3 }] } } }).to_string(),
        "not json at all".to_string(),
        json!({ "ts": T0 + 10_000, "kind": "condition", "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 25, "sessions_used": 24, "duration_ms": 730, "index_generation": "8216e29f00", "changes": [
            { "route": "lookup-incident", "track": "harbor-dispatch/incidents", "change": "strength", "from": "prime", "to": "fire", "summary": "reflex prime → fire (24 of 25 traces, lower bound 0.80)" },
            { "route": "dead-end-npm-test", "track": "harbor-dispatch/incidents", "change": "new", "to": "inhibit", "summary": "6/7 traces" },
        ] }).to_string(),
        json!({ "ts": T0 + 20_000, "kind": "condition_error", "repo": "harbor-dispatch", "class": "io", "error": "tracks directory is not writable" }).to_string(),
    ];
    std::fs::write(paths.learn_log(), log.join("\n") + "\n").unwrap();
    paths
}

fn health() -> Health {
    use crate::doctor::Fix as DoctorFix;
    let check = |name: &str, status, detail: &str, fix: Option<DoctorFix>| Check {
        check: name.into(),
        label: crate::doctor::label(name),
        status,
        detail: detail.into(),
        fix,
    };
    Health {
        checks: vec![
            check("binary", Status::Pass, "adage 0.1.0 at ~/bin/adage", None),
            check("index", Status::Pass, "generation 8216e29f00, 4 route(s) in 1 repository", None),
            check(
                "embedding model",
                Status::Warn,
                "no embedding model, so the hashed fallback is in use; intent reflexes cannot fire",
                Some(DoctorFix::run_in("run {}", "adage models fetch")),
            ),
            check(
                "hooks: claude-code",
                Status::Pass,
                "~/work/harbor-dispatch/.claude/settings.local.json runs ~/bin/adage (this binary)",
                None,
            ),
            check(
                "hooks: codex",
                Status::Warn,
                "codex is installed, but Adage's hooks are not registered with it",
                Some(DoctorFix::run_in("{}", "adage install codex --write")),
            ),
            check("fail-open errors", Status::Pass, "none in the last 7 days", None),
        ],
        fixes: vec![None, None, Some(Fix::FetchModel), None, Some(Fix::InstallHooks("codex".into())), None],
        lights: vec![
            Light::new("hooks claude", Lamp::Ok),
            Light::new("4 reflexes", Lamp::Ok),
            Light::new("auto-condition", Lamp::Ok),
            Light { label: "no model".into(), lamp: Lamp::Warn, hint: Some("adage models fetch".into()) },
        ],
        harnesses: vec!["claude".into()],
        generation: Some("8216e29f00".into()),
        repo_key: Some("harbor-dispatch".into()),
        savings: None,
        notice: None,
    }
}

fn ctx(paths: Paths, color: bool) -> Ctx {
    Ctx { paths, root: None, label: "harbor-dispatch".into(), theme: Theme::with_color(color), tz: 0, demo: false }
}

fn live_from(paths: &Paths) -> Live {
    let mut live = Live::default();
    let c = ctx(paths.clone(), false);
    Sources::new(&c).poll(&mut live, Some("harbor-dispatch"));
    live.waiting = Some(1);
    live
}

/// A frame snapshot (insta: `cargo insta review`, or `INSTA_UPDATE=always` to accept).
fn snap(name: &str, text: &str) {
    insta::assert_snapshot!(name, text);
}

fn render(w: u16, h: u16, f: impl FnOnce(&mut ratatui::Frame)) -> (String, Buffer) {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(f).unwrap();
    let buf = term.backend().buffer().clone();
    (buffer_text(&buf), buf)
}

/// The buffer with each run of styled cells tagged: `[green b]● fire[/]`.
pub(super) fn styled(b: &Buffer) -> String {
    let tag = |c: &ratatui::buffer::Cell| -> String {
        let mut parts = Vec::new();
        if c.fg != Color::Reset {
            parts.push(format!("{:?}", c.fg).to_lowercase());
        }
        for (m, s) in [(Modifier::BOLD, "b"), (Modifier::DIM, "d"), (Modifier::ITALIC, "i"), (Modifier::REVERSED, "r")]
        {
            if c.modifier.contains(m) {
                parts.push(s.to_string());
            }
        }
        parts.join(" ")
    };
    let mut out = String::new();
    for y in 0..b.area.height {
        let mut line = String::new();
        let mut open = String::new();
        for x in 0..b.area.width {
            let c = &b[(x, y)];
            let t = tag(c);
            if t != open {
                if !open.is_empty() {
                    line.push_str("[/]");
                }
                if !t.is_empty() {
                    line.push_str(&format!("[{t}]"));
                }
                open = t;
            }
            line.push_str(c.symbol());
        }
        if !open.is_empty() {
            line.push_str("[/]");
        }
        out.push_str(line.trim_end());
        out.push('\n');
    }
    out
}

/// The last row (from the top) where `needle` shows, and its column.
fn find_last(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
    (0..buf.area.height).rev().find_map(|y| {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        row.find(needle).map(|i| (row[..i].chars().count() as u16, y))
    })
}

fn find(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        if let Some(i) = row.find(needle) {
            return Some((row[..i].chars().count() as u16, y));
        }
    }
    None
}

/// The live view's graph as text: one line per node (`boxes` false) or the box tier.
fn flow_text(
    items: &[&feed::FeedItem],
    cx: &Ctx,
    width: u16,
    boxes: bool,
    gists: &std::collections::BTreeMap<String, String>,
) -> Vec<String> {
    let o = super::flow::Opts { width, boxes, repo: false, gists, pulse: Default::default(), glow: false };
    let (lines, edge) = super::flow::lines(items, cx, &o);
    lines.iter().map(|l| l.render(edge, 0, &cx.theme).to_string().trim_end().to_string()).collect()
}

// ---------------------------------------------------------------------------------------------
// Parsing

#[test]
fn feed_items_say_what_adage_did() {
    let d = decision(
        vec![directive("lookup-incident", "fire", LOOKUP)],
        vec![directive("lookup-incident", "fire", LOOKUP)],
        vec![],
        None,
    );
    let i = feed::from_record("s-1", &rec(0, "treated", "UserPromptSubmit", "prompt", d.clone()));
    assert_eq!(
        i.acts,
        vec![Act::Directed {
            route: "lookup-incident".into(),
            strength: Strength::Fire,
            detail: "bin/fetch-incident INC-2045".into(),
            instinct: false
        }],
        "would-have is not repeated"
    );
    assert_eq!(i.texts, vec![("lookup-incident".to_string(), LOOKUP.to_string())]);
    assert_eq!((i.event.as_str(), i.harness.as_str(), i.control), ("prompt", "claude", false));
    let h = feed::from_record(
        "s-2",
        &rec(
            0,
            "control",
            "UserPromptSubmit",
            "prompt",
            decision(vec![], vec![directive("x", "prime", "")], vec![], Some("r")),
        ),
    );
    assert_eq!(h.acts, vec![Act::Withheld { route: "x".into(), strength: Strength::Prime }, Act::StopRequest]);
    // An older version's control arm was `holdout`.
    let h = feed::from_record(
        "s-2",
        &rec(0, "holdout", "Stop", "stop", decision(vec![], vec![directive("x", "prime", "")], vec![], None)),
    );
    assert!(h.control && matches!(h.acts[0], Act::Withheld { .. }), "{h:?}");
    // An older record: no kind, no repo, no harness, and the older judgement word.
    let old = feed::from_record(
        "s-3",
        &json!({ "ts": 5, "event": "PostToolUseFailure", "arm": "treated", "decision": { "judgements": [{ "kind": "derailed", "route": "r" }] } }),
    );
    assert_eq!(
        (old.event.as_str(), old.repo.as_str(), old.acts.clone()),
        ("failure", "", vec![Act::Ignored("r".into())])
    );
    // Judgements: ignored and misfired (and their older words); a kind this version does not know
    // is shown as it is, not dropped.
    let j = feed::from_record(
        "s-4",
        &rec(
            0,
            "treated",
            "PostToolBatch",
            "batch",
            decision(
                vec![],
                vec![],
                vec![
                    json!({ "kind": "ignored", "route": "a" }),
                    json!({ "kind": "misfired", "route": "b" }),
                    json!({ "kind": "followed_failed", "route": "c" }),
                    json!({ "kind": "rewarded_somehow", "route": "d" }),
                ],
                None,
            ),
        ),
    );
    assert_eq!(
        j.acts,
        vec![
            Act::Ignored("a".into()),
            Act::Misfired("b".into()),
            Act::Misfired("c".into()),
            Act::Other("rewarded somehow".into(), "d".into())
        ]
    );
    assert!(feed::from_record("s", &json!({})).quiet());
    assert_eq!(feed::short_session("7c1e04d2-11aa-4a31-9f0e-00000000a11c", 6), "00a11c");
    // The command a directive gives, or its first sentence.
    assert_eq!(feed::detail_of(DEAD_END), "npm run check");
    assert_eq!(
        feed::detail_of("That result lists these files: read them. More."),
        "That result lists these files: read them"
    );
    assert_eq!(feed::detail_of(""), "");
    // Older stored words read the same as the new.
    for w in ["red", "inhibit"] {
        let n = feed::from_record(
            "s",
            &json!({ "arm": "treated", "decision": { "directives": [{ "route": "r", "aspect": w }] } }),
        );
        assert!(matches!(&n.acts[0], Act::Directed { strength: Strength::Inhibit, .. }), "{w}");
    }
    // `strength` wins over `aspect` when a record has both.
    let n = feed::from_record(
        "s",
        &json!({ "decision": { "directives": [{ "route": "r", "strength": "fire", "aspect": "red" }] } }),
    );
    assert!(matches!(&n.acts[0], Act::Directed { strength: Strength::Fire, .. }));
}

#[test]
fn pretool_records_and_cards_are_shown() {
    // A PreToolUse rewrite: no decision, and never quiet.
    let r = json!({ "ts": T0, "event": "PreToolUse", "kind": "pretool", "arm": "treated", "repo": "harbor-dispatch",
        "pretool": { "rewrite": "test", "interpreter": ".venv/bin/python" } });
    let i = feed::from_record("s", &r);
    assert_eq!(i.event, "pretool");
    assert!(!i.quiet());
    assert_eq!(
        i.acts,
        vec![Act::Pretool {
            rewrite: true,
            kind: "test".into(),
            interpreter: ".venv/bin/python".into(),
            withheld: false
        }]
    );
    // Advice, and the control arm's withheld rewrite.
    let a = feed::from_record(
        "s",
        &json!({ "kind": "pretool", "arm": "treated", "pretool": { "advise": "pip-install", "interpreter": "x" } }),
    );
    assert!(matches!(&a.acts[0], Act::Pretool { rewrite: false, withheld: false, .. }));
    let w = feed::from_record(
        "s",
        &json!({ "kind": "pretool", "arm": "control", "withheld": true, "pretool": { "rewrite": "test", "interpreter": "x" } }),
    );
    assert!(w.control && matches!(&w.acts[0], Act::Pretool { withheld: true, .. }));
    // A session start is a quiet row, not a mystery.
    let st = feed::from_record("s", &json!({ "event": "SessionStart", "kind": "session_start", "arm": "treated" }));
    assert_eq!((st.event.as_str(), st.quiet()), ("start", true));
    // A procedure card says what it is, not its first file as if it were a command.
    let card = "<adage-reflex track=\"r/learned@0.1.0\" route=\"card-1\" strength=\"prime\" kind=\"card\">\nHow changes of this kind were done here before: learned from 3 earlier sessions of the same kind of task in this repository (2 passed the project's checks). Use it as a checklist; the specifics of your task differ.\nFiles such a change touched (in order):\n  - src/a.py (3/3 sessions)\n  - tests/test_a.py (2/3 sessions)\nCommands:\n  - pytest (ran in 3/3 sessions, 3/3 succeeded)\n</adage-reflex>";
    let c = feed::from_record(
        "s",
        &rec(
            0,
            "treated",
            "UserPromptSubmit",
            "prompt",
            decision(
                vec![json!({ "route": "card-1", "strength": "prime", "kind": "card", "text": card })],
                vec![],
                vec![],
                None,
            ),
        ),
    );
    assert!(
        matches!(&c.acts[0], Act::Directed { detail, .. } if detail == "card · 2 files · 3 traces"),
        "{:?}",
        c.acts
    );
    // The grey zone: a card delivered at an edit, not at the prompt, says so.
    let grey = feed::from_record(
        "s",
        &rec(
            0,
            "treated",
            "PostToolBatch",
            "batch",
            decision(
                vec![json!({ "route": "card-1", "strength": "prime", "kind": "card", "text": card })],
                vec![],
                vec![],
                None,
            ),
        ),
    );
    assert!(
        matches!(&grey.acts[0], Act::Directed { detail, .. } if detail == "card · 2 files · 3 traces · at the first edit"),
        "{:?}",
        grey.acts
    );
    // Drawn: the rewrite, the card, the misfire.
    let paths = Paths::at(Path::new("/nonexistent"));
    let cx = ctx(paths, false);
    let mut m = feed::from_record(
        "s",
        &rec(
            1,
            "treated",
            "PostToolBatch",
            "batch",
            decision(vec![], vec![], vec![json!({ "kind": "misfired", "route": "run-tests" })], None),
        ),
    );
    m.session = "s-misfire".into();
    let items = [&i, &w, &c, &m];
    let text = flow_text(&items, &cx, 110, false, &Default::default()).join("\n");
    assert!(text.contains("◆ adage ↪ pointed the test run at .venv/bin/python"), "a rewrite has its own mark: {text}");
    assert!(text.contains("◆ adage · PRIME · ▤ card"), "a card delivery has the card's mark: {text}");
    assert!(text.contains("would have pointed the test run at x (withheld · control session)"), "{text}");
    assert!(text.contains("card · 2 files · 3 traces"), "{text}");
    assert!(text.contains("misfired run-tests"), "{text}");
}

/// A line about a reflex names it by what it told that session; never by what it told another
/// session (another ticket); else by its gist once the reflexes are loaded; else by its id.
#[test]
fn a_reflex_is_named_by_what_it_told_this_session() {
    let cx = ctx(Paths::at(Path::new("/nonexistent")), false);
    let delivered = |sid: &str, ticket: &str| {
        let text = format!("A known next step here is:\n  bin/fetch-incident {ticket}");
        feed::from_record(
            sid,
            &json!({ "ts": T0, "event": "UserPromptSubmit", "kind": "prompt", "arm": "treated",
            "decision": { "directives": [{ "route": "lookup-incident", "strength": "fire", "text": text }] } }),
        )
    };
    let ignored = |sid: &str| {
        feed::from_record(
            sid,
            &json!({ "ts": T0, "event": "PostToolBatch", "kind": "batch", "arm": "treated",
            "decision": { "judgements": [{ "kind": "ignored", "route": "lookup-incident" }] } }),
        )
    };
    let (a, b, a2, b2, c2) =
        (delivered("s-a", "INC-24"), delivered("s-b", "INC-17"), ignored("s-a"), ignored("s-b"), ignored("s-c"));
    let lines = |gists: &std::collections::BTreeMap<String, String>| -> String {
        flow_text(&[&a, &b, &a2, &b2, &c2], &cx, 150, false, gists).join("\n")
    };
    let none = lines(&Default::default());
    // Each session's delivery shows what it told that session, and that it was not followed.
    assert!(none.contains("──▶ bin/fetch-incident INC-24  ✗ not followed"), "{none}");
    assert!(none.contains("──▶ bin/fetch-incident INC-17  ✗ not followed"), "{none}");
    assert!(none.contains("✗ not observed lookup-incident"), "never told s-c: {none}");
    let gists =
        [("lookup-incident".to_string(), "on a prompt that names a ticket, run bin/fetch-incident {item}".to_string())]
            .into();
    let named = lines(&gists);
    assert!(named.contains("✗ not observed on a prompt that names a ticket, run bin/fetch-incident {item}"), "{named}");
    assert!(named.contains("◆ adage · FIRE · on a prompt that names a ticket"), "{named}");
}

/// A session in a repository with no reflexes yet is shown as watched, with its calls counted
/// (docs/design/everyday-ux.md, state S1), not as "nothing to say".
#[test]
fn a_session_with_no_reflexes_here_is_shown_as_watched() {
    let cx = ctx(Paths::at(Path::new("/nonexistent")), false);
    let seen = |kind: &str, event: &str, calls: Option<u64>| {
        let mut r = json!({ "ts": T0, "event": event, "kind": kind, "repo": "fresh@path-1", "unrouted": true });
        if let Some(n) = calls {
            r["calls"] = json!(n);
        }
        feed::from_record("s-fresh", &r)
    };
    let (start, batch) = (seen("session_start", "SessionStart", None), seen("batch", "PostToolBatch", Some(23)));
    assert!(batch.unrouted && batch.quiet() && batch.observed == 23);
    let text = flow_text(&[&start, &batch], &cx, 110, false, &Default::default());
    assert!(text[0].contains("session started 12:04 · watching, no reflexes here yet"), "{text:?}");
    assert!(text.iter().any(|l| l.contains("├ 23 calls")), "{text:?}");
    // A repository with reflexes keeps its words.
    let routed = feed::from_record(
        "s",
        &json!({ "ts": T0, "event": "PostToolBatch", "kind": "batch", "decision": { "observed": ["exec ls"] } }),
    );
    let text = flow_text(&[&routed], &cx, 110, false, &Default::default());
    assert!(!text[0].contains("watching") && text.iter().any(|l| l.contains("├ Run  ls")), "{text:?}");
}

#[test]
fn the_maps_json_reads_in_either_words() {
    let new = json!({ "routes": [{ "reference": "r/t", "id": "a", "strength": "fire", "state": "firm",
        "live": { "emitted": 4, "followed": 2, "ignored": 1, "control": { "unaided": 3, "not_reached": 1 } },
        "arms": { "treated": { "applied": 5, "followed": 2 }, "control": { "applied": 2, "met": 1 } } }] });
    let old = json!({ "routes": [{ "reference": "r/t", "id": "a", "aspect": "green", "state": "active",
        "live": { "emitted": 4, "followed": 2, "derailed": 1, "holdout": { "unaided": 3, "not_reached": 1 } },
        "arms": { "treated": { "applied": 5, "followed": 2 }, "holdout": { "applied": 2, "met": 1 } } }] });
    let none = Default::default();
    let rows: Vec<RouteRow> =
        [new, old].iter().map(|g| super::routes::from_graph(g, &none, &Default::default()).rows.remove(0)).collect();
    for r in &rows {
        assert_eq!(r.strength, Strength::Fire);
        assert_eq!(r.lifecycle(), "firm");
        assert_eq!((r.live.ignored, r.live.unaided, r.live.not_reached), (1, 3, 1));
        assert_eq!((r.control.applied, r.control.took), (2, 1));
    }
}

#[test]
fn learn_log_reads_both_shapes_and_skips_the_rest() {
    // Today's words, and the older ones (`learn`, `aspect`, `yellow` → `green`), read the same.
    for (kind, change, from, to) in [("condition", "strength", "prime", "fire"), ("learn", "aspect", "yellow", "green")]
    {
        let t = learnlog::parse(
            &json!({ "ts": T0, "kind": kind, "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 25, "sessions_used": 24, "changes": [
                { "route": "lookup-work-item", "track": "harbor-dispatch/work-items", "change": change, "from": from, "to": to, "summary": "24/25 traces" }
            ] })
            .to_string(),
        );
        assert_eq!(t.len(), 2, "{kind}");
        assert_eq!(t[0].tone, Tone::Run);
        assert_eq!(t[0].text, "auto-condition harbor-dispatch: 24/25 traces used, 1 change");
        assert_eq!(t[1].text, "harbor-dispatch/work-items: lookup-work-item prime → fire (24/25)");
        assert_eq!(t[1].short, "work-items: lookup-work-item prime → fire (24/25)");
        assert_eq!(t[1].strength, Some(Strength::Fire));
        let c = t[1].change.as_ref().unwrap();
        assert_eq!((c.what, c.from.as_deref(), c.to.as_deref()), ("strength", Some("prime"), Some("fire")));
    }
    // `adage_learn::diff`'s summaries: the counts and bound, or the evidence counts alone.
    let t = learnlog::parse(
        &json!({ "ts": T0, "kind": "condition", "trigger": "manual", "repo": "r", "changes": [
            { "route": "finish-line-6a01b2", "track": "r/finish-line", "change": "strength", "from": "prime", "to": "fire",
              "summary": "reflex prime → fire (9 of 9 traces, lower bound 0.70)" },
            { "route": "finish-line-6a01b2", "track": "r/finish-line", "change": "evidence", "from": null, "to": null,
              "summary": "fire reflex evidence 6/8 → 7/8 traces" },
            { "route": "old", "track": "r/finish-line", "change": "dormant" },
            { "route": "nascent-one", "track": "r/finish-line", "change": "nascent", "summary": "3/4 traces" },
            { "route": "gone", "track": "r/finish-line", "change": "extinguished" },
          ], "repos": { "r": { "tracks": [{ "name": "r/finish-line" }] } } })
        .to_string(),
    );
    assert_eq!(t.len(), 6, "the legacy per-track fields beside `changes` add nothing: {t:?}");
    assert_eq!(t[0].text, "condition r: 5 changes");
    assert_eq!(t[1].text, "r/finish-line: finish-line-6a01b2 prime → fire (9/9 · lb 0.70)");
    assert_eq!(t[2].text, "r/finish-line: finish-line-6a01b2 evidence (6/8 → 7/8)");
    assert_eq!(t[3].text, "r/finish-line: old firm → dormant");
    assert_eq!(t[4].text, "r/finish-line: nascent-one → nascent (3/4)");
    assert_eq!(t[5].text, "r/finish-line: gone → extinguished");
    // A change recorded with what its reflex tells the agent reads as that, not its id.
    let t = learnlog::parse(
        &json!({ "ts": T0, "kind": "condition", "trigger": "auto", "repo": "r", "changes": [
            { "route": "run-7c62ab", "track": "r/learned", "change": "new", "to": "prime", "summary": "new prime reflex (26 of 36 traces)",
              "gist": "after editing crates/**/*.rs, run cargo clippy --workspace --all-targets" },
          ] })
        .to_string(),
    );
    let c = t[1].change.as_ref().unwrap();
    assert_eq!(
        (c.route.as_str(), c.name()),
        ("run-7c62ab", "after editing crates/**/*.rs, run cargo clippy --workspace --all-targets")
    );
    assert_eq!(
        t[1].short,
        "learned: after editing crates/**/*.rs, run cargo clippy --workspace --all-targets new · prime (26/36)"
    );
    // The older change words.
    let t = learnlog::parse(
        &json!({ "kind": "learn", "repo": "r", "changes": [
            { "route": "old", "track": "r/t", "change": "stale" },
            { "route": "gone", "track": "r/t", "change": "retired" },
            { "route": "new-one", "track": "r/t", "change": "draft" },
        ] })
        .to_string(),
    );
    let words: Vec<&str> = t[1..].iter().map(|k| k.change.as_ref().unwrap().what).collect();
    assert_eq!(words, ["dormant", "extinguished", "nascent"]);
    // Candidates are not changes: counted apart, and said in one line, without their ids.
    let cand = |route: &str, support: u64| {
        json!({ "route": route, "track": "r/learned", "change": "candidate", "support": support, "s_min": 5,
                "summary": format!("candidate seen in {support} traces; a reflex needs at least 5") })
    };
    let t = learnlog::parse(
        &json!({ "kind": "condition", "repo": "r", "sessions_read": 9, "sessions_used": 8,
                 "changes": [cand("run-970842", 4), cand("sequence-36a858", 4)] })
        .to_string(),
    );
    assert_eq!(t.len(), 2, "{t:#?}");
    assert_eq!(t[0].run.as_ref().map(|r| (r.changes, r.forming)), Some((Some(0), 2)));
    assert_eq!(t[0].short, "condition: 8/9 traces used, no reflex changed, 2 patterns forming");
    assert_eq!(t[1].text, "2 patterns forming (seen in 4 traces · a reflex needs 5)");
    assert!(!t[1].text.contains("run-970842"));
    for kind in ["condition_error", "learn_error"] {
        let e =
            learnlog::parse(&format!(r#"{{"ts":1,"kind":"{kind}","trigger":"auto","class":"lock","error":"busy"}}"#));
        assert_eq!((e[0].tone, e[0].text.as_str()), (Tone::Error, "conditioning failed (lock): busy"), "{kind}");
    }
    // Missing fields are tolerated; unknown kinds and non-JSON are skipped.
    assert_eq!(learnlog::parse(r#"{"kind":"condition"}"#)[0].text, "condition");
    assert_eq!(learnlog::parse(r#"{"kind":"learn"}"#)[0].text, "condition");
    assert!(learnlog::parse(r#"{"kind":"something-else"}"#).is_empty());
    assert!(learnlog::parse("[1,2]").is_empty() && learnlog::parse("garbage").is_empty());
    let old = learnlog::parse(
        r#"{"ts":1,"sessions_read":3,"sessions_used":2,"repos":{"acme":{"tracks":[{"name":"acme/work","version":"0.1.0","change":"new","routes":2}]}}}"#,
    );
    assert_eq!(old[0].text, "condition: 2/3 traces used, 1 track");
    assert_eq!(old[1].text, "wrote acme/work @ 0.1.0 (new, 2 reflexes)");
}

#[test]
fn settings_show_every_config_key_with_its_source() {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::at(d.path());
    std::fs::write(paths.config(), "[learn]\nmin_interval_secs = 600\nbogus = 1\n").unwrap();
    let mut s = AppState::new(paths.config());
    s.load_settings(&adage_store::config::load(&paths));
    let row = |k: &str| s.settings.iter().find(|r| r.key == k).unwrap();
    assert_eq!(s.settings.len(), adage_store::config::KEYS.len());
    assert_eq!(
        (row("learn.min_interval_secs").value.as_str(), row("learn.min_interval_secs").source.as_str()),
        ("600", "file")
    );
    assert_eq!(row("learn.thresholds").source, "default");
    assert!(s.settings_problems.iter().any(|p| p.contains("learn.bogus")), "{:?}", s.settings_problems);
    // The selected setting stays on screen however its neighbours wrap.
    let c = ctx(paths, false);
    s.tab = Tab::Settings;
    for sel in 0..s.settings.len() {
        s.setting_sel = sel;
        let text = app_frame(&s, &c, 60, 20);
        let key = s.settings[sel].key;
        let label = tabs::settings::label(key);
        assert!(text.lines().any(|l| l.starts_with(&format!("▌   {label}"))), "{key} is not shown:\n{text}");
        assert!(text.contains(key), "{key}'s card is not shown:\n{text}");
    }
}

// ---------------------------------------------------------------------------------------------
// Tailing

#[test]
fn spool_tail_sees_concurrent_appends_once_each_in_order() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().join("sessions");
    std::fs::create_dir_all(&dir).unwrap();
    let mut tail = SpoolTail::new(dir.clone());
    assert!(tail.poll().is_empty());
    const N: usize = 300;
    let writers: Vec<_> = (0..3)
        .map(|w| {
            let p = dir.join(format!("s{w}.spool"));
            std::thread::spawn(move || {
                for i in 0..N {
                    spool::append(&p, &json!({ "ts": i, "w": w, "i": i, "pad": "x".repeat(i % 50) })).unwrap();
                    if i % 17 == 0 {
                        std::thread::sleep(std::time::Duration::from_millis(1));
                    }
                }
            })
        })
        .collect();
    let mut seen: Vec<Vec<u64>> = vec![vec![]; 3];
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
    while seen.iter().map(Vec::len).sum::<usize>() < 3 * N && std::time::Instant::now() < deadline {
        for r in tail.poll() {
            seen[r.value["w"].as_u64().unwrap() as usize].push(r.value["i"].as_u64().unwrap());
        }
    }
    for w in writers {
        w.join().unwrap();
    }
    for r in tail.poll() {
        seen[r.value["w"].as_u64().unwrap() as usize].push(r.value["i"].as_u64().unwrap());
    }
    for s in &seen {
        assert_eq!(*s, (0..N as u64).collect::<Vec<_>>(), "every record once, in order");
    }
}

#[test]
fn spool_tail_handles_truncation_new_files_and_partial_frames() {
    let d = tempfile::tempdir().unwrap();
    let dir = d.path().to_path_buf();
    let p = dir.join("a.spool");
    for i in 0..3 {
        spool::append(&p, &json!({ "ts": i })).unwrap();
    }
    let mut tail = SpoolTail::new(dir.clone());
    assert_eq!(tail.poll().len(), 3, "recent history is read at the start");
    assert!(tail.poll().is_empty());
    // Truncated and restarted: read again from the top.
    std::fs::write(&p, b"").unwrap();
    spool::append(&p, &json!({ "ts": 10 })).unwrap();
    let got = tail.poll();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].value["ts"], 10);
    // A new session appears.
    spool::append(&dir.join("b.spool"), &json!({ "ts": 11 })).unwrap();
    let got = tail.poll();
    assert_eq!((got.len(), got[0].session.as_str()), (1, "b"));
    // A frame written in two pieces is seen once, when complete.
    let mut whole = Vec::new();
    {
        let tmp = dir.join("frame.bin");
        spool::append(&tmp, &json!({ "ts": 12 })).unwrap();
        whole.extend(std::fs::read(&tmp).unwrap());
        std::fs::remove_file(&tmp).unwrap();
    }
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(&whole[..9]).unwrap();
    assert!(tail.poll().is_empty());
    f.write_all(&whole[9..]).unwrap();
    let got = tail.poll();
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].value["ts"], 12);
    // Files that are not spools are ignored; a removed spool is forgotten.
    std::fs::write(dir.join("a.state"), b"xx").unwrap();
    std::fs::remove_file(dir.join("b.spool")).unwrap();
    assert!(tail.poll().is_empty());
}

#[test]
fn line_tail_waits_for_whole_lines_and_follows_rotation() {
    let d = tempfile::tempdir().unwrap();
    let p = d.path().join("learn.log");
    let mut tail = LineTail::new(p.clone());
    assert!(tail.poll().is_empty(), "no file yet");
    std::fs::write(&p, "one\ntw").unwrap();
    assert_eq!(tail.poll(), vec!["one"]);
    let mut f = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
    f.write_all(b"o\nthree\n").unwrap();
    assert_eq!(tail.poll(), vec!["two", "three"]);
    // Rotated: a new, shorter file.
    std::fs::rename(&p, d.path().join("learn.log.1")).unwrap();
    std::fs::write(&p, "four\n").unwrap();
    assert_eq!(tail.poll(), vec!["four"]);
    // Primed on a long log, the first line read is a whole one.
    let long = d.path().join("long.log");
    let body: String = (0..5000).map(|i| format!("line {i:05} {}\n", "x".repeat(20))).collect();
    std::fs::write(&long, &body).unwrap();
    let lines = LineTail::new(long).poll();
    assert!(lines.iter().all(|l| l.starts_with("line ") && l.len() == 31), "{:?}", &lines[..2]);
    assert_eq!(lines.last().unwrap(), &format!("line 04999 {}", "x".repeat(20)));
}

// ---------------------------------------------------------------------------------------------
// `adage watch`

#[test]
fn watch_frames_at_split_pane_and_wide_sizes_in_colour_and_no_color() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let live = live_from(&paths);
    assert_eq!(live.feed.len(), 10);
    let h = health();
    for (w, hgt) in [(60, 20), (80, 24), (110, 30)] {
        let plain = ctx(paths.clone(), false);
        let (text, buf) = render(w, hgt, |f| view::draw_live(f, f.area(), &live, Some(&h), &plain, true));
        snap(&format!("watch-{w}x{hgt}"), &text);
        assert_eq!(text.lines().count(), hgt as usize);
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                assert_eq!(buf[(x, y)].fg, Color::Reset, "NO_COLOR paints nothing, at {x},{y}");
            }
        }
        let colour = ctx(paths.clone(), true);
        let (_, buf) = render(w, hgt, |f| view::draw_live(f, f.area(), &live, Some(&h), &colour, true));
        snap(&format!("watch-{w}x{hgt}-colour"), &styled(&buf));
        for y in 0..buf.area.height {
            for x in 0..buf.area.width {
                assert_eq!(buf[(x, y)].bg, Color::Reset, "nothing paints the background, at {x},{y}");
            }
        }
    }
    let c = ctx(paths.clone(), true);
    let (wide, buf) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    // Colour carries the strength: the delivered reflex's box and lamp are green.
    let (x, y) = find(&buf, "◆ adage · ● FIRE").expect("a delivery is shown");
    assert_eq!(buf[(x + 10, y)].fg, Color::Green);
    assert_eq!(buf[(x - 3, y)].fg, Color::Green, "its box is drawn in its strength");
    for s in [
        "adage watch  harbor-dispatch · gen 8216e29f · claude",
        "● live",
        // The band: what Adage did, counted.
        "─ ◠ Fired ─",
        "─ ✓ Followed ─",
        "1 of 3",
        // The session's graph: the turn, the delivery and its response, lit when followed.
        "╭─ ❯ PROMPT",
        "2 steps · Run ×2 · adage fired 2 · 1 followed",
        "a reflex learned here",
        "─ RUN ─",
        "bin/fetch-incident INC-2045",
        "─ ✓ followed ─",
        "✓ followed adage",
        "◆ adage ✓ finish line met: codegen-after-registry",
        // The last conditioning, the review line, the lamps, the keys.
        "◆ conditioned 12:04 · 24 traces · 2 changes",
        "conditioning failed (io)",
        "1 to look over · adage review",
        "! no model · adage models fetch",
        "p  pause   f  scope   /  filter   enter  inspect   m  open map   ?  keys",
    ] {
        assert!(wide.contains(s), "the wide frame lacks {s:?}:\n{wide}");
    }
    // Every conditioning run in full, with what each changed (`l`).
    let mut runs = live_from(&paths);
    runs.key('l');
    let (popup, _) = render(110, 30, |f| view::draw_live(f, f.area(), &runs, Some(&h), &c, true));
    for s in [
        "conditioning runs",
        "lookup-incident",
        "prime → fire",
        "24/25 · lb 0.80",
        "conditioning failed for harbor-dispatch (io)",
    ] {
        assert!(popup.contains(s), "the runs lack {s:?}:\n{popup}");
    }
    // Taller, every session shows: the control arm's is dimmed, whole rows of it, and the
    // inhibit's box is red.
    let (tall, buf) = render(110, 60, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    let (x, y) = find(&buf, "would fire bin/fetch-incident").unwrap_or_else(|| panic!("{tall}"));
    assert_eq!(buf[(x, y)].fg, Color::DarkGray);
    assert!(tall.contains("control: Adage holds back"), "{tall}");
    // Never delivered to that session in the feed, and no reflexes loaded: its id, not what
    // another session was told; and the finish line Adage held it to.
    assert!(tall.contains("✗ not observed lookup-incident"), "{tall}");
    assert!(tall.contains("◆ adage ■ finish line: asked the agent to go on before it stops"), "{tall}");
    let (x, y) = find(&buf, "◆ adage · ● INHIBIT").unwrap_or_else(|| panic!("{tall}"));
    assert_eq!(buf[(x, y)].fg, Color::Red);
    assert!(tall.contains("npm run check") && tall.contains("after a failure"), "{tall}");

    // All repositories, quiet events hidden, paused.
    let mut live = live;
    live.key('f');
    live.key('v');
    live.key('p');
    let (all, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(all.contains("now · all repositories · quiet hidden") && all.contains("paused +0"), "{all}");
    assert!(all.contains("PAUSED") && all.contains("p resumes"), "a clear paused banner:\n{all}");
    assert!(
        all.contains("Codex  acme-billing · session, since 12:04") && all.contains("❯ PROMPT"),
        "the codex trace from another repository:\n{all}"
    );
    assert!(!all.contains("nothing to say"));
}

#[test]
fn ascii_glyphs_when_the_terminal_cannot_draw_them() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let live = live_from(&paths);
    let h = health();
    let c = Ctx { theme: Theme::with_color(false).ascii(), ..ctx(paths, false) };
    let (text, _) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    snap("watch-110x30-ascii", &text);
    assert!(text.contains("* adage - FIRE") && text.contains("+- RUN -") && text.contains("--> |"), "{text}");
    assert!(!text.chars().any(|ch| matches!(ch, '●' | '◠' | '─' | '→' | '✓')), "{text}");
}

/// Every screen under `ADAGE_ASCII=1`: each tab, the popups, the palette, a toast, the watch
/// pane, and every step of the setup wizard, with its scrollback, draw nothing but ASCII.
#[test]
fn ascii_reaches_every_screen() {
    use crate::detect::Detected;
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let c = Ctx { theme: Theme::with_color(false).ascii(), ..ctx(paths.clone(), false) };
    let colour = Ctx { theme: Theme::with_color(true).ascii(), ..ctx(paths.clone(), false) };
    let ascii = |what: &str, text: &str| {
        let bad: String = text.chars().filter(|ch| !ch.is_ascii() && *ch != '\u{a0}').collect();
        assert!(bad.is_empty(), "{what} draws {bad:?} under ADAGE_ASCII:\n{text}");
    };
    let mut s = app_state();
    s.live = live_from(&paths);
    s.notices.push("a notice… with → arrows".into());
    s.toast("read-linked-files: strengthen recorded · 1 change", true);
    for tab in [Tab::Live, Tab::Reflexes, Tab::Map, Tab::Health, Tab::Settings] {
        s.tab = tab;
        for (w, h) in [(150, 35), (80, 24), (44, 12)] {
            for cx in [&c, &colour] {
                ascii(&format!("{tab:?} {w}x{h}"), &app_frame(&s, cx, w, h));
            }
            s.key(KeyCode::Char('?'), NONE);
            ascii(&format!("{tab:?} keys {w}x{h}"), &app_frame(&s, &c, w, h));
            s.key(KeyCode::Esc, NONE);
        }
    }
    // Compact map rows, the Reflexes filter, a group, a closed track.
    s.tab = Tab::Map;
    s.map.compact = true;
    ascii("map rows", &app_frame(&s, &c, 100, 30));
    s.tab = Tab::Reflexes;
    s.reflexes.sel = Some("group:nascent".into());
    s.key(KeyCode::Enter, NONE);
    ascii("a closed group", &app_frame(&s, &c, 150, 35));
    s.key(KeyCode::Char('/'), NONE);
    s.key(KeyCode::Char('z'), NONE);
    ascii("the filter", &app_frame(&s, &c, 150, 35));
    s.key(KeyCode::Esc, NONE);
    // Popups: a review confirmation, a message, the traces viewer, the palette, each fix.
    tabs::reflexes::select(&mut s, "harbor-dispatch/intents", "start-with-npm-check");
    for k in ['s', 't', 'o', 'd', 'w'] {
        s.key(KeyCode::Char(k), NONE);
        ascii(&format!("reflexes {k}"), &app_frame(&s, &c, 100, 30));
        s.modal = None;
    }
    s.key(KeyCode::Char(':'), NONE);
    ascii("the palette", &app_frame(&s, &c, 150, 35));
    s.key(KeyCode::Char('s'), NONE);
    ascii("the palette, typed", &app_frame(&s, &c, 150, 35));
    s.palette = None;
    for fix in [super::health::Fix::FetchModel, super::health::Fix::LearnNow, super::health::Fix::EnableAutoLearn] {
        s.start_fix(fix);
        ascii("a fix", &app_frame(&s, &c, 100, 30));
        s.modal = None;
    }
    s.modal = Some(Modal::Progress { title: "Fetch…".into(), status: "reading traces…".into(), ratio: 0.5 });
    ascii("progress", &app_frame(&s, &c, 100, 30));
    s.modal = Some(Modal::Scope { harness: "codex".into() });
    ascii("scope", &app_frame(&s, &c, 100, 30));
    s.modal = Some(Modal::Edit { key: "learn.min_interval_secs", buffer: "120".into(), error: Some("no…".into()) });
    ascii("edit", &app_frame(&s, &c, 100, 30));
    s.modal = None;

    // The watch pane: every size, each popup, filtered, empty.
    let mut live = live_from(&paths);
    let h = health();
    for (w, hgt) in [(110, 30), (80, 24), (60, 20), (40, 12), (30, 8)] {
        ascii("watch", &render(w, hgt, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true)).0);
    }
    for k in ['?', 'm'] {
        live.key(k);
        watch::live_key(&mut live, KeyCode::Char(k), NONE);
        ascii("watch popup", &render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true)).0);
        watch::live_key(&mut live, KeyCode::Esc, NONE);
    }
    watch::live_key(&mut live, KeyCode::Enter, NONE);
    ascii("inspect", &render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true)).0);
    watch::live_key(&mut live, KeyCode::Esc, NONE);
    live.filter = "nothing-like-it".into();
    ascii("no match", &render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true)).0);
    let mut empty = Live::default();
    empty.waiting = Some(2);
    empty.learning = vec!["x".into()];
    ascii("empty", &render(40, 12, |f| view::draw_live(f, f.area(), &empty, None, &c, true)).0);

    // The wizard, every step, then its scrollback.
    let detected = vec![Detected { harness: "claude-code", evidence: vec!["~/.claude exists".into()] }];
    let mut w = Wizard::new(detected, vec![], true);
    w.config_path = "~/.config/adage/config.toml".into();
    w.auto_learn = Some((false, "default".into()));
    ascii("wizard header", &c.theme.lines(wizard::header(&c)).iter().map(|l| l.to_string()).collect::<String>());
    loop {
        ascii(&format!("wizard {:?}", w.current()), &render(80, 18, |f| wizard::draw(f, &w, &c)).0);
        w.progress = Some(("reading traces…".into(), 0.4));
        ascii("wizard progress", &render(80, 18, |f| wizard::draw(f, &w, &c)).0);
        w.progress = None;
        if *w.current() == wizard::Kind::Finish {
            break;
        }
        w.advance(wizard::StepStatus::Done("on · learn.auto = true".into()));
    }
    w.health = Some(health());
    ascii("wizard summary", &render(80, 18, |f| wizard::draw(f, &w, &c)).0);
    for step in &w.steps {
        ascii(
            "wizard record",
            &c.theme.lines(wizard::record(step, &c.theme)).iter().map(|l| l.to_string()).collect::<String>(),
        );
    }
    let card = c.theme.lines(wizard::finish_card(&w, &c.theme, true, 80));
    ascii("finish card", &card.iter().map(|l| l.to_string()).collect::<String>());
}

#[test]
fn the_newest_firing_pulses_once() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let c = ctx(paths.clone(), true);
    let mut live = live_from(&paths);
    assert!(!live.pulsing(), "history read at the start does not pulse");
    // A new firing arrives.
    let mut sources = Sources::new(&c);
    sources.poll(&mut live, Some("harbor-dispatch")); // primes the new tail on what is there
    spool::append(
        &paths.spool("7c1e04d2-11aa-4a31-9f0e-00000000a11c"),
        &rec(
            30,
            "treated",
            "UserPromptSubmit",
            "prompt",
            decision(vec![directive("lookup-incident", "green", LOOKUP)], vec![], vec![], None),
        ),
    )
    .unwrap();
    sources.poll(&mut live, Some("harbor-dispatch"));
    assert!(live.pulsing() && live.pulse.level == 3);
    let h = health();
    // The new delivery's strength flashes (reversed), and the newest node glows: its time is now.
    let at = live.pulse.at.unwrap();
    live.tick_pulse(at + std::time::Duration::from_millis(100), true);
    assert!(live.anim.glow() && live.anim.beat == 3, "the heartbeat beats");
    let (text, buf) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    let (x, y) = find_last(&buf, "● FIRE").expect("the new delivery");
    assert!(buf[(x, y)].modifier.contains(Modifier::REVERSED), "the new delivery flashes:\n{text}");
    assert!(text.contains("at the prompt · now"), "the newest glows:\n{text}");
    // It dims, then stops, within 900 ms; the heartbeat settles.
    live.tick_pulse(at + std::time::Duration::from_millis(450), true);
    assert_eq!(live.pulse.level, 2);
    live.tick_pulse(at + std::time::Duration::from_millis(950), true);
    assert!(!live.pulsing() && live.pulse.level == 0 && !live.anim.glow());
    let (text, buf) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    let (x, y) = find_last(&buf, "● FIRE").unwrap();
    assert!(!buf[(x, y)].modifier.contains(Modifier::REVERSED), "{text}");
    assert!(text.contains("at the prompt · 12:04:30"), "{text}");
    // Reduced motion: no pulse, no heartbeat at all.
    live.pulse.at = Some(at);
    live.tick_pulse(at, false);
    assert_eq!(live.pulse.level, 0);
    assert!(!live.anim.glow());
}

#[test]
fn watch_keys_filter_inspect_and_help() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let mut live = live_from(&paths);
    let h = health();
    let c = ctx(paths, false);
    // `/` then letters filter; `q` typed is text, not quit; enter keeps, esc clears.
    assert!(!watch::live_key(&mut live, KeyCode::Char('/'), NONE));
    for ch in "dead".chars() {
        assert!(!watch::live_key(&mut live, KeyCode::Char(ch), NONE));
    }
    assert!(!watch::live_key(&mut live, KeyCode::Char('q'), NONE), "typing, not quitting");
    watch::live_key(&mut live, KeyCode::Backspace, NONE);
    watch::live_key(&mut live, KeyCode::Enter, NONE);
    assert_eq!((live.filter.as_str(), live.filtering), ("dead", false));
    let (text, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(
        text.contains("INHIBIT") && text.contains("npm run check") && !text.contains("bin/fetch-incident"),
        "only matching events in the graph:\n{text}"
    );
    assert!(text.contains("filter “dead”"), "{text}");
    // Enter inspects the newest event shown: what the agent saw, exactly.
    watch::live_key(&mut live, KeyCode::Enter, NONE);
    assert!(live.inspect);
    let (text, _) = render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(text.contains("WHAT THE AGENT SAW") && text.contains("npm run check"), "{text}");
    assert!(!watch::live_key(&mut live, KeyCode::Esc, NONE) && !live.inspect, "esc closes the popup");
    assert!(!watch::live_key(&mut live, KeyCode::Esc, NONE) && live.filter.is_empty(), "esc clears the filter");
    // `?` lists the same keys as the footer.
    watch::live_key(&mut live, KeyCode::Char('?'), NONE);
    let (text, _) = render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    for k in keys::WATCH {
        assert!(text.contains(k.about), "the overlay lacks {:?}:\n{text}", k.about);
    }
    watch::live_key(&mut live, KeyCode::Esc, NONE);
    watch::live_key(&mut live, KeyCode::Char('m'), NONE);
    let (text, _) = render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(text.contains("adage map"), "{text}");
    watch::live_key(&mut live, KeyCode::Esc, NONE);
    assert!(watch::live_key(&mut live, KeyCode::Char('q'), NONE));
}

#[test]
fn empty_home_says_what_to_do() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let live = Live::default();
    let (text, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, None, &c, true));
    assert!(
        text.contains("checking health")
            && text.contains("No traces yet")
            && text.contains("adage init")
            && text.contains("Nothing conditioned yet")
            && text.contains("adage condition"),
        "{text}"
    );
    let (tiny, _) = render(25, 6, |f| view::draw_live(f, f.area(), &live, None, &c, true));
    assert!(tiny.contains("larger"), "{tiny}");
    // Short panes keep each section's next command, and the review line, on screen.
    let mut live = Live::default();
    live.waiting = Some(4);
    let h = health();
    for (w, hgt) in [(40, 12), (40, 10), (30, 8), (60, 14)] {
        let (text, _) = render(w, hgt, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
        for want in ["adage init", "adage condition", "adage review", "q  quit"] {
            assert!(text.contains(want), "{w}x{hgt} lacks {want:?}:\n{text}");
        }
    }
    let paths = fixture(d.path());
    let mut busy = live_from(&paths);
    busy.waiting = Some(4);
    let (text, _) = render(40, 12, |f| view::draw_live(f, f.area(), &busy, Some(&h), &c, true));
    assert!(text.contains("4 to look over · adage review") && text.contains("adage watch"), "{text}");
}

// ---------------------------------------------------------------------------------------------
// `adage ui`

#[allow(clippy::too_many_arguments)]
fn route_row(
    reference: &str,
    id: &str,
    strength: Strength,
    state: &str,
    (support, opportunities): (u64, u64),
    trigger: (&str, &str),
    step: (&str, &str),
    verbs: Vec<&'static str>,
) -> RouteRow {
    RouteRow {
        reference: reference.into(),
        track: reference.into(),
        version: "0.3.0".into(),
        repo: "harbor-dispatch".into(),
        imported: false,
        in_index: state != "nascent" && state != "extinguished",
        instinct: false,
        id: id.into(),
        gist: format!("the gist of {id}"),
        strength,
        ceiling: "fire".into(),
        effect: "observe".into(),
        state: state.into(),
        authored: false,
        approved: false,
        trigger_kind: trigger.0.into(),
        trigger: trigger.1.into(),
        step_kind: step.0.into(),
        step: step.1.into(),
        executable: step.0 == "run",
        execution: Default::default(),
        support,
        opportunities,
        rate: support as f64 / opportunities.max(1) as f64,
        lower_bound: 0.53,
        turns_saved: 0.0,
        live: LiveCounts::default(),
        with: Arm::default(),
        control: Arm::default(),
        directive: String::new(),
        review_preview: Some(adage_app::review::ReviewPreview {
            revision: "fixture-revision".into(),
            current_directive: "Fixture response".into(),
            proposal: None,
        }),
        verbs,
        reasons: vec![],
        more: Default::default(),
    }
}

fn routes() -> Routes {
    let t = "harbor-dispatch/incidents";
    let mut lookup = route_row(
        t,
        "lookup-incident",
        Strength::Fire,
        "firm",
        (24, 25),
        ("prompt", "a prompt names a ticket"),
        ("run", "bin/fetch-incident {item}"),
        vec!["approve", "extinguish", "weaken", "pin"],
    );
    lookup.lower_bound = 0.80;
    lookup.turns_saved = 8.0;
    lookup.live = LiveCounts { emitted: 3, followed: 2, ignored: 1, met: 0, would_have: 1, unaided: 1, not_reached: 0 };
    lookup.with = Arm { applied: 12, took: 10 };
    lookup.control = Arm { applied: 5, took: 1 };
    lookup.reasons = vec![("new".into(), "conditioned and never reviewed".into())];
    lookup.directive = "<adage-reflex track=\"harbor-dispatch/incidents@0.3.0\" reflex=\"lookup-incident\" strength=\"fire\">\nIncidents here are looked up with a script. A known next step here is:\n  bin/fetch-incident INC-2045. Ignore this if the user is asking for something else.\nLearned by Adage from this repository's earlier agent sessions; not requested by the user.\n</adage-reflex>".into();
    let linked = route_row(
        t,
        "read-linked-files",
        Strength::Fire,
        "firm",
        (13, 17),
        ("after_command", "`bin/fetch-incident {item}` succeeds"),
        ("read_batch", "read ≤5 listed files at once"),
        vec!["approve", "extinguish", "weaken", "pin"],
    );
    let dead = route_row(
        t,
        "dead-end-npm-test",
        Strength::Inhibit,
        "firm",
        (11, 43),
        ("after_failure", "`npm test` fails: HARBOR_ENV is not set"),
        ("avoid", "npm run check (not npm test)"),
        vec!["approve", "extinguish", "pin"],
    );
    let forming = route_row(
        "harbor-dispatch/intents",
        "start-with-npm-check",
        Strength::Prime,
        "forming",
        (6, 9),
        ("intent", "a prompt means: run the tests"),
        ("avoid", "npm run check (not npm test)"),
        vec!["approve", "extinguish", "strengthen", "pin"],
    );
    let mut instinct = route_row(
        "instincts/finish-line",
        "finish-line/regenerate",
        Strength::Prime,
        "firm",
        (0, 0),
        ("after_edit", "an edit touches registry/*.csv"),
        ("run", "npm run codegen"),
        vec!["approve", "extinguish", "pin"],
    );
    instinct.instinct = true;
    instinct.track = "adage/finish-line".into();
    instinct.version = "0.1.0".into();
    instinct.effect = "regenerate".into();
    let nascent = route_row(
        t,
        "regen-openapi-client",
        Strength::Prime,
        "nascent",
        (3, 4),
        ("after_edit", "an edit touches api/*.yaml"),
        ("run", "npm run openapi"),
        vec!["approve", "extinguish", "pin"],
    );
    let gone = route_row(
        t,
        "grep-for-incident",
        Strength::Prime,
        "extinguished",
        (2, 9),
        ("prompt", "a prompt names a ticket"),
        ("run", "grep -rn {item} src"),
        vec!["approve"],
    );
    Routes { rows: vec![lookup, linked, dead, nascent, gone, forming, instinct], traces: 54 }
}

fn app_state() -> AppState {
    let mut s = AppState::new("/home/me/.local/share/adage/config.toml".into());
    // Every key at its default, whatever this shell's environment says.
    s.settings = adage_store::config::KEYS
        .iter()
        .map(|k| app::SettingRow {
            key: k.name,
            value: k.default.to_string(),
            source: "default".into(),
            doc: k.doc,
            default: k.default,
        })
        .collect();
    s.health = Some(health());
    s.routes = Some(Ok(routes()));
    s
}

fn app_frame(s: &AppState, c: &Ctx, w: u16, h: u16) -> String {
    render(w, h, |f| app::draw(f, s, c)).0
}

#[test]
fn reflex_detail_prioritizes_execution_and_unknown_outcome() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    // One relevant result remains readable when the tree stacks above the detail.
    s.routes.as_mut().unwrap().as_mut().unwrap().rows.truncate(1);
    let row = &mut s.routes.as_mut().unwrap().as_mut().unwrap().rows[0];
    row.execution.mode = "Needs recovery".into();
    row.execution.reason = "Inspect the worktree before acknowledging".into();
    row.execution.last_result = Some("Unknown · completion could not be recorded".into());
    for width in [70, 110] {
        let text = app_frame(&s, &c, width, 30);
        assert!(text.contains("Needs recovery") && text.contains("Unknown"), "{text}");
        assert!(!text.contains("Completed"), "unknown must never render as success");
        snap(&format!("app-reflexes-recovery-{width}"), &text);
    }
    s.key(KeyCode::Char('a'), NONE);
    assert!(matches!(&s.modal, Some(Modal::Confirm { lines, .. })
        if lines.iter().any(|l| l.to_string().contains("does not grant script execution"))));
}

#[test]
fn app_frames_every_tab() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let c = ctx(paths.clone(), false);
    let mut s = app_state();
    s.live = live_from(&paths);
    s.settings[0].value = "false".into();
    s.settings[0].source = "file".into();
    for (tab, name) in [
        (Tab::Live, "app-live"),
        (Tab::Reflexes, "app-reflexes"),
        (Tab::Map, "app-map"),
        (Tab::Health, "app-health"),
        (Tab::Settings, "app-settings"),
        (Tab::Home, "app-home"),
    ] {
        s.tab = tab;
        let text = app_frame(&s, &c, 150, 35);
        snap(name, &text);
        assert!(text.contains(" adage   ") && text.contains("q  quit"), "{text}");
    }
    // Narrower: the Reflexes tab stacks tree and detail; the map draws smaller boxes.
    s.tab = Tab::Reflexes;
    snap("app-reflexes-80x30", &app_frame(&s, &c, 80, 30));
    s.tab = Tab::Map;
    snap("app-map-110x30", &app_frame(&s, &c, 110, 30));
    s.map.compact = true;
    snap("app-map-compact-80x24", &app_frame(&s, &c, 80, 24));
    // In colour: the selected tab in the accent, lamps in their strength.
    s.map.compact = false;
    let colour = ctx(paths, true);
    s.tab = Tab::Reflexes;
    let (_, buf) = render(150, 35, |f| app::draw(f, &s, &colour));
    snap("app-reflexes-colour", &styled(&buf));
    let (x, y) = find(&buf, " 2 Reflexes ").unwrap();
    assert_eq!(buf[(x, y)].fg, Color::Magenta);
    assert!(buf[(x, y)].modifier.contains(Modifier::REVERSED));
}

#[test]
fn reflexes_tab_tree_detail_and_empty_state() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    let text = app_frame(&s, &c, 150, 35);
    for want in [
        "▾ incidents · v0.3.0",
        "3 reflexes",
        "▾ intents · v0.3.0",
        "▾ Waiting for review",
        "▾ Instincts · v0.1.0",
        "▸ Extinguished",
        "→ the gist of lookup-incident",
        "24 of 25 traces",
        "Review evidence and decisions",
        "WHEN",
        "DO",
    ] {
        assert!(text.contains(want), "the overview lacks {want:?}:\n{text}");
    }
    assert!(!text.contains("grep-for-incident"));
    assert!(!text.contains("What the agent sees"), "long review belongs in the modal");
    s.key(KeyCode::Enter, NONE);
    s.key(KeyCode::End, NONE);
    let text = app_frame(&s, &c, 150, 35);
    assert!(text.contains("adage review weaken harbor-dispatch/incidents lookup-incident"), "{text}");
    s.key(KeyCode::Esc, NONE);
    s.key(KeyCode::Down, NONE);
    assert!(app_frame(&s, &c, 150, 35).contains("The gist of read-linked-files"));
    s.routes = Some(Ok(Routes::default()));
    let text = app_frame(&s, &c, 100, 24);
    assert!(text.contains("No reflexes yet.") && text.contains("adage condition"), "{text}");
    s.tab = Tab::Map;
    let text = app_frame(&s, &c, 100, 24);
    assert!(text.contains("Nothing to map yet.") && text.contains("adage condition"), "{text}");
}

#[test]
fn reflexes_keys_filter_chips_groups_and_review() {
    let mut s = app_state();
    assert!(matches!(s.key(KeyCode::Char('2'), NONE), Some(Cmd::Want(app::WantKind::Routes))));
    assert_eq!(s.tab, Tab::Reflexes);
    assert_eq!(s.selected_route().unwrap().id, "lookup-incident", "the first reflex is selected");
    // Strengthen is not open to a fire reflex: a message, no command.
    assert!(s.key(KeyCode::Char('s'), NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::Message { error: true, .. })));
    assert!(s.key(KeyCode::Esc, NONE).is_none() && s.modal.is_none());
    // Weaken asks first, naming the CLI command; n cancels.
    assert!(s.key(KeyCode::Char('w'), NONE).is_none());
    match &s.modal {
        Some(Modal::Confirm { lines, .. }) => {
            let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
            assert!(
                text.iter().any(|l| l.contains("adage review weaken harbor-dispatch/incidents lookup-incident")),
                "{text:#?}"
            );
        }
        _ => panic!("expected a confirmation"),
    }
    assert!(s.key(KeyCode::Char('n'), NONE).is_none() && s.modal.is_none());
    // The fuzzy filter: `/npmchk` finds start-with-npm-check (its group opens); `q` is text.
    s.key(KeyCode::Char('/'), NONE);
    for ch in "npmchk".chars() {
        assert!(s.key(KeyCode::Char(ch), NONE).is_none());
    }
    assert!(s.key(KeyCode::Char('q'), NONE).is_none(), "typing, not quitting");
    s.key(KeyCode::Backspace, NONE);
    s.key(KeyCode::Enter, NONE);
    let nodes = tabs::reflexes::tree(&s);
    let ids: Vec<&str> = nodes
        .iter()
        .filter_map(|n| match n {
            tabs::reflexes::Node::Reflex(i) => Some(s.route_rows()[*i].id.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(ids, vec!["start-with-npm-check"]);
    // Strengthen, confirmed, records `strengthen` for the prime reflex.
    s.key(KeyCode::Down, NONE);
    assert_eq!(s.selected_route().unwrap().id, "start-with-npm-check");
    s.key(KeyCode::Char('s'), NONE);
    match s.key(KeyCode::Char('y'), NONE) {
        Some(Cmd::Do(app::Action::Review { verb, reference, route, .. })) => {
            assert_eq!(
                (verb, reference.as_str(), route.as_str()),
                ("strengthen", "harbor-dispatch/intents", "start-with-npm-check")
            )
        }
        other => panic!("expected a review command, got {other:?}"),
    }
    s.key(KeyCode::Esc, NONE);
    assert!(s.reflexes.filter.is_empty());
    // Chips: F hides fire; S cycles the lifecycle filter.
    s.key(KeyCode::Char('F'), NONE);
    assert!(tabs::reflexes::tree(&s).iter().all(|n| match n {
        tabs::reflexes::Node::Reflex(i) => s.route_rows()[*i].strength != Strength::Fire,
        _ => true,
    }));
    s.key(KeyCode::Char('F'), NONE);
    s.key(KeyCode::Char('S'), NONE);
    assert_eq!(tabs::reflexes::STATES[s.reflexes.states], "firm");
    for _ in 1..tabs::reflexes::STATES.len() {
        s.key(KeyCode::Char('S'), NONE);
    }
    // Groups: enter on a group closes it; the extinguished group opens.
    let before = tabs::reflexes::tree(&s).len();
    s.reflexes.sel = Some("group:extinguished".into());
    s.key(KeyCode::Enter, NONE);
    assert_eq!(tabs::reflexes::tree(&s).len(), before + 1);
    // Browser navigation returns an executable command, and delegation opens guided inputs.
    tabs::reflexes::select(&mut s, "harbor-dispatch/incidents", "lookup-incident");
    assert!(
        matches!(s.key(KeyCode::Char('o'), NONE),Some(Cmd::OpenWeb {focus:Some(key),traces:false}) if key.contains("lookup-incident") && key.contains("harbor-dispatch/incidents"))
    );
    s.key(KeyCode::Char('d'), NONE);
    assert!(matches!(&s.modal, Some(Modal::Form(form)) if form.kind == super::forms::Kind::Delegate));
    s.key(KeyCode::Esc, NONE);
    s.key(KeyCode::Char('t'), NONE);
    assert!(matches!(&s.modal, Some(Modal::Viewer { .. })));
}

#[test]
fn map_layers_share_ends_focus_and_keys() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Map;
    let g = tabs::map::graph(&s);
    // Live reflexes only (not nascent, not extinguished), instincts last.
    let ids: Vec<&str> = g.reflexes.iter().map(|&i| s.route_rows()[i].id.as_str()).collect();
    assert_eq!(
        ids,
        ["lookup-incident", "read-linked-files", "dead-end-npm-test", "start-with-npm-check", "finish-line/regenerate"]
    );
    // Two reflexes converge on one response.
    assert_eq!(g.responses.len(), 4);
    assert_eq!(g.resp_of[2], g.resp_of[3]);
    let text = app_frame(&s, &c, 150, 35);
    for want in [
        "STIMULUS",
        "REFLEX",
        "RESPONSE",
        "╭",
        "▶",
        "RUN INSTEAD",
        "adage map --focus lookup-incident",
        "4 reflexes · 1 instinct · 54 traces",
    ] {
        assert!(text.contains(want), "the map lacks {want:?}:\n{text}");
    }
    assert!(text.contains('╯') || text.contains('┤') || text.contains('┬'), "the converging edge joins:\n{text}");
    // Moving, focusing: focus on dead-end keeps the reflexes two hops away.
    s.key(KeyCode::Char('j'), NONE);
    s.key(KeyCode::Char('j'), NONE);
    assert_eq!(tabs::map::selected(&s).unwrap().id, "dead-end-npm-test");
    s.key(KeyCode::Char('f'), NONE);
    let g = tabs::map::graph(&s);
    let ids: Vec<&str> = g.reflexes.iter().map(|&i| s.route_rows()[i].id.as_str()).collect();
    assert_eq!(ids, ["dead-end-npm-test", "start-with-npm-check"]);
    assert_eq!(tabs::map::selected(&s).unwrap().id, "dead-end-npm-test", "the selection survives focus");
    // Zoom: + at boxes, - at rows, say they are at the end; the info line names the level.
    assert!(app_frame(&s, &c, 150, 35).contains("zoom boxes (max)"));
    s.key(KeyCode::Char('+'), NONE);
    assert!(!s.map.compact && s.toasts.last().is_some_and(|t| t.text.contains("boxes is the largest")));
    s.key(KeyCode::Char('-'), NONE);
    assert!(s.map.compact);
    s.key(KeyCode::Char('-'), NONE);
    assert!(s.map.compact && s.toasts.last().is_some_and(|t| t.text.contains("rows is the smallest")));
    assert!(app_frame(&s, &c, 150, 35).contains("zoom rows (min)"));
    // `o` opens the live map directly; Enter navigates inside the terminal app.
    assert!(
        matches!(s.key(KeyCode::Char('o'), NONE),Some(Cmd::OpenWeb {focus:Some(key),traces:false}) if key.contains("dead-end-npm-test"))
    );
    s.key(KeyCode::Enter, NONE);
    assert_eq!(s.tab, Tab::Reflexes);
    assert_eq!(s.selected_route().unwrap().id, "dead-end-npm-test");
}

#[test]
fn palette_finds_actions_and_reflexes_and_runs_them() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Health;
    // `:` opens it; so does ctrl-k.
    s.key(KeyCode::Char(':'), NONE);
    assert!(s.palette.is_some());
    s.key(KeyCode::Esc, NONE);
    assert!(s.palette.is_none());
    s.key(KeyCode::Char('k'), KeyModifiers::CONTROL);
    for ch in "str".chars() {
        s.key(KeyCode::Char(ch), NONE);
    }
    let found = palette::matches(&s, "str");
    assert_eq!(found[0].0.title, "Strengthen reflex…");
    assert_eq!(found[0].0.note, "adage review strengthen");
    assert!(found.iter().any(|(e, _)| e.title == "start-with-npm-check"), "reflexes match too");
    let text = app_frame(&s, &c, 150, 35);
    snap("app-palette", &text);
    assert!(text.contains("› Strengthen reflex…") && text.contains("adage review strengthen"), "{text}");
    // At 60 columns titles keep their room; the notes give way.
    let narrow = app_frame(&s, &c, 60, 24);
    assert!(narrow.contains("› Strengthen reflex…") && narrow.contains("start-with-npm-check"), "{narrow}");
    // Every action shows the command that does the same.
    for (e, _) in palette::matches(&s, "") {
        if e.section == palette::Section::Actions && e.title.ends_with('…') {
            assert!(e.note.starts_with("adage "), "{e:?}");
        }
    }
    // Enter runs it: Strengthen applies to the selected reflex (lookup-incident is fire, so no).
    s.palette = Some(Palette { query: "weak".into(), sel: 0 });
    assert!(!matches!(s.key(KeyCode::Enter, NONE), Some(Cmd::Do(_))), "nothing runs before it is confirmed");
    assert_eq!(s.tab, Tab::Reflexes);
    assert!(matches!(&s.modal, Some(Modal::Confirm { title, .. }) if title == "Weaken"));
    s.key(KeyCode::Esc, NONE);
    // A reflex entry selects it; tab completes the query; a setting entry opens Settings on it.
    s.palette = Some(Palette { query: "readlinked".into(), sel: 0 });
    s.key(KeyCode::Enter, NONE);
    assert_eq!(s.selected_route().unwrap().id, "read-linked-files");
    s.palette = Some(Palette { query: "thresholds".into(), sel: 0 });
    s.key(KeyCode::Tab, NONE);
    assert_eq!(s.palette.as_ref().unwrap().query, "Settings · learn.thresholds");
    s.key(KeyCode::Enter, NONE);
    assert_eq!(s.tab, Tab::Settings);
    assert_eq!(s.settings[s.setting_sel].key, "learn.thresholds");
    // Nothing matches: says so.
    s.palette = Some(Palette { query: "zzzzqqq".into(), sel: 0 });
    let text = app_frame(&s, &c, 100, 30);
    assert!(text.contains("Nothing matches") && text.contains("0 matches"), "{text}");
}

#[test]
fn keys_overlay_and_status_bar_come_from_one_table() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    for tab in [Tab::Live, Tab::Reflexes, Tab::Map, Tab::Health, Tab::Settings] {
        s.tab = tab;
        s.modal = None;
        let bar = app_frame(&s, &c, 150, 40);
        let status = bar.lines().last().unwrap().to_string();
        s.key(KeyCode::Char('?'), NONE);
        let overlay = app_frame(&s, &c, 150, 40);
        for k in keys::GLOBAL.iter().chain(tab.keys()) {
            assert!(overlay.contains(k.about), "{tab:?}: the overlay lacks {:?}:\n{overlay}", k.about);
            if k.bar && status.contains(&format!("{}  {}", k.keys, k.label)) {
                continue;
            }
            assert!(!k.bar || status.len() > 140, "{tab:?}: the bar lacks {} {}: {status}", k.keys, k.label);
        }
        if tab == Tab::Reflexes {
            snap("app-keys-reflexes", &overlay);
        }
        s.key(KeyCode::Esc, NONE);
    }
}

#[test]
fn fail_open_viewer_has_a_title_and_keeps_the_file_name() {
    let d = tempfile::tempdir().unwrap();
    let deep = d.path().join("a-rather-long-directory-name/and-another-one-below-it/adage");
    let paths = Paths::at(&deep);
    std::fs::create_dir_all(&deep).unwrap();
    let line = json!({ "ts": T0, "harness": "claude", "event": "Stop", "class": "io", "error": "disk full" });
    std::fs::write(deep.join(crate::doctor::FAIL_OPEN_LOG), format!("{line}\n")).unwrap();
    let c = ctx(paths.clone(), false);
    let mut s = app_state();
    s.modal = Some(app::fail_open_viewer(&paths));
    let text = app_frame(&s, &c, 60, 20);
    assert!(text.contains(" Fail-open errors "), "{text}");
    let path_line = text.lines().find(|l| l.contains("fail-open")).unwrap();
    let file = crate::doctor::FAIL_OPEN_LOG.rsplit('/').next().unwrap();
    assert!(path_line.contains('…') && path_line.contains(&format!("{file} ")), "{text}");
    assert!(text.contains("disk full"), "{text}");
}

#[test]
fn keys_overlay_wraps_under_the_description_column() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    s.key(KeyCode::Char('?'), NONE);
    let (text, buf) = render(60, 60, |f| app::draw(f, &s, &c));
    let (x, y) = find(&buf, "fuzzy filter reflexes").expect("the / key's description");
    let next: Vec<String> = (0..buf.area.width).map(|x| buf[(x, y + 1)].symbol().to_string()).collect();
    let border = next.iter().position(|c| c == "│").expect("inside the popup");
    let starts = (border + 1..next.len()).find(|&i| next[i] != " ").map(|i| i as u16);
    assert_eq!(starts, Some(x), "the continuation starts under the description, not the key:\n{text}");
}

#[test]
fn tabs_toasts_and_health_keys() {
    let mut s = app_state();
    for (k, tab) in [('1', Tab::Live), ('3', Tab::Map), ('4', Tab::Health), ('5', Tab::Settings), ('2', Tab::Reflexes)]
    {
        s.key(KeyCode::Char(k), NONE);
        assert_eq!(s.tab, tab);
    }
    s.key(KeyCode::Tab, NONE);
    assert_eq!(s.tab, Tab::Map);
    s.key(KeyCode::BackTab, NONE);
    assert_eq!(s.tab, Tab::Reflexes);
    // Esc on the Live tab does not quit; q does.
    s.key(KeyCode::Char('1'), NONE);
    assert!(s.key(KeyCode::Esc, NONE).is_none());
    // Toasts age out.
    s.toast("read-linked-files: strengthen recorded", true);
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    assert!(app_frame(&s, &c, 100, 30).contains("✓ read-linked-files: strengthen recorded"));
    for _ in 0..app::TOAST_FRAMES {
        s.tick();
    }
    assert!(s.toasts.is_empty());
    // A toast covers nothing: the newest sits on the rule above the status bar, older ones take
    // rows of their own.
    for tab in ['1', '2', '4', '5'] {
        s.key(KeyCode::Char(tab), NONE);
        let before: Vec<String> = app_frame(&s, &c, 60, 20).lines().map(String::from).collect();
        s.toast("read-linked-files: strengthen recorded", true);
        let after: Vec<String> = app_frame(&s, &c, 60, 20).lines().map(String::from).collect();
        assert_eq!(before[..18], after[..18], "the tab is untouched:\n{}", after.join("\n"));
        assert!(after[18].contains("✓ read-linked-files: strengthen recorded"), "{}", after.join("\n"));
        s.toast("learn.auto = false", true);
        let two = app_frame(&s, &c, 60, 20);
        let rows: Vec<&str> = two.lines().collect();
        assert!(rows[17].contains("strengthen recorded") && rows[18].contains("learn.auto = false"), "{two}");
        s.toasts.clear();
    }
    s.key(KeyCode::Char('2'), NONE);
    // Health: enter on a fixable check starts its fix; install asks for a scope first; c conditions.
    s.key(KeyCode::Char('4'), NONE);
    s.check_sel = 4;
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::Scope { harness }) if harness == "codex"));
    assert!(matches!(s.key(KeyCode::Char('u'), NONE), Some(Cmd::PlanInstall { scope: "user", .. })));
    assert!(matches!(s.key(KeyCode::Char('c'), NONE), Some(Cmd::PreviewLearning)));
    s.pending_job_action = Some(app::Action::LearnNow);
    s.complete_job("Preview", Ok(super::conditioned::sample_sources(8).into()));
    assert!(matches!(&s.modal, Some(Modal::Conditioning(_))), "the sources card asks first");
    assert!(matches!(s.key(KeyCode::Char('y'), NONE), Some(Cmd::Do(app::Action::LearnNow))));
    // Settings: a boolean toggles; thresholds switch with Space; a number opens the editor, whose
    // value `adage_store::config::set` validates when it is saved.
    // learn.auto asks first, in the words Health's fix uses.
    s.key(KeyCode::Char('5'), NONE);
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    let asked = |s: &AppState| match &s.modal {
        Some(Modal::Confirm { title, lines, .. }) => {
            format!("{title}\n{}", lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n"))
        }
        _ => panic!("expected a confirmation"),
    };
    let off = asked(&s);
    assert!(off.starts_with("Turn auto-condition off") && off.contains("adage config set learn.auto false"), "{off}");
    assert!(matches!(
        s.key(KeyCode::Char('y'), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "learn.auto", ref value })) if value == "false"
    ));
    s.settings[0].value = "false".into();
    s.key(KeyCode::Char(' '), NONE);
    let on = asked(&s);
    s.modal = None;
    s.start_fix(Fix::EnableAutoLearn);
    assert_eq!(on, asked(&s), "the Settings toggle and the Health fix ask the same");
    assert!(on.starts_with("Turn auto-condition on"), "{on}");
    s.modal = None;
    s.key(KeyCode::Down, NONE);
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::Edit { key: "learn.min_interval_secs", .. })));
    s.key(KeyCode::Backspace, NONE);
    s.key(KeyCode::Char('9'), NONE);
    assert!(matches!(
        s.key(KeyCode::Enter, NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "learn.min_interval_secs", ref value })) if value == "129"
    ));
    s.key(KeyCode::Down, NONE);
    assert!(matches!(
        s.key(KeyCode::Char(' '), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "learn.thresholds", ref value })) if value == "demo"
    ));
    assert!(matches!(s.key(KeyCode::Char('q'), NONE), Some(Cmd::Quit)));
}

#[test]
fn install_confirmation_shows_the_file_and_diff() {
    let p = crate::install::Plan {
        harness: "claude-code".into(),
        target: "/work/r/.claude/settings.local.json".into(),
        before: Some("{\n  \"model\": \"x\"\n}\n".into()),
        after: "{\n  \"hooks\": {},\n  \"model\": \"x\"\n}\n".into(),
    };
    let lines = app::install_confirm(&p, &Theme::with_color(false));
    let text: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    assert!(text[0].contains("/work/r/.claude/settings.local.json"));
    assert!(text.iter().any(|l| l == "+   \"hooks\": {},"), "{text:#?}");
    assert!(text.iter().any(|l| l == "    \"model\": \"x\""), "{text:#?}");
}

#[test]
fn wizard_walks_every_step() {
    use crate::detect::Detected;
    let detected = vec![
        Detected { harness: "claude-code", evidence: vec!["~/.claude exists".into()] },
        Detected { harness: "opencode", evidence: vec![] },
    ];
    let mut w = Wizard::new(detected, vec![], true);
    let titles: Vec<String> = w.steps.iter().map(|s| format!("{:?}", s.kind)).collect();
    assert_eq!(
        titles,
        ["Harnesses", "Binaries", "Hooks(\"claude-code\")", "Model", "Repos", "Learn", "AutoLearn", "Finish"]
    );
    let app =
        crate::repos::Found { key: "github.com/acme/app".into(), root: std::env::temp_dir(), sessions: 12, last_ms: 0 };
    w.here = Some(app.key.clone());
    w.set_repos(vec![app], &["github.com/acme/app".into()]);
    assert!(w.key(KeyCode::Enter, NONE).is_none(), "on to the binaries");
    assert_eq!(*w.current(), wizard::Kind::Binaries);
    assert!(
        matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Replan)),
        "all three beside adage: on to the hooks"
    );
    // The plugin first (here typed in Claude Code: no `claude` on PATH), then the settings file.
    assert!(w.choices()[0].starts_with("The Adage plugin (recommended)"), "{:?}", w.choices());
    assert!(w.key(KeyCode::Down, NONE).is_none(), "this repository's settings file, already planned");
    assert!(matches!(w.key(KeyCode::Down, NONE), Some(wizard::Cmd::Replan)));
    assert_eq!(w.scope, "user");
    assert!(w.key(KeyCode::Enter, NONE).is_none(), "nothing to install until planned");
    assert!(w.key(KeyCode::Char('n'), NONE).is_none());
    assert_eq!(*w.current(), wizard::Kind::Model);
    assert!(matches!(w.key(KeyCode::Char('y'), NONE), Some(wizard::Cmd::FetchModel)));
    w.key(KeyCode::Char('n'), NONE);
    // Which repositories: the current one is ticked; space unticks it, `a` ticks every one.
    assert_eq!(*w.current(), wizard::Kind::Repos);
    w.key(KeyCode::Char(' '), NONE);
    assert!(w.chosen().is_empty());
    w.key(KeyCode::Char('a'), NONE);
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::ChooseRepos)));
    let note = w.chosen_note();
    assert_eq!(note, "conditions on github.com/acme/app");
    w.advance(wizard::StepStatus::Done(note));
    assert!(matches!(w.key(KeyCode::Char('y'), NONE), Some(wizard::Cmd::Learn)));
    // The learn ran and showed its output; any answer moves on.
    w.steps[w.cur].status = wizard::StepStatus::Done("12 sessions read".into());
    w.output = vec!["adage learn: 12 sessions read".into()];
    w.key(KeyCode::Enter, NONE);
    assert!(matches!(w.key(KeyCode::Char('y'), NONE), Some(wizard::Cmd::AutoLearn)));
    assert!(matches!(w.advance(wizard::StepStatus::Done("learn.auto = true".into())), Some(wizard::Cmd::Finish)));
    w.health = Some(health());
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let (text, _) = render(100, 26, |f| wizard::draw(f, &w, &c));
    snap("wizard-summary", &text);
    let report = w.report().join("\n");
    assert!(
        report.contains("◇  Hooks · claude-code · skipped")
            && report.contains("◇  Repositories to condition on · conditions on github.com/acme/app")
            && report.contains("◇  Condition from past traces · 12 sessions read"),
        "{report}"
    );
    assert!(report.contains("Later: `adage update` updates Adage, its hooks and the plugin together"), "{report}");
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Quit)));
}

/// T-O4-05: the Claude Code step offers the Adage plugin first when it is not installed (installed
/// by adage with `claude plugin`, or typed in Claude Code without `claude` on PATH), the settings
/// file second; with the plugin installed, no second registration, and a settings file beside it
/// is offered for removal; the closing screen has at most two `next` lines.
#[test]
fn wizard_offers_the_plugin_first() {
    use crate::detect::{Detected, Registration};
    let detected = vec![Detected { harness: "claude-code", evidence: vec!["`claude` on PATH".into()] }];
    let to_hooks = |w: &mut Wizard| {
        while !matches!(w.current(), wizard::Kind::Hooks(_)) {
            w.advance(wizard::StepStatus::Done(String::new()));
        }
    };
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    // Not installed, `claude` on PATH: adage installs it.
    let mut w = Wizard::new(detected.clone(), vec![], true);
    w.claude_on_path = true;
    w.plugin_root = "~/.local/share/adage/plugin".into();
    to_hooks(&mut w);
    assert_eq!(
        w.choices(),
        [
            "The Adage plugin (recommended) · adage installs it with claude plugin",
            "The settings file · this repository",
            "The settings file · every repository you open with claude-code",
            "Skip claude-code",
        ]
    );
    let (text, _) = render(100, 18, |f| wizard::draw(f, &w, &c));
    snap("wizard-plugin-route", &text);
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Connect)));
    // No `claude` on PATH: the two commands to type, and nothing written.
    let mut w = Wizard::new(detected.clone(), vec![], true);
    to_hooks(&mut w);
    let (text, _) = render(100, 18, |f| wizard::draw(f, &w, &c));
    assert!(
        text.contains("/plugin marketplace add stevenvaughanbooth/adage")
            && text.contains("/plugin install adage@adage"),
        "{text}"
    );
    assert!(w.key(KeyCode::Enter, NONE).is_none());
    assert_eq!(
        w.steps[2].status,
        wizard::StepStatus::Done(
            "in Claude Code: /plugin marketplace add stevenvaughanbooth/adage, then /plugin install adage@adage".into()
        )
    );
    // Installed: kept, never a second registration.
    let plugin = Registration {
        harness: "claude-code",
        location: "/h/.claude/plugins/data/adage-adage-local".into(),
        binary: None,
        kind: "plugin-data",
        events: vec![],
    };
    let mut w = Wizard::new(detected.clone(), vec![plugin.clone()], true);
    w.claude_on_path = true;
    to_hooks(&mut w);
    assert_eq!(w.choices(), ["Keep it · the Adage plugin is installed", "Skip claude-code"]);
    assert!(w.key(KeyCode::Enter, NONE).is_none());
    assert_eq!(
        w.steps[2].status,
        wizard::StepStatus::Done("already registered by the Adage plugin · nothing written".into())
    );
    // Installed, and a settings file registers the hooks too: taking it out comes first.
    let settings = Registration {
        harness: "claude-code",
        location: "/h/.claude/settings.json".into(),
        binary: Some("/opt/adage".into()),
        kind: "settings",
        events: vec![],
    };
    let mut w = Wizard::new(detected, vec![plugin, settings], true);
    to_hooks(&mut w);
    w.removal = Some(Ok(crate::install::Plan {
        harness: "claude-code".into(),
        target: "/h/.claude/settings.json".into(),
        before: Some("{\n  \"hooks\": {},\n  \"model\": \"x\"\n}\n".into()),
        after: "{\n  \"model\": \"x\"\n}\n".into(),
    }));
    assert_eq!(w.choices()[0], "Take Adage's hooks out of /h/.claude/settings.json · the plugin runs each one already");
    let (text, _) = render(100, 18, |f| wizard::draw(f, &w, &c));
    assert!(text.contains("-   \"hooks\": {},") && text.contains("Change /h/.claude/settings.json"), "{text}");
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Remove(_))));
    // The closing screen: two `next` lines, and how to update later.
    let t = Theme::with_color(false);
    w.health = Some(health());
    let card: Vec<String> = wizard::finish_card(&w, &t, true, 100).iter().map(|l| l.to_string()).collect();
    let next: Vec<&String> = card.iter().skip_while(|l| !l.starts_with("   next")).collect();
    assert!(next[0].starts_with("   next  adage init") && next[1].trim_start().starts_with("claude "), "{card:#?}");
    assert!(next.len() == 3 && next[2].contains("Later: `adage update`"), "two next lines, then the note: {card:#?}");
}

/// docs/design/everyday-ux.md §5.5: the repositories Claude Code worked in, the current one first
/// and ticked; a long list shows a window that follows the highlight.
#[test]
fn wizard_asks_which_repositories_and_a_long_list_scrolls() {
    let day = 86_400_000;
    let now = 400 * day;
    let rows: Vec<crate::repos::Found> = (0..12)
        .map(|i| crate::repos::Found {
            key: format!("github.com/acme/repo{i:02}"),
            root: std::path::PathBuf::from(format!("/r/repo{i:02}")),
            sessions: 12 - i,
            last_ms: now - i as u64 * day,
        })
        .collect();
    let mut w = Wizard::new(vec![], vec![], true);
    w.now_ms = now;
    w.here = Some("github.com/acme/repo00".into());
    w.set_repos(rows, &["github.com/acme/repo00".into()]);
    while *w.current() != wizard::Kind::Repos {
        w.advance(wizard::StepStatus::Skipped);
    }
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let (text, _) = render(100, 18, |f| wizard::draw(f, &w, &c));
    assert!(text.contains("◆  Repositories to condition on"), "{text}");
    assert!(text.contains("› [x] github.com/acme/repo00  12 sessions · last today · this repository"), "{text}");
    assert!(text.contains("  [ ] github.com/acme/repo01  11 sessions · last yesterday"), "{text}");
    assert!(text.contains("0 above · 4 below") && !text.contains("repo08"), "a window of eight: {text}");
    assert!(text.contains("space tick · a all · n none"), "{text}");
    for _ in 0..9 {
        w.key(KeyCode::Down, NONE);
    }
    w.key(KeyCode::Char(' '), NONE);
    let (text, _) = render(100, 18, |f| wizard::draw(f, &w, &c));
    assert!(text.contains("› [x] github.com/acme/repo09  3 sessions · last 9 days ago"), "{text}");
    assert!(!text.contains("repo00") && text.contains("below"), "the window follows: {text}");
    assert_eq!(w.chosen_note(), "conditions on 2: github.com/acme/repo00, github.com/acme/repo09");
    w.key(KeyCode::Char('n'), NONE);
    assert_eq!(w.chosen_note(), "conditions on none · sessions are recorded, not read");
    w.key(KeyCode::Char('a'), NONE);
    assert!(w.chosen_note().starts_with("conditions on 12: github.com/acme/repo00, "), "{}", w.chosen_note());
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::ChooseRepos)));
}

#[test]
fn wizard_next_commands_keep_their_columns() {
    let claude = crate::detect::Registration {
        harness: "claude-code",
        location: "/h/.claude/settings.json".into(),
        binary: None,
        kind: "settings",
        events: vec![],
    };
    let mut w = Wizard::new(vec![], vec![claude], true);
    w.health = Some(health()); // codex's hooks are not registered: `adage init` again, first
    let t = Theme::with_color(false);
    for cols in [100, 64, 44] {
        let card: Vec<String> = wizard::finish_card(&w, &t, true, cols).iter().map(|l| l.to_string()).collect();
        let next: Vec<&String> =
            card.iter().skip_while(|l| !l.starts_with("   next")).take_while(|l| !l.contains("Later:")).collect();
        assert!(next[0].starts_with("   next  adage init"), "{card:#?}");
        if cols == 100 {
            assert!(next[0].contains("adage init      again, to connect codex"), "room for both columns: {card:#?}");
        }
        // Every description, and each continuation, starts in one column; a command too long for
        // the command column has a line to itself.
        let desc = next.iter().find_map(|l| l.find("start a session")).unwrap();
        for l in &next {
            assert!(l.chars().count() <= cols, "{l:?} is wider than {cols}");
            if ["adage init", "claude"].contains(&l[9..].trim_end()) {
                continue; // a command on its own line
            }
            let lead = &l[..desc];
            assert!(
                (lead.ends_with("  ") || lead.trim().is_empty()) && !l[desc..].starts_with(' '),
                "{cols} columns: {l:?} is out of line: {card:#?}"
            );
        }
    }
}

#[test]
fn wizard_choices_follow_the_arrows() {
    use crate::detect::Detected;
    let detected = vec![Detected { harness: "codex", evidence: vec!["~/.codex exists".into()] }];
    let mut w = Wizard::new(detected, vec![], true);
    w.key(KeyCode::Enter, NONE);
    w.key(KeyCode::Enter, NONE);
    assert_eq!(w.choices().len(), 3, "{:?}", w.choices());
    assert_eq!(w.scope, "project");
    // Down to "for this user" replans; down again to the skip does not.
    assert!(matches!(w.key(KeyCode::Down, NONE), Some(wizard::Cmd::Replan)));
    assert_eq!(w.scope, "user");
    assert!(w.key(KeyCode::Down, NONE).is_none());
    assert!(w.key(KeyCode::Down, NONE).is_none(), "the last choice stays selected");
    // Enter on the skip skips.
    w.key(KeyCode::Enter, NONE);
    assert_eq!(w.steps[2].status, wizard::StepStatus::Skipped);
    assert_eq!(*w.current(), wizard::Kind::Model);
    assert_eq!(w.choice, 0, "each step starts on its first choice");
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let (text, _) = render(80, 14, |f| wizard::draw(f, &w, &c));
    assert!(text.contains("◆  Embedding model"), "{text}");
    assert!(text.contains("› Fetch it"), "{text}");
    assert!(text.contains("└  ↑↓ choose"), "{text}");
}

/// QA first-run #2: hooks registered at user scope already; the hooks step defaults to keeping
/// them and offers no second registration (which would run every hook twice).
#[test]
fn wizard_keeps_an_existing_registration_and_never_adds_a_second() {
    use crate::detect::{Detected, Registration};
    let detected = vec![Detected { harness: "codex", evidence: vec!["~/.codex exists".into()] }];
    let regs = vec![Registration {
        harness: "codex",
        location: "/nowhere/a/.codex/hooks.json".into(),
        binary: Some("/opt/adage".into()),
        kind: "settings",
        events: Vec::new(),
    }];
    let mut w = Wizard::new(detected, regs, true);
    w.key(KeyCode::Enter, NONE);
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Replan)));
    let choices = w.choices();
    assert!(choices[0].starts_with("Keep it · already registered"), "{choices:?}");
    assert!(!choices.iter().any(|c| c.starts_with("For this")), "no second registration offered: {choices:?}");
    assert!(w.key(KeyCode::Tab, NONE).is_none(), "no switching to another scope");
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let (text, _) = render(100, 18, |f| wizard::draw(f, &w, &c));
    assert!(text.contains("Already registered in /nowhere/a/.codex/hooks.json."), "{text}");
    assert!(text.contains("Nothing to write: a second registration would run every hook twice"), "{text}");
    // Enter keeps it: nothing is installed, and the step says so.
    assert!(!matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Install(_))));
    assert_eq!(
        w.steps[2].status,
        wizard::StepStatus::Done("already registered in /nowhere/a/.codex/hooks.json · nothing written".into())
    );
    assert_eq!(*w.current(), wizard::Kind::Model);
}

#[test]
fn clock_times_use_the_offset() {
    assert_eq!(view::clock(T0, 0, true), "12:04:00");
    assert_eq!(view::clock(T0, -7 * 3600, false), "05:04");
    assert_eq!(view::clock(T0, 10 * 3600 + 1800, true), "22:34:00");
}

#[test]
fn loader_delivers_health_at_once_and_reflexes_on_request() {
    use super::jobs::{HealthCadence, Loaded, Loader, Want};
    let home = super::demo::setup().unwrap();
    let loader = Loader::spawn(home.paths.clone(), Some(home.repo.clone()), HealthCadence::Terminal);
    let wait = |want: &dyn Fn(&Loaded) -> bool| {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::time::Instant::now() < deadline {
            if let Ok(m) = loader.rx.recv_timeout(std::time::Duration::from_millis(100)) {
                if want(&m) {
                    return Some(m);
                }
            }
        }
        None
    };
    match wait(&|m| matches!(m, Loaded::Health(_))) {
        Some(Loaded::Health(h)) => {
            assert_eq!(h.repo_key.as_deref(), Some("harbor-dispatch"));
            let index = h.checks.iter().find(|c| c.check == "index").unwrap();
            assert_eq!(index.status, Status::Pass, "the demo's index is intact: {}", index.detail);
        }
        _ => panic!("no health"),
    }
    loader.want(Want::Routes);
    match wait(&|m| matches!(m, Loaded::Routes(_))) {
        Some(Loaded::Routes(Ok(r))) => {
            assert_eq!(r.rows.len(), 4);
            let lookup = r.rows.iter().find(|r| r.id == "lookup-incident").unwrap();
            assert_eq!(
                (lookup.trigger_kind.as_str(), lookup.step_kind.as_str(), lookup.instinct),
                ("prompt", "run", false)
            );
            assert_eq!(lookup.lifecycle(), "firm");
        }
        Some(Loaded::Routes(Err(e))) => panic!("{e}"),
        _ => panic!("no reflexes"),
    }
}

#[test]
fn narrow_screens_keep_indents_answers_and_the_current_tab() {
    use ratatui::text::{Line, Span};
    // Continuations stay under the text they continue; a word wider than the room breaks.
    let wrapped = view::wrap_lines(vec![Line::from(vec![Span::raw("   alpha beta "), Span::raw("gamma delta")])], 13);
    let text: Vec<String> =
        wrapped.iter().map(|l| l.spans.iter().map(|s| s.content.as_ref()).collect::<String>()).collect();
    assert_eq!(text, ["   alpha beta", "   gamma", "   delta"]);
    let long = view::wrap_lines(vec![Line::raw("  abcdefghij")], 8);
    assert!(long.iter().all(|l| l.width() <= 8), "{long:?}");

    // A confirmation whose explanation wraps still shows the keys that answer it.
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.confirm(
        "Extinguish",
        vec![
            Line::raw(
                "Extinguish: retire the reflex; conditioning keeps it extinguished unless it strengthens materially.",
            ),
            Line::raw(""),
            Line::raw("y confirm · n cancel"),
        ],
        app::Action::LearnNow,
    );
    assert!(app_frame(&s, &c, 60, 20).contains("y confirm · n cancel"));

    // The keys overlay scrolls when it is taller than the screen, and says so.
    s.modal = None;
    s.tab = Tab::Reflexes;
    s.key(KeyCode::Char('?'), NONE);
    let before = app_frame(&s, &c, 60, 16);
    assert!(before.contains("more · j/k scroll"), "{before}");
    s.key(KeyCode::Char('j'), NONE);
    assert!(matches!(s.modal, Some(Modal::Help { scroll: 1 })));
    s.key(KeyCode::Esc, NONE);
    assert!(s.modal.is_none());

    // Narrow: the current tab is named, the others keep their numbers.
    s.tab = Tab::Settings;
    let bar = app_frame(&s, &c, 44, 12).lines().next().unwrap_or_default().to_string();
    assert!(bar.contains("5 Settings") && bar.contains(" 1 2"), "{bar}");

    // Health lamps differ in shape, not only colour; enter on a check with no fix answers.
    let t = Theme::with_color(true);
    let marks: Vec<&str> = [Lamp::Ok, Lamp::Warn, Lamp::Fail, Lamp::Off].iter().map(|l| t.lamp_mark(*l)).collect();
    assert_eq!(marks, ["✓", "!", "✗", "○"]);
    s.health = Some(health());
    s.tab = Tab::Health;
    s.check_sel = s.health.as_ref().unwrap().fixes.iter().position(|f| f.is_none()).unwrap();
    s.toasts.clear();
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    assert!(matches!(s.modal, Some(Modal::Viewer { .. })), "read the whole check, not a disappearing toast");
}

// ---------------------------------------------------------------------------------------------
// Main's newer concepts: quiet reflexes, procedure cards, value-based evidence, skills, savings,
// rewrites and notices.

/// The fixture reflexes plus one of each newer kind: a quiet reflex (its uplift is below the
/// bar), a procedure card, a reflex whose strength rests on a value estimate (and is a finish
/// line), and one compiled from a skill.
fn feature_routes() -> Routes {
    use adage_tracks::card::{Card, CardCommand, CardFile};
    let mut r = routes();
    let t = "harbor-dispatch/learned";
    let mut quiet = route_row(
        t,
        "read-before-edit-9a1c",
        Strength::Prime,
        "nascent",
        (21, 23),
        ("after_command", "`bin/fetch-incident {item}` succeeds"),
        ("read_batch", "read ≤5 listed files at once"),
        vec!["approve", "extinguish", "pin"],
    );
    quiet.reasons = vec![("quiet".into(), "uplift +0.04 (lower bound -0.02); `approve` delivers it anyway".into())];
    quiet.more.quiet = true;
    quiet.more.quiet_line = Some("quiet: uplift +0.04 < 0.10 · approve to deliver".into());
    quiet.more.uplift = Some(adage_tracks::Uplift {
        kind: "read-batch".into(),
        p_follow: 0.95,
        p_self: 0.91,
        mean: 0.04,
        lower: -0.02,
        traces: 22,
        traces_took: 20,
        gated: true,
        ..Default::default()
    });
    let mut card = route_row(
        t,
        "card-carrier-rules",
        Strength::Prime,
        "forming",
        (0, 0),
        ("intent", "a prompt means: add a carrier rule"),
        ("card", "procedure card: 3 file(s), from 4 session(s)"),
        vec!["approve", "extinguish", "weaken", "pin"],
    );
    card.ceiling = "prime".into();
    card.more.card = Some(Box::new(Card {
        sessions: 4,
        passed: 3,
        files: vec![
            CardFile { path: "registry/carriers.csv".into(), sessions: 4 },
            CardFile { path: "src/dispatch/rules_<x>.ts".into(), sessions: 4 },
            CardFile { path: "test/rules.test.ts".into(), sessions: 3 },
        ],
        exemplars: vec![],
        commands: vec![
            CardCommand {
                command: "npm run codegen".into(),
                sessions: 4,
                runs: 4,
                failed: 0,
                error: String::new(),
                verified: Some(true),
            },
            CardCommand {
                command: "npm run check".into(),
                sessions: 3,
                runs: 5,
                failed: 1,
                error: String::new(),
                verified: None,
            },
        ],
        avoid: vec![CardCommand {
            command: "npm test".into(),
            sessions: 2,
            runs: 2,
            failed: 2,
            error: "HARBOR_ENV is not set".into(),
            verified: None,
        }],
        finish: Some("npm run check".into()),
        prompts: vec![],
    }));
    card.more.card_threshold = Some(0.811);
    let mut valued = route_row(
        t,
        "tests-after-rules-edit",
        Strength::Prime,
        "firm",
        (0, 7),
        ("after_edit", "an edit touches src/dispatch/rules_*.ts"),
        ("run", "npm run check -- {tests}"),
        vec!["approve", "extinguish", "strengthen", "pin"],
    );
    valued.more.value = Some("value: saves ~1.25 calls when followed (P(saves) 0.93, 90% lower bound 0.40); 7 triggers, 0 ran it — a desire path: no trace took the step yet, so its strength rests on the value estimate".into());
    valued.more.saves = Some(super::routes::Saves { calls: 1.25, p: 0.93, lower: 0.40 });
    valued.more.obligation = Some("finish line: after an edit to src/dispatch/rules_*.ts the agent is told to run `npm run check -- {tests}`; if it then stops without running it, Adage asks it once to run it before finishing (at most 2 such requests a session)".into());
    let mut skill = route_row(
        "@imported/@bound/harbor-skills",
        "release-notes-s2-changelog",
        Strength::Fire,
        "firm",
        (0, 0),
        ("after_edit", "an edit touches CHANGELOG.md"),
        ("run", "npm run changelog:check"),
        vec!["approve", "extinguish", "weaken", "pin"],
    );
    skill.imported = true;
    skill.more.skill = true;
    r.rows.extend([quiet, card, valued, skill]);
    r
}

fn feature_state() -> AppState {
    let mut s = app_state();
    s.routes = Some(Ok(feature_routes()));
    s.tab = Tab::Reflexes;
    s
}

#[test]
fn reflexes_tab_tells_quiet_from_install_steps() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = feature_state();
    let rows = s.route_rows().to_vec();
    let quiet = rows.iter().find(|r| r.id == "read-before-edit-9a1c").unwrap();
    assert!(quiet.quiet() && quiet.stage() == "quiet");
    let install = rows.iter().find(|r| r.id == "regen-openapi-client").unwrap();
    assert!(!install.quiet() && install.stage() == "nascent");
    tabs::reflexes::select(&mut s, "harbor-dispatch/learned", "read-before-edit-9a1c");
    let overview = app_frame(&s, &c, 150, 40);
    for want in [
        "▾ Waiting for review",
        "▾ Quiet · agents already do this; approve to deliver",
        "◌ bin/fetch-incident",
        "→ the gist of read-before-edit",
    ] {
        assert!(overview.contains(want), "lacks {want:?}:\n{overview}");
    }
    s.key(KeyCode::Enter, NONE);
    let text = app_frame(&s, &c, 150, 70);
    for want in [
        "◌ quiet: uplift +0.04 < 0.10 · approve to deliver",
        "when told",
        "0.95",
        "unaided",
        "0.91",
        "uplift +0.04 · lower bound -0.02",
    ] {
        assert!(text.contains(want), "lacks {want:?}:\n{text}");
    }
    assert!(!text.contains("waits for review: quiet"), "quiet is said once, in its own words:\n{text}");
    snap("app-reflexes-quiet", &text);
    // A quiet reflex waits to be approved: its details lead with that command.
    s.key(KeyCode::End, NONE);
    let text = app_frame(&s, &c, 150, 40);
    assert!(text.contains("a approve is the same as, from any shell:"), "{text}");
    assert!(text.contains("adage review approve harbor-dispatch/learned read-before-edit-9a1c"), "{text}");
    s.key(KeyCode::Esc, NONE);
    // The lifecycle filter has its own `quiet` choice; `nascent` then means install steps.
    s.reflexes.states = tabs::reflexes::STATES.iter().position(|x| *x == "quiet").unwrap();
    let shown: Vec<String> = tabs::reflexes::tree(&s)
        .iter()
        .filter_map(|n| match n {
            tabs::reflexes::Node::Reflex(i) => Some(s.route_rows()[*i].id.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(shown, ["read-before-edit-9a1c"]);
    s.reflexes.states = tabs::reflexes::STATES.iter().position(|x| *x == "nascent").unwrap();
    let text = app_frame(&s, &c, 150, 40);
    assert!(text.contains("regen-openapi-client") && !text.contains("read-before-edit-9a1c"), "{text}");
    // The group says what quiet means.
    s.reflexes.states = 0;
    s.reflexes.sel = Some("group:quiet".into());
    let text = app_frame(&s, &c, 150, 40);
    assert!(text.contains("They are not install") && text.contains("1 reflex   PRIME 1"), "{text}");
    // The watch pane's review line counts them apart.
    let mut live = Live::default();
    (live.waiting, live.quiet, live.needs, live.all_repos) = (Some(3), 1, 1, true);
    let (text, _) = render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&health()), &c, true));
    assert!(text.contains("1 needs you · 2 to look over · adage review"), "{text}");
}

#[test]
fn reflexes_tab_shows_a_procedure_card() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = feature_state();
    tabs::reflexes::select(&mut s, "harbor-dispatch/learned", "card-carrier-rules");
    s.key(KeyCode::Enter, NONE);
    let text = app_frame(&s, &c, 150, 70);
    for want in [
        "▤ The gist of card-carrier-rules",
        "procedure card",
        "Procedure card ─",
        "Files            1 registry/carriers.csv",
        "4/4 traces",
        "Commands         ✓ npm run codegen",
        "verified on a clean checkout",
        "○ npm run check",
        "not replayed",
        "Avoid            ✗ npm test",
        "HARBOR_ENV is not set",
        "Finish with      npm run check",
        "Gets it          a prompt at similarity ≥ 0.81, once",
        "0.71–0.81: once one of these files is edited",
        "Limits           prime at most, never strengthened",
        "                 first-hand only, never shared",
        "3 of 4 traces passed",
    ] {
        assert!(text.contains(want), "lacks {want:?}:\n{text}");
    }
    snap("app-reflexes-card", &text);
    // ASCII: the card's glyph falls back.
    let mut a = ctx(Paths::at(d.path()), false);
    a.theme = a.theme.ascii();
    let text = app_frame(&s, &a, 150, 70);
    assert!(text.contains("[=] The gist of card-carrier-rules") && !text.contains('▤'), "{text}");
}

#[test]
fn reflexes_tab_shows_value_evidence_obligations_and_skills() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = feature_state();
    tabs::reflexes::select(&mut s, "harbor-dispatch/learned", "tests-after-rules-edit");
    let overview = app_frame(&s, &c, 150, 40);
    assert!(overview.contains("by value"), "{overview}");
    s.key(KeyCode::Enter, NONE);
    let text = app_frame(&s, &c, 150, 70);
    for want in [
        "◠ The gist of tests-after-rules-edit",
        "Saves            ~1.25 calls when followed",
        "Confidence",
        "──  0.93 · lower bound 0.40",
        "0 of 7 triggers took it · a desire path",
        "Finish line      adage asks for it once if the agent stops without",
    ] {
        assert!(text.contains(want), "lacks {want:?}:\n{text}");
    }
    // A desire path's follow count is context, never its meter or its row's evidence.
    assert!(!overview.contains("0/7") && !overview.contains("0 of 7 traces"), "{overview}");
    snap("app-reflexes-value", &text);
    s.key(KeyCode::Esc, NONE);
    tabs::reflexes::select(&mut s, "@imported/@bound/harbor-skills", "release-notes-s2-changelog");
    let text = app_frame(&s, &c, 150, 40);
    assert!(text.contains("◈ The gist of release-notes-s2-changelog") && text.contains("from a skill"), "{text}");
}

/// The reflex the Reflexes card was redrawn for, as Adage's own repository has it: a learned
/// `cargo build` after edits (an id that says nothing, a track directory that is a slug, an uplift,
/// live counts, a wrapped advisory), beside an instinct whose gist is advice.
fn card_routes() -> Routes {
    let mut r = scoped_routes();
    let learned = "github-com-stevenvaughanbooth-adage/learned";
    let mut build = route_row(
        learned,
        "run-137805",
        Strength::Fire,
        "firm",
        (22, 35),
        ("after_edit", "an edit touches crates/adage-cli/src/*.rs"),
        ("run", "cargo build -p adage-cli"),
        vec!["approve", "extinguish", "weaken", "pin"],
    );
    (build.repo, build.version, build.lower_bound) = (ADAGE.into(), "17.0.12".into(), 0.46);
    build.gist = "after editing crates/adage-cli/src/*.rs, run cargo build -p adage-cli".into();
    build.live = LiveCounts { emitted: 4, followed: 3, ignored: 1, met: 2, ..Default::default() };
    build.more.uplift = Some(adage_tracks::Uplift {
        kind: "run-after-edit".into(),
        p_follow: 0.88,
        p_self: 0.60,
        mean: 0.28,
        lower: 0.13,
        traces: 35,
        traces_took: 22,
        ..Default::default()
    });
    build.more.obligation = Some("finish line: after an edit to crates/adage-cli/src/*.rs the agent is told to run `cargo build -p adage-cli`; if it then stops without running it, Adage asks it once to run it before finishing (at most 2 such requests a session)".into());
    build.directive = "<adage-reflex track=\"github-com-stevenvaughanbooth-adage/learned@17.0.12\" route=\"run-137805\" strength=\"fire\">\nIn this repository, changes to files matching crates/adage-cli/src/*.rs must be followed by running:\n  cargo build -p adage-cli\nAgents in this repository run it after such changes. Ignore this if the user is asking for something else.\nLearned by Adage from this repository's earlier agent sessions; not requested by the user.\n</adage-reflex>".into();
    let mut fmt = route_row(
        learned,
        "run-5f2a10",
        Strength::Fire,
        "firm",
        (22, 39),
        ("after_edit", "an edit touches crates/**/*.rs"),
        ("run", "cargo fmt --all"),
        vec!["approve", "extinguish", "weaken", "pin"],
    );
    (fmt.repo, fmt.version) = (ADAGE.into(), "17.0.12".into());
    fmt.gist = "after editing crates/**/*.rs, run cargo fmt --all".into();
    let mut lock = route_row(
        "github-com-stevenvaughanbooth-adage/instincts",
        "lockfile-cargo",
        Strength::Prime,
        "forming",
        (0, 0),
        ("after_edit", "an edit touches Cargo.lock"),
        ("advise", "Cargo.lock is updated by cargo"),
        vec!["approve", "extinguish", "pin"],
    );
    (lock.repo, lock.version, lock.instinct) = (ADAGE.into(), "3.0.0".into(), true);
    lock.gist = "Cargo.lock: let cargo update it; don't edit it by hand".into();
    r.rows.retain(|x| x.repo != ADAGE);
    r.rows.extend([fmt, build, lock]);
    r
}

/// T-UI: the Reflexes card reads in sections, plain words first: the gist as its title, chips for
/// the strength and what it may do, the arc, the evidence as meters, what the agent sees without
/// the wrapper; the id, the track's directory and the wrapper only when asked for (`enter`, `r`).
#[test]
fn reflexes_card_reads_in_sections() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.here = Some(ADAGE.into());
    s.routes = Some(Ok(card_routes()));
    s.tab = Tab::Reflexes;
    tabs::reflexes::select(&mut s, "github-com-stevenvaughanbooth-adage/learned", "run-137805");
    let wide = app_frame(&s, &c, 140, 40);
    snap("app-reflexes-card-140x40", &wide);
    for want in [
        "▾ Learned here · v17.0.12",
        "▾ Instincts · v3.0.0",
        "edit crates/**/*.rs → cargo fmt --all",
        "edit …/src/*.rs     → cargo build -p adage-cli",
        "▰▰▰▰▱▱ 22/35",
        "edit Cargo.lock     → let cargo update it…  forming      built in",
        "◠ After editing crates/adage-cli/src/*.rs, run\n",
        "   cargo build -p adage-cli\n",
        " FIRE ",
        " Suggests ",
        "╭─ WHEN ─",
        "╭─ DO ─",
        "│─── ◠ ──▶│ cargo build -p adage-cli │",
        "22 of 35 traces",
        "Review evidence and decisions",
    ] {
        assert!(wide.contains(want), "the card lacks {want:?}:\n{wide}");
    }
    for later in ["run-137805", "github-com-", "<adage-reflex", "@17.0.12"] {
        assert!(!wide.contains(later), "{later:?} on the first read:\n{wide}");
    }
    // Under 120 columns the card stacks under the list; short of rows, the arc is one line.
    snap("app-reflexes-card-100x40", &app_frame(&s, &c, 100, 40));
    let narrow = app_frame(&s, &c, 80, 30);
    snap("app-reflexes-card-80x30", &narrow);
    assert!(narrow.contains("crates/adage-cli/src/*.rs") && narrow.contains("│─── ◠ ──▶│"), "{narrow}");
    assert!(narrow.contains("cargo build -p adage-cli"), "{narrow}");
    // `r` shows the advisory as delivered; enter opens the details.
    s.key(KeyCode::Char('r'), NONE);
    s.key(KeyCode::Enter, NONE);
    let opened = app_frame(&s, &c, 140, 70);
    snap("app-reflexes-card-details-140x70", &opened);
    for want in [
        "▎ <adage-reflex",
        "route=\"run-137805\"",
        "strength=\"fire\">",
        "▎ </adage-reflex>",
        "▾ Details",
        "reflex       run-137805",
        "track        github-com-stevenvaughanbooth-adage/learned",
        "version      17.0.12",
        "w weaken is the same as, from any shell:",
        "  adage review weaken github-com-stevenvaughanbooth-adage/learned",
    ] {
        assert!(opened.contains(want), "the opened card lacks {want:?}:\n{opened}");
    }
    // Both stay as chosen from reflex to reflex; the keys turn them back.
    s.key(KeyCode::Esc, NONE);
    s.key(KeyCode::Up, NONE);
    assert!(s.reflexes.raw && s.reflexes.details);
    s.key(KeyCode::Char('r'), NONE);
    s.key(KeyCode::Char('v'), NONE);
    assert!(!s.reflexes.raw && !s.reflexes.details);
    // Under ASCII, nothing but ASCII.
    let a = Ctx { theme: Theme::with_color(false).ascii(), ..ctx(Paths::at(d.path()), false) };
    for (w, h) in [(140, 40), (80, 30), (44, 12)] {
        let text = app_frame(&s, &a, w, h);
        let bad: String = text.chars().filter(|ch| !ch.is_ascii() && *ch != '\u{a0}').collect();
        assert!(bad.is_empty(), "{w}x{h} draws {bad:?} under ADAGE_ASCII:\n{text}");
    }
}

fn savings_summary(saved: bool) -> crate::savings::Summary {
    let unit = |i: usize, control: bool, cost: f64| crate::savings::Unit {
        id: format!("s{i:03}"),
        control,
        propensity: if control { 0.1 } else { 0.9 },
        cost: Some(cost),
        turns: cost * 100.0,
        first_edit: None,
        asked_for_help: false,
    };
    let units: Vec<crate::savings::Unit> = if saved {
        (0..52)
            .map(|i| {
                unit(i, i >= 40, if i >= 40 { 0.06 + (i % 3) as f64 * 0.001 } else { 0.03 + (i % 5) as f64 * 0.001 })
            })
            .collect()
    } else {
        (0..29).map(|i| unit(i, i < 4, 0.1)).collect()
    };
    let mut s = crate::savings::summarise("harbor-dispatch", &units, Default::default(), 0);
    s.pricing_as_of = "2026-06-24".into();
    // Measuring on (it is off by default): these screens are the measured ones.
    s.held_out = 0.1;
    s
}

#[test]
fn savings_rewrites_and_notices_on_the_live_and_health_screens() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let c = ctx(paths.clone(), false);
    let mut h = health();
    // Below the gate: what is being counted, never dollars.
    h.savings = Some(savings_summary(false));
    let live = live_from(&paths);
    let (text, _) = render(100, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(text.contains(" savings measuring: 4 control / 25 treated traces"), "{text}");
    assert!(!text.contains('$'), "{text}");
    snap("watch-100x30-savings-measuring", &text);
    // Past it: the figure with its interval; in colour the verdict's mark, a ✓ in green.
    h.savings = Some(savings_summary(true));
    let colour = ctx(paths.clone(), true);
    let (text, buf) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &colour, true));
    assert!(text.contains("✓ savings saved ~$0.029 per trace, API-list equivalent (95% CI $"), "{text}");
    let (x, y) = find(&buf, "✓ savings").unwrap();
    assert_eq!(buf[(x, y)].fg, Color::Green);
    snap("watch-110x30-savings-saved-colour", &styled(&buf));
    // Without colour the verdict is the words alone.
    let (text, _) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(text.contains(" savings saved ~$0.029") && !text.contains("✓ savings"), "{text}");

    // The Health tab: the estimate, today's notice, and the rewrites check with its switch.
    let mut s = app_state();
    h.notice = Some("adage: 2 new reflexes active.".into());
    h.checks.push(Check {
        check: "rewrites".into(),
        label: "rewrites".into(),
        status: Status::Pass,
        detail: "python tests → .venv (on)".into(),
        fix: Some(crate::doctor::Fix::run("adage delegate rewrites off")),
    });
    h.fixes.push(None);
    s.health = Some(h);
    s.tab = Tab::Health;
    s.check_sel = 6;
    let text = app_frame(&s, &c, 150, 35);
    for want in [
        " savings saved ~$0.029 per trace",
        " » today adage: 2 new reflexes active.",
        "rewrites",
        "python tests → .venv (on)",
        "switch  adage delegate rewrites off",
    ] {
        assert!(text.contains(want), "lacks {want:?}:\n{text}");
    }
    snap("app-health-savings", &text);
}

#[test]
fn settings_group_into_sections_with_plain_labels_and_one_card() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Settings;
    let at = |s: &AppState, k: &str| s.settings.iter().position(|r| r.key == k).unwrap();
    let i = at(&s, "learn.min_interval_secs");
    (s.settings[i].value, s.settings[i].source) = ("600".into(), "file".into());
    let i = at(&s, "runtime.control");
    (s.settings[i].value, s.settings[i].source) = ("0.1".into(), "env ADAGE_CONTROL".into());
    // Plain labels and values in words; a mark only where a value differs or the environment
    // sets it; the dotted key only on the selected setting's card.
    let narrow = app_frame(&s, &c, 80, 24);
    assert!(narrow.contains("▌   Learn automatically") && narrow.contains("● on"), "{narrow}");
    assert!(narrow.contains("10 min  • changed") && narrow.contains("─ Learn automatically · learn.auto"), "{narrow}");
    assert!(
        !narrow.contains("(default)") && !narrow.contains("\"\"") && !narrow.contains("learn.thresholds"),
        "{narrow}"
    );
    snap("app-settings-80x24", &narrow);
    // j and k move in the order shown, across sections; [ and ] jump between sections.
    s.setting_sel = at(&s, "learn.card_threshold");
    let sel = |s: &AppState| s.settings[s.setting_sel].key;
    s.key(KeyCode::Char('j'), NONE);
    assert_eq!(sel(&s), "learn.claude_projects");
    s.key(KeyCode::Char('k'), NONE);
    assert_eq!(sel(&s), "learn.card_threshold");
    s.key(KeyCode::Char(']'), NONE);
    assert_eq!(sel(&s), "learn.claude_projects");
    s.key(KeyCode::Char(']'), NONE);
    assert_eq!(sel(&s), "instincts.enabled");
    s.key(KeyCode::Down, NONE);
    s.key(KeyCode::Char('['), NONE);
    assert_eq!(sel(&s), "instincts.enabled", "first back to the top of its own section");
    s.key(KeyCode::Char('['), NONE);
    assert_eq!(sel(&s), "learn.claude_projects");
    s.key(KeyCode::Char('G'), NONE);
    assert_eq!(sel(&s), "update.check");
    assert!(matches!(
        s.key(KeyCode::Char(' '), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "update.check", ref value })) if value == "off"
    ));
    s.key(KeyCode::Char('g'), NONE);
    assert_eq!(sel(&s), "learn.auto");
    // d puts a setting back to its default: a typed value asks first, showing both; a switch goes
    // straight back; one already at its default says so.
    s.setting_sel = at(&s, "learn.min_interval_secs");
    assert!(app_frame(&s, &c, 150, 35).contains(" d        back to the default, 2 min"));
    assert!(app_frame(&s, &c, 80, 30).contains(" d back to the default, 2 min"));
    assert!(s.key(KeyCode::Char('d'), NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::Confirm { title, lines, .. })
        if title == "Reset Pause between runs to its default"
            && lines.iter().any(|l| l.to_string().contains("10 min"))
            && lines.iter().any(|l| l.to_string().contains("2 min"))));
    assert!(matches!(
        s.key(KeyCode::Char('y'), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "learn.min_interval_secs", ref value })) if value == "120"
    ));
    s.modal = None;
    let i = at(&s, "ui.spinner");
    (s.settings[i].value, s.settings[i].source) = ("true".into(), "file".into());
    s.setting_sel = i;
    assert!(matches!(
        s.key(KeyCode::Char('d'), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "ui.spinner", ref value })) if value == "false"
    ));
    s.settings[i].value = "false".into();
    assert!(s.key(KeyCode::Char('d'), NONE).is_none() && s.modal.is_none());
    assert!(s.toasts.last().is_some_and(|t| t.text == "Spinner note is already the default"));
    s.toasts.clear();
    // A value the environment sets says so, on its row and on its card.
    s.setting_sel = at(&s, "runtime.control");
    let mid = app_frame(&s, &c, 100, 30);
    assert!(mid.contains("10% of sessions held out  env ADAGE_CONTROL"), "{mid}");
    assert!(mid.contains("ADAGE_CONTROL wins over config.toml"), "{mid}");
    assert!(mid.contains("1 setting is set by environment variables"), "{mid}");
    snap("app-settings-100x30", &mid);
    // Without Unicode, the toggles and marks fall back to ASCII.
    let ascii = Ctx { theme: Theme::with_color(false).ascii(), ..ctx(Paths::at(d.path()), false) };
    let text = app_frame(&s, &ascii, 80, 24);
    assert!(text.contains("* on") && text.contains("o off") && !text.contains('●'), "{text}");
}

#[test]
fn settings_show_rewrites_off_by_the_older_marker_and_notices() {
    let d = tempfile::tempdir().unwrap();
    let paths = Paths::at(d.path());
    let marker = adage_hook::rewrite_off(&paths);
    let mut s = AppState::new(paths.config());
    s.rewrite_marker = Some(marker.clone());
    s.load_settings(&adage_store::config::load(&paths));
    let row = |s: &AppState, k: &str| {
        let r = s.settings.iter().find(|r| r.key == k).unwrap();
        (r.value.clone(), r.source.clone())
    };
    assert_eq!(row(&s, "runtime.rewrite"), ("true".into(), "default".into()));
    assert_eq!(row(&s, "ui.notices"), ("daily".into(), "default".into()));
    std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
    std::fs::write(&marker, "").unwrap();
    s.load_settings(&adage_store::config::load(&paths));
    assert_eq!(row(&s, "runtime.rewrite"), ("false".into(), "exec/rewrite-off marker".into()));
    // Shown with the switch that turns them back on; space turns them on (and clears the marker).
    s.tab = Tab::Settings;
    s.config_path = "/home/me/.local/share/adage/config.toml".into();
    s.setting_sel = s.settings.iter().position(|r| r.key == "runtime.rewrite").unwrap();
    let c = ctx(paths.clone(), false);
    let text = app_frame(&s, &c, 150, 40);
    assert!(text.contains("off by marker") && text.contains("exec/rewrite-off marker, which wins"), "{text}");
    assert!(text.contains("↪ rewrites off · same as adage delegate rewrites on to switch"), "{text}");
    snap("app-settings-rewrite-marker", &text);
    assert!(matches!(
        s.key(KeyCode::Char(' '), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "runtime.rewrite", ref value })) if value == "true"
    ));
    // Notices toggle daily ⇄ off with space.
    s.setting_sel = s.settings.iter().position(|r| r.key == "ui.notices").unwrap();
    assert!(matches!(
        s.key(KeyCode::Char(' '), NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "ui.notices", ref value })) if value == "off"
    ));
}

#[test]
fn reflexes_escape_recovers_from_combined_filters() {
    let mut s = app_state();
    s.key(KeyCode::Char('2'), NONE);
    for ch in ['F', 'P', 'I', 'S'] {
        s.key(KeyCode::Char(ch), NONE);
    }
    assert!(tabs::reflexes::tree(&s).is_empty());
    s.key(KeyCode::Esc, NONE);
    assert_eq!(s.reflexes.hidden, [false; 3]);
    assert_eq!(s.reflexes.states, 0);
    assert!(!tabs::reflexes::tree(&s).is_empty());
    s.key(KeyCode::Char('F'), NONE);
    s.key(KeyCode::Char('/'), NONE);
    s.key(KeyCode::Char('z'), NONE);
    s.key(KeyCode::Esc, NONE);
    assert!(!s.reflexes.filtering);
    assert!(s.reflexes.filter.is_empty());
    assert_eq!(s.reflexes.hidden, [false; 3]);
}

#[test]
fn setup_palette_reaches_every_source_and_current_pack_workflow() {
    use super::{forms::Kind, tools::Tool};
    let mut s = app_state();
    assert_eq!(AppState::new("unused".into()).tab, Tab::Home);
    for (query, expected) in [
        ("connect codex", palette::Run::Connect("codex")),
        ("configure codex", palette::Run::Form(Kind::HistoryCodex)),
        ("history sources", palette::Run::Tool(Tool::HistorySources)),
        ("import opencode", palette::Run::Tool(Tool::ExportOpenCode)),
        ("condition now", palette::Run::Key(Tab::Health, 'c')),
        ("available packs", palette::Run::Tool(Tool::Marketplace)),
    ] {
        let found = palette::matches(&s, query);
        assert!(found.iter().any(|(e, _)| e.run == expected), "{query} must be discoverable");
    }
    s.run_entry(palette::Run::Connect("codex"));
    assert!(matches!(s.modal,Some(Modal::Scope{ref harness}) if harness=="codex"));
}

#[test]
fn source_form_preserves_spaces_and_requires_confirmation() {
    use super::{
        forms::{Form, Kind},
        tools::Tool,
    };
    let mut s = app_state();
    s.modal = Some(Modal::Form(Form::new(Kind::HistoryCodex)));
    for ch in "/tmp/history with $(literal)".chars() {
        assert!(s.key(KeyCode::Char(ch), NONE).is_none());
    }
    assert!(
        matches!(s.key(KeyCode::Enter,NONE),Some(Cmd::PrepareTool(Tool::SetHistorySource{harness,directory})) if harness=="codex" && directory==std::path::Path::new("/tmp/history with $(literal)"))
    );
    s.run_entry(palette::Run::Form(Kind::AddPack));
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    assert!(matches!(&s.modal,Some(Modal::Form(f)) if f.error.is_some()));
}

#[test]
fn conditioning_preflight_cannot_skip_to_mutation_or_survive_failure() {
    let mut s = app_state();
    assert!(matches!(s.start_fix(super::health::Fix::LearnNow), Some(Cmd::PreviewLearning)));
    assert!(s.modal.is_none());
    s.pending_job_action = Some(app::Action::LearnNow);
    s.complete_job("Preview", Err("History folder is unreadable".into()));
    assert!(s.pending_job_action.is_none());
    assert!(matches!(s.modal, Some(Modal::Message { error: true, .. })));
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    s.pending_job_action = Some(app::Action::LearnNow);
    s.complete_job("Preview", Ok(super::conditioned::sample_sources(12).into()));
    let d = tempfile::tempdir().unwrap();
    let text = app_frame(&s, &ctx(Paths::at(d.path()), false), 100, 30);
    assert!(text.contains("12 unsteered"), "{text}");
    assert!(s.key(KeyCode::Esc, NONE).is_none());
    assert!(s.modal.is_none(), "esc cancels: nothing runs");
}

#[test]
fn reviewed_revision_survives_background_reload_and_full_proposal_is_visible() {
    use adage_app::review::{ProposalPreview, ReviewPreview};
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    tabs::reflexes::select(&mut s, "harbor-dispatch/incidents", "lookup-incident");
    let rows = &mut s.routes.as_mut().unwrap().as_mut().unwrap().rows;
    rows[0].review_preview = Some(ReviewPreview {
        revision: "original".into(),
        current_directive: "old response".into(),
        proposal: Some(ProposalPreview {
            on: "v2".into(),
            reason: "evidence changed".into(),
            diff: vec![("step", "old".into(), "new".into())],
            directive: "exact proposed response".into(),
        }),
    });
    s.key(KeyCode::Char('a'), NONE);
    let Some(Modal::Confirm { lines, .. }) = &s.modal else { panic!("confirmation") };
    let text = lines.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n");
    assert!(
        text.contains("old response") && text.contains("exact proposed response") && text.contains("step: old → new")
    );
    s.routes.as_mut().unwrap().as_mut().unwrap().rows[0].review_preview.as_mut().unwrap().revision = "reloaded".into();
    assert!(
        matches!(s.key(KeyCode::Enter,NONE),Some(Cmd::Do(app::Action::Review{expected_revision,..})) if expected_revision=="original")
    );
}

#[test]
fn unreadable_review_fails_closed_and_manual_health_steps_do_not_disappear() {
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    tabs::reflexes::select(&mut s, "harbor-dispatch/incidents", "lookup-incident");
    s.routes.as_mut().unwrap().as_mut().unwrap().rows[0].review_preview = None;
    s.key(KeyCode::Char('a'), NONE);
    assert!(matches!(s.modal, Some(Modal::Message { error: true, .. })));
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    s.tab = Tab::Health;
    s.check_sel = 0;
    let h = s.health.as_mut().unwrap();
    h.fixes[0] = None;
    h.checks[0].detail = "Registered; waiting for first delivery".into();
    h.checks[0].fix = Some(crate::doctor::Fix::manual("Open /hooks inside Codex and accept Adage"));
    s.key(KeyCode::Enter, NONE);
    assert!(matches!(&s.modal,Some(Modal::Viewer{lines,..}) if lines.iter().any(|l|l.contains("/hooks"))));
}

#[test]
fn leaving_demo_uses_persistent_data_and_cannot_reopen_the_demo_root() {
    let d = tempfile::tempdir().unwrap();
    let demo = d.path().join("demo");
    let repo = d.path().join("real project");
    std::fs::create_dir_all(demo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let mut c = Ctx::at(Paths::at(d.path().join("temporary-data")), Some(demo.clone()));
    c.demo = true;
    assert!(app::repository_context(&c, &demo).is_err());
    let next = app::repository_context(&c, &repo).unwrap();
    assert!(!next.demo);
    assert_eq!(next.paths.root, Paths::from_env().root);
    assert_ne!(next.paths.root, c.paths.root);
    assert_eq!(next.root.unwrap(), repo.canonicalize().unwrap());
}

/// T-O6-05: Home says a newer Adage in its first rows, in plain ink, with the key that runs
/// `adage update` as a job (confirmed first); without a newer release Home is as before (the
/// `app-home` snapshot). The palette's `Update Adage` is the same tool.
#[test]
fn home_says_a_newer_release() {
    use super::tools::Tool;
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let version = env!("CARGO_PKG_VERSION");
    let latest = adage_store::release::Latest {
        checked_at: adage_core::time::now_ms(),
        have: version.into(),
        latest: Some(adage_store::release::Release { version: "99.0.0".into(), ..Default::default() }),
        failure: None,
    };
    latest.write(&paths).unwrap();
    let c = ctx(paths.clone(), false);
    let mut s = app_state();
    s.live = live_from(&paths);
    s.tab = Tab::Home;
    let text = app_frame(&s, &c, 150, 35);
    assert!(text.contains(&format!(" adage {version} · 99.0.0 is out · u update (adage update")), "{text}");
    snap("app-home-update", &text.replace(version, "{VERSION}"));
    // In colour the row is ink and the key the accent: never the warning colour.
    let colour = ctx(paths.clone(), true);
    let (_, buf) = render(150, 35, |f| app::draw(f, &s, &colour));
    let (x, y) = find(&buf, "99.0.0 is out").unwrap();
    assert_ne!(buf[(x, y)].fg, colour.theme.yellow().fg.unwrap_or_default(), "never amber");
    // `u` prepares the tool; nothing runs before the confirmation.
    assert!(matches!(s.key(KeyCode::Char('u'), NONE), Some(Cmd::PrepareTool(Tool::Update))));
    let entry =
        palette::matches(&s, "update adage").into_iter().map(|(e, _)| e).find(|e| e.title == "Update Adage").unwrap();
    assert_eq!(entry.note, "adage update");
    // `adage watch`'s header says the same pair while there is room, and drops it first.
    let live = live_from(&paths);
    let h = health();
    let (wide, _) = render(160, 20, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(wide.lines().next().unwrap().contains("99.0.0 is out · adage update"), "{wide}");
    let (narrow, _) = render(80, 20, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(!narrow.lines().next().unwrap().contains("is out"), "{narrow}");
}

// ---------------------------------------------------------------------------------------------
// Repository scope: Live, Reflexes and Map show one repository at a time.

const ADAGE: &str = "github.com/stevenvaughanbooth/adage";
const ACME: &str = "acme-billing@5ab83fcedeea";

/// Hook calls in three repositories: harbor-dispatch (opened here) with a firing between runs of
/// quiet tool batches, and one call each in two other repositories.
fn scoped_live() -> Live {
    let mut live = Live::default();
    let call = |k: u64, repo: &str, session: &str, decision: Value| super::tail::Record {
        session: session.into(),
        value: json!({ "ts": T0 + k * 1000, "event": "PostToolBatch", "kind": "batch", "arm": "treated",
            "repo": repo, "harness": "claude", "decision": decision }),
    };
    let quiet = |n: usize| json!({ "observed": vec!["exec ls"; n] });
    let a = "7c1e04d2-11aa-4a31-9f0e-00000000a11c";
    let b = "3f9b62e8-22bb-4c7d-8b21-000000000b0b";
    live.push_records(vec![
        call(0, "harbor-dispatch", a, quiet(2)),
        call(1, ADAGE, "019a-adage-session-0001", quiet(1)),
        call(2, "harbor-dispatch", a, quiet(3)),
        call(3, "harbor-dispatch", b, quiet(1)),
        call(4, "harbor-dispatch", a, quiet(4)),
        call(
            5,
            "harbor-dispatch",
            b,
            json!({ "directives": [directive("dead-end-npm-test", "inhibit", DEAD_END)], "observed": ["npm test"] }),
        ),
        call(6, "harbor-dispatch", b, quiet(2)),
        call(7, ACME, "019a-acme-session-0002", quiet(1)),
        call(8, "harbor-dispatch", a, quiet(1)),
    ]);
    live.waiting = Some(1);
    live
}

/// The fixture's reflexes, and two other repositories': a pack in acme-billing, and Adage's own
/// learned track.
fn scoped_routes() -> Routes {
    let mut r = routes();
    let mut pack = route_row(
        "acme-billing-5ab83fcedeea/work-items",
        "close-work-item",
        Strength::Prime,
        "firm",
        (5, 7),
        ("prompt", "a prompt names a work item"),
        ("run", "bin/close-item {item}"),
        vec!["approve", "extinguish", "pin"],
    );
    (pack.repo, pack.imported, pack.version) = (ACME.into(), true, "0.1.0".into());
    let mut learned = route_row(
        "github-com-stevenvaughanbooth-adage/learned",
        "cargo-test-after-edit",
        Strength::Fire,
        "firm",
        (30, 33),
        ("after_edit", "an edit touches crates/**/*.rs"),
        ("run", "cargo test -p adage-cli"),
        vec!["approve", "extinguish", "pin"],
    );
    (learned.repo, learned.version) = (ADAGE.into(), "17.0.4".into());
    r.rows.extend([pack, learned]);
    r
}

#[test]
fn repository_names_are_plain_words() {
    use super::scope::{friendly, short};
    assert_eq!(friendly(ADAGE), ADAGE, "an origin is already readable");
    assert_eq!(friendly(ACME), "acme-billing");
    assert_eq!(friendly("app@path-0123456789ab"), "app");
    assert_eq!(friendly("harbor-dispatch"), "harbor-dispatch", "a legacy key is its name");
    assert_eq!(friendly("git@example"), "git@example", "only a root-commit tail is dropped");
    assert_eq!(short(ADAGE), "adage");
    // Two repositories that read alike keep their keys apart in the switcher.
    let a = feed::from_record("s", &json!({ "ts": T0, "event": "Stop", "repo": "app@1a2b3c4d5e6f" }));
    let b = feed::from_record("s", &json!({ "ts": T0 + 1, "event": "Stop", "repo": "app@9f8e7d6c5b4a" }));
    let names: Vec<String> = super::scope::known(&[&a, &b], &[], &[], None, None).into_iter().map(|k| k.name).collect();
    assert_eq!(names, ["app (9f8e7d6c5b4a)", "app (1a2b3c4d5e6f)"], "most recently active first");
}

#[test]
fn live_shows_one_repository_and_folds_quiet_runs() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.here = Some("harbor-dispatch".into());
    s.live = scoped_live();
    s.tab = Tab::Live;
    let text = app_frame(&s, &c, 120, 30);
    snap("app-live-scoped", &text);
    assert!(text.lines().next().unwrap().contains("◆ harbor-dispatch ▾"), "the scope heads the screen:\n{text}");
    assert!(!text.contains("0001") && !text.contains("0002"), "other repositories' calls are not shown:\n{text}");
    // Each session is one section of the graph; a run of quiet steps folds into one line, and
    // what Adage said branches off its step.
    assert_eq!(text.matches("Claude Code  session").count(), 2, "{text}");
    assert!(text.contains("┊ 3 steps, nothing to say   Run ×9"), "{text}");
    assert!(text.contains("◆ adage · INHIBIT") && text.contains("npm run check"), "{text}");
    // Every repository: each session names its repository.
    s.live.all_repos = true;
    let all = app_frame(&s, &c, 120, 40);
    snap("app-live-all", &all);
    assert!(all.lines().next().unwrap().contains("◆ all repositories ▾"), "{all}");
    assert!(
        all.contains("adage · session")
            && all.contains("acme-billing · session")
            && all.contains("now · all repositories"),
        "{all}"
    );
    // The watch pane folds them the same way.
    s.live.all_repos = false;
    let (pane, _) = render(100, 30, |f| view::draw_live(f, f.area(), &s.live, Some(&health()), &c, true));
    assert!(pane.contains("3 steps, nothing to say"), "{pane}");
}

#[test]
fn reflexes_and_map_show_the_repository_in_scope() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.here = Some("harbor-dispatch".into());
    s.routes = Some(Ok(scoped_routes()));
    s.tab = Tab::Reflexes;
    let text = app_frame(&s, &c, 150, 35);
    snap("app-reflexes-scoped", &text);
    assert!(text.contains("▾ incidents · v0.3.0"), "{text}");
    for other in ["work-items", "close-work-item", "Learned here", "Learned in", "cargo test"] {
        assert!(!text.contains(other), "{other} is another repository's:\n{text}");
    }
    // Every repository: each track says whose it is, in plain words.
    s.set_scope(None);
    let all = app_frame(&s, &c, 150, 40);
    snap("app-reflexes-all", &all);
    assert!(all.contains("▾ From pack · work-items · acme-billing · v0.1.0"), "{all}");
    assert!(all.contains("▾ Learned in adage · v17.0.4"), "{all}");
    // Adage's own repository: its learned track is "Learned here"; the map shows only its reflex.
    s.set_scope(Some(ADAGE.into()));
    let text = app_frame(&s, &c, 150, 35);
    assert!(text.contains("▾ Learned here · v17.0.4") && !text.contains("incidents"), "{text}");
    let narrow = app_frame(&s, &c, 100, 30);
    assert!(narrow.lines().next().unwrap().contains("◆ adage ▾"), "short of room, the last name:\n{narrow}");
    s.reflexes.sel = Some("group:track:github-com-stevenvaughanbooth-adage/learned".into());
    let text = app_frame(&s, &c, 150, 35);
    assert!(text.contains("track  github-com-stevenvaughanbooth-adage/learned"), "the group names its track:\n{text}");
    s.tab = Tab::Map;
    let map = app_frame(&s, &c, 150, 35);
    assert!(map.contains("cargo-test-after-edit") && !map.contains("lookup-incident"), "{map}");
    // A palette reflex of another repository shows that repository.
    s.palette = Some(Palette { query: "close work item".into(), sel: 0 });
    assert!(!palette::matches(&s, "close work item").iter().any(|(e, _)| e.title == "close-work-item"));
    s.palette = None;
    tabs::reflexes::select(&mut s, "acme-billing-5ab83fcedeea/work-items", "close-work-item");
    assert_eq!(s.scope_key(), Some(ACME));
    assert_eq!(s.selected_route().unwrap().id, "close-work-item");
}

#[test]
fn the_repository_switcher_finds_and_shows_a_repository() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.here = Some("harbor-dispatch".into());
    s.live = scoped_live();
    s.routes = Some(Ok(scoped_routes()));
    s.refresh_counts();
    s.tab = Tab::Live;
    // `f` opens it on the repository shown.
    assert!(s.key(KeyCode::Char('f'), NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::Repos { sel: 0, .. })), "harbor-dispatch is the newest, and shown");
    let text = app_frame(&s, &c, 120, 30);
    snap("app-repos-switcher", &text);
    for want in ["Repository", "harbor-dispatch", "opened here", "acme-billing", ADAGE, "All repositories", "✓ shown"]
    {
        assert!(text.contains(want), "the switcher lacks {want:?}:\n{text}");
    }
    // Typed letters search; enter shows the repository everywhere.
    for ch in "acme".chars() {
        s.key(KeyCode::Char(ch), NONE);
    }
    assert_eq!(s.repo_choices("acme").len(), 1);
    s.key(KeyCode::Enter, NONE);
    assert!(s.modal.is_none());
    assert_eq!(s.scope_key(), Some(ACME));
    assert_eq!(s.live.waiting, Some(0), "the review count is acme-billing's");
    assert!(s.toasts.iter().any(|t| t.text == "Live, Reflexes and Map show acme-billing"));
    let live = app_frame(&s, &c, 120, 30);
    assert!(
        live.contains("Run  ls") && !live.contains("INHIBIT"),
        "acme-billing's one call, not harbor-dispatch's:\n{live}"
    );
    assert!(live.contains("now · acme-billing"), "another repository than the one opened here is named:\n{live}");
    // From Reflexes, the last choice: every repository. The palette reaches the same choices.
    s.tab = Tab::Reflexes;
    s.key(KeyCode::Char('f'), NONE);
    s.key(KeyCode::End, NONE);
    for _ in 0..4 {
        s.key(KeyCode::Down, NONE);
    }
    s.key(KeyCode::Enter, NONE);
    assert_eq!(s.scope_key(), None);
    assert!(palette::matches(&s, "repository adage")
        .iter()
        .any(|(e, _)| e.run == palette::Run::Scope(Some(ADAGE.into()))));
    assert!(palette::matches(&s, "choose a repository").iter().any(|(e, _)| e.run == palette::Run::Repos));
    // Esc closes it, nothing changed.
    s.key(KeyCode::Char('f'), NONE);
    s.key(KeyCode::Esc, NONE);
    assert!(s.modal.is_none() && s.scope_key().is_none());
}

#[test]
fn outside_a_repository_the_screens_settle_on_the_most_recent() {
    let mut live = scoped_live();
    assert_eq!(live.scope(None), None, "nothing chosen: every repository");
    live.settle(None);
    assert_eq!(live.scope(None), Some("harbor-dispatch"), "the newest call's repository");
    // Settled, it stays: newer calls elsewhere do not move it.
    let later =
        super::tail::Record { session: "s".into(), value: json!({ "ts": T0 + 60_000, "event": "Stop", "repo": ACME }) };
    live.push_records(vec![later]);
    live.settle(None);
    assert_eq!(live.scope(None), Some("harbor-dispatch"));
    // Opened in a repository, that one.
    let mut here = scoped_live();
    here.settle(Some(ADAGE));
    assert_eq!(here.scope(Some(ADAGE)), Some(ADAGE));
}

// ---------------------------------------------------------------------------------------------
// The design kit at work: Health and Home as dashboards.

#[test]
fn health_is_a_dashboard_with_problems_pinned_and_sections() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let c = ctx(paths.clone(), false);
    let mut s = app_state();
    s.live = live_from(&paths);
    s.tab = Tab::Health;
    s.check_sel = tabs::health::first(s.health.as_ref().unwrap());
    for (w, h) in [(80, 24), (140, 40)] {
        let text = app_frame(&s, &c, w, h);
        for want in ["Healthy", "4 checks passed", "2 to look at", "Needs a look", "Hooks & delivery"] {
            assert!(text.contains(want), "{w}x{h} lacks {want:?}:\n{text}");
        }
        // Plain words first: no generation hash on a row.
        assert!(!text.contains("8216e29f00,"), "{text}");
        snap(&format!("app-health-{w}x{h}"), &text);
    }
    // The first problem is selected first, and its card says what enter does.
    assert_eq!(s.health.as_ref().unwrap().checks[s.check_sel].check, "embedding model");
    let text = app_frame(&s, &c, 140, 40);
    assert!(text.contains("enter  fetch the intent model"), "{text}");
    // j/k move through the checks in the order shown: problems, then each section.
    s.key(KeyCode::Char('j'), NONE);
    assert_eq!(s.health.as_ref().unwrap().checks[s.check_sel].check, "hooks: codex");
    s.key(KeyCode::Char('j'), NONE);
    assert_eq!(s.health.as_ref().unwrap().checks[s.check_sel].check, "hooks: claude-code");
    s.key(KeyCode::Char('G'), NONE);
    assert_eq!(s.health.as_ref().unwrap().checks[s.check_sel].check, "binary");
    // Plain labels and short values.
    let check = |name: &str, detail: &str| Check {
        check: name.into(),
        label: name.into(),
        status: Status::Warn,
        detail: detail.into(),
        fix: None,
    };
    assert_eq!(tabs::health::label("hook activity: codex"), "Codex activity");
    assert_eq!(tabs::health::label("hooks: claude-code (twice)"), "Claude Code hooks (twice)");
    let activity = check("hook activity: codex", "Codex is set up but hasn't sent anything from this repository yet.");
    assert_eq!(tabs::health::short(&activity), "Codex is set up but hasn't sent anything from this repository yet");
    let model = check("embedding model", "potion-base-8m@f65d0f325faa installed; checksums verified");
    assert_eq!(tabs::health::short(&model), "potion-base-8m installed");
    let market = check("marketplace", "adage (built into Adage 0.1.0): 8 packs, every one installed as pinned");
    assert_eq!(tabs::health::short(&market), "8 packs, every one installed as pinned");
    let index = check("index", "generation 8216e29f00, 4 reflex(es) in 1 repository");
    assert_eq!(tabs::health::short(&index), "4 reflex(es) in 1 repository");
    for name in ["hooks: codex", "hook activity: codex", "auto-learn", "index", "embedding model", "binary"] {
        assert_ne!(tabs::health::group(name), tabs::health::Group::Other, "{name}");
    }
}

#[test]
fn home_is_a_dashboard_of_tiles_and_buttons() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let c = ctx(paths.clone(), false);
    let mut s = app_state();
    s.live = live_from(&paths);
    s.tab = Tab::Home;
    for (w, h) in [(80, 24), (140, 40)] {
        let text = app_frame(&s, &c, w, h);
        for want in ["harbor-dispatch", "Reflexes", "Fired", "Conditioned", "Health", "Open repository"] {
            assert!(text.contains(want), "{w}x{h} lacks {want:?}:\n{text}");
        }
        snap(&format!("app-home-{w}x{h}"), &text);
    }
    // The buttons move with the arrows, either way, and enter opens the one selected.
    s.key(KeyCode::Right, NONE);
    s.key(KeyCode::Down, NONE);
    assert_eq!(s.home_sel, 2);
    s.key(KeyCode::Left, NONE);
    assert_eq!(s.home_sel, 1);
    s.key(KeyCode::Enter, NONE);
    assert!(s.palette.as_ref().is_some_and(|p| p.query == "connect"));
}

// ---------------------------------------------------------------------------------------------
// The live view as a cockpit: the pulse band, the session's graph, the rail.

/// Harbor-dispatch over half an hour. First a control session, where Adage held back. Then one
/// session: a turn where a reflex fired at the prompt and was followed after a quiet stretch, a
/// finish line was set, a dead end was headed off, the agent was asked to go on, and the finish
/// line was met; a quiet turn; and a turn still going, with a test run pointed at the project's
/// interpreter. Three conditioning runs, the last with a change and patterns forming.
fn cockpit_live() -> Live {
    const CTRL: &str = "91d5aa70-33cc-47e2-a5c3-00000000c4a1";
    const A: &str = "7c1e04d2-11aa-4a31-9f0e-00000000a11c";
    let at = |k: u64| T0 + k * 70_000;
    let r = |k: u64, session: &str, arm: &str, event: &str, kind: Option<&str>, extra: Value| {
        let mut v = json!({ "ts": at(k), "event": event, "arm": arm, "repo": "harbor-dispatch", "harness": "claude" });
        if let Some(kind) = kind {
            v["kind"] = json!(kind);
        }
        for (key, val) in extra.as_object().unwrap() {
            v[key] = val.clone();
        }
        super::tail::Record { session: session.into(), value: v }
    };
    let lookup = || directive("lookup-incident", "fire", LOOKUP);
    let codegen = json!({ "route": "codegen-after-registry", "track": "harbor-dispatch/incidents", "strength": "fire",
        "text": "After editing registry/*.csv, run:\n  npm run codegen" });
    let quiet = |k| {
        r(
            k,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["read {path}", "search:content {pattern}"] } }),
        )
    };
    let mut records = vec![
        r(0, CTRL, "control", "SessionStart", Some("session_start"), json!({})),
        r(1, CTRL, "control", "UserPromptSubmit", Some("prompt"), json!({ "decision": { "would_have": [lookup()] } })),
        r(
            2,
            CTRL,
            "control",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["exec bin/fetch-incident INC-17"],
            "judgements": [{ "kind": "followed", "route": "lookup-incident" }] } }),
        ),
        r(3, CTRL, "control", "SessionEnd", None, json!({})),
        r(4, A, "treated", "SessionStart", Some("session_start"), json!({})),
        r(5, A, "treated", "UserPromptSubmit", Some("prompt"), json!({ "decision": { "directives": [lookup()] } })),
    ];
    records.extend((6..=10).map(quiet));
    records.extend([
        r(
            11,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["exec bin/fetch-incident INC-2045"],
            "judgements": [{ "kind": "followed", "route": "lookup-incident" }] } }),
        ),
        r(
            12,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["edit {path}"], "directives": [codegen] } }),
        ),
        r(
            13,
            A,
            "treated",
            "PostToolUseFailure",
            Some("failure"),
            json!({ "decision": { "observed": ["exec npm test !failed"],
            "directives": [directive("dead-end-npm-test", "inhibit", DEAD_END)] } }),
        ),
        r(
            14,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["exec npm run check"],
            "judgements": [{ "kind": "followed", "route": "dead-end-npm-test" }] } }),
        ),
        r(
            15,
            A,
            "treated",
            "Stop",
            Some("stop"),
            json!({ "decision": { "continue_reason": "Adage finish-line check: …" } }),
        ),
        r(
            16,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["exec npm run codegen"],
            "judgements": [{ "kind": "obligation_met", "route": "codegen-after-registry" }] } }),
        ),
        r(17, A, "treated", "Stop", Some("stop"), json!({ "decision": {} })),
        r(18, A, "treated", "UserPromptSubmit", Some("prompt"), json!({ "decision": {} })),
        r(19, A, "treated", "PostToolBatch", Some("batch"), json!({ "decision": { "observed": ["read {path}"] } })),
        r(
            20,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["read {path}", "read {path}"] } }),
        ),
        r(21, A, "treated", "Stop", Some("stop"), json!({ "decision": {} })),
        r(22, A, "treated", "UserPromptSubmit", Some("prompt"), json!({ "decision": {} })),
        r(
            23,
            A,
            "treated",
            "PreToolUse",
            Some("pretool"),
            json!({ "pretool": { "rewrite": "test", "interpreter": ".venv/bin/python" } }),
        ),
        r(
            24,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["exec pytest tests/"] } }),
        ),
        r(
            25,
            A,
            "treated",
            "PostToolBatch",
            Some("batch"),
            json!({ "decision": { "observed": ["exec npm run build", "read {path}"] } }),
        ),
    ]);
    let mut live = Live::default();
    live.push_records(records);
    let cand = |route: &str, support: u64| json!({ "route": route, "track": "harbor-dispatch/learned", "change": "candidate", "support": support, "s_min": 5 });
    live.push_log(vec![
        json!({ "ts": at(1), "kind": "condition", "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 20, "sessions_used": 18, "duration_ms": 640, "changes": [] }).to_string(),
        json!({ "ts": at(9), "kind": "condition", "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 22, "sessions_used": 21, "duration_ms": 690, "changes": [cand("run-1", 3)] }).to_string(),
        json!({ "ts": at(26), "kind": "condition", "trigger": "manual", "repo": "harbor-dispatch", "sessions_read": 25, "sessions_used": 24, "duration_ms": 730, "changes": [
            { "route": "lookup-incident", "track": "harbor-dispatch/incidents", "change": "strength", "from": "prime", "to": "fire",
              "summary": "reflex prime → fire (24 of 25 traces, lower bound 0.80)", "gist": "on a prompt that names a ticket, run bin/fetch-incident {item}" },
            cand("run-1", 4), cand("sequence-2", 3) ] }).to_string(),
    ]);
    live.gists = [
        ("lookup-incident", "on a prompt that names a ticket, run bin/fetch-incident {item}"),
        ("codegen-after-registry", "after editing registry/*.csv, run npm run codegen"),
        ("dead-end-npm-test", "when npm test fails here, run npm run check"),
    ]
    .into_iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    (live.waiting, live.needs) = (Some(3), 1);
    live
}

#[test]
fn the_live_cockpit_at_its_sizes() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let h = health();
    // The watch pane in a split: the band as chips, the graph one line a node.
    let live = cockpit_live();
    let (pane, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    snap("cockpit-watch-80x24", &pane);
    for want in [
        "◠ 3 fired · ✓ 2 followed",
        "■ 1 finish line",
        "❯ PROMPT · 2 steps, 3 calls · Run ×2 · Read",
        "◆ adage ↪ pointed the test run at .venv/bin/python",
        "├ Run · Read",
        "◇ turn 12:25–12:28 · 2 steps, nothing to say",
        "◆ conditioned 12:34 · 24 traces · 1 change · 2 patterns forming",
        "1 needs you · 2 to look over · adage review",
    ] {
        assert!(pane.contains(want), "the pane lacks {want:?}:\n{pane}");
    }
    // The app's Live tab: the band as tiles, the graph in boxes, the rail beside it.
    let mut s = app_state();
    s.here = Some("harbor-dispatch".into());
    s.live = cockpit_live();
    s.tab = Tab::Live;
    for (w, hgt) in [(140, 46), (180, 50)] {
        let text = app_frame(&s, &c, w, hgt);
        snap(&format!("cockpit-app-{w}x{hgt}"), &text);
        for want in [
            // The band.
            "LIVE   harbor-dispatch · 2 sessions · since 12:04 · last call 12:33:10",
            "─ ◠ Fired ─",
            "reflexes delivered",
            "67%",
            "of 3 fired",
            "1 reminder sent",
            // The graph: deliveries off their steps, lit when followed; the dead end headed off;
            // the finish line asked for, then met; quiet runs and a quiet turn folded.
            "◆ adage · FIRE",
            // What the reflex tells any agent, in its plain words (in two lines where it needs them).
            "│ on a prompt that names a ticket, run",
            "│ bin/fetch-incident {item}",
            "─ ✓ followed ─",
            "✓ followed adage",
            "◆ adage · INHIBIT",
            "─ ✓ headed off ─",
            "◆ adage ■ finish line: asked the agent to go on before it stops",
            "◆ adage ✓ finish line met: npm run codegen",
            "┊   5 steps, nothing to say   Read ×5 · Search ×5",
            "◇ turn 12:25–12:28 · 2 steps, nothing to say",
            "◆ adage ↪ pointed the test run at .venv/bin/python",
            // The rail.
            "Reflexes in play  3",
            "on a prompt that names a ticket",
            "Learning",
            "conditioned 12:34",
            "prime → fire",
            "2 patterns forming",
            "closest 4 of 5 traces",
            "3 runs",
            " · adage review",
            "Health",
        ] {
            assert!(text.contains(want), "{w}x{hgt} lacks {want:?}:\n{text}");
        }
        assert!(!text.contains("a11c") && !text.contains("lookup-incident"), "no ids on first read:\n{text}");
    }
    // In colour: the delivery's box in its strength, the follow lit green, Adage's own lines in
    // the accent.
    let colour = ctx(Paths::at(d.path()), true);
    let (_, buf) = render(140, 46, |f| app::draw(f, &s, &colour));
    snap("cockpit-app-140x46-colour", &styled(&buf));
    let (x, y) = find(&buf, "◆ adage · ● INHIBIT").unwrap();
    assert_eq!(buf[(x, y)].fg, Color::Red);
    let (x, y) = find(&buf, "✓ headed off").unwrap();
    assert_eq!(buf[(x, y)].fg, Color::Green);
    let (x, y) = find(&buf, "◆ adage ✓ finish line met").unwrap();
    assert_eq!(buf[(x, y)].fg, Color::Magenta);
    // Scrolled back: the newest is below, and a line says how to get back to it.
    s.live.scroll = 6;
    let back = app_frame(&s, &c, 140, 46);
    assert!(back.contains("6 newer lines · G back to the newest") && back.contains("↑ scrolled 6"), "{back}");
    // The control session, further up: withheld, and dimmed.
    s.live.scroll = 40;
    let top = app_frame(&s, &c, 140, 46);
    assert!(
        top.contains("control: Adage holds back") && top.contains("○ would fire bin/fetch-incident INC-2045"),
        "{top}"
    );
}

#[test]
fn the_live_cockpit_waits_for_an_agent() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.here = Some("harbor-dispatch".into());
    s.tab = Tab::Live;
    let text = app_frame(&s, &c, 140, 46);
    snap("cockpit-empty-140x46", &text);
    for want in
        ["watching harbor-dispatch", "No traces yet: waiting for your agent.", "adage init", "Nothing conditioned yet."]
    {
        assert!(text.contains(want), "lacks {want:?}:\n{text}");
    }
    let live = Live::default();
    let (pane, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&health()), &c, true));
    snap("cockpit-empty-watch-80x24", &pane);
    assert!(
        pane.contains("No traces yet") && pane.contains("adage init") && pane.contains("adage condition"),
        "{pane}"
    );
}

#[test]
fn enter_reviews_a_frozen_reflex_with_scroll_and_explicit_confirmation() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    let original = s.selected_route().unwrap().clone();
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::ReflexReview(review)) if review.row.id == original.id));
    let text = app_frame(&s, &c, 80, 24);
    assert!(text.contains("Review reflex") && text.contains("approve") && text.contains("esc"), "{text}");
    snap("app-reflex-review-80x24", &text);
    s.key(KeyCode::PageDown, NONE);
    assert!(matches!(&s.modal, Some(Modal::ReflexReview(review)) if review.scroll > 0));
    s.key(KeyCode::End, NONE);
    let bottom = app_frame(&s, &c, 80, 24);
    assert!(bottom.contains("approve") && bottom.contains("esc"), "decisions stay visible:\n{bottom}");
    s.key(KeyCode::Up, NONE);
    assert_ne!(app_frame(&s, &c, 80, 24), bottom, "End then Up scrolls immediately");
    // A background refresh must not substitute a different revision under the review.
    s.routes.as_mut().unwrap().as_mut().unwrap().rows[0].review_preview.as_mut().unwrap().revision = "newer".into();
    s.key(KeyCode::Char('a'), NONE);
    let confirm = app_frame(&s, &c, 80, 24);
    assert!(confirm.contains("confirm") && confirm.contains("back"), "{confirm}");
    assert!(s.key(KeyCode::Esc, NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::ReflexReview(_))), "cancel returns to review");
    s.key(KeyCode::Char('a'), NONE);
    match s.key(KeyCode::Enter, NONE) {
        Some(Cmd::Do(app::Action::Review { verb, expected_revision, .. })) => {
            assert_eq!(verb, "approve");
            assert_eq!(expected_revision, original.review_preview.unwrap().revision);
        }
        other => panic!("expected exact reviewed approval, got {other:?}"),
    }
    s.key(KeyCode::Enter, NONE);
    assert!(s.key(KeyCode::Char('q'), NONE).is_none(), "q closes review, not the app");
    assert!(s.modal.is_none());
}

#[test]
fn review_and_summary_distinguish_approval_from_strength_and_execution() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Reflexes;
    s.routes = Some(Ok(card_routes()));
    let r = s.routes.as_mut().unwrap().as_mut().unwrap().rows.iter_mut().find(|r| r.id == "lockfile-cargo").unwrap();
    r.approved = true;
    r.ceiling = "prime".into();
    let (reference, id) = (r.reference.clone(), r.id.clone());
    tabs::reflexes::select(&mut s, &reference, &id);
    let text = app_frame(&s, &c, 100, 40);
    assert!(text.contains("Approved") && text.contains("active advice") && text.contains("Ceiling: Prime"), "{text}");
    assert!(text.contains("WHEN") || text.contains("when an edit"), "the lower graph remains:\n{text}");
    s.key(KeyCode::Enter, NONE);
    for (width, height) in [(80, 24), (44, 12), (40, 10)] {
        let text = app_frame(&s, &c, width, height);
        assert!(text.contains("esc"), "close remains discoverable at {width}x{height}:\n{text}");
    }
    s.key(KeyCode::Esc, NONE);
    let rows = &mut s.routes.as_mut().unwrap().as_mut().unwrap().rows;
    let r = rows.iter_mut().find(|r| r.id == id).unwrap();
    r.state = "nascent".into();
    let text = app_frame(&s, &c, 100, 40);
    assert!(!text.contains("Approved · active"), "a non-live route is never called active:\n{text}");
}

#[test]
fn traces_separate_recorded_activity_from_latest_conditioning_and_open_exact_session() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.here = Some("harbor-dispatch".into());
    s.traces.data = Some(Ok(json!({
        "trace_sessions": [
            {"session":"first","repo":"harbor-dispatch","end_ts":T0,"events_total":2,"harnesses":["claude-code"]},
            {"session":"other","repo":"another","end_ts":T0,"events_total":99,"harnesses":["codex"]}
        ],
        "learning_history":{"repositories":[{"repo":"harbor-dispatch","sessions_used":6,"files_read":12,"last_conditioned_at":T0}]}
    })));
    s.live.anim.now_ms = Some(T0 + 120_000);
    assert!(matches!(s.key(KeyCode::Char('7'), NONE), Some(Cmd::Want(app::WantKind::Traces))));
    let text = app_frame(&s, &c, 100, 30);
    for want in [
        "Recorded hook activity: 1 session",
        "Last conditioning · 2 min ago",
        "6 sessions used / 12 files read",
        "not a lifetime total",
        "History sources",
    ] {
        assert!(text.contains(want), "missing {want:?}:\n{text}");
    }
    snap("app-traces-100x30", &text);
    assert!(
        matches!(s.key(KeyCode::Enter, NONE), Some(Cmd::OpenTrace {session,repo}) if session == "first" && repo == "harbor-dispatch")
    );
    assert!(matches!(
        s.key(KeyCode::Char('h'), NONE),
        Some(Cmd::Do(app::Action::Tool(super::tools::Tool::HistorySources)))
    ));
    assert!(matches!(s.key(KeyCode::Char('c'), NONE), Some(Cmd::ConditionTraces)));
    assert!(matches!(tabs::traces::condition(&mut s, false), Some(Cmd::PreviewLearning)));
    let lines = tabs::traces::detail_lines(&json!({"session":"first","repo":"harbor-dispatch","events_total":3,"events":[
        {"ts":T0,"event":"PostToolUse","harness":"claude-code","arm":"treated","observed":["exec pwd"],"directives":[],"judgements":[]},
        {"ts":T0,"event":"Stop","harness":"claude-code","arm":"control","observed":[],"directives":[],"would_have":[{"route":"verify","text":"Run the check"}],"judgements":[]},
        {"ts":T0,"event":"PreToolUse","harness":"claude-code","arm":"treated","pretool":{"rewrite":"project-python","interpreter":".venv/bin/python"},"predicted":"verify","substituted":"repair","answered":"receipt"}
    ]})).join("\n");
    assert!(
        lines.contains("Observed: exec pwd")
            && lines.contains("Withheld (control): verify")
            && lines.contains("Run the check")
    );
    for action in [
        "Applied command rewrite: project-python",
        "Interpreter: .venv/bin/python",
        "Chose next step: verify",
        "Replaced dead end: repair",
        "Answered from recorded result: receipt",
    ] {
        assert!(lines.contains(action), "{lines}");
    }
    s.traces.data = Some(Ok(
        json!({"trace_sessions":[],"learning_history":{"repositories":[{"repo":"harbor-dispatch","sessions_used":null,"files_read":null}]}}),
    ));
    let empty = app_frame(&s, &c, 80, 24);
    assert!(
        empty.contains("not recorded sessions used") && empty.contains("No hook sessions"),
        "unknown is not zero:\n{empty}"
    );
}

#[test]
fn traces_condition_action_stays_visible_before_loaded_data_and_in_small_terminals() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    let mut s = app_state();
    s.tab = Tab::Traces;
    for data in [
        None,
        Some(Err("unreadable activity".into())),
        Some(Ok(
            json!({"trace_sessions":[],"learning_history":{"repositories":(0..40).map(|n|json!({"repo":format!("repo-{n}"),"sessions_used":6,"files_read":12})).collect::<Vec<_>>()}}),
        )),
    ] {
        s.traces.data = data;
        s.set_scope(None);
        for (width, height) in [(100, 30), (80, 24), (40, 10)] {
            let text = app_frame(&s, &c, width, height);
            assert!(
                text.lines().nth(2).is_some_and(|line| line.contains("c Condition")),
                "the main action stays above every inventory at {width}x{height}:\n{text}"
            );
            assert!(
                text.lines().last().is_some_and(|line| line.contains("condition")),
                "the footer keeps the key at {width}x{height}:\n{text}"
            );
        }
    }
}

#[test]
fn traces_condition_opens_selected_checkout_and_never_skips_preview() {
    fn condition(s: &mut AppState, demo: bool) -> Option<Cmd> {
        assert!(matches!(s.key(KeyCode::Char('c'), NONE), Some(Cmd::ConditionTraces)));
        tabs::traces::condition(s, demo)
    }
    use super::forms::Kind;
    let mut s = app_state();
    s.tab = Tab::Traces;
    s.here = Some("harbor-dispatch".into());
    s.set_scope(Some("other-repository".into()));
    assert!(condition(&mut s, false).is_none());
    assert!(
        matches!(&s.modal,Some(Modal::Form(form)) if form.kind==Kind::ConditionRepository && form.repository.as_deref()==Some("other-repository"))
    );
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), false);
    for (width, height) in [(80, 24), (40, 10)] {
        let text = app_frame(&s, &c, width, height);
        assert!(text.contains("other-repository") && text.contains("preview") && text.contains("cancel"), "{text}");
        assert!(text.contains("Repository folder") || text.contains("Folder: >"), "input remains visible:\n{text}");
    }
    assert!(s.key(KeyCode::Enter, NONE).is_none(), "an empty folder cannot submit");
    assert!(s.key(KeyCode::Esc, NONE).is_none());
    assert!(s.modal.is_none());
    condition(&mut s, false);
    let Some(Modal::Form(form)) = s.modal.as_mut() else { panic!("folder form") };
    form.values[0] = "/tmp/other project $(literal)".into();
    assert!(
        matches!(s.key(KeyCode::Enter,NONE),Some(Cmd::OpenRepositoryToCondition { directory, repo }) if directory==std::path::Path::new("/tmp/other project $(literal)") && repo.as_deref()==Some("other-repository"))
    );
    for here in [Some("harbor-dispatch".into()), None] {
        s.here = here;
        s.health = None;
        s.set_scope(None);
        assert!(condition(&mut s, false).is_none(), "all repositories never silently learns the open checkout");
        assert!(
            matches!(&s.modal,Some(Modal::Form(form)) if form.kind==Kind::ConditionRepository && form.repository.is_none())
        );
        s.key(KeyCode::Esc, NONE);
    }
    s.here = Some("harbor-dispatch".into());
    s.set_scope(Some("harbor-dispatch".into()));
    assert!(matches!(condition(&mut s, false), Some(Cmd::PreviewLearning)));
    s.pending_job_action = Some(app::Action::LearnNow);
    s.complete_job("Preview", Ok(super::conditioned::sample_sources(8).into()));
    assert!(matches!(&s.modal, Some(Modal::Conditioning(_))));
    assert!(s.key(KeyCode::Esc, NONE).is_none(), "c opens a preview, not a learning run");
    assert!(s.modal.is_none());
    // Demo identities are samples, never a constraint on the person's real checkout.
    for scope in [Some("harbor-dispatch".into()), Some("other-sample".into()), None] {
        s.set_scope(scope);
        assert!(condition(&mut s, true).is_none());
        assert!(matches!(&s.modal,Some(Modal::Form(form)) if form.repository.is_none()));
        s.key(KeyCode::Esc, NONE);
    }
}

#[test]
fn traces_condition_rejects_a_different_repository_before_switching() {
    let d = tempfile::tempdir().unwrap();
    let selected = d.path().join("selected");
    let other = d.path().join("other");
    std::fs::create_dir_all(selected.join(".git")).unwrap();
    std::fs::create_dir_all(other.join(".git")).unwrap();
    let c = ctx(Paths::at(d.path().join("adage-data")), false);
    let expected = adage_core::repo::repo_key(&selected);
    let mismatch = app::repository_to_condition(&c, &other, Some(&expected));
    assert!(mismatch.is_err());
    assert!(mismatch.err().unwrap().to_string().contains("Choose the checkout for selected"));
    let next = app::repository_to_condition(&c, &selected, Some(&expected)).unwrap();
    assert_eq!(next.root.as_deref(), Some(selected.canonicalize().unwrap().as_path()));
    assert!(
        app::repository_to_condition(&c, &other, None).is_ok(),
        "an explicit choice from all repositories is allowed"
    );
    assert!(!c.paths.root.exists(), "opening a folder to preview must not write Adage data");
}
