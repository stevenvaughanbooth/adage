//! T-D1-07: the simulator's history, with the plan's deliberately sub-support sequence (in 2 of
//! the 12 Claude Code sessions the agent runs `make lint` right after `make test`,
//! `adage_sim::scenario::LINT_SESSIONS`), learned with
//! `adage condition --demo`: `candidates.json` holds the `make test → make lint` candidate (support
//! 2, `s_min` 3), the index carries it apart from the routes, and `adage status --json` lists it
//! under `candidates`. Every run uses an isolated `ADAGE_HOME` and `HOME`.

use adage_sim::scenario::{self, Act};
use adage_sim::{claude, CLAUDE_SEED};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

mod common;

fn adage(root: &Path, args: &[&str]) -> (String, String, i32) {
    let o = common::scrub(&mut Command::new(env!("CARGO_BIN_EXE_adage")))
        .args(args)
        .current_dir(root)
        .env("ADAGE_HOME", root.join("adage"))
        .env("ADAGE_AUTO_LEARN", "0")
        .env("ADAGE_ARM", "treated")
        .env("HOME", root.join("home"))
        .env("XDG_CONFIG_HOME", root.join("home/.config"))
        .output()
        .unwrap();
    (
        String::from_utf8_lossy(&o.stdout).into(),
        String::from_utf8_lossy(&o.stderr).into(),
        o.status.code().unwrap_or(-1),
    )
}

/// The Claude Code history of the simulator's scenario, whose first two work-item sessions run
/// `make lint` right after the last `make test`.
fn history(dir: &Path) -> Vec<PathBuf> {
    let sessions = scenario::history(CLAUDE_SEED);
    let lints = |s: &scenario::Session| {
        s.turns.iter().flat_map(|t| &t.calls).any(|c| matches!(&c.act, Act::Shell { cmd, .. } if cmd == "make lint"))
    };
    assert_eq!(sessions.iter().filter(|s| lints(s)).count(), scenario::LINT_SESSIONS, "2 of the 12");
    assert!(sessions.iter().take(scenario::LINT_SESSIONS).all(lints));
    sessions
        .iter()
        .map(|s| {
            let p = dir.join(format!("{}.jsonl", claude::session_id(s)));
            std::fs::write(&p, claude::transcript(s, scenario::CWD)).unwrap();
            p
        })
        .collect()
}

#[test]
fn the_sims_sub_support_sequence_is_a_candidate() {
    adage_sim::assume_no_checkout_at_cwd();
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    for sub in ["home", "adage", "history"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    let files = history(&root.join("history"));
    assert_eq!(files.len(), 12);
    let mut args = vec!["condition", "--demo", "--json", "--transcripts"];
    let names: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    args.extend(names.iter().map(String::as_str));
    let (out, err, code) = adage(&root, &args);
    assert_eq!(code, 0, "{err}");
    let rep: Value = serde_json::from_str(&out).unwrap();
    let repo = rep["repos"].as_object().unwrap().keys().next().unwrap().clone();

    let file: Value = serde_json::from_slice(
        &std::fs::read(adage_tracks::candidates::path(&root.join("adage/tracks"), &repo)).expect("candidates.json"),
    )
    .unwrap();
    assert_eq!(file["schema"], "adage-candidates/1");
    assert_eq!(file["repo"], repo.as_str());
    assert_eq!(file["s_min"], 3, "the demo preset");
    let lint: Vec<&Value> =
        file["candidates"].as_array().unwrap().iter().filter(|c| c["step"]["command"] == "make lint").collect();
    assert_eq!(lint.len(), 1, "one candidate for the habit: {file:#}");
    let c = lint[0];
    assert_eq!((c["support"].as_u64(), c["review"].as_bool()), (Some(2), Some(false)), "{c:#}");
    let items = c["trigger"]["items"].as_array().unwrap();
    assert!((2..=3).contains(&items.len()), "a sequence: {c:#}");
    assert_eq!(items.last().unwrap()["keys"][0], "exec make test", "make test, then make lint: {c:#}");
    let id = c["id"].as_str().unwrap();

    // Never a route; the routes do not direct it.
    let index: Value = serde_json::from_slice(&std::fs::read(root.join("adage/index.json")).unwrap()).unwrap();
    let routes = index["routes"].as_array().unwrap();
    assert!(routes.iter().all(|r| r["route"]["id"] != id && r["route"]["step"]["command"] != "make lint"));
    assert!(index["candidates"][&repo].as_array().unwrap().iter().any(|c| c["id"] == id));
    assert!(index["candidate_contexts"][&repo].is_object());

    // `adage status --json` lists it under `candidates`, in the reflex shape.
    let (out, err, code) = adage(&root, &["status", "--json"]);
    assert_eq!(code, 0, "{err}");
    let status: Value = serde_json::from_str(&out).unwrap();
    let listed = status["candidates"].as_array().expect("candidates").iter().find(|x| x["id"] == id).expect("listed");
    assert_eq!(listed["repo"], repo.as_str());
    assert_eq!(listed["state"], "candidate");
    assert!(listed["directive"].is_null(), "counted, never delivered");
    assert_eq!(listed["shape"], "run");
    assert_eq!(listed["candidate"]["support"], 2);
    assert_eq!(listed["candidate"]["s_min"], 3);
    assert_eq!(listed["candidate"]["review"], false);
    assert!(listed["next"].is_null());
    assert!(listed["gist"].as_str().unwrap().contains("make lint"), "{listed:#}");
}
