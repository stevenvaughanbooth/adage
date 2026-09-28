//! `adage watch`, `adage ui` and the interactive `adage init` through the binary. Tests have no
//! terminal, so the screens are exercised with `--once` (one frame, as text) and the demo, which
//! replays real hook calls; everything else must refuse cleanly or fall back to plain output.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn adage(home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
    c.args(args)
        .env("ADAGE_HOME", home.join("adage"))
        .env("HOME", home.join("home"))
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env("CODEX_HOME", home.join("codex"))
        .env("NO_COLOR", "1")
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("ADAGE_DISABLE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for v in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"] {
        c.env_remove(v);
    }
    c
}

fn run(home: &Path, cwd: &Path, args: &[&str]) -> Output {
    adage(home, args).current_dir(cwd).output().unwrap()
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn world() -> (tempfile::TempDir, PathBuf) {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    for sub in ["home", "xdg", "work/.git"] {
        std::fs::create_dir_all(root.join(sub)).unwrap();
    }
    (d, root)
}

#[test]
fn watch_demo_replays_real_hook_decisions() {
    let (_d, root) = world();
    let o = run(&root, &root.join("work"), &["watch", "--demo", "--once", "--size", "100x40"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    for s in [
        "adage watch  harbor-dispatch",
        "demo",
        "→ ◠ lookup-incident          FIRE",
        "○ would fire lookup-incident (withheld)",
        "✓ followed lookup-incident",
        "✗ ignored lookup-incident",
        "→ ◠ dead-end-npm-test        INHIBIT",
        "■ finish line: asked the agent to continue",
        "✓ finish line met codegen-after-registry",
        "read-linked-files         prime → fire",
        "hooks claude",
        "auto-condition",
    ] {
        assert!(t.contains(s), "the demo frame lacks {s:?}:\n{t}");
    }
    // The demo lives in a temporary directory and leaves nothing behind in ADAGE_HOME.
    assert!(!root.join("adage").exists());
}

#[test]
fn watch_once_on_a_fresh_machine_and_doctor_watch_alias() {
    let (_d, root) = world();
    let o = run(&root, &root.join("work"), &["watch", "--once", "--size", "60x20"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(
        t.contains("adage watch  work")
            && t.contains("No traces yet for work")
            && t.contains("Nothing conditioned yet"),
        "{t}"
    );
    assert_eq!(t.lines().count(), 20);
    let o = run(&root, &root.join("work"), &["doctor", "--watch", "--once", "--size", "60x20"]);
    assert!(o.status.success() && text(&o).contains("adage watch"), "{}", text(&o));
    // Without a terminal, the live screens refuse instead of scribbling on a pipe.
    for args in [&["watch"][..], &["ui"][..]] {
        let o = run(&root, &root.join("work"), args);
        assert!(!o.status.success() && text(&o).contains("needs a terminal"), "{}", text(&o));
    }
    let o = run(&root, &root.join("work"), &["watch", "--bogus"]);
    assert!(!o.status.success() && text(&o).contains("unexpected argument '--bogus'"), "{}", text(&o));
}

#[test]
fn bare_adage_without_a_terminal_prints_usage() {
    let (_d, root) = world();
    for args in [&[][..], &["--help"][..]] {
        let o = run(&root, &root, args);
        let t = text(&o);
        assert!(o.status.success() && t.contains("Usage: adage") && t.contains("watch"), "{t}");
    }
}

#[test]
fn init_without_a_terminal_keeps_the_printed_report() {
    let (_d, root) = world();
    let o = adage(&root, &["init", "--print"])
        .current_dir(root.join("work"))
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
        .output()
        .unwrap();
    let t = text(&o);
    assert!(o.status.success() && t.contains("1  Harnesses") && t.contains("4  Sense your traces"), "{t}");
    let o = run(&root, &root.join("work"), &["init", "--nope"]);
    assert!(!o.status.success() && text(&o).contains("unexpected argument '--nope'"), "{}", text(&o));
}
