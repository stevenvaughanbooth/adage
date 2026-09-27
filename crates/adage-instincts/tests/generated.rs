//! Binding on the procedurally generated sandboxes (`evals/gen`, suite `smoke`), against their
//! ground truth (`conventions.json`): nothing binds for a decoy, and nothing directs the command an
//! instinct violation retires. Skipped (with a note) where python3 or git is unavailable.

use adage_tracks::{Step, Trigger};
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

fn available(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").output().is_ok_and(|o| o.status.success())
}

#[test]
fn generated_smoke_suite_binds_no_decoy_and_no_violation() {
    if !available("python3") || !available("git") {
        eprintln!("skipped: python3 and git are needed to generate the sandboxes");
        return;
    }
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out = tempfile::tempdir().unwrap();
    let suite = out.path().join("smoke");
    let st = Command::new("python3")
        .arg(root.join("evals/gen/gen.py"))
        .args(["--suite", "smoke", "--out"])
        .arg(&suite)
        .output()
        .unwrap();
    assert!(st.status.success(), "gen.py: {}", String::from_utf8_lossy(&st.stderr));

    let mut sandboxes: Vec<_> =
        std::fs::read_dir(&suite).unwrap().filter_map(|e| e.ok()).map(|e| e.path()).filter(|p| p.is_dir()).collect();
    sandboxes.sort();
    assert_eq!(sandboxes.len(), 6);
    let (mut decoys, mut violations, mut finish_lines, mut recovered) = (0, 0, 0, 0);
    let (mut dead_ends, mut dead_ends_recovered) = (0u32, 0u32);
    let table = adage_core::CommandTable::load();
    for sb in sandboxes {
        let name = sb.file_name().unwrap().to_string_lossy().to_string();
        let conv: Vec<serde_json::Value> =
            serde_json::from_str(&std::fs::read_to_string(sb.join("conventions.json")).unwrap()).unwrap();
        let r = adage_instincts::bind(&sb.join("repo"), &[], &BTreeSet::new());
        let directed: Vec<String> = r
            .routes
            .iter()
            .filter_map(|x| match &x.step {
                Step::Run { command } => Some(command.clone()),
                _ => None,
            })
            .chain(r.advisories().iter().filter_map(|a| a.slots.get("command").cloned()))
            .collect();
        for x in &r.routes {
            eprintln!("{name}: {} {:?} -> {:?}", x.id, x.trigger, x.step);
        }
        for c in &conv {
            let kind = c["kind"].as_str().unwrap_or("");
            let command = c["command"].as_str().unwrap_or("");
            match kind {
                "decoy" => {
                    decoys += 1;
                    assert!(!directed.iter().any(|d| d == command), "{name}: decoy {command} was bound");
                }
                "instinct_violation" => {
                    violations += 1;
                    let avoided = c["avoided"].as_str().unwrap();
                    assert!(!directed.iter().any(|d| d == avoided), "{name}: retired {avoided} was bound");
                }
                "dead_end" => {
                    dead_ends += 1;
                    let key = adage_instincts::safety::command_key(c["avoided"].as_str().unwrap(), &table);
                    let hit = r.routes.iter().any(|x| {
                        matches!(&x.trigger, Trigger::AfterFailure { command: k, .. } if Some(k) == key.as_ref())
                            && x.step == Step::Run { command: command.into() }
                    });
                    eprintln!(
                        "{name}: dead end {} -> {command}: {}",
                        c["avoided"],
                        if hit { "recovered" } else { "not recovered" }
                    );
                    dead_ends_recovered += hit as u32;
                }
                "finish_line" => {
                    finish_lines += 1;
                    let glob = c["glob"].as_str().unwrap();
                    let hit = r.routes.iter().any(|x| {
                        x.trigger == Trigger::AfterEdit { paths: glob.into() }
                            && x.step == Step::Run { command: command.into() }
                    });
                    if hit {
                        recovered += 1;
                    }
                }
                _ => {}
            }
        }
        // Whatever binds is valid and capped (the fixture tests check the details).
        for x in &r.routes {
            assert_eq!(x.aspect, adage_tracks::Aspect::Yellow);
        }
    }
    eprintln!(
        "decoys {decoys} (none bound), violations {violations} (none bound), finish lines recovered \
         {recovered}/{finish_lines}, dead ends recovered {dead_ends_recovered}/{dead_ends}"
    );
    assert!(
        decoys >= 10 && violations >= 3,
        "the suite's ground truth changed: {decoys} decoys, {violations} violations"
    );
    // Day-0 recall on this suite: every finish line is declared (a generator script naming its
    // inputs), and every dead end is a declared wrapper (env guard, alias loader, or a runner
    // script such as tools/testkit.py).
    assert_eq!(recovered, finish_lines);
    assert_eq!(dead_ends_recovered, dead_ends);
}
