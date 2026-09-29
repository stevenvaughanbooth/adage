//! Learning time with and without the parse cache, and for automatic learning of one repository.
//!
//! ```text
//! cargo build --release -p adage-cli --examples
//! target/release/examples/learn_bench <adage binary> [sessions per repository] [repositories]
//! ```
//! Writes a Claude Code history of the simulator's scenario (`adage-sim`, default 300 sessions in
//! each of 2 repositories) under a scratch `HOME`, then times, through the binary:
//! `adage condition --no-cache` (every transcript parsed), `adage condition` with a cold and a warm parse
//! cache, and `adage condition --auto` for one repository with a warm cache after one of its
//! transcripts grew (what a hook starts at the end of a turn). Prints JSON, in milliseconds.

use adage_sim::{claude, scenario};
use serde_json::json;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

fn run(bin: &str, root: &Path, args: &[&str]) -> f64 {
    let t = Instant::now();
    let o = Command::new(bin)
        .args(args)
        .env("ADAGE_HOME", root.join("adage"))
        .env("HOME", root.join("home"))
        .env_remove("ADAGE_AUTO_LEARN")
        .output()
        .unwrap();
    assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
    t.elapsed().as_secs_f64() * 1e3
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bin = &args[1];
    let per_repo: usize = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(300);
    let repos: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(2);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let mut first_repo = String::new();
    let mut a_transcript = None;
    let mut bytes = 0u64;
    for r in 0..repos {
        let repo = root.join(format!("repo-{r}"));
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let cwd = repo.to_string_lossy().to_string();
        if r == 0 {
            first_repo = cwd.clone();
        }
        let dir = root.join("home/.claude/projects").join(claude::project_dir(&cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let mut n = 0;
        let mut seed = 1000 * (r as u64 + 1);
        while n < per_repo {
            for s in scenario::history(seed) {
                if n == per_repo {
                    break;
                }
                let p = dir.join(format!("{}-{seed}.jsonl", claude::session_id(&s)));
                let body = claude::transcript(&s, &cwd);
                bytes += body.len() as u64;
                std::fs::write(&p, body).unwrap();
                if r == 0 && a_transcript.is_none() {
                    a_transcript = Some(p.clone());
                }
                n += 1;
            }
            seed += 1;
        }
    }
    std::fs::create_dir_all(root.join("adage")).unwrap();
    std::fs::write(root.join("adage/config.toml"), "[learn]\nthresholds = \"demo\"\n").unwrap();

    let full = run(bin, &root, &["condition", "--no-cache"]);
    let cold = run(bin, &root, &["condition"]);
    let warm = run(bin, &root, &["condition"]);
    let auto_warm = run(bin, &root, &["condition", "--auto", "--repo-root", &first_repo]);
    // One transcript of the first repository grows by a turn, as at the end of a session.
    let grown = a_transcript.unwrap();
    let mut text = std::fs::read_to_string(&grown).unwrap();
    let line = text.lines().rev().find(|l| l.contains("\"type\":\"user\"")).unwrap().to_string();
    text.push_str(&line);
    text.push('\n');
    std::fs::write(&grown, text).unwrap();
    let auto_one_changed = run(bin, &root, &["condition", "--auto", "--repo-root", &first_repo]);
    let log = std::fs::read_to_string(root.join("adage/learn.log")).unwrap();
    let last: serde_json::Value = serde_json::from_str(log.lines().last().unwrap()).unwrap();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "sessions": per_repo * repos,
            "repositories": repos,
            "history_mb": bytes as f64 / 1e6,
            "learn_no_cache_ms": full,
            "learn_cold_cache_ms": cold,
            "learn_warm_cache_ms": warm,
            "auto_one_repo_warm_ms": auto_warm,
            "auto_one_repo_one_transcript_changed_ms": auto_one_changed,
            "auto_learner_duration_ms": last["duration_ms"],
            "auto_sessions_read": last["sessions_read"],
        }))
        .unwrap()
    );
}
