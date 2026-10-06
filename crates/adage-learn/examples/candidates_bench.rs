//! Learning time with and without the candidate pass (plan D1, test plan T-D1-08), in process,
//! and the hook's fast-path skip rate with and without candidates compiled.
//!
//! ```text
//! cargo build --release -p adage-learn --examples
//! target/release/examples/candidates_bench [sessions per repository] [repositories] [repetitions]
//! ```
//!
//! Writes a Claude Code history of the simulator's scenario (`adage-sim`, default 300 sessions in
//! each of 2 repositories, as `adage-cli/examples/learn_bench.rs` does) to a scratch directory,
//! then, with candidates off (the reflexes' pass alone: the base) and on, times the learner as
//! `adage condition` runs it (`adage_learn::learn` into the files store): a full learn with a cold
//! parse cache, the same with a warm cache, and automatic learning of one repository with a warm
//! cache after one of its transcripts grew by a turn (what a hook starts at the end of a turn).
//! Each is the median of `repetitions` runs (default 3), each on a fresh home. Then, over every
//! tool call of the history, the share the hook's no-match fast path skips (no trigger word or glob
//! in the call) with each index. Prints JSON, times in milliseconds.

use adage_learn::{learn, Inputs, LearnConfig};
use adage_sim::{claude, scenario};
use adage_tracks::Index;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Instant;

struct Corpus {
    files: Vec<PathBuf>,
    first_repo: Vec<PathBuf>,
}

fn write_history(root: &Path, per_repo: usize, repos: usize) -> Corpus {
    let mut files = Vec::new();
    let mut first_repo = Vec::new();
    for r in 0..repos {
        let repo = root.join(format!("repo-{r}"));
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let cwd = repo.to_string_lossy().to_string();
        let dir = root.join("history").join(claude::project_dir(&cwd));
        std::fs::create_dir_all(&dir).unwrap();
        let (mut n, mut seed) = (0, 1000 * (r as u64 + 1));
        while n < per_repo {
            for s in scenario::history(seed) {
                if n == per_repo {
                    break;
                }
                let p = dir.join(format!("{}-{seed}.jsonl", claude::session_id(&s)));
                std::fs::write(&p, claude::transcript(&s, &cwd)).unwrap();
                if r == 0 {
                    first_repo.push(p.clone());
                }
                files.push(p);
                n += 1;
            }
            seed += 1;
        }
    }
    Corpus { files, first_repo }
}

fn inputs(files: &[PathBuf], cache: &Path, only: Option<&str>) -> Inputs {
    Inputs {
        identities: Default::default(),
        transcripts: files.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: None,
        models: None,
        cache: Some(cache.to_path_buf()),
        only_repo: only.map(str::to_string),
    }
}

fn timed(f: impl FnOnce()) -> f64 {
    let t = Instant::now();
    f();
    t.elapsed().as_secs_f64() * 1e3
}

fn median(mut v: Vec<f64>) -> f64 {
    v.sort_by(f64::total_cmp);
    v[v.len() / 2]
}

/// One repetition on a fresh home: (cold, warm, auto after one transcript grew) and the index.
fn once(c: &Corpus, home: &Path, cfg: &LearnConfig) -> ([f64; 3], Index) {
    let (tracks, index, cache) = (home.join("tracks"), home.join("index.json"), home.join("cache"));
    // The first repository's key, as ingestion derives it from the trace's directory.
    let first = adage_ingest::batch::read(&c.first_repo[..1], &Default::default()).sessions[0].repo.clone();
    let cold = timed(|| {
        learn(&inputs(&c.files, &cache, None), &tracks, &index, cfg).unwrap();
    });
    let warm = timed(|| {
        learn(&inputs(&c.files, &cache, None), &tracks, &index, cfg).unwrap();
    });
    // One transcript of the first repository grows by a turn, as at the end of a session.
    let grown = &c.first_repo[0];
    let mut text = std::fs::read_to_string(grown).unwrap();
    let line = text.lines().rev().find(|l| l.contains("\"type\":\"user\"")).unwrap().to_string();
    text.push_str(&line);
    text.push('\n');
    std::fs::write(grown, text).unwrap();
    let auto = timed(|| {
        learn(&inputs(&c.first_repo, &cache, Some(&first)), &tracks, &index, cfg).unwrap();
    });
    ([cold, warm, auto], Index::load(&index))
}

/// The share of the history's tool calls the fast path skips for their repository's triggers.
fn skip_rate(c: &Corpus, index: &Index) -> (f64, usize) {
    let batch = adage_ingest::batch::read(&c.files, &Default::default());
    let (mut calls, mut skipped) = (0usize, 0usize);
    for s in &batch.sessions {
        let t = index.triggers(&s.repo);
        let cwd = s.root.to_string_lossy();
        for st in &s.steps {
            calls += 1;
            // As `adage_runtime::fast` builds it: the working directory, the tool, its input.
            let haystack = format!("{cwd}\n{}\n{}", st.call.tool, st.call.input);
            if !(t.calls_matter() && t.call_might_match(&haystack)) {
                skipped += 1;
            }
        }
    }
    (skipped as f64 / calls.max(1) as f64, calls)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let per_repo: usize = args.get(1).and_then(|x| x.parse().ok()).unwrap_or(300);
    let repos: usize = args.get(2).and_then(|x| x.parse().ok()).unwrap_or(2);
    let reps: usize = args.get(3).and_then(|x| x.parse().ok()).unwrap_or(3);
    let tmp = tempfile::tempdir().unwrap();
    let root = tmp.path().canonicalize().unwrap();
    let mut out = serde_json::Map::new();
    let mut medians: BTreeMap<&str, [f64; 3]> = BTreeMap::new();
    let mut indexes: BTreeMap<&str, Index> = BTreeMap::new();
    for (mode, on) in [("off", false), ("on", true)] {
        let cfg = LearnConfig { candidates: on, ..LearnConfig::demo() };
        let mut runs: Vec<[f64; 3]> = Vec::new();
        let mut last = None;
        for k in 0..reps {
            // A fresh history each time (the auto run grows a transcript), under the same
            // repository directories, so every run learns the same repositories.
            let _ = std::fs::remove_dir_all(root.join("history"));
            let c = write_history(&root, per_repo, repos);
            let (t, index) = once(&c, &root.join(format!("home-{mode}-{k}")), &cfg);
            runs.push(t);
            indexes.insert(mode, index);
            last = Some(c);
        }
        let m = [0, 1, 2].map(|i| median(runs.iter().map(|r| r[i]).collect()));
        medians.insert(mode, m);
        let index = &indexes[mode];
        let (rate, calls) = skip_rate(last.as_ref().expect("one repetition"), index);
        out.insert(
            mode.to_string(),
            json!({
                "learn_cold_cache_ms": m[0],
                "learn_warm_cache_ms": m[1],
                "auto_one_repo_one_transcript_changed_ms": m[2],
                "runs": runs,
                "routes": index.routes.len(),
                "candidates": index.candidates.values().map(Vec::len).sum::<usize>(),
                "fast_path_skip_rate": rate,
                "calls": calls,
            }),
        );
    }
    let (off, on) = (medians["off"], medians["on"]);
    let same_routes = indexes["off"].routes == indexes["on"].routes;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "sessions": per_repo * repos,
            "repositories": repos,
            "repetitions": reps,
            "candidates_off": out["off"],
            "candidates_on": out["on"],
            "ratio_cold": on[0] / off[0],
            "ratio_warm": on[1] / off[1],
            "ratio_auto": on[2] / off[2],
            "routes_unchanged": same_routes,
        }))
        .unwrap()
    );
}
