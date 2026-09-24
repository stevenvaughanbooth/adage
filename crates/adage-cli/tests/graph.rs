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
