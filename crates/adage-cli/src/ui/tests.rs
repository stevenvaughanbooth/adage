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
use adage_tracks::Aspect;
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

fn directive(route: &str, aspect: &str, text: &str) -> Value {
    json!({ "route": route, "track": "harbor-dispatch/incidents", "version": "0.3.0", "aspect": aspect, "text": text })
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
    let lookup = || directive("lookup-incident", "green", LOOKUP);
    spool::append(
        &s(a),
        &rec(0, "treated", "UserPromptSubmit", "prompt", decision(vec![lookup()], vec![lookup()], vec![], None)),
    )
    .unwrap();
    spool::append(
        &s(c),
        &rec(1, "holdout", "UserPromptSubmit", "prompt", decision(vec![], vec![lookup()], vec![], None)),
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
                vec![directive("read-linked-files", "yellow", "That result lists these files: read them together.")],
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
            decision(vec![directive("dead-end-npm-test", "red", DEAD_END)], vec![], vec![], None),
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
            decision(vec![], vec![], vec![json!({ "kind": "derailed", "route": "lookup-incident" })], None),
        ),
    )
    .unwrap();
    spool::append(
        &s(c),
        &rec(
            6,
            "holdout",
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
    // A record from another repository, and one from an older version (no kind, repo, harness).
    let mut other = rec(
        9,
        "treated",
        "UserPromptSubmit",
        "prompt",
        decision(vec![directive("lookup", "green", "")], vec![], vec![], None),
    );
    other["repo"] = json!("acme-billing");
    other["harness"] = json!("codex");
    spool::append(&s("019a-codex-session-0001"), &other).unwrap();
    let log = [
        json!({ "ts": T0 - 3_600_000, "sessions_read": 12, "sessions_used": 12, "index_generation": "1a2b3c4d5e", "repos": { "harbor-dispatch": { "sessions": 12, "tracks": [{ "name": "harbor-dispatch/incidents", "version": "0.2.0", "change": "new", "routes": 3 }] } } }).to_string(),
        "not json at all".to_string(),
        json!({ "ts": T0 + 10_000, "kind": "learn", "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 25, "sessions_used": 24, "duration_ms": 730, "index_generation": "8216e29f00", "changes": [
            { "route": "lookup-incident", "track": "harbor-dispatch/incidents", "change": "aspect", "from": "yellow", "to": "green", "summary": "work item route yellow → green (24 of 25 sessions, lower bound 0.80)" },
            { "route": "dead-end-npm-test", "track": "harbor-dispatch/incidents", "change": "new", "to": "red", "summary": "6/7 sessions" },
        ] }).to_string(),
        json!({ "ts": T0 + 20_000, "kind": "learn_error", "repo": "harbor-dispatch", "class": "io", "error": "tracks directory is not writable" }).to_string(),
    ];
    std::fs::write(paths.learn_log(), log.join("\n") + "\n").unwrap();
    paths
}

fn health() -> Health {
    let check = |name: &str, status, detail: &str, fix: Option<&str>| Check {
        check: name.into(),
        status,
        detail: detail.into(),
        fix: fix.map(String::from),
    };
    Health {
        checks: vec![
            check("binary", Status::Pass, "adage 0.1.0 at ~/bin/adage", None),
            check("index", Status::Pass, "generation 8216e29f00, 4 route(s) in 1 repository", None),
            check(
                "embedding model",
                Status::Warn,
                "not installed, so the index's intent routes cannot fire",
                Some("run `adage models fetch`"),
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
                Some("`adage install codex --write`"),
            ),
            check("fail-open errors", Status::Pass, "none in the last 7 days", None),
        ],
        fixes: vec![None, None, Some(Fix::FetchModel), None, Some(Fix::InstallHooks("codex".into())), None],
        lights: vec![
            Light::new("hooks claude", Lamp::Ok),
            Light::new("4 reflexes", Lamp::Ok),
            Light::new("auto-condition", Lamp::Ok),
            Light {
                label: "intent model not fetched".into(),
                lamp: Lamp::Off,
                hint: Some("adage models fetch".into()),
            },
        ],
        harnesses: vec!["claude".into()],
        generation: Some("8216e29f00".into()),
        repo_key: Some("harbor-dispatch".into()),
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
fn styled(b: &Buffer) -> String {
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

fn find(buf: &Buffer, needle: &str) -> Option<(u16, u16)> {
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        if let Some(i) = row.find(needle) {
            return Some((row[..i].chars().count() as u16, y));
        }
    }
    None
}

// ---------------------------------------------------------------------------------------------
// Parsing

#[test]
fn feed_items_say_what_adage_did() {
    let d = decision(
        vec![directive("lookup-incident", "green", LOOKUP)],
        vec![directive("lookup-incident", "green", LOOKUP)],
        vec![],
        None,
    );
    let i = feed::from_record("s-1", &rec(0, "treated", "UserPromptSubmit", "prompt", d.clone()));
    assert_eq!(
        i.acts,
        vec![Act::Directed {
            route: "lookup-incident".into(),
            aspect: Aspect::Green,
            detail: "bin/fetch-incident INC-2045".into(),
            instinct: false
        }],
        "would-have is not repeated"
    );
    assert_eq!(i.texts, vec![("lookup-incident".to_string(), LOOKUP.to_string())]);
    assert_eq!((i.event.as_str(), i.harness.as_str(), i.holdout), ("prompt", "claude", false));
    let h = feed::from_record(
        "s-2",
        &rec(
            0,
            "holdout",
            "UserPromptSubmit",
            "prompt",
            decision(vec![], vec![directive("x", "yellow", "")], vec![], Some("r")),
        ),
    );
    assert_eq!(h.acts, vec![Act::Withheld { route: "x".into(), aspect: Aspect::Yellow }, Act::StopRequest]);
    // An older record: no kind, no repo, no harness.
    let old = feed::from_record(
        "s-3",
        &json!({ "ts": 5, "event": "PostToolUseFailure", "arm": "treated", "decision": { "judgements": [{ "kind": "derailed", "route": "r" }] } }),
    );
    assert_eq!(
        (old.event.as_str(), old.repo.as_str(), old.acts.clone()),
        ("failure", "", vec![Act::Derailed("r".into())])
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
    // New stored words read the same as the old.
    let n = feed::from_record(
        "s",
        &json!({ "arm": "treated", "decision": { "directives": [{ "route": "r", "aspect": "inhibit" }] } }),
    );
    assert!(matches!(&n.acts[0], Act::Directed { aspect: Aspect::Red, .. }));
}

#[test]
fn learn_log_reads_both_shapes_and_skips_the_rest() {
    let t = learnlog::parse(
        &json!({ "ts": T0, "kind": "learn", "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 25, "sessions_used": 24, "changes": [
            { "route": "lookup-work-item", "track": "harbor-dispatch/work-items", "change": "aspect", "from": "yellow", "to": "green", "summary": "24/25 sessions" }
        ] })
        .to_string(),
    );
    assert_eq!(t.len(), 2);
    assert_eq!(t[0].tone, Tone::Run);
    assert_eq!(t[0].text, "auto-condition harbor-dispatch: 24/25 traces used, 1 change");
    assert_eq!(t[1].text, "harbor-dispatch/work-items: lookup-work-item prime → fire (24/25)");
    assert_eq!(t[1].short, "work-items: lookup-work-item prime → fire (24/25)");
    assert_eq!(t[1].aspect, Some(Aspect::Green));
    let c = t[1].change.as_ref().unwrap();
    assert_eq!((c.what, c.from.as_deref(), c.to.as_deref()), ("strength", Some("prime"), Some("fire")));
    // `adage_learn::diff`'s summaries: the counts and bound, or the evidence counts alone.
    let t = learnlog::parse(
        &json!({ "ts": T0, "kind": "learn", "trigger": "manual", "repo": "r", "changes": [
            { "route": "finish-line-6a01b2", "track": "r/finish-line", "change": "aspect", "from": "yellow", "to": "green",
              "summary": "finish line route yellow → green (9 of 9 sessions, lower bound 0.70)" },
            { "route": "finish-line-6a01b2", "track": "r/finish-line", "change": "evidence", "from": null, "to": null,
              "summary": "green finish line route evidence 6/8 → 7/8 sessions" },
            { "route": "old", "track": "r/finish-line", "change": "stale" },
            { "route": "nascent-one", "track": "r/finish-line", "change": "draft", "summary": "3/4 sessions" },
          ], "repos": { "r": { "tracks": [{ "name": "r/finish-line" }] } } })
        .to_string(),
    );
    assert_eq!(t.len(), 5, "the legacy per-track fields beside `changes` add nothing: {t:?}");
    assert_eq!(t[0].text, "condition r: 4 changes");
    assert_eq!(t[1].text, "r/finish-line: finish-line-6a01b2 prime → fire (9/9 · lb 0.70)");
    assert_eq!(t[2].text, "r/finish-line: finish-line-6a01b2 evidence (6/8 → 7/8)");
    assert_eq!(t[3].text, "r/finish-line: old firm → dormant");
    assert_eq!(t[4].text, "r/finish-line: nascent-one → nascent (3/4)");
    let e = learnlog::parse(r#"{"ts":1,"kind":"learn_error","trigger":"auto","class":"lock","error":"busy"}"#);
    assert_eq!((e[0].tone, e[0].text.as_str()), (Tone::Error, "conditioning failed (lock): busy"));
    // Missing fields are tolerated; unknown kinds and non-JSON are skipped.
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
    // Colour carries the strength: the fire lamp of the directive is green.
    let (x, y) = find(&buf, "lookup-incident          ● fire").expect("a firing is shown");
    assert_eq!(buf[(x + 25, y)].fg, Color::Green);
    for s in [
        "adage watch  harbor-dispatch · gen 8216e29f · claude",
        "● live   ● fire 1   ● prime 1   ● inhibit 1",
        "─ now ─",
        "→ ◠ lookup-incident          ● fire     bin/fetch-incident INC-2045",
        "○ would fire lookup-incident (withheld)",
        "✓ followed lookup-incident",
        "✗ ignored lookup-incident",
        "→ ◠ dead-end-npm-test        ● inhibit  npm run check",
        "─ conditioning ─",
        "lookup-incident           prime → fire       24/25 · lb 0.80",
        "conditioning failed for harbor-dispatch (io)",
        "1 reflex waits for review · adage review",
        "─ health ─",
        "○ intent model not fetched · adage models fetch",
        "p pause · f scope · / filter · enter inspect · m open map · ? keys",
    ] {
        assert!(wide.contains(s), "the wide frame lacks {s:?}:\n{wide}");
    }
    // The control arm is dimmed, whole rows of it.
    let (x, y) = find(&buf, "would fire lookup-incident").unwrap();
    assert_eq!(buf[(x, y)].fg, Color::DarkGray);
    assert_eq!(buf[(x - 20, y)].fg, Color::DarkGray, "the trace id of a control row is dim too");

    // All repositories, quiet events hidden, paused.
    let mut live = live;
    live.key('f');
    live.key('v');
    live.key('p');
    let (all, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(all.contains("now · all repositories · quiet hidden") && all.contains("paused +0"), "{all}");
    assert!(all.contains("cx  prompt"), "the codex trace from another repository:\n{all}");
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
    assert!(text.contains("-> ~ lookup-incident") && text.contains("- now -"), "{text}");
    assert!(!text.chars().any(|ch| matches!(ch, '●' | '◠' | '─' | '→' | '✓')), "{text}");
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
    let (_, buf) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    let (_, y) = find(&buf, "12:04:30").expect("the new row");
    assert_eq!(buf[(0, y)].symbol(), "▌", "the pulsing row is marked");
    let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
    let rx = row[..row.find("lookup-incident").expect("the reflex id on the new row")].chars().count() as u16;
    assert!(buf[(rx, y)].modifier.contains(Modifier::BOLD), "and its reflex is bright");
    // It dims, then stops, within 900 ms.
    let at = live.pulse.at.unwrap();
    live.tick_pulse(at + std::time::Duration::from_millis(450), true);
    assert_eq!(live.pulse.level, 2);
    live.tick_pulse(at + std::time::Duration::from_millis(950), true);
    assert!(!live.pulsing() && live.pulse.level == 0);
    let (_, buf) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert_eq!(buf[(0, y)].symbol(), " ");
    // Reduced motion: no pulse at all.
    live.pulse.at = Some(at);
    live.tick_pulse(at, false);
    assert_eq!(live.pulse.level, 0);
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
        text.contains("dead-end-npm-test") && !text.contains("→ ◠ lookup-incident"),
        "only matching rows in the feed:\n{text}"
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
}

// ---------------------------------------------------------------------------------------------
// `adage ui`

#[allow(clippy::too_many_arguments)]
fn route_row(
    reference: &str,
    id: &str,
    aspect: Aspect,
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
        in_index: state != "draft" && state != "retired",
        instinct: false,
        id: id.into(),
        aspect,
        ceiling: "green".into(),
        effect: "observe".into(),
        state: state.into(),
        authored: false,
        trigger_kind: trigger.0.into(),
        trigger: trigger.1.into(),
        step_kind: step.0.into(),
        step: step.1.into(),
        support,
        opportunities,
        rate: support as f64 / opportunities.max(1) as f64,
        lower_bound: 0.53,
        turns_saved: 0.0,
        live: LiveCounts::default(),
        with: Arm::default(),
        control: Arm::default(),
        directive: String::new(),
        verbs,
        reasons: vec![],
    }
}

fn routes() -> Routes {
    let t = "harbor-dispatch/incidents";
    let mut lookup = route_row(
        t,
        "lookup-incident",
        Aspect::Green,
        "active",
        (24, 25),
        ("prompt", "a prompt names a ticket"),
        ("run", "bin/fetch-incident {item}"),
        vec!["approve", "reject", "demote", "pin"],
    );
    lookup.lower_bound = 0.80;
    lookup.turns_saved = 8.0;
    lookup.live =
        LiveCounts { emitted: 3, followed: 2, derailed: 1, met: 0, would_have: 1, unaided: 1, not_reached: 0 };
    lookup.with = Arm { applied: 12, took: 10 };
    lookup.control = Arm { applied: 5, took: 1 };
    lookup.reasons = vec![("new".into(), "conditioned and never reviewed".into())];
    lookup.directive = "<adage-reflex track=\"harbor-dispatch/incidents@0.3.0\" reflex=\"lookup-incident\" strength=\"fire\">\nIncidents here are looked up with a script. A known next step here is:\n  bin/fetch-incident INC-2045. Ignore this if the user is asking for something else.\nLearned by Adage from this repository's earlier agent sessions; not requested by the user.\n</adage-reflex>".into();
    let linked = route_row(
        t,
        "read-linked-files",
        Aspect::Green,
        "active",
        (13, 17),
        ("after_command", "`bin/fetch-incident {item}` succeeds"),
        ("read_batch", "read ≤5 listed files at once"),
        vec!["approve", "reject", "demote", "pin"],
    );
    let dead = route_row(
        t,
        "dead-end-npm-test",
        Aspect::Red,
        "active",
        (11, 43),
        ("after_failure", "`npm test` fails: HARBOR_ENV is not set"),
        ("avoid", "npm run check (not npm test)"),
        vec!["approve", "reject", "pin"],
    );
    let forming = route_row(
        "harbor-dispatch/intents",
        "start-with-npm-check",
        Aspect::Yellow,
        "probation",
        (6, 9),
        ("intent", "a prompt means: run the tests"),
        ("avoid", "npm run check (not npm test)"),
        vec!["approve", "reject", "promote", "pin"],
    );
    let mut instinct = route_row(
        "instincts/finish-line",
        "finish-line/regenerate",
        Aspect::Yellow,
        "active",
        (0, 0),
        ("after_edit", "an edit touches registry/*.csv"),
        ("run", "npm run codegen"),
        vec!["approve", "reject", "pin"],
    );
    instinct.instinct = true;
    instinct.track = "adage/finish-line".into();
    instinct.version = "0.1.0".into();
    instinct.effect = "regenerate".into();
    let nascent = route_row(
        t,
        "regen-openapi-client",
        Aspect::Yellow,
        "draft",
        (3, 4),
        ("after_edit", "an edit touches api/*.yaml"),
        ("run", "npm run openapi"),
        vec!["approve", "reject", "pin"],
    );
    let gone = route_row(
        t,
        "grep-for-incident",
        Aspect::Yellow,
        "retired",
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
    ] {
        s.tab = tab;
        let text = app_frame(&s, &c, 150, 35);
        snap(name, &text);
        assert!(text.contains(" adage   ") && text.contains("q quit"), "{text}");
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
        "▾ harbor-dispatch/incidents@0.3.0",
        "▾ harbor-dispatch/intents@0.3.0",
        "▾ nascent · waits for review",
        "▾ instincts · adage/finish-line@0.1.0",
        "▸ extinguished · 1",
        "WHAT THE AGENT SEES",
        "  bin/fetch-incident INC-2045",
        "EVIDENCE",
        "24/25 traces",
        "WITH THE REFLEX vs CONTROL",
        "10/12",
        "1/5",
        "LIFECYCLE",
        "nascent ──── ● forming ──── ● firm",
        "ACTIONS",
        "same as adage review weaken harbor-dispatch/incidents lookup-incident from any shell",
    ] {
        assert!(text.contains(want), "the Reflexes tab lacks {want:?}:\n{text}");
    }
    assert!(!text.contains("grep-for-incident"), "the extinguished group starts closed");
    // No control traces, no with-vs-control section.
    s.key(KeyCode::Down, NONE);
    let text = app_frame(&s, &c, 150, 35);
    assert!(text.contains("read-linked-files") && !text.contains("WITH THE REFLEX vs CONTROL"), "{text}");
    // Empty: the next command.
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
    // Strengthen, confirmed, records `promote` for the prime reflex.
    s.key(KeyCode::Down, NONE);
    assert_eq!(s.selected_route().unwrap().id, "start-with-npm-check");
    s.key(KeyCode::Char('s'), NONE);
    match s.key(KeyCode::Char('y'), NONE) {
        Some(Cmd::Do(app::Action::Review { verb, reference, route })) => {
            assert_eq!(
                (verb, reference.as_str(), route.as_str()),
                ("promote", "harbor-dispatch/intents", "start-with-npm-check")
            )
        }
        other => panic!("expected a review command, got {other:?}"),
    }
    s.key(KeyCode::Esc, NONE);
    assert!(s.reflexes.filter.is_empty());
    // Chips: F hides fire; S cycles the lifecycle filter.
    s.key(KeyCode::Char('F'), NONE);
    assert!(tabs::reflexes::tree(&s).iter().all(|n| match n {
        tabs::reflexes::Node::Reflex(i) => s.route_rows()[*i].aspect != Aspect::Green,
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
    // `t` lists traces; `o` names the map command; `d` the delegate command.
    tabs::reflexes::select(&mut s, "harbor-dispatch/incidents", "lookup-incident");
    s.key(KeyCode::Char('o'), NONE);
    assert!(
        matches!(&s.modal, Some(Modal::Message { lines, .. }) if lines.iter().any(|l| l.contains("adage map --focus lookup-incident")))
    );
    s.key(KeyCode::Esc, NONE);
    s.key(KeyCode::Char('d'), NONE);
    assert!(
        matches!(&s.modal, Some(Modal::Message { lines, .. }) if lines.iter().any(|l| l.contains("adage review delegate")))
    );
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
    s.key(KeyCode::Char('-'), NONE);
    assert!(s.map.compact);
    // `o` names the web map command; enter opens the reflex in the Reflexes tab.
    s.key(KeyCode::Char('o'), NONE);
    assert!(
        matches!(&s.modal, Some(Modal::Message { lines, .. }) if lines.iter().any(|l| l.contains("adage map --focus dead-end-npm-test")))
    );
    s.key(KeyCode::Esc, NONE);
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
            if k.bar && status.contains(&format!("{} {}", k.keys, k.label)) {
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
    // Health: enter on a fixable check starts its fix; install asks for a scope first; c conditions.
    s.key(KeyCode::Char('4'), NONE);
    s.check_sel = 4;
    assert!(s.key(KeyCode::Enter, NONE).is_none());
    assert!(matches!(&s.modal, Some(Modal::Scope { harness }) if harness == "codex"));
    assert!(matches!(s.key(KeyCode::Char('u'), NONE), Some(Cmd::PlanInstall { scope: "user", .. })));
    s.key(KeyCode::Char('c'), NONE);
    assert!(matches!(&s.modal, Some(Modal::Confirm { title, .. }) if title == "Condition now"));
    assert!(matches!(s.key(KeyCode::Char('y'), NONE), Some(Cmd::Do(app::Action::LearnNow))));
    // Settings: a boolean toggles; thresholds switch with Space; a number opens the editor, whose
    // value `adage_store::config::set` validates when it is saved.
    s.key(KeyCode::Char('5'), NONE);
    assert!(matches!(
        s.key(KeyCode::Enter, NONE),
        Some(Cmd::Do(app::Action::SetConfig { key: "learn.auto", ref value })) if value == "false"
    ));
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
    assert_eq!(titles, ["Harnesses", "Hooks(\"claude-code\")", "Model", "Learn", "AutoLearn", "Finish"]);
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Replan)));
    assert!(matches!(w.key(KeyCode::Tab, NONE), Some(wizard::Cmd::Replan)));
    assert_eq!(w.scope, "user");
    assert!(w.key(KeyCode::Char('y'), NONE).is_none(), "nothing to install until planned");
    assert!(w.key(KeyCode::Char('n'), NONE).is_none());
    assert_eq!(*w.current(), wizard::Kind::Model);
    assert!(matches!(w.key(KeyCode::Char('y'), NONE), Some(wizard::Cmd::FetchModel)));
    w.key(KeyCode::Char('n'), NONE);
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
        report.contains("– Hooks: claude-code: skipped") && report.contains("✓ First learn: 12 sessions read"),
        "{report}"
    );
    assert!(report.contains("adage watch"));
    assert!(matches!(w.key(KeyCode::Enter, NONE), Some(wizard::Cmd::Quit)));
}

#[test]
fn clock_times_use_the_offset() {
    assert_eq!(view::clock(T0, 0, true), "12:04:00");
    assert_eq!(view::clock(T0, -7 * 3600, false), "05:04");
    assert_eq!(view::clock(T0, 10 * 3600 + 1800, true), "22:34:00");
}

#[test]
fn loader_delivers_health_at_once_and_reflexes_on_request() {
    use super::jobs::{Loaded, Loader, Want};
    let home = super::demo::setup().unwrap();
    let loader = Loader::spawn(home.paths.clone(), Some(home.repo.clone()));
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
