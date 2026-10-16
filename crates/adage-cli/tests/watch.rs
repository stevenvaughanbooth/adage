//! `adage watch`, `adage ui` and the interactive `adage init` through the binary. Tests have no
//! terminal, so the screens are exercised with `--once` (one frame, as text) and the demo, which
//! replays real hook calls; everything else must refuse cleanly or fall back to plain output.

use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

fn adage(home: &Path, args: &[&str]) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
    common::scrub(&mut c);
    c.args(args)
        // Text assertions use UTF-8 glyphs regardless of the invoking terminal.
        .env("TERM", "xterm-256color")
        .env("LC_ALL", "C.UTF-8")
        .env("ADAGE_ASCII", "0")
        .env("NO_COLOR", "1")
        .env("ADAGE_HOME", home.join("adage"))
        .env("HOME", home.join("home"))
        .env("XDG_CONFIG_HOME", home.join("xdg"))
        .env("CODEX_HOME", home.join("codex"))
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
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
    // Tall and wide enough for every session, and the rail beside the graph.
    let o = run(&root, &root.join("work"), &["watch", "--demo", "--once", "--size", "140x80"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    for s in [
        "adage watch  harbor-dispatch",
        "demo",
        // Each delivery branches off its step, its response lit when the agent took it.
        "◆ adage · FIRE",
        "bin/fetch-incident INC-",
        "─ ✓ followed ─",
        "✓ followed adage",
        "─ ✗ not followed ─",
        "○ would fire bin/fetch-incident INC-",
        "◆ adage · INHIBIT",
        "npm run check",
        "◆ adage ■ finish line: asked the agent to go on before it stops",
        "◆ adage ✓ finish line met: npm run codegen",
        "after bin/fetch-incident {id}, read",
        // The rail: what conditioning changed, and health.
        "Reflexes in play",
        "prime → fire",
        "claude hook received",
        ":  palette",
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
            && t.contains("watching work")
            && t.contains("No traces yet")
            && t.contains("adage init")
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
fn bare_adage_prints_the_status_card_and_help_prints_usage() {
    let (_d, root) = world();
    let o = run(&root, &root, &[]);
    let t = text(&o);
    assert!(o.status.success() && t.starts_with("adage ") && !t.contains("Usage: adage"), "{t}");
    assert!(t.contains("adage doctor"), "{t}");
    let o = run(&root, &root, &["--help"]);
    let t = text(&o);
    assert!(
        o.status.success() && t.contains("Usage: adage") && t.contains("watch") && t.contains("status card"),
        "{t}"
    );
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
    assert!(o.status.success() && t.contains("1  Harnesses") && t.contains("6  Sense this repository's traces"), "{t}");
    let o = run(&root, &root.join("work"), &["init", "--nope"]);
    assert!(!o.status.success() && text(&o).contains("unexpected argument '--nope'"), "{}", text(&o));
}
