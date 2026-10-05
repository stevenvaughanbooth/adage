//! Where `adage condition` and `adage sense` read history from: by default, what was recorded in
//! the repository they run in (`--all` for every repository's), from Claude Code history and,
//! once its hooks are registered, Codex's. An explicit source that finds nothing is an error,
//! never a silent fallback to the person's Claude Code history.

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
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ADAGE_HOME", home.join("adage"))
        .env("CODEX_HOME", home.join(".codex"))
        .env_remove("CLAUDE_CONFIG_DIR")
        .output()
        .unwrap();
    (String::from_utf8_lossy(&o.stdout).into(), String::from_utf8_lossy(&o.stderr).into(), o.status.success())
}

/// Register Adage's Codex hooks for every repository (`adage install codex --scope user --write`):
/// only then is Codex history read by default.
fn register_codex(home: &Path) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
    c.args(["install", "codex", "--scope", "user", "--write"])
        .current_dir(home)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("ADAGE_HOME", home.join("adage"))
        .env("CODEX_HOME", home.join(".codex"))
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1");
    for v in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"] {
        c.env_remove(v);
    }
    let o = c.output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
}

#[test]
fn a_missing_explicit_directory_is_an_error_in_both_commands() {
    let home = home_with_history();
    let missing = home.path().join("missing-history");
    for command in ["condition", "sense"] {
        for flag in ["--claude-projects", "--codex-sessions"] {
            let (_, err, ok) = adage(home.path(), &[command, flag, missing.to_str().unwrap()]);
            assert!(!ok && err.contains("is not a directory"), "{command} {flag}: {err}");
            assert!(!home.path().join("adage/tracks").exists(), "no default history is published");
        }
    }
}

#[test]
fn an_empty_explicit_source_never_falls_back_to_claude_history() {
    let home = home_with_history();
    let empty = tempfile::tempdir().unwrap();
    let (_, err, ok) = adage(home.path(), &["condition", "--demo", "--codex-sessions", empty.path().to_str().unwrap()]);
    assert!(!ok, "an explicit source that finds nothing fails");
    assert!(err.contains("no traces found"), "{err}");
    assert!(!home.path().join("adage/tracks").exists(), "nothing was learned from the fallback");

    let (out, _, _) = adage(home.path(), &["sense", "--codex-sessions", empty.path().to_str().unwrap()]);
    assert!(out.contains("0 traces"), "sense read only the named source:\n{out}");
    assert!(!out.contains("Claude Code history"), "{out}");
}

#[test]
fn codex_history_is_learned_and_sensed_with_its_harness() {
    adage_sim::assume_no_checkout_at_cwd();
    let home = tempfile::tempdir().unwrap();
    let sessions = fixtures().join("codex/sessions");
    let s = sessions.to_str().unwrap();
    let (out, err, ok) = adage(home.path(), &["sense", "--demo", "--codex-sessions", s]);
    assert!(ok, "{err}");
    assert!(out.contains("Codex rollout(s)"), "{out}");
    let (out, err, ok) = adage(home.path(), &["sense", "--demo", "--codex-sessions", s, "--json"]);
    assert!(ok, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(report["repos"].as_array().unwrap().iter().any(|repo| repo["harnesses"]
        .as_array()
        .unwrap()
        .iter()
        .any(|h| h == "codex")));

    let (out, err, ok) = adage(home.path(), &["condition", "--demo", "--codex-sessions", s]);
    assert!(ok, "{err}");
    let tracks = walk(&home.path().join("adage/tracks"));
    let manifest = tracks
        .iter()
        .find(|p| p.ends_with("track.toml"))
        .unwrap_or_else(|| panic!("a track was learned:\n{out}{err}\n{tracks:#?}"));
    let text = std::fs::read_to_string(manifest).unwrap();
    assert!(text.contains("harness = [\"codex\"]"), "{text}");
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap().flatten() {
        if e.path().is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

#[test]
fn default_discovery_reads_codex_history_once_its_hooks_are_registered_and_respects_off() {
    let home = tempfile::tempdir().unwrap();
    copy_tree(&fixtures().join("codex/sessions"), &home.path().join(".codex/sessions"));
    // Outside a repository, every repository's history is read only when asked for.
    for command in ["sense", "condition"] {
        let (_, err, ok) = adage(home.path(), &[command, "--json"]);
        assert!(!ok && err.contains("not inside a repository") && err.contains("--all"), "{command}: {err}");
    }
    // Claude Code history alone until Codex's hooks are registered.
    let (out, err, ok) = adage(home.path(), &["sense", "--all", "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sessions"], 0, "{out}");
    assert!(!v["source"].as_str().unwrap().contains("Codex"), "{out}");
    register_codex(home.path());
    let (out, err, ok) = adage(home.path(), &["sense", "--all", "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(v["sessions"].as_u64().unwrap() > 0, "{out}");
    assert!(v["source"].as_str().unwrap().contains("Codex rollout"));
    let (out, err, ok) = adage(home.path(), &["condition", "--all", "--json"]);
    assert!(ok, "{err}");
    let learned: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(learned["sessions_read"], v["sessions"]);

    std::fs::write(home.path().join("adage/config.toml"), "[learn]\ncodex_sessions = \"\"\n").unwrap();
    let (out, err, ok) = adage(home.path(), &["sense", "--all", "--json"]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sessions"], 0);
    assert!(!v["source"].as_str().unwrap().contains("Codex"));
}

#[test]
fn configured_sources_are_shared_by_preview_and_learning_and_explicit_sources_replace_them() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join("old-codex/sessions");
    copy_tree(&fixtures().join("codex/sessions"), &source);
    std::fs::create_dir_all(home.path().join("adage")).unwrap();
    std::fs::write(
        home.path().join("adage/config.toml"),
        format!("[learn]\nthresholds = \"demo\"\ncodex_sessions = {:?}\n", source.to_str().unwrap()),
    )
    .unwrap();
    let (out, err, ok) = adage(home.path(), &["sense", "--all", "--json"]);
    assert!(ok, "{err}");
    let preview: serde_json::Value = serde_json::from_str(&out).unwrap();
    let (out, err, ok) = adage(home.path(), &["condition", "--all", "--json"]);
    assert!(ok, "{err}");
    let learned: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(preview["sessions"].as_u64().unwrap() > 0);
    assert_eq!(learned["sessions_read"], preview["sessions"]);
    assert_eq!(learned["thresholds"], preview["thresholds"], "preview uses the configured evidence bars");
    let empty = tempfile::tempdir().unwrap();
    let (out, err, ok) = adage(home.path(), &["sense", "--json", "--claude-projects", empty.path().to_str().unwrap()]);
    assert!(ok, "{err}");
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["sessions"], 0);
    let (_, err, ok) = adage(home.path(), &["condition", "--claude-projects", empty.path().to_str().unwrap()]);
    assert!(!ok && err.contains("the sources you named hold no traces"), "{err}");
    let (out, err, ok) = adage(home.path(), &["sense", "--transcripts", "--json"]);
    assert!(!ok && err.contains("--transcripts"), "an empty explicit list is a usage error: {err}");
    assert!(out.is_empty(), "an empty explicit list never falls back to a default source report");
}

#[test]
fn no_default_history_binds_real_repository_instincts_but_explicit_empty_still_fails() {
    // No origin: the scope retains a mixed-case, dot-prefixed local repository identity.
    let home = tempfile::Builder::new().prefix(".MixedCase.repository-").tempdir().unwrap();
    let sandbox = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/sandboxes/acme-billing");
    copy_tree(&sandbox, home.path());
    std::fs::create_dir_all(home.path().join(".git")).unwrap();
    let (out, err, ok) = adage(home.path(), &["condition", "--json"]);
    assert!(ok, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["sessions_read"], 0);
    let index: serde_json::Value =
        serde_json::from_slice(&std::fs::read(home.path().join("adage/index.json")).unwrap()).unwrap();
    assert!(!index["routes"].as_array().unwrap().is_empty(), "binds real Makefile instincts");
    assert!(
        report["repos"].as_object().unwrap().keys().any(|k| k.contains("MixedCase")),
        "scope identity is unchanged"
    );
    assert!(err.contains("binding this repository's instincts"), "{err}");
    let empty = tempfile::tempdir().unwrap();
    let (_, err, ok) = adage(home.path(), &["condition", "--claude-projects", empty.path().to_str().unwrap()]);
    assert!(!ok && err.contains("the sources you named hold no traces"), "{err}");
    let (_, err, ok) = adage(home.path(), &["condition", "--no-instincts"]);
    assert!(!ok && err.contains("no traces found"), "{err}");
}

/// Launch review: `adage condition --repo-root <dir>` (README, docs/first-reflex.md) conditions
/// that repository alone: only traces recorded in it are read, its instincts bind from it, and a
/// repository with no traces gets its instincts. Before, the flag was silently ignored without
/// `--auto`, and every repository in the history was conditioned.
#[test]
fn repo_root_conditions_only_that_repository() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join(".codex/sessions");
    std::fs::create_dir_all(&source).unwrap();
    let one = walk(&fixtures().join("codex/sessions")).remove(0);
    let original = std::fs::read_to_string(one).unwrap();
    let work = home.path().canonicalize().unwrap().join("work");
    for name in ["app", "other"] {
        let repo = work.join(name);
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        let text = original.replace("/fixture/acme-billing", repo.to_str().unwrap());
        std::fs::write(source.join(format!("rollout-{name}.jsonl")), text).unwrap();
    }
    register_codex(home.path());
    let app = work.join("app");
    let (out, err, ok) = adage(home.path(), &["condition", "--repo-root", app.to_str().unwrap(), "--json"]);
    assert!(ok, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["sessions_read"], 1, "only the trace recorded in this repository: {out}");
    let repos: Vec<&String> = report["repos"].as_object().unwrap().keys().collect();
    assert!(repos.iter().all(|k| !k.contains("other")), "{repos:?}");
    // From inside the repository, as the docs write it, and with nothing named: the repository it
    // runs in is the default; `--all` reads every one.
    let in_app = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_adage"))
            .args(args)
            .current_dir(&app)
            .env("HOME", home.path())
            .env("USERPROFILE", home.path())
            .env("ADAGE_HOME", home.path().join("adage"))
            .env("CODEX_HOME", home.path().join(".codex"))
            .env_remove("CLAUDE_CONFIG_DIR")
            .output()
            .unwrap();
        assert!(o.status.success(), "{args:?}: {}", String::from_utf8_lossy(&o.stderr));
        serde_json::from_slice::<serde_json::Value>(&o.stdout).unwrap()
    };
    assert_eq!(in_app(&["condition", "--repo-root", ".", "--json"])["sessions_read"], 1);
    assert_eq!(in_app(&["condition", "--json"])["sessions_read"], 1, "the repository it runs in");
    assert_eq!(in_app(&["sense", "--json"])["sessions"], 1, "sense reads as condition does");
    assert_eq!(in_app(&["sense", "--all", "--json"])["sessions"], 2, "--all reads every repository's");
    // A repository with no traces: its own instincts, not every other repository's traces.
    let fresh = home.path().join("fresh");
    copy_tree(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/sandboxes/acme-billing"), &fresh);
    std::fs::create_dir_all(fresh.join(".git")).unwrap();
    let (out, err, ok) = adage(home.path(), &["condition", "--repo-root", fresh.to_str().unwrap(), "--json"]);
    assert!(ok, "{err}");
    assert!(err.contains("binding this repository's instincts"), "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert!(report.to_string().contains("fresh") && !report.to_string().contains("other"), "{out}");
    let (_, err, ok) = adage(home.path(), &["condition", "--repo-root", home.path().to_str().unwrap()]);
    assert!(!ok && err.contains("is not inside a git repository"), "{err}");
}

#[test]
fn cwd_prefix_uses_recorded_path_components_with_json_whitespace() {
    let home = tempfile::tempdir().unwrap();
    let source = home.path().join(".codex/sessions");
    std::fs::create_dir_all(&source).unwrap();
    let one = walk(&fixtures().join("codex/sessions")).remove(0);
    let original = std::fs::read_to_string(one).unwrap();
    for (name, cwd) in [("yes", "/work/app/src"), ("no", "/work/app-other")] {
        let text = original.replace("/fixture/acme-billing", cwd).replace("\"cwd\":", "\"cwd\": ");
        std::fs::write(source.join(format!("rollout-{name}.jsonl")), text).unwrap();
    }
    register_codex(home.path());
    let (out, err, ok) = adage(home.path(), &["condition", "--cwd-prefix", "/work/app", "--json"]);
    assert!(ok, "{err}");
    let report: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(report["sessions_read"], 1, "{out}");
}
