//! `adage graph` through the binary: counts come from real hook decisions, split by arm, so a
//! control session (nothing delivered) is never counted as an ignore.

use adage_core::Effect;
use adage_tracks::card;
use adage_tracks::testkit::{self, kinds};
use adage_tracks::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

mod common;

fn track() -> Track {
    let lookup = kinds::lookup("lookup", "./scripts/wi-lookup {item}")
        .summary("lookup")
        .until_intent_changes()
        .support(7, 8)
        .provenance("desire-path");
    testkit::track("acme/work", "acme").route(lookup).build()
}

struct World {
    _w: common::TempWorld,
    root: PathBuf,
    home: common::LearnedHome,
}

impl World {
    fn new() -> World {
        let w = common::TempWorld::new(&["home", "acme/.git"]);
        let world = World { root: w.root.clone(), home: w.home("adage"), _w: w };
        world.install(&track());
        world
    }
    /// `t` as the learned track `acme/work`, and the index compiled from it alone.
    fn install(&self, t: &Track) {
        self.home.install("acme/work", t);
        self.home.compile(std::slice::from_ref(t));
    }
    fn run(&self, args: &[&str], stdin: &Value, arm: &str) -> String {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        common::scrub(&mut c);
        c.args(args)
            .current_dir(&self.root)
            .env("ADAGE_HOME", self.root.join("adage"))
            .env("ADAGE_AUTO_LEARN", "0")
            .env("HOME", self.root.join("home"))
            .env("ADAGE_ARM", arm)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = c.spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.to_string().as_bytes());
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).to_string()
    }
    fn session(&self, id: &str, arm: &str, looks_up: bool) {
        let cwd = self.root.join("acme").to_string_lossy().to_string();
        let base =
            |ev: &str| json!({ "session_id": id, "transcript_path": "/dev/null", "cwd": cwd, "hook_event_name": ev });
        let mut p = base("UserPromptSubmit");
        p["prompt"] = json!("Pick up W-2042 please");
        self.run(&["hook", "claude", "UserPromptSubmit"], &p, arm);
        let mut b = base("PostToolBatch");
        let cmd = if looks_up { "./scripts/wi-lookup W-2042" } else { "ls" };
        b["tool_calls"] = json!([{ "tool_name": "Bash", "tool_input": { "command": cmd }, "tool_response": "ok" }]);
        for _ in 0..4 {
            self.run(&["hook", "claude", "PostToolBatch"], &b, arm);
        }
    }
}

#[test]
fn graph_splits_judgements_by_arm() {
    let w = World::new();
    w.session("s-treated", "treated", true);
    w.session("s-control", "control", false);
    let g: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    assert_eq!(g["sessions"], json!({ "total": 2, "treated": 1, "control": 1 }));
    let r = &g["routes"][0];
    assert_eq!(r["id"], "lookup");
    assert_eq!(r["trigger"]["kind"], "prompt");
    assert!(r["directive"].as_str().unwrap().contains("wi-lookup W-"), "{}", r["directive"]);
    assert_eq!(r["live"]["emitted"], 1, "delivered once, in the treated session");
    assert_eq!(r["live"]["followed"], 1);
    assert_eq!(r["live"]["ignored"], 0, "the control session never saw it, so it cannot have ignored");
    assert_eq!(r["live"]["control"]["not_reached"], 1);
    assert_eq!(r["arms"]["treated"], json!({ "sessions": 1, "applied": 1, "followed": 1, "met": 0, "hit_avoided": 0 }));
    assert_eq!(r["arms"]["control"], json!({ "sessions": 1, "applied": 1, "followed": 0, "met": 0, "hit_avoided": 0 }));
    let railed = g["observed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["route"] == "lookup")
        .expect("a reflex directs the lookup command");
    assert_eq!(railed["how"], "directs");
    let kinds: Vec<&str> = g["events"].as_array().unwrap().iter().filter_map(|e| e["arm"].as_str()).collect();
    assert!(kinds.contains(&"treated") && kinds.contains(&"control"));
}

#[test]
fn graph_on_an_empty_home_is_empty_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    let o = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["graph", "--json"])
        .env("ADAGE_HOME", dir.path())
        .env("ADAGE_AUTO_LEARN", "0")
        .output()
        .unwrap();
    assert!(o.status.success());
    let g: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(g["routes"], json!([]));
    assert_eq!(g["sessions"]["total"], 0);
}

/// A reflex kept quiet on its uplift, a procedure card, an ordered sequence and a context
/// trigger: the graph carries what the map shows for each, and the repository's savings
/// estimate, rewrite counts and the rewrite switch.
#[test]
fn graph_carries_quiet_cards_sequences_savings_and_rewrites() {
    let w = World::new();
    let mut t = track();
    let base = t.routes[0].clone();
    t.routes.push(Route {
        id: "quiet-lint".into(),
        summary: "agents already lint".into(),
        trigger: Trigger::AfterEdit { paths: "src/*.py".into() },
        step: Step::Run { command: "make lint".into() },
        slots: vec![],
        effect: Effect::Verify,
        strength: Strength::Prime,
        state: RouteState::Nascent,
        evidence: Evidence {
            support: 9,
            opportunities: 10,
            provenance: "desire-path".into(),
            uplift: Some(Uplift {
                kind: "run-after-edit".into(),
                p_follow: 0.95,
                p_self: 0.9,
                mean: 0.05,
                lower: -0.02,
                prior_self: 0.6,
                traces: 10,
                traces_took: 9,
                gated: true,
                ..Default::default()
            }),
            ..Default::default()
        },
        ..base.clone()
    });
    t.routes.push(Route {
        id: "card-add-flag".into(),
        summary: "adding a flag".into(),
        trigger: Trigger::Intent {
            family: "add-flag".into(),
            model: "potion-base-8m@b9c0e3a1b2c3".into(),
            centroid: vec![3536; 8],
            threshold: 0.811,
        },
        step: Step::Card {
            card: card::Card {
                sessions: 4,
                passed: 3,
                files: vec![
                    card::CardFile { path: "flags/flag_<x>.go".into(), sessions: 4 },
                    card::CardFile { path: "docs/flags.md".into(), sessions: 3 },
                ],
                exemplars: vec![],
                commands: vec![card::CardCommand {
                    command: "go test ./flags".into(),
                    sessions: 4,
                    runs: 5,
                    failed: 1,
                    error: String::new(),
                    verified: Some(true),
                }],
                avoid: vec![],
                finish: Some("go vet ./...".into()),
                prompts: vec![],
            },
        },
        slots: vec![],
        effect: Effect::Observe,
        strength: Strength::Prime,
        state: RouteState::Firm,
        ..base.clone()
    });
    t.routes.push(Route {
        id: "seq-gen-test".into(),
        summary: "generate, then test".into(),
        trigger: Trigger::AfterEdit { paths: "schema/*.json".into() },
        step: Step::Sequence { commands: vec!["make gen".into(), "make test".into()] },
        slots: vec![],
        effect: Effect::Regenerate,
        ..base.clone()
    });
    w.install(&t);
    io::read(&w.root.join("adage/tracks/acme/work")).expect("the track reads back");
    w.session("s-treated", "treated", true);
    // A PreToolUse rewrite, and one withheld in a control session.
    let spool = w.root.join("adage/sessions/s-treated.spool");
    assert!(spool.exists(), "the hook spooled the session");
    for (arm, withheld) in [("treated", false), ("control", true)] {
        let mut r = json!({ "ts": 9, "event": "PreToolUse", "kind": "pretool", "arm": arm, "repo": "acme",
                            "pretool": { "rewrite": "test", "interpreter": ".venv/bin/python" } });
        if withheld {
            r["withheld"] = json!(true);
        }
        adage_store::spool::append(&spool, &r).unwrap();
    }

    let g: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    let route = |id: &str| g["routes"].as_array().unwrap().iter().find(|r| r["id"] == id).cloned().expect(id);

    let q = route("quiet-lint");
    assert_eq!(q["quiet"], true);
    assert_eq!(q["evidence"]["uplift"]["gated"], true);
    assert!(q["evidence"]["uplift"]["line"].as_str().unwrap().contains("Quiet: agents already do this"), "{q}");
    assert!(q["review"]["reasons"].as_array().unwrap().iter().any(|r| r["kind"] == "quiet"), "{q}");
    assert_eq!(q["evidence"]["basis"], "count");
    assert!(q["evidence"]["describe"].as_str().unwrap().starts_with("9/10 traces"), "{q}");
    assert_eq!(g["bars"]["uplift_min"], 0.1);

    let c = route("card-add-flag");
    assert_eq!(c["shape"], "card");
    assert_eq!(c["quiet"], false);
    assert_eq!(c["trigger"]["threshold"], 0.811);
    let card = &c["step"]["card"];
    assert_eq!(card["sessions"], 4);
    assert_eq!(card["passed"], 3);
    assert_eq!(card["files"][0]["path"], "flags/flag_<x>.go");
    assert_eq!(
        card["commands"][0],
        json!({ "command": "go test ./flags", "sessions": 4, "runs": 5, "failed": 1, "error": null, "verified": true })
    );
    assert_eq!(card["finish"], "go vet ./...");
    assert_eq!(card["grey_from"], 0.711);
    assert_eq!(card["shared"], false);
    assert_eq!(c["evidence"]["basis"], "family");
    assert!(!c["review"]["verbs"].as_array().unwrap().contains(&json!("strengthen")), "a card is prime at most");

    let s = route("seq-gen-test");
    assert_eq!(s["step"]["commands"], json!(["make gen", "make test"]));
    assert_eq!(s["step"]["label"], "make gen → make test");

    // The lookup reflex is followed and delivered in the treated session; nothing misfired.
    assert_eq!(route("lookup")["live"]["misfired"], 0);
    assert!(route("lookup")["obligation"].is_null());

    // PreToolUse: one rewrite, one withheld in the control arm; rewrites are on by default.
    assert_eq!(g["pretool"], json!([{ "repo": "acme", "rewritten": 1, "advised": 0, "withheld": 1 }]));
    assert_eq!(g["runtime"]["rewrite"], json!({ "on": true, "config": true, "marker": false }));
    assert_eq!(g["runtime"]["card_threshold"], 0.811);
    let kinds: Vec<&str> = g["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e["kind"].as_str())
        .filter(|kind| *kind == "pretool")
        .collect();
    assert_eq!(kinds, ["pretool", "pretool"], "rewrites reach the timeline");

    // Savings: below the gate, counts only, never dollars.
    let sv = g["savings"].as_array().unwrap().iter().find(|s| s["repo"] == "acme").expect("acme's savings");
    assert_eq!(sv["verdict"], "measuring");
    assert_eq!(sv["gate_met"], false);
    assert!(sv.get("cost_usd").is_none(), "{sv}");
    assert_eq!(sv["need"], json!({ "control": 10, "treated": 30 }));
    assert!(sv["lines"][0].as_str().unwrap().starts_with("measuring savings: 0 of 10"), "{sv}");
    assert!(sv["guardrail"].is_null());
    assert_eq!(sv["counts"]["rewritten"], 1);
    assert!(!w.root.join("adage/ledger").exists(), "the graph writes no summary");

    // The marker `delegate off` wrote before the switch turns them off too.
    std::fs::create_dir_all(w.root.join("adage/exec")).unwrap();
    std::fs::write(w.root.join("adage/exec/rewrite-off"), "").unwrap();
    let g: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    assert_eq!(g["runtime"]["rewrite"], json!({ "on": false, "config": true, "marker": true }));
}

#[test]
fn graph_exposes_cpu_candidate_evidence_without_an_advisory_preview() {
    let w = World::new();
    let mut t = track();
    let mut r = t.routes[0].clone();
    r.id = "routine-test".into();
    r.trigger = Trigger::AfterEdit { paths: "src/*.rs".into() };
    r.step = Step::Run { command: "cargo test".into() };
    r.slots.clear();
    r.effect = Effect::Verify;
    r.strength = Strength::Prime;
    r.evidence.execution = Some(ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 });
    t.routes.push(r);
    w.install(&t);
    let g: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    let candidate = g["routes"].as_array().unwrap().iter().find(|r| r["id"] == "routine-test").unwrap();
    assert_eq!(candidate["execution_only"], true);
    assert!(candidate["directive"].is_null(), "an execution-only reflex directs nothing");
    assert_eq!(candidate["evidence"]["execution"], json!({"opportunities":5,"successful":5,"sessions":5}));
    assert_eq!(candidate["review"]["next"], "adage review delegate acme/work routine-test --level e2");
    assert!(candidate["review"]["reasons"].as_array().unwrap().iter().any(|r| r["kind"] == "execution"));
    let advisory = g["routes"].as_array().unwrap().iter().find(|r| r["id"] == "lookup").unwrap();
    assert_eq!(advisory["execution_only"], false);
    assert!(advisory["evidence"]["execution"].is_null());
    assert!(advisory["review"]["next"].is_null());
    assert!(advisory["directive"].as_str().unwrap().contains("wi-lookup"));
}

/// One reflex, one shape: `adage status --json`, `adage review --json` and the map
/// (`adage graph --json`) render the same reflex with the same fields and values
/// (`adage_app::reflex::Reflex`); each surface only adds what is its own.
#[test]
fn every_surface_renders_a_reflex_the_same_way() {
    let w = World::new();
    // Waiting for a person, so the review queue lists it too.
    let mut t = track();
    t.routes[0].state = RouteState::Nascent;
    w.install(&t);
    w.session("s-treated", "treated", true);
    let json = |args: &[&str]| -> Value { serde_json::from_str(&w.run(args, &json!({}), "treated")).unwrap() };
    let map = json(&["graph", "--json"])["routes"][0].clone();
    let status = json(&["status", "--json"])["tracks"][0]["reflexes"][0].clone();
    let review = json(&["review", "--json"])["queue"][0].clone();
    let keys = |v: &Value| -> std::collections::BTreeSet<String> { v.as_object().unwrap().keys().cloned().collect() };
    // Every field of the reflex itself, in each surface, equal.
    let shared = keys(&status);
    for k in ["id", "track", "reference", "state", "strength", "origin", "trigger", "step", "evidence", "directive"] {
        assert!(shared.contains(k), "{k} is part of every reflex");
    }
    for k in &shared {
        assert_eq!(map[k], status[k], "the map and status disagree on {k}");
        assert_eq!(review[k], status[k], "review and status disagree on {k}");
    }
    // What each surface adds, the same object where two of them carry it.
    assert_eq!(
        keys(&map).difference(&shared).cloned().collect::<Vec<_>>(),
        ["arms", "in_index", "live", "measurement_history", "measurement_info", "measurement_sessions", "review"]
    );
    assert!(keys(&review).is_superset(&["review".to_string(), "live".to_string()].into()));
    assert_eq!(review["review"]["verbs"], map["review"]["verbs"]);
    assert_eq!(review["live"], map["live"]);
}
