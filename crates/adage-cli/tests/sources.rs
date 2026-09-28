//! Where `adage condition` and `adage sense` read history from. An explicit source that finds nothing
//! is an error, never a silent fallback to the person's Claude Code history.

use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sim/fixtures")
}

/// A home whose Claude Code history holds one transcript, so a fallback would be visible.
fn home_with_history() -> tempfile::TempDir {
    let home = tempfile::tempdir().unwrap();
    let projects = home.path().join(".claude/projects/-x");
    std::fs::create_dir_all(&projects).unwrap();
    let claude = fixtures().join("claude");
    let one =
        walk(&claude).into_iter().find(|p| p.extension().is_some_and(|x| x == "jsonl")).expect("a claude fixture");
    std::fs::copy(&one, projects.join(one.file_name().unwrap())).unwrap();
    home
}

fn walk(d: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(d).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

fn adage(home: &Path, args: &[&str]) -> (String, String, bool) {
    let o = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ADAGE_HOME", home.join("adage"))
        .output()
        .unwrap();
    (String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into(), o.status.success())
}

#[test]
fn an_empty_explicit_source_never_falls_back_to_claude_history() {
    let home = home_with_history();
    let empty = tempfile::tempdir().unwrap();
    let (_, err, ok) = adage(home.path(), &["condition", "--demo", "--codex-sessions", empty.path().to_str().unwrap()]);
    assert!(!ok, "an explicit source that finds nothing fails");
    assert!(err.contains("no transcripts found"), "{err}");
    assert!(!home.path().join("adage/tracks").exists(), "nothing was learned from the fallback");

    let (out, _, _) = adage(home.path(), &["sense", "--codex-sessions", empty.path().to_str().unwrap()]);
    assert!(out.contains("0 session(s)"), "survey read only the named source:\n{out}");
    assert!(!out.contains("Claude Code history"), "{out}");
}

#[test]
fn codex_history_is_learned_and_surveyed_with_its_harness() {
    let home = tempfile::tempdir().unwrap();
    let sessions = fixtures().join("codex/sessions");
    let s = sessions.to_str().unwrap();
    let (out, err, ok) = adage(home.path(), &["sense", "--demo", "--codex-sessions", s]);
    assert!(ok, "{err}");
    assert!(out.contains("Codex rollout(s)") && out.contains("(codex)"), "{out}");

    let (_, err, ok) = adage(home.path(), &["condition", "--demo", "--codex-sessions", s]);
    assert!(ok, "{err}");
    let tracks = walk(&home.path().join("adage/tracks"));
    let manifest = tracks.iter().find(|p| p.ends_with("track.toml")).expect("a track was learned");
    let text = std::fs::read_to_string(manifest).unwrap();
    assert!(text.contains("harness = [\"codex\"]"), "{text}");
}
