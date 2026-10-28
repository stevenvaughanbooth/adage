//! Q4a (Track C §3.2, §5.1): "`adage` spawns nothing" stays checkable by grep. Execution lives in
//! `adage-exec`; the crates behind the `adage` binary and its hooks may not start processes.
//!
//! This scans their `src/` trees for `Command::new` and `process::Command`. The uses that exist
//! today are listed below, each with why it is not execution on the agent's behalf; any other
//! occurrence (a new file, or a new use in a listed file) fails the test. Tests, examples and
//! benches are not scanned: they drive the binary from outside.

use std::path::{Path, PathBuf};

/// Crates behind the `adage` binary and its hooks.
const CRATES: &[&str] = &[
    "adage-cli",
    "adage-app",
    "adage-hook",
    "adage-runtime",
    "adage-store",
    "adage-tracks",
    "adage-learn",
    "adage-world",
];

/// (file relative to `crates/`, occurrences, why it is allowed).
const ALLOWED: &[(&str, usize, &str)] = &[
    // Explicit palette actions run Adage's own commands with bounded captured output.
    ("adage-cli/src/ui/tools.rs", 1, "person-requested Adage reports and configuration from the palette"),
    // An explicit, Person-authorized import lists and exports local OpenCode history. Never
    // called from a hook or learner; no command from a transcript is executed.
    ("adage-app/src/history.rs", 2, "person-requested local OpenCode session list/export"),
    // Test-only: `trust.rs`'s unit tests build a git repository and commit a pack with `git`. The
    // module itself reads git's object store (`adage_core::gitobj`) and starts nothing.
    ("adage-app/src/trust.rs", 3, "git in its #[cfg(test)] module only, to commit a fixture pack"),
    // Live learning: at the end of a turn the hook (`adage-hook` → `autolearn::on_turn_end`) may
    // start `adage condition --auto`, re-executing the adage binary itself, detached. It runs Adage's
    // own learner over transcripts, never a repository or agent command.
    ("adage-hook/src/autolearn.rs", 1, "spawns `adage condition` (itself), detached"),
    // The terminal UI's background jobs: `adage condition` again (itself), from `adage ui`.
    ("adage-cli/src/ui/jobs.rs", 1, "spawns `adage condition` (itself) from the terminal UI"),
    // `adage ui --demo`: the demo re-runs the adage binary against a throwaway home.
    ("adage-cli/src/ui/demo.rs", 1, "spawns the adage binary (itself) for the demo"),
    // `adage train`: hands over to the separate adage-train binary (exec on Unix, so nothing is
    // left running in `adage`). adage-train, not adage, drives the agent sessions, sandboxed, at a
    // person's request; adage-train is not one of the crates scanned here.
    ("adage-cli/src/train.rs", 1, "execs the separate adage-train binary for `adage train`"),
    // The one place `adage` reaches the network (`net.rs`): curl for `adage models fetch`, `adage
    // init`'s Binaries step and `adage update --check`, at a person's request or in the detached
    // learner's daily check (never a hook); and tar, to unpack a release archive only after its
    // SHA-256 matched the release's. Downloads of Adage's own files, never a repository command.
    ("adage-cli/src/net.rs", 2, "curl downloads; tar unpacks a verified release archive"),
    // `adage init` installs the Claude Code plugin it staged with Claude Code's own command line
    // (`claude plugin marketplace add`, `install`, `list`), at a person's request (`--yes
    // --hooks`, or the wizard's choice); `adage update` refreshes it the same way.
    ("adage-cli/src/claude_plugin.rs", 1, "`claude plugin …` to install Adage's own plugin"),
    // Terminal mode switching for the interactive UI (stty on the controlling terminal).
    ("adage-cli/src/tty.rs", 1, "stty for the terminal UI"),
    // `adage map`: opens the person's browser on the local page (`open`, `cmd /c start` or
    // `xdg-open`, one per platform), at their request; `--no-open` skips it. A URL, never a
    // repository or agent command.
    ("adage-cli/src/map/mod.rs", 3, "the platform's URL opener for `adage map`"),
    // `adage update`: replaces Adage's own binaries the way they were installed (`cargo install`,
    // `git pull --ff-only` then `cargo build` in a checkout, `npm` or `brew`), each command
    // printed first, only when a person runs it; no hook or learner calls it. A package manager
    // on Adage itself, never a repository or agent command.
    ("adage-cli/src/update.rs", 1, "the install's package manager for `adage update`"),
    // `adage update` asks the binary it installed for its version (verify) and hands it the rest
    // of the update (`update --finish`); `adage doctor` asks installed Adage binaries outside the
    // repository their version. Adage's own binaries only (`probe::askable`), never a repository
    // or agent command.
    ("adage-cli/src/update/probe.rs", 1, "Adage's own binaries: `version`, and `update --finish`"),
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
