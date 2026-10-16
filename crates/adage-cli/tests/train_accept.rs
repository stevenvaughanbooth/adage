//! `adage train accept <name>`: install the pack `adage train <skill>` staged, exactly as reviewed,
//! as first-hand reflexes at the reviewed strengths.

use std::path::Path;
use std::process::{Command, Output, Stdio};

mod common;

const SKILL: &str = "---
name: widget-work
---

1. **Read the ticket.** Run `scripts/ticket <ID>` (for example `scripts/ticket WD-123`).
2. After editing `schema/*.json`, run `make gen`.
3. **Make the change** in the files under `src/`.
4. **Before you finish, run `make ci`.**

Do not commit; leave the working tree for review.
";

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

fn repo(d: &Path) {
    std::fs::create_dir_all(d.join("scripts")).unwrap();
    std::fs::create_dir_all(d.join("schema")).unwrap();
    std::fs::write(d.join("scripts/ticket"), "#!/bin/sh\necho \"ticket $1\"\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.join("scripts/ticket"), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(d.join("Makefile"), "gen:\n\t@echo gen\nci:\n\t@echo ci ok\n").unwrap();
    std::fs::write(d.join("schema/w.json"), "{}\n").unwrap();
    std::fs::write(d.join("SKILL.md"), SKILL).unwrap();
    git(d, &["init", "-q"]);
    git(d, &["add", "."]);
    git(d, &["commit", "-q", "-m", "init"]);
}

fn accept(home: &Path, cwd: &Path, name: &str) -> Output {
    common::scrub(&mut Command::new(env!("CARGO_BIN_EXE_adage")))
        .args(["train", "accept", name])
        .current_dir(cwd)
        .env("ADAGE_HOME", home)
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn has(t: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(t).is_file()))
}

#[test]
fn an_accepted_pack_is_installed_as_reviewed() {
    if !has("make") || !has("git") {
        eprintln!("skipped: make or git is missing");
        return;
    }
    let t = tempfile::tempdir().unwrap();
    let (root, home) = (t.path().join("widget"), t.path().join("home"));
    std::fs::create_dir_all(&root).unwrap();
    repo(&root);
    let replay = adage_exec::sandbox::detect().available();
    let out = adage_train::skill::run(&adage_train::skill::Options {
        skill: root.join("SKILL.md"),
        repo: Some(root.clone()),
        replay: adage_train::verify::Options { enabled: replay, wall_ms: 60_000 },
        home: home.clone(),
    })
    .unwrap();
    assert!(out.pack.is_some());

    // A staged pack changed after its review is refused.
    let route = std::fs::read_dir(out.dir.join("pack/routes")).unwrap().next().unwrap().unwrap().path();
    let original = std::fs::read(&route).unwrap();
    std::fs::write(&route, String::from_utf8(original.clone()).unwrap().replace("strength:", "summary: x\nstrength:"))
        .unwrap();
    let o = accept(&home, &root, "widget-work");
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stderr).contains("changed after its review"), "{o:?}");
    std::fs::write(&route, original).unwrap();

    let o = accept(&home, &root, "widget-work");
    let text = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(o.status.success(), "{o:?}");
    assert!(text.contains("signed      no"), "{text}");
    let bound = home.join("tracks/@imported/@bound").join(&out.pack_name);
    let track_dir = std::fs::read_dir(&bound).unwrap().next().unwrap().unwrap().path();
    let track = adage_tracks::io::read(&track_dir).unwrap();
    assert_eq!(track.manifest.track.provenance, "trained:reviewed");
    let index: adage_tracks::Index = serde_json::from_slice(&std::fs::read(home.join("index.json")).unwrap()).unwrap();
    let strength = |id: &str| {
        index.routes.iter().find(|r| r.route.id == id).map(|r| r.route.strength).unwrap_or_else(|| panic!("{id}"))
    };
    assert_eq!(strength("widget-work-uncommitted"), adage_tracks::Strength::Prime);
    // Verified by replay, accepted here: the finish line may fire; without a sandbox, prime.
    let want = if replay { adage_tracks::Strength::Fire } else { adage_tracks::Strength::Prime };
    assert_eq!(strength("widget-work-s2-regenerate"), want, "{text}");
    // The before-finish check (after any edit) and the regenerate step (after a schema edit) are
    // two fires at one decision point, an edit of a schema: the index's interlock keeps the first
    // and weakens the other to prime (`adage_tracks::interlock`), whatever the pack says.
    assert_eq!(strength("widget-work-s4-before-finish"), adage_tracks::Strength::Prime, "{text}");
    let accepted: std::collections::BTreeMap<String, String> =
        serde_json::from_slice(&std::fs::read(home.join("train/accepted.json")).unwrap()).unwrap();
    assert_eq!(accepted.get(&out.pack_name), out.digest.as_ref());

    // Inside an agent session, accepting is refused.
    let o = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["train", "accept", "widget-work"])
        .current_dir(&root)
        .env("ADAGE_HOME", &home)
        .env("CLAUDECODE", "1")
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!o.status.success());
    // An unknown name says how to make one.
    let o = accept(&home, &root, "nope");
    assert!(String::from_utf8_lossy(&o.stderr).contains("adage train <SKILL.md>"), "{o:?}");
}
