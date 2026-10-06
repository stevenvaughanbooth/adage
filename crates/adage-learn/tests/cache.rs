//! The parse cache changes nothing but speed: learning with it (cold, then warm, then after a
//! transcript grows) writes the same tracks and index, byte for byte, as learning without it. And
//! learning one repository (`only_repo`) writes exactly what a full learn writes for it.

use adage_learn::{learn, Inputs, LearnConfig};
use adage_sim::{claude, scenario};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Every file under `dir`, relative path → bytes.
fn files(dir: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.insert(p.strip_prefix(dir).unwrap().to_string_lossy().to_string(), std::fs::read(&p).unwrap());
            }
        }
    }
    out
}

/// Claude Code transcripts for two repositories (the scenario at two working directories), plus
/// the OpenCode and Codex fixture histories.
fn corpus(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for (seed, cwd) in [(11, "/fixture/acme-billing"), (44, "/fixture/acme-ledger")] {
        for s in scenario::history(seed) {
            let p = dir.join(claude::project_dir(cwd)).join(format!("{}.jsonl", claude::session_id(&s)));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, claude::transcript(&s, cwd)).unwrap();
            out.push(p);
        }
    }
    out.extend(adage_sim::fixture_files("opencode"));
    out.extend(adage_sim::fixture_files("codex"));
    out
}

fn run(home: &Path, transcripts: &[PathBuf], cache: bool, only: Option<&str>) -> adage_learn::Report {
    let inputs = Inputs {
        identities: Default::default(),
        transcripts: transcripts.to_vec(),
        outcomes: BTreeMap::new(),
        repo_override: None,
        models: None,
        cache: cache.then(|| home.join("cache/sessions")),
        only_repo: only.map(str::to_string),
    };
    learn(&inputs, &home.join("tracks"), &home.join("index.json"), &LearnConfig::demo()).unwrap()
}

fn learned(home: &Path) -> BTreeMap<String, Vec<u8>> {
    let mut f = files(&home.join("tracks"));
    f.insert("index.json".into(), std::fs::read(home.join("index.json")).unwrap());
    f
}

#[test]
fn cached_and_uncached_learns_write_identical_bytes() {
    let d = tempfile::tempdir().unwrap();
    let transcripts = corpus(&d.path().join("history"));
    let (plain, cached) = (d.path().join("plain"), d.path().join("cached"));

    let r0 = run(&plain, &transcripts, false, None);
    assert_eq!((r0.cache_hits, r0.cache_misses), (0, 0));
    assert!(r0.repos.len() >= 2, "two repositories: {:?}", r0.repos.keys().collect::<Vec<_>>());
    let cold = run(&cached, &transcripts, true, None);
    assert_eq!((cold.cache_hits, cold.cache_misses), (0, transcripts.len()));
    assert_eq!(learned(&plain), learned(&cached), "a cold cache learns what no cache learns");
    assert!(cached.join("cache/sessions/manifest.json").exists());

    // Warm: nothing is parsed, and the result is the same again (learn is idempotent).
    let warm = run(&cached, &transcripts, true, None);
    assert_eq!((warm.cache_hits, warm.cache_misses), (transcripts.len(), 0));
    run(&plain, &transcripts, false, None);
    assert_eq!(learned(&plain), learned(&cached), "a warm cache learns what no cache learns");
    assert_eq!(
        serde_json::to_value(&warm).unwrap(),
        serde_json::to_value(run(&plain, &transcripts, false, None)).unwrap(),
        "the same report"
    );

    // A transcript that changes is parsed again; the others are not.
    let grown = &transcripts[0];
    let mut text = std::fs::read_to_string(grown).unwrap();
    let first_prompt = text.lines().find(|l| l.contains("\"type\":\"user\"")).unwrap().to_string();
    text.push_str(&first_prompt.replace("\"type\":\"user\"", "\"type\":\"user\",\"extra\":1"));
    text.push('\n');
    std::fs::write(grown, text).unwrap();
    let after = run(&cached, &transcripts, true, None);
    assert_eq!((after.cache_hits, after.cache_misses), (transcripts.len() - 1, 1));
    run(&plain, &transcripts, false, None);
    assert_eq!(learned(&plain), learned(&cached));

    // A foreign or damaged manifest is an empty cache, never an error.
    std::fs::write(cached.join("cache/sessions/manifest.json"), b"{not json").unwrap();
    let again = run(&cached, &transcripts, true, None);
    assert_eq!(again.cache_misses, transcripts.len());
    assert_eq!(learned(&plain), learned(&cached));
}

#[test]
fn learning_one_repository_writes_what_a_full_learn_writes_for_it() {
    let d = tempfile::tempdir().unwrap();
    let transcripts = corpus(&d.path().join("history"));
    let (full, one, one_cached) = (d.path().join("full"), d.path().join("one"), d.path().join("one-cached"));
    let rep = run(&full, &transcripts, false, None);
    let repo = "acme-ledger";
    let rr = &rep.repos[repo];
    let slug = adage_learn::slug(repo);
    let only_repo = |f: BTreeMap<String, Vec<u8>>| -> BTreeMap<String, Vec<u8>> {
        f.into_iter().filter(|(k, _)| k.starts_with(&format!("{slug}/"))).collect()
    };
    for (home, cache) in [(&one, false), (&one_cached, true), (&one_cached, true)] {
        let r = run(home, &transcripts, cache, Some(repo));
        assert_eq!(r.repos.keys().collect::<Vec<_>>(), vec![repo], "only that repository");
        assert_eq!(r.sessions_read, rr.sessions, "only its sessions are counted");
        let got = files(&home.join("tracks"));
        assert!(got.keys().all(|k| k.starts_with(&format!("{slug}/"))), "{:?}", got.keys());
        assert_eq!(got, only_repo(files(&full.join("tracks"))));
    }
    let r = run(&one_cached, &transcripts, true, Some(repo));
    assert_eq!(r.cache_misses, 0);
    // The warm run skipped the other repositories' sessions without loading them.
    assert_eq!(r.cache_hits, transcripts.len());
}
