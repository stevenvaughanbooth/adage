//! Repository hygiene, checked on every test run: what may be committed (evals/README.md, "Data
//! policy").
//!
//! Raw run output (logs, archives, multi-megabyte result dumps) does not belong in git. Every
//! tracked file must be at most [`MAX_BYTES`] and must not be a log or an archive, unless it is
//! listed in [`ALLOWED`] with the reason it has to be tracked. The test fails on a new violation,
//! and on a listed entry that no longer exists, so the list only ever shrinks.

/// The largest file that may be committed without an entry in [`ALLOWED`].
pub const MAX_BYTES: u64 = 1 << 20;

/// Extensions of raw run output: never committed without an entry in [`ALLOWED`].
pub const RAW_EXTENSIONS: &[&str] = &["log", "err", "tgz", "tar", "gz", "zip"];

/// Tracked files that break the rules above, by path prefix, and why each must be tracked.
pub const ALLOWED: &[(&str, &str)] = &[
    ("crates/adage-intent/bundled/", "the bundled embedding model, compiled into the binary"),
    ("crates/adage-cli/assets/", "the embedded `adage map` web app, precompressed"),
    (
        "evals/reflex-proof/harness/results/20260930-live-cpu-transfer/report.json",
        "read by the harness tests (test_curve_transfer.py)",
    ),
    ("evals/lab/results/lab.json", "the lab's default results, read by compile.py and report.py"),
    (
        "evals/reflex-proof/harness/results/20260930-launch-experience/",
        "read by the harness tests (test_curve_transfer.py)",
    ),
    ("evals/reflex-proof/harness/results/20261001-heldout-tasks/", "frozen held-out task material (heldout_cohort.py)"),
    ("evals/showcase/live/", "archived showcase runs: ADAGE_HOME archives seed the real-repository gate"),
    ("spikes/S1-hook-latency/results/uptime.log", "the load record S1's findings cite (FINDINGS.md)"),
    ("evals/lab/research/case-retrieval-scale/result_raw.json", "the study's raw result, cited by its README"),
    ("evals/lab/research/hindsight-shortcuts/results.json", "the study's result, cited by its README"),
    (
        "evals/reflex-proof/harness/results/20260929-real-repository-preparation/sessions/",
        "session files whose hashes the run's report.json records",
    ),
    (
        "evals/reflex-proof/harness/results/20261001-heldout-marshmallow/run-heldout-20261001T164838Z.log",
        "the held-out run's log, cited by its RESULT.json",
    ),
];

/// Every violation among `(path, bytes)` tracked files, and every [`ALLOWED`] entry no file uses.
pub fn violations(files: &[(String, u64)]) -> Vec<String> {
    let allowed = |p: &str| ALLOWED.iter().any(|(prefix, _)| p.starts_with(prefix));
    let mut out = Vec::new();
    for (path, bytes) in files {
        if allowed(path) {
            continue;
        }
        if *bytes > MAX_BYTES {
            out.push(format!("{path}: {bytes} bytes, over {MAX_BYTES}"));
        }
        let ext = path.rsplit_once('.').map(|(_, e)| e).unwrap_or("");
        if RAW_EXTENSIONS.contains(&ext) {
            out.push(format!("{path}: raw run output (.{ext})"));
        }
    }
    for (prefix, why) in ALLOWED {
        if !files.iter().any(|(p, _)| p.starts_with(prefix)) {
            out.push(format!("{prefix}: allowed ({why}) but no tracked file uses it; remove the entry"));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use std::process::Command;

    #[test]
    fn the_repository_holds_no_raw_run_output() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let out = Command::new("git").args(["ls-files", "-z"]).current_dir(root).output().unwrap();
        assert!(out.status.success(), "git ls-files failed");
        let files: Vec<(String, u64)> = String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|p| !p.is_empty())
            // A tracked file deleted in the working tree is not checked.
            .filter_map(|p| std::fs::metadata(root.join(p)).ok().map(|m| (p.to_string(), m.len())))
            .collect();
        assert!(files.len() > 100, "the listing looks wrong: {} files", files.len());
        let v = violations(&files);
        assert!(v.is_empty(), "repository hygiene (xtask/src/hygiene.rs, evals/README.md):\n  {}", v.join("\n  "));
    }

    #[test]
    fn the_rules_catch_what_they_should() {
        let f = |p: &str, b: u64| (p.to_string(), b);
        let all_allowed: Vec<(String, u64)> = ALLOWED.iter().map(|(p, _)| f(&format!("{p}x"), 1)).collect();
        let check = |extra: Vec<(String, u64)>| violations(&[all_allowed.clone(), extra].concat());
        assert!(check(vec![f("src/a.rs", 10)]).is_empty());
        assert!(check(vec![f("evals/x/results/big.json", MAX_BYTES + 1)])[0].contains("over"));
        assert!(check(vec![f("evals/x/run.log", 10)])[0].contains("raw run output"));
        assert!(check(vec![f("crates/adage-intent/bundled/model.bin", 50 << 20)]).is_empty(), "allowed");
        assert!(violations(&[]).iter().all(|v| v.contains("remove the entry")), "unused entries are reported");
    }
}
