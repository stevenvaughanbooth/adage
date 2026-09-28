//! Q4a (Track C §3.2, §5.1): "`adage` spawns nothing" stays checkable by grep. Execution lives in
//! `adage-exec`; the crates behind the `adage` binary and its hooks may not start processes.
//!
//! This scans their `src/` trees for `Command::new` and `process::Command`. The uses that exist
//! today are listed below, each with why it is not execution on the agent's behalf; any other
//! occurrence (a new file, or a new use in a listed file) fails the test. Tests, examples and
//! benches are not scanned: they drive the binary from outside.

use std::path::{Path, PathBuf};

/// Crates behind the `adage` binary and its hooks.
const CRATES: &[&str] = &["adage-cli", "adage-hook", "adage-runtime", "adage-store", "adage-tracks", "adage-learn"];

/// (file relative to `crates/`, occurrences, why it is allowed).
const ALLOWED: &[(&str, usize, &str)] = &[
    // Live learning: at the end of a turn the hook (`adage-hook` → `autolearn::on_turn_end`) may
    // start `adage learn --auto`, re-executing the adage binary itself, detached. It runs Adage's
    // own learner over transcripts, never a repository or agent command.
    ("adage-hook/src/autolearn.rs", 1, "spawns `adage learn` (itself), detached"),
    // The terminal UI's background jobs: `adage learn` again (itself), from `adage ui`.
    ("adage-cli/src/ui/jobs.rs", 1, "spawns `adage learn` (itself) from the terminal UI"),
    // `adage ui --demo`: the demo re-runs the adage binary against a throwaway home.
    ("adage-cli/src/ui/demo.rs", 1, "spawns the adage binary (itself) for the demo"),
    // `adage models download`: fetches a model file with curl, at a person's request.
    ("adage-cli/src/models.rs", 1, "curl for `adage models download`"),
    // Terminal mode switching for the interactive UI (stty on the controlling terminal).
    ("adage-cli/src/tty.rs", 1, "stty for the terminal UI"),
];

fn rs_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rs_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Lines (outside comments) that name `Command::new` or `process::Command` (not the
/// `CommandExt` traits, which configure a command something else created).
fn occurrences(text: &str) -> usize {
    text.lines()
        .map(|l| l.trim_start().replace("process::CommandExt", ""))
        .filter(|l| !l.starts_with("//"))
        .filter(|l| l.contains("Command::new") || l.contains("process::Command"))
        .count()
}

#[test]
fn adage_crates_spawn_nothing_new() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut found = Vec::new();
    for c in CRATES {
        let mut files = Vec::new();
        rs_files(&crates.join(c).join("src"), &mut files);
        assert!(!files.is_empty(), "no sources found for {c}");
        for f in files {
            let n = occurrences(&std::fs::read_to_string(&f).unwrap());
            if n > 0 {
                let rel = f.strip_prefix(crates).unwrap().to_string_lossy().replace('\\', "/");
                found.push((rel, n));
            }
        }
    }
    let mut unexpected = Vec::new();
    for (rel, n) in &found {
        match ALLOWED.iter().find(|(f, _, _)| f == rel) {
            Some((_, max, _)) if n <= max => {}
            Some((_, max, why)) => unexpected.push(format!("{rel}: {n} uses (allowed {max}: {why})")),
            None => unexpected.push(format!("{rel}: {n} uses")),
        }
    }
    assert!(
        unexpected.is_empty(),
        "the adage crates must not spawn processes (Q4a); execution belongs in adage-exec:\n  {}",
        unexpected.join("\n  ")
    );
}

#[test]
fn the_scan_sees_what_it_should() {
    assert_eq!(occurrences("let c = std::process::Command::new(\"x\");"), 1);
    assert_eq!(occurrences("use std::os::unix::process::CommandExt;"), 0);
    assert_eq!(occurrences("// Command::new in a comment"), 0);
    assert_eq!(occurrences("use std::process::{Command, Stdio};\nCommand::new(x)"), 1);
}
