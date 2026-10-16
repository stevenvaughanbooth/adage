//! A Rust workspace's conventions form from traces like the ones agents leave (Track G, the
//! dogfood gate): `fixtures/rust-workspace.json` is distilled from real Claude Code traces of
//! engineers building a Rust workspace, anonymised (invented crates, prompts and paths). Agents
//! narrow their test runs while they work (`cargo test -p <crate>`), chain the finishing steps with
//! `&&`, add flags (`-j4`, `--no-fail-fast`), check formatting after a late fix, and also edit
//! docs, or only read. What a developer would expect learned:
//!
//! - after editing the crates' Rust files, `cargo fmt --all` is owed (an obligation);
//! - `cargo clippy --workspace --all-targets --locked -- -D warnings` and/or
//!   `cargo test --workspace --locked` follow as checks (prime: runs, or a sequence after the format step);
//! - nothing is learned for docs edits (the checks after a docs edit are explained by the Rust
//!   edits in the same traces), and `cargo fmt --all --check` is not directed beside the
//!   obligation it checks.

use adage_learn::{learn, Inputs, LearnConfig};
use adage_tracks::{io, shape, Step, Strength, Trigger};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const FIXTURE: &str = include_str!("fixtures/rust-workspace.json");

/// One fixture session as a Claude Code transcript in the real on-disk format.
fn transcript(dir: &Path, n: usize, s: &Value) -> PathBuf {
    let sid = format!("rw-{n:02}");
    let cwd = "/fixture/ledger";
    let day = s["day"].as_u64().unwrap();
    let ts = format!("2026-09-{day:02}T10:00:00Z");
    let mut lines = vec![json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":ts,
        "message":{"role":"user","content":s["prompt"]}})];
    for (i, st) in s["steps"].as_array().unwrap().iter().enumerate() {
        let st = st.as_array().unwrap();
        let id = format!("toolu_{i}");
        let (name, input, out, err) = match st[0].as_str().unwrap() {
            "bash" => {
                ("Bash", json!({"command": st[1]}), st[2].as_str().unwrap().to_string(), st[3].as_bool().unwrap())
            }
            "read" => {
                ("Read", json!({"file_path": format!("{cwd}/{}", st[1].as_str().unwrap())}), "code".into(), false)
            }
            "edit" => (
                "Edit",
                json!({"file_path": format!("{cwd}/{}", st[1].as_str().unwrap()), "old_string": "a", "new_string": "b"}),
                "ok".into(),
                false,
            ),
            other => panic!("unknown step {other}"),
        };
        lines.push(json!({"type":"assistant","sessionId":sid,"cwd":cwd,"message":{"id":format!("msg_{i}"),"model":"claude-test",
            "content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"message":{"role":"user",
            "content":[{"type":"tool_result","tool_use_id":id,"content":out,"is_error":err}]}}));
    }
    let p = dir.join(format!("{sid}.jsonl"));
    std::fs::write(&p, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
    p
}

#[test]
fn the_rust_workspace_reflexes_form() {
    let fixture: Value = serde_json::from_str(FIXTURE).unwrap();
    let d = tempfile::tempdir().unwrap();
    let files: Vec<PathBuf> =
        fixture["sessions"].as_array().unwrap().iter().enumerate().map(|(n, s)| transcript(d.path(), n, s)).collect();
    let root = d.path().join("tracks");
    let index = d.path().join("index.json");
    let inputs = Inputs {
        identities: Default::default(),
        transcripts: files,
        outcomes: Default::default(),
        repo_override: Some("ledger".into()),
        models: None,
        cache: None,
        only_repo: None,
    };
    learn(&inputs, &root, &index, &LearnConfig::default()).unwrap();
    let routes: Vec<adage_tracks::Route> = io::read_all(&root).unwrap().into_iter().flat_map(|t| t.routes).collect();
    let listing: Vec<String> =
        routes.iter().map(|r| format!("{} {:?} {:?} {:?}", r.id, r.trigger, r.step, r.strength)).collect();
    let rust_edit = |r: &adage_tracks::Route| matches!(&r.trigger, Trigger::AfterEdit { paths } if paths.starts_with("crates/") && paths.ends_with("*.rs"));
    let directs = |r: &adage_tracks::Route, c: &str| matches!(&r.step, Step::Run { command } if command == c);

    // The finishing format step is owed after Rust edits: an obligation.
    let fmt = routes.iter().find(|r| rust_edit(r) && directs(r, "cargo fmt --all"));
    let fmt = fmt.unwrap_or_else(|| panic!("no cargo fmt obligation; learned:\n{}", listing.join("\n")));
    assert_eq!(shape(fmt), "obligation", "{}", listing.join("\n"));

    // The checks follow, at prime: a repo-wide form agents ran, though most runs were narrowed or
    // carried extra flags, as a run of its own or as the later steps of a sequence that starts
    // with the format step (agents here always ran them, in that order).
    let checks = ["cargo clippy --workspace --all-targets --locked -- -D warnings", "cargo test --workspace --locked"];
    let names = |r: &adage_tracks::Route| match &r.step {
        Step::Run { command } => checks.contains(&command.as_str()),
        Step::Sequence { commands } => {
            commands[0] == "cargo fmt --all" && commands[1..].iter().any(|c| checks.contains(&c.as_str()))
        }
        _ => false,
    };
    let found: Vec<&adage_tracks::Route> = routes.iter().filter(|r| rust_edit(r) && names(r)).collect();
    assert!(!found.is_empty(), "no check after Rust edits; learned:\n{}", listing.join("\n"));
    for r in found {
        assert!(["run", "sequence"].contains(&shape(r)));
        assert_eq!(r.strength, Strength::Prime, "a check after an edit is suggested, never fired");
    }

    // No junk: nothing after docs edits, no check of what the obligation owes, nothing narrow.
    for r in &routes {
        if let Trigger::AfterEdit { paths } = &r.trigger {
            assert!(paths.starts_with("crates/"), "a reflex on {paths}: {}", listing.join("\n"));
        }
        assert!(!directs(r, "cargo fmt --all --check"), "{}", listing.join("\n"));
        assert!(
            !adage_learn::engine::named_commands(r).iter().any(|c| c.contains(" -p ")),
            "a narrow run was directed: {}",
            listing.join("\n")
        );
    }
}
