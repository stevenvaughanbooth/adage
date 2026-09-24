//! UI tests: the tailers under concurrent appends and truncation, the parsers, and frames
//! rendered with `TestBackend` from fixture data, compared with snapshots in `ui/snapshots/`
//! (`ADAGE_BLESS=1` rewrites them).

use super::app::{self, AppState, Cmd, Modal, Tab};
use super::config::{self, ConfigStore};
use super::feed::{self, Act};
use super::health::{Fix, Health, Light};
use super::learnlog::{self, Tone};
use super::routes::{LiveCounts, RouteRow, Routes};
use super::tail::{LineTail, SpoolTail};
use super::theme::{Lamp, Theme};
use super::view::{self, Ctx, Live};
use super::watch::{buffer_text, Sources};
use super::wizard::{self, Wizard};
use crate::doctor::{Check, Status};
use adage_store::{spool, Paths};
use adage_tracks::Aspect;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use ratatui::style::Color;
use ratatui::Terminal;
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;

/// 2026-09-23 12:04:00 UTC.
const T0: u64 = 1_790_165_040_000;

fn rec(k: u64, arm: &str, event: &str, kind: &str, decision: Value) -> Value {
    json!({ "ts": T0 + k * 1000, "event": event, "kind": kind, "arm": arm, "repo": "harbor-dispatch", "generation": "8216e29f00", "decision": decision })
}

fn directive(route: &str, aspect: &str) -> Value {
    json!({ "route": route, "track": "harbor-dispatch/incidents", "version": "0.3.0", "aspect": aspect, "text": "…" })
}

fn decision(directives: Vec<Value>, would: Vec<Value>, judgements: Vec<Value>, cont: Option<&str>) -> Value {
    json!({
        "directives": directives, "would_have": would, "judgements": judgements,
        "continue_reason": cont, "would_continue": cont, "observed": ["exec ls"],
    })
}

/// A data directory with two treated sessions and a holdout, and a learn.log.
fn fixture(dir: &Path) -> Paths {
    let paths = Paths::at(dir);
    let s = |id: &str| paths.spool(id);
    let a = "7c1e04d2-11aa-4a31-9f0e-00000000a11c";
    let b = "3f9b62e8-22bb-4c7d-8b21-000000000b0b";
    let c = "91d5aa70-33cc-47e2-a5c3-00000000c4a1";
    let d = |x| x;
    spool::append(
        &s(a),
        &rec(
            0,
            "treated",
            "UserPromptSubmit",
            "prompt",
            decision(
                vec![directive("lookup-incident", "green")],
                vec![directive("lookup-incident", "green")],
                vec![],
                None,
            ),
        ),
    )
    .unwrap();
    spool::append(
        &s(c),
        &rec(
            1,
            "holdout",
            "UserPromptSubmit",
            "prompt",
            decision(vec![], vec![directive("lookup-incident", "green")], vec![], None),
        ),
    )
    .unwrap();
    spool::append(
        &s(a),
        &rec(
            2,
            "treated",
            "PostToolBatch",
            "batch",
            d(decision(
                vec![directive("read-linked-files", "yellow")],
                vec![],
                vec![json!({ "kind": "followed", "route": "lookup-incident" })],
                None,
            )),
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
            decision(vec![directive("dead-end-npm-test", "red")], vec![], vec![], None),
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
        decision(vec![directive("lookup", "green")], vec![], vec![], None),
    );
    other["repo"] = json!("acme-billing");
    other["harness"] = json!("codex");
    spool::append(&s("019a-codex-session-0001"), &other).unwrap();
    let log = [
        json!({ "ts": T0 - 3_600_000, "sessions_read": 12, "sessions_used": 12, "index_generation": "1a2b3c4d5e", "repos": { "harbor-dispatch": { "sessions": 12, "tracks": [{ "name": "harbor-dispatch/incidents", "version": "0.2.0", "change": "new", "routes": 3 }] } } }).to_string(),
        "not json at all".to_string(),
        json!({ "ts": T0 + 10_000, "kind": "learn", "trigger": "auto", "repo": "harbor-dispatch", "sessions_read": 25, "sessions_used": 24, "duration_ms": 730, "index_generation": "8216e29f00", "changes": [
            { "route": "lookup-incident", "track": "harbor-dispatch/incidents", "change": "aspect", "from": "yellow", "to": "green", "summary": "24/25 sessions" },
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
            Light { label: "adage 0.1.0".into(), lamp: Lamp::Ok },
            Light { label: "hooks cc".into(), lamp: Lamp::Ok },
            Light { label: "4 routes".into(), lamp: Lamp::Ok },
            Light { label: "auto-learn on".into(), lamp: Lamp::Ok },
            Light { label: "no model".into(), lamp: Lamp::Warn },
            Light { label: "fail-open 0".into(), lamp: Lamp::Ok },
        ],
        generation: Some("8216e29f00".into()),
        routes_total: 5,
        routes_repo: Some(4),
        repo_key: Some("harbor-dispatch".into()),
        auto_learn: Some(true),
    }
}

fn ctx(paths: Paths, color: bool) -> Ctx {
    Ctx { paths, root: None, label: "harbor-dispatch".into(), theme: Theme { color }, tz: 0, demo: false }
}

fn live_from(paths: &Paths) -> Live {
    let mut live = Live::default();
    Sources { spool: SpoolTail::new(paths.sessions()), log: LineTail::new(paths.learn_log()) }.poll(&mut live);
    live
}

fn snap(name: &str, text: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/snapshots").join(format!("{name}.txt"));
    if std::env::var("ADAGE_BLESS").is_ok_and(|v| v == "1") {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
    }
    let want = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing snapshot {}; run with ADAGE_BLESS=1", path.display()));
    assert_eq!(text, want, "snapshot {name} changed (ADAGE_BLESS=1 to accept):\n{text}");
}

fn render(w: u16, h: u16, f: impl FnOnce(&mut ratatui::Frame)) -> (String, ratatui::buffer::Buffer) {
    let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
    term.draw(f).unwrap();
    let buf = term.backend().buffer().clone();
    (buffer_text(&buf), buf)
}

// ---------------------------------------------------------------------------------------------
// Parsing

#[test]
fn feed_items_say_what_adage_did() {
    let d = decision(
        vec![directive("lookup-incident", "green")],
        vec![directive("lookup-incident", "green")],
        vec![],
        None,
    );
    let i = feed::from_record("s-1", &rec(0, "treated", "UserPromptSubmit", "prompt", d.clone()));
    assert_eq!(
        i.acts,
        vec![Act::Directed { route: "lookup-incident".into(), aspect: Aspect::Green }],
        "would-have is not repeated"
    );
    assert_eq!((i.event.as_str(), i.harness.as_str(), i.holdout), ("prompt", "claude", false));
    let h = feed::from_record(
        "s-2",
        &rec(
            0,
            "holdout",
            "UserPromptSubmit",
            "prompt",
            decision(vec![], vec![directive("x", "yellow")], vec![], Some("r")),
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
    assert_eq!(t[0].text, "auto-learn harbor-dispatch: 24/25 sessions used, 1 change");
    assert_eq!(t[1].text, "learned harbor-dispatch/work-items: lookup-work-item yellow→green (24/25 sessions)");
    assert_eq!(t[1].short, "learned work-items: lookup-work-item yellow→green (24/25 sessions)");
    assert_eq!(t[1].aspect, Some(Aspect::Green));
    let e = learnlog::parse(r#"{"ts":1,"kind":"learn_error","class":"lock","error":"busy"}"#);
    assert_eq!((e[0].tone, e[0].text.as_str()), (Tone::Error, "learning failed (lock): busy"));
    // Missing fields are tolerated; unknown kinds and non-JSON are skipped.
    assert_eq!(learnlog::parse(r#"{"kind":"learn"}"#)[0].text, "learn");
    assert!(learnlog::parse(r#"{"kind":"something-else"}"#).is_empty());
    assert!(learnlog::parse("[1,2]").is_empty() && learnlog::parse("garbage").is_empty());
    let old = learnlog::parse(
        r#"{"ts":1,"sessions_read":3,"sessions_used":2,"repos":{"acme":{"tracks":[{"name":"acme/work","version":"0.1.0","change":"new","routes":2}]}}}"#,
    );
    assert_eq!(old[0].text, "learn: 2/3 sessions used, 1 track(s)");
    assert_eq!(old[1].text, "wrote acme/work @ 0.1.0 (new, 2 routes)");
}

#[test]
fn config_edits_keep_comments_and_validate() {
    let text = "# my settings\n[learn]\n# learn after sessions\nauto = false\n\n[other]\nx = 1\n";
    let out = config::set_in(text, "learn.auto", &toml::Value::Boolean(true)).unwrap();
    assert_eq!(out, "# my settings\n[learn]\n# learn after sessions\nauto = true\n\n[other]\nx = 1\n");
    let out = config::set_in(&out, "learn.min_interval_secs", &toml::Value::Integer(900)).unwrap();
    assert!(out.contains("auto = true\nmin_interval_secs = 900\n"), "{out}");
    let out = config::set_in(&out, "runtime.holdout", &toml::Value::Float(0.2)).unwrap();
    assert!(out.ends_with("[runtime]\nholdout = 0.2\n"), "{out}");
    let s = config::setting("runtime.holdout").unwrap();
    assert!(config::parse_value(s, "1.5").is_err() && config::parse_value(s, "x").is_err());
    assert_eq!(config::parse_value(s, "0.25").unwrap(), toml::Value::Float(0.25));
    let s = config::setting("learn.min_interval_secs").unwrap();
    assert!(config::parse_value(s, "5").is_err());
    let s = config::setting("learn.thresholds").unwrap();
    assert!(config::parse_value(s, "strict").is_err());

    let d = tempfile::tempdir().unwrap();
    let store = config::TomlFile::in_home(d.path());
    assert_eq!(store.get("learn.auto").unwrap(), None);
    store.set("learn.auto", toml::Value::Boolean(true)).unwrap();
    assert_eq!(config::auto_learn(d.path()), Some(true));
    // A file that is not TOML is never rewritten.
    std::fs::write(store.path(), "[learn\nauto = ").unwrap();
    assert!(store.set("learn.auto", toml::Value::Boolean(false)).is_err());
    assert_eq!(std::fs::read_to_string(store.path()).unwrap(), "[learn\nauto = ");
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
// Frames

#[test]
fn watch_frames_at_split_pane_and_wide_sizes() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let live = live_from(&paths);
    assert_eq!(live.feed.len(), 10);
    let h = health();
    let c = ctx(paths.clone(), true);
    let (small, buf) = render(60, 20, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    snap("watch-60x20", &small);
    // Colour carries the aspect: the green directive's text is green.
    let (x, y) = find(&buf, "→ lookup-incident").expect("a directive is shown");
    assert_eq!(buf[(x + 2, y)].fg, Color::Green);
    let (wide, _) = render(110, 30, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    snap("watch-110x30", &wide);
    assert!(wide.contains("learned harbor-dispatch/incidents: lookup-incident yellow→green (24/25 sessions)"));

    // All repositories, quiet events hidden, paused.
    let mut live = live;
    live.key('f');
    live.key('v');
    live.key('p');
    let (all, _) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(all.contains("activity · all repositories") && all.contains("PAUSED +0"), "{all}");
    assert!(
        all.contains("cx      prompt") || all.contains("cx prompt"),
        "the codex session from another repository:\n{all}"
    );
    assert!(!all.contains("nothing to say"));
}

#[test]
fn no_color_spells_aspects_out() {
    let d = tempfile::tempdir().unwrap();
    let paths = fixture(d.path());
    let live = live_from(&paths);
    let h = health();
    let c = ctx(paths, false);
    let (text, buf) = render(80, 24, |f| view::draw_live(f, f.area(), &live, Some(&h), &c, true));
    assert!(text.contains("→ lookup-incident [G]") && text.contains("→ dead-end-npm-test [R]"), "{text}");
    assert!(text.contains("+ adage 0.1.0") && text.contains("! no model"), "{text}");
    for y in 0..buf.area.height {
        for x in 0..buf.area.width {
            assert!(matches!(buf[(x, y)].fg, Color::Reset), "no colour at {x},{y}");
        }
    }
}

#[test]
fn empty_home_says_what_to_do() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let live = live_from(&c.paths);
    let (text, _) = render(60, 20, |f| view::draw_live(f, f.area(), &live, None, &c, true));
    assert!(
        text.contains("checking health")
            && text.contains("No hook activity yet")
            && text.contains("Nothing learned yet"),
        "{text}"
    );
    let (tiny, _) = render(25, 6, |f| view::draw_live(f, f.area(), &live, None, &c, true));
    assert!(tiny.contains("larger"), "{tiny}");
}

fn find(buf: &ratatui::buffer::Buffer, needle: &str) -> Option<(u16, u16)> {
    for y in 0..buf.area.height {
        let row: String = (0..buf.area.width).map(|x| buf[(x, y)].symbol().to_string()).collect();
        if let Some(i) = row.find(needle) {
            return Some((row[..i].chars().count() as u16, y));
        }
    }
    None
}

fn route_row(id: &str, aspect: Aspect, verbs: Vec<&'static str>, reasons: Vec<(String, String)>) -> RouteRow {
    RouteRow {
        reference: "harbor-dispatch/incidents".into(),
        track: "harbor-dispatch/incidents".into(),
        version: "0.3.0".into(),
        repo: "harbor-dispatch".into(),
        imported: false,
        in_index: true,
        id: id.into(),
        aspect,
        ceiling: "green".into(),
        effect: "observe".into(),
        state: "active".into(),
        authored: false,
        trigger: "a prompt names a ticket".into(),
        step: "run `bin/fetch-incident {item}`".into(),
        support: 24,
        opportunities: 25,
        rate: 0.96,
        lower_bound: 0.8,
        live: LiveCounts { emitted: 3, followed: 2, derailed: 1, met: 0, would_have: 1, unaided: 1, not_reached: 0 },
        directive: "<adage-signal track=\"harbor-dispatch/incidents@0.3.0\" route=\"lookup-incident\" aspect=\"green\">\nIncidents are looked up with: `bin/fetch-incident INC-12`\n</adage-signal>".into(),
        verbs,
        reasons,
    }
}

fn app_state() -> AppState {
    let mut s = AppState::new("/home/me/.local/share/adage/config.toml".into());
    s.health = Some(health());
    s.routes = Some(Ok(Routes {
        rows: vec![
            route_row(
                "lookup-incident",
                Aspect::Green,
                vec!["approve", "reject", "demote", "pin"],
                vec![("new".into(), "learned by `adage learn` and never reviewed".into())],
            ),
            route_row("read-linked-files", Aspect::Yellow, vec!["approve", "reject", "promote", "pin"], vec![]),
        ],
    }));
    s
}

#[test]
fn app_frames_routes_health_settings() {
    let d = tempfile::tempdir().unwrap();
    let c = ctx(Paths::at(d.path()), true);
    let mut s = app_state();
    s.settings[0].value = Some("true".into());
    for (tab, name) in [(Tab::Routes, "app-routes"), (Tab::Health, "app-health"), (Tab::Settings, "app-settings")] {
        s.tab = tab;
        let (text, _) = render(110, 30, |f| app::draw(f, &s, &c));
        snap(name, &text);
    }
    // The routes tab stacks list and detail on a narrow screen.
    s.tab = Tab::Routes;
    let (narrow, _) = render(70, 30, |f| app::draw(f, &s, &c));
    assert!(narrow.contains("lookup-incident") && narrow.contains("Wilson lb 0.80"), "{narrow}");
}

#[test]
fn review_keys_confirm_and_respect_the_verbs_that_apply() {
    let mut s = app_state();
    let none = KeyModifiers::NONE;
    assert!(matches!(s.key(KeyCode::Char('2'), none), Some(Cmd::Want(app::WantKind::Routes))));
    assert_eq!(s.tab, Tab::Routes);
    // Promote is not open to a green route: a message, no command.
    assert!(s.key(KeyCode::Char('p'), none).is_none());
    assert!(matches!(&s.modal, Some(Modal::Message { error: true, .. })));
    assert!(s.key(KeyCode::Esc, none).is_none() && s.modal.is_none());
    // Approve asks first; n cancels, y confirms with the same arguments `adage review` takes.
    assert!(s.key(KeyCode::Char('a'), none).is_none());
    assert!(matches!(&s.modal, Some(Modal::Confirm { .. })));
    assert!(s.key(KeyCode::Char('n'), none).is_none() && s.modal.is_none());
    s.key(KeyCode::Down, none);
    s.key(KeyCode::Char('p'), none);
    match s.key(KeyCode::Char('y'), none) {
        Some(Cmd::Do(app::Action::Review { verb, reference, route })) => {
            assert_eq!(
                (verb, reference.as_str(), route.as_str()),
                ("promote", "harbor-dispatch/incidents", "read-linked-files")
            )
        }
        _ => panic!("expected a review command"),
    }
    // Health: Enter on a fixable check starts its fix; install asks for a scope first.
    s.key(KeyCode::Char('3'), none);
    s.check_sel = 4;
    assert!(s.key(KeyCode::Enter, none).is_none());
    assert!(matches!(&s.modal, Some(Modal::Scope { harness }) if harness == "codex"));
    assert!(matches!(s.key(KeyCode::Char('u'), none), Some(Cmd::PlanInstall { scope: "user", .. })));
    // Settings: a boolean toggles; a number opens the editor, which validates.
    s.key(KeyCode::Char('4'), none);
    assert!(matches!(s.key(KeyCode::Enter, none), Some(Cmd::Do(app::Action::SetConfig { key: "learn.auto", .. }))));
    s.key(KeyCode::Down, none);
    assert!(s.key(KeyCode::Enter, none).is_none());
    for ch in "12".chars() {
        s.key(KeyCode::Char(ch), none);
    }
    assert!(s.key(KeyCode::Enter, none).is_none());
    assert!(matches!(&s.modal, Some(Modal::Edit { error: Some(e), .. }) if e.contains("between 60")));
    s.key(KeyCode::Char('0'), none);
    assert!(matches!(
        s.key(KeyCode::Enter, none),
        Some(Cmd::Do(app::Action::SetConfig { key: "learn.min_interval_secs", .. }))
    ));
    assert!(matches!(s.key(KeyCode::Char('q'), none), Some(Cmd::Quit)));
}

#[test]
fn install_confirmation_shows_the_file_and_diff() {
    let p = crate::install::Plan {
        harness: "claude-code".into(),
        target: "/work/r/.claude/settings.local.json".into(),
        before: Some("{\n  \"model\": \"x\"\n}\n".into()),
        after: "{\n  \"hooks\": {},\n  \"model\": \"x\"\n}\n".into(),
    };
    let lines = app::install_confirm(&p, &Theme { color: false });
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
    let none = KeyModifiers::NONE;
    assert!(matches!(w.key(KeyCode::Enter, none), Some(wizard::Cmd::Replan)));
    assert!(matches!(w.key(KeyCode::Tab, none), Some(wizard::Cmd::Replan)));
    assert_eq!(w.scope, "user");
    assert!(w.key(KeyCode::Char('y'), none).is_none(), "nothing to install until planned");
    assert!(w.key(KeyCode::Char('n'), none).is_none());
    assert_eq!(*w.current(), wizard::Kind::Model);
    assert!(matches!(w.key(KeyCode::Char('y'), none), Some(wizard::Cmd::FetchModel)));
    w.key(KeyCode::Char('n'), none);
    assert!(matches!(w.key(KeyCode::Char('y'), none), Some(wizard::Cmd::Learn)));
    // The learn ran and showed its output; any answer moves on.
    w.steps[w.cur].status = wizard::StepStatus::Done("12 sessions read".into());
    w.output = vec!["adage learn: 12 sessions read".into()];
    w.key(KeyCode::Enter, none);
    assert!(matches!(w.key(KeyCode::Char('y'), none), Some(wizard::Cmd::AutoLearn)));
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
    assert!(matches!(w.key(KeyCode::Enter, none), Some(wizard::Cmd::Quit)));
}

#[test]
fn clock_times_use_the_offset() {
    assert_eq!(view::clock(T0, 0, true), "12:04:00");
    assert_eq!(view::clock(T0, -7 * 3600, false), "05:04");
    assert_eq!(view::clock(T0, 10 * 3600 + 1800, true), "22:34:00");
}
