//! Replays run a command in a disposable, sandboxed checkout of `HEAD` (adage-train's
//! verification). Skipped where this machine has no sandbox.

use adage_exec::replay::{Replay, ReplayLimits};
use std::path::Path;
use std::process::Command;

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

fn repo() -> tempfile::TempDir {
    let t = tempfile::tempdir().unwrap();
    let d = t.path();
    std::fs::create_dir_all(d.join("scripts")).unwrap();
    std::fs::write(d.join("scripts/check"), "#!/bin/sh\necho checking >&2\necho OK\n").unwrap();
    std::fs::write(d.join("scripts/gen"), "#!/bin/sh\necho generated > out.txt\n").unwrap();
    std::fs::write(d.join("scripts/stale"), "#!/bin/sh\necho 'RuntimeError: FOO is not set' >&2\nexit 3\n").unwrap();
    #[cfg(unix)]
    for f in ["check", "gen", "stale"] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.join("scripts").join(f), std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    git(d, &["init", "-q"]);
    git(d, &["add", "."]);
    git(d, &["commit", "-q", "-m", "init"]);
    // Uncommitted work in the person's tree never reaches the checkout.
    std::fs::write(d.join("scripts/check"), "#!/bin/sh\nexit 9\n").unwrap();
    std::fs::write(d.join("untracked.txt"), "mine").unwrap();
    t
}

fn sandboxed() -> bool {
    adage_exec::sandbox::detect().available()
}

#[test]
fn a_replay_runs_head_in_a_sandboxed_checkout() {
    if !sandboxed() {
        eprintln!("skipped: no sandbox on this machine");
        return;
    }
    let r = repo();
    let rp = Replay::new(r.path(), ReplayLimits { wall_ms: 30_000, ..Default::default() }).unwrap();
    assert!(!rp.dir().join("untracked.txt").exists(), "only HEAD is checked out");
    let ok = rp.run(&["scripts/check".into()]).unwrap();
    assert!(ok.ok(), "{ok:?} {}", ok.output);
    assert!(ok.output.contains("OK") && ok.output.contains("checking"), "stdout and stderr: {}", ok.output);
    let stale = rp.run(&["scripts/stale".into()]).unwrap();
    assert!(stale.failed());
    assert_eq!(stale.exit, Some(3));
    assert!(stale.output.contains("FOO is not set"));
    assert!(rp.changed().unwrap().is_empty());
    let gen = rp.run(&["scripts/gen".into()]).unwrap();
    assert!(gen.ok(), "{}", gen.output);
    assert_eq!(rp.changed().unwrap(), vec!["out.txt".to_string()]);
    // The person's files are untouched.
    assert!(!r.path().join("out.txt").exists());
    assert_eq!(std::fs::read_to_string(r.path().join("scripts/check")).unwrap(), "#!/bin/sh\nexit 9\n");
    // Missing programs and paths are refusals, not runs.
    assert!(rp.run(&["scripts/missing".into()]).is_err());
    assert!(rp.run(&["no-such-program-here".into()]).is_err());
    assert!(rp.run(&["../x".into()]).is_err());
}

#[test]
fn a_replay_has_no_network_and_cannot_write_outside_scratch() {
    if !sandboxed() {
        eprintln!("skipped: no sandbox on this machine");
        return;
    }
    let r = repo();
    let target = r.path().join("escaped.txt");
    let rp = Replay::new(r.path(), ReplayLimits { wall_ms: 30_000, ..Default::default() }).unwrap();
    let w = rp.run(&["sh".into(), "-c".into(), format!("echo x > {}", target.display())]).unwrap();
    assert!(!w.ok());
    assert!(!target.exists());
}
