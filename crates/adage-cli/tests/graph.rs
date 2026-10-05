//! `adage graph` through the binary: counts come from real hook decisions, split by arm, so a
//! holdout session (nothing delivered) is never counted as a derail.

use adage_core::spans::SpanKind;
use adage_core::Effect;
use adage_tracks::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn track() -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/work".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "lookup".into(),
            summary: "lookup".into(),
            trigger: Trigger::Prompt { span: SpanKind::Ticket },
            step: Step::Run { command: "./scripts/wi-lookup {item}".into() },
            slots: vec![Slot {
                name: "item".into(),
                source: SlotSource::Prompt { span: SpanKind::Ticket },
                contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
            }],
            effect: Effect::Observe,
            aspect: Aspect::Green,
            state: RouteState::Active,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 7, opportunities: 8, provenance: "desire-path".into(), ..Default::default() },
            authored: false,
        }],
    }
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "adage/tracks", "acme/.git"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        io::write(&root.join("adage/tracks/acme/work"), &track()).unwrap();
        Index::compile(&[track()]).write_atomic(&root.join("adage/index.json")).unwrap();
        World { _dir: dir, root }
    }
    fn run(&self, args: &[&str], stdin: &Value, arm: &str) -> String {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(&self.root)
            .env("ADAGE_HOME", self.root.join("adage"))
            .env("ADAGE_AUTO_LEARN", "0")
            .env("HOME", self.root.join("home"))
            .env("ADAGE_ARM", arm)
            .env_remove("ADAGE_DISABLE")
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
    w.session("s-holdout", "holdout", false);
    let g: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    assert_eq!(g["sessions"], json!({ "total": 2, "treated": 1, "holdout": 1 }));
    let r = &g["routes"][0];
    assert_eq!(r["id"], "lookup");
    assert_eq!(r["trigger"]["kind"], "prompt");
    assert!(r["directive"].as_str().unwrap().contains("wi-lookup W-"), "{}", r["directive"]);
    assert_eq!(r["live"]["emitted"], 1, "delivered once, in the treated session");
    assert_eq!(r["live"]["followed"], 1);
    assert_eq!(r["live"]["derailed"], 0, "the holdout session never saw it, so it cannot have derailed");
    assert_eq!(r["live"]["holdout"]["not_reached"], 1);
    assert_eq!(r["arms"]["treated"], json!({ "sessions": 1, "applied": 1, "followed": 1, "met": 0, "hit_avoided": 0 }));
    assert_eq!(r["arms"]["holdout"], json!({ "sessions": 1, "applied": 1, "followed": 0, "met": 0, "hit_avoided": 0 }));
    let railed = g["observed"]
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["route"] == "lookup")
        .expect("the lookup command is on rails");
    assert_eq!(railed["how"], "directs");
    let kinds: Vec<&str> = g["events"].as_array().unwrap().iter().filter_map(|e| e["arm"].as_str()).collect();
    assert!(kinds.contains(&"treated") && kinds.contains(&"holdout"));
    let report = w.run(&["report"], &json!({}), "treated");
    assert!(report.contains("Guidance delivered: 1"), "{report}");
    assert!(report.contains("With guidance: 1 followed · 0 not observed"), "{report}");
    assert!(report.contains("Control: 1 opportunities withheld · 0 followed unaided · 1 not observed"), "{report}");
    assert!(report.contains("next 3 observed tool batches"), "{report}");
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

#[test]
fn traces_include_quiet_boundaries_and_report_truncation_without_cached_state() {
    let w = World::new();
    w.session("trace-session", "treated", true);
    // A recorded quiet boundary must survive graph rendering, even though current hook fast
    // paths can skip recording Stop when nothing is pending. Older/versioned spools can hold it.
    let spool = w.root.join("adage/sessions/trace-session.spool");
    let records: Vec<Value> = adage_store::spool::read_all(&spool).unwrap();
    let total = records.len() + 1;
    adage_store::spool::append(
        &spool,
        &json!({
            "ts": records.last().unwrap()["ts"].as_u64().unwrap() + 1,
            "arm": "treated", "repo": "acme", "event": "Stop", "kind": "stop",
            "decision": { "directives": [], "would_have": [], "judgements": [], "observed": [] },
        }),
    )
    .unwrap();
    std::fs::remove_file(w.root.join("adage/sessions/trace-session.state")).unwrap();
    let g: Value = serde_json::from_str(&w.run(&["graph", "--json", "--events", "2"], &json!({}), "treated")).unwrap();
    let trace = g["trace_events"].as_array().unwrap();
    assert_eq!(trace.len(), 2);
    assert_eq!(trace[1]["event"], "Stop", "quiet stop is part of the recorded sequence");
    assert_eq!(trace[1]["sequence"], total);
    assert_eq!(trace[1]["harness"], "claude");
    assert_eq!(trace[1]["repo"], "acme", "the spool preserves repository identity without cached state");
    assert_eq!(g["trace_info"]["events_total"], total);
    assert_eq!(g["trace_info"]["events_visible"], 2);
    assert_eq!(g["trace_info"]["truncated"], true);
    assert_eq!(g["trace_sessions"][0]["events_total"], total);
    assert_eq!(g["trace_sessions"][0]["events_visible"], 2);
    assert_eq!(g["trace_sessions"][0]["truncated"], true);
    assert_eq!(g["routes"][0]["live"]["emitted"], 1);
    assert!(g["trace_info"]["description"].as_str().unwrap().contains("private agent reasoning are not recorded"));
    assert!(trace.iter().all(|event| event.get("prompt").is_none() && event.get("thinking").is_none()));
}

#[test]
fn graph_demo_is_reviewable_and_does_not_touch_the_requested_home() {
    let dir = tempfile::tempdir().unwrap();
    let real_home = dir.path().join("adage");
    let output = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["graph", "--demo", "--json", "--events", "1000"])
        .env("ADAGE_HOME", &real_home)
        .env("HOME", dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let graph: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(graph["source"]["kind"], "demo");
    assert_eq!(graph["sessions"]["total"], 3);
    assert_eq!(graph["trace_sessions"].as_array().unwrap().len(), 3);
    assert!(!graph["trace_events"].as_array().unwrap().is_empty());
    assert_eq!(graph["trace_info"]["truncated"], false);
    assert!(graph["trace_events"]
        .as_array()
        .unwrap()
        .iter()
        .any(|event| !event["directives"].as_array().unwrap().is_empty()));
    assert!(!real_home.exists(), "demo never initializes the user's data directory");
    assert!(!std::path::Path::new(graph["home"].as_str().unwrap()).exists(), "snapshot cleans its temporary home");
}

#[test]
fn traces_split_repository_changes_and_keep_tied_steps_in_recorded_order() {
    let w = World::new();
    let spool = w.root.join("adage/sessions/cross-repo.spool");
    for repo in ["beta", "alpha", "beta"] {
        adage_store::spool::append(
            &spool,
            &json!({
                "ts": 1000, "arm": "treated", "repo": repo, "event": "PostToolBatch", "kind": "batch",
                "decision": { "directives": [], "would_have": [], "judgements": [], "observed": ["exec ls"] },
            }),
        )
        .unwrap();
    }
    let graph: Value =
        serde_json::from_str(&w.run(&["graph", "--json", "--events", "2"], &json!({}), "treated")).unwrap();
    let events = graph["trace_events"].as_array().unwrap();
    assert_eq!(events.iter().map(|event| event["sequence"].as_u64().unwrap()).collect::<Vec<_>>(), vec![2, 3]);
    let summaries = graph["trace_sessions"].as_array().unwrap();
    assert_eq!(summaries.len(), 2, "every recorded repository remains selectable");
    let alpha = summaries.iter().find(|summary| summary["repo"] == "alpha").unwrap();
    let beta = summaries.iter().find(|summary| summary["repo"] == "beta").unwrap();
    assert_eq!((alpha["events_total"].as_u64(), alpha["events_visible"].as_u64()), (Some(1), Some(1)));
    assert_eq!(alpha["truncated"], false);
    assert_eq!((beta["events_total"].as_u64(), beta["events_visible"].as_u64()), (Some(2), Some(1)));
    assert_eq!(beta["truncated"], true);
    let hidden: Value =
        serde_json::from_str(&w.run(&["graph", "--json", "--events", "0"], &json!({}), "treated")).unwrap();
    assert_eq!(hidden["trace_events"], json!([]));
    assert_eq!(hidden["trace_sessions"].as_array().unwrap().len(), 2, "inventory survives a hidden event window");
    assert_eq!(hidden["trace_info"]["events_total"], 3);
    assert_eq!(hidden["trace_info"]["events_visible"], 0);
}

fn append_record(w: &World, session: &str, ts: u64, decision: Value) {
    adage_store::spool::append(
        &w.root.join(format!("adage/sessions/{session}.spool")),
        &json!({
            "ts": ts, "repo": "acme", "arm": "treated", "event": "PostToolBatch", "decision": decision,
        }),
    )
    .unwrap();
}

fn directive(track: &str, version: &str) -> Value {
    json!({ "route": "lookup", "track": track, "version": version, "aspect": "green", "text": "The recorded response" })
}

#[test]
fn measurements_use_recorded_collection_and_version_not_only_route_id() {
    let w = World::new();
    let mut imported = track();
    imported.manifest.track.name = "shared/work".into();
    imported.manifest.track.provenance = "imported".into();
    io::write(&w.root.join("adage/tracks/@imported/shared/work"), &imported).unwrap();
    append_record(
        &w,
        "old-version",
        1,
        json!({ "directives": [directive("acme/work", "0.1.0")], "would_have": [directive("acme/work", "0.1.0")] }),
    );
    append_record(
        &w,
        "old-version",
        2,
        json!({ "judgements": [{"kind": "followed", "route": "lookup"}], "observed": ["exec ./scripts/wi-lookup {item}"] }),
    );
    let mut current = track();
    current.manifest.track.version = "0.2.0".into();
    io::write(&w.root.join("adage/tracks/acme/work"), &current).unwrap();
    append_record(&w, "new-version", 3, json!({ "directives": [directive("acme/work", "0.2.0")] }));
    let graph: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    let routes = graph["routes"].as_array().unwrap();
    let local = routes.iter().find(|route| route["track"] == "acme/work").unwrap();
    let imported = routes.iter().find(|route| route["track"] == "shared/work").unwrap();
    assert_eq!(graph["totals"]["emitted"], 2);
    assert_eq!(graph["totals"]["would_have"], 0, "treated would-have mirrors must not count as control exposures");
    assert_eq!(graph["totals"]["followed"], 1);
    assert_eq!(local["live"]["emitted"], 1);
    assert_eq!(local["live"]["followed"], 0, "old outcomes cannot attach to changed reflexes");
    assert_eq!(imported["live"]["emitted"], 0, "same route id in another collection is not a match");
    assert_eq!(imported["measurement_history"], json!([]));
    let historical = &local["measurement_history"][0];
    assert_eq!(historical["version"], "0.1.0");
    assert_eq!(historical["live"]["emitted"], 1);
    assert_eq!(historical["live"]["followed"], 1);
    assert_eq!(historical["measurement_sessions"][0]["session"], "old-version");
    assert_eq!(historical["measurement_sessions"][0]["applied"], json!([1]));
    assert_eq!(historical["measurement_sessions"][0]["followed"], json!([2]));
    let trace: Value = serde_json::from_str(&w.run(
        &["graph", "--json", "--session", "old-version", "--repo", "acme"],
        &json!({}),
        "treated",
    ))
    .unwrap();
    assert_eq!(trace["events"][1]["judgements"][0]["version"], "0.1.0");
    assert_eq!(trace["events"][1]["judgements"][0]["exposure_sequence"], 1);
    assert_eq!(trace["events"][1]["judgements"][0]["attribution"], "historical");
    assert_eq!(trace["events"][0]["directives"][0]["text"], "The recorded response");
}

#[test]
fn duplicate_installations_and_legacy_records_never_inflate_measurements() {
    let w = World::new();
    io::write(&w.root.join("adage/tracks/@imported/acme/work"), &track()).unwrap();
    append_record(&w, "ambiguous", 1, json!({ "directives": [directive("acme/work", "0.1.0")] }));
    append_record(&w, "ambiguous", 2, json!({ "judgements": [{"kind": "followed", "route": "lookup"}] }));
    append_record(&w, "legacy", 3, json!({ "directives": [{"route": "lookup", "text": "Legacy response"}] }));
    append_record(&w, "legacy", 4, json!({ "judgements": [{"kind": "derailed", "route": "lookup"}] }));
    let graph: Value = serde_json::from_str(&w.run(&["graph", "--json"], &json!({}), "treated")).unwrap();
    assert_eq!(graph["routes"].as_array().unwrap().len(), 2);
    for route in graph["routes"].as_array().unwrap() {
        assert_eq!(route["live"]["emitted"], 0);
        assert_eq!(route["live"]["followed"], 0);
        assert_eq!(route["live"]["derailed"], 0);
        assert_eq!(route["measurement_info"]["status"], "ambiguous");
        assert_eq!(route["measurement_sessions"], json!([]));
    }
    assert_eq!(graph["totals"]["emitted"], 2);
    assert_eq!(graph["totals"]["followed"], 1);
    assert_eq!(graph["totals"]["derailed"], 1);
    assert_eq!(graph["attribution"]["exposures"]["ambiguous"], 1);
    assert_eq!(graph["attribution"]["exposures"]["legacy"], 1);
    assert_eq!(graph["attribution"]["judgements"]["ambiguous"], 1);
    assert_eq!(graph["attribution"]["judgements"]["legacy"], 1);
}

#[test]
fn all_sessions_remain_selectable_outside_the_recent_event_window() {
    let w = World::new();
    for ts in 0..450 {
        append_record(&w, "old-session", ts, json!({ "observed": ["exec ls"] }));
    }
    for ts in 450..1050 {
        append_record(&w, "new-session", ts, json!({ "observed": ["exec pwd"] }));
    }
    let graph: Value =
        serde_json::from_str(&w.run(&["graph", "--json", "--events", "400"], &json!({}), "treated")).unwrap();
    let old = graph["trace_sessions"].as_array().unwrap().iter().find(|s| s["session"] == "old-session").unwrap();
    assert_eq!(old["events_total"], 450);
    assert_eq!(old["events_visible"], 0);
    let trace: Value = serde_json::from_str(&w.run(
        &["graph", "--json", "--session", "old-session", "--repo", "acme"],
        &json!({}),
        "treated",
    ))
    .unwrap();
    assert_eq!(trace["complete"], true);
    assert_eq!(trace["events_total"], 450);
    assert_eq!(trace["events"][0]["sequence"], 1);
    assert_eq!(trace["events"][449]["sequence"], 450);
}
