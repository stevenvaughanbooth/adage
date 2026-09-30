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

/// Exercise the actual macOS backend: the cross-backend escape helper currently enumerates
/// Linux backends only. A successful loader startup must not grant reads of root descendants.
#[cfg(target_os = "macos")]
#[test]
fn macos_loader_access_preserves_file_and_network_confinement() {
    use std::net::TcpListener;

    assert!(sandboxed(), "macOS smoke test requires the production sandbox");
    let r = repo();
    let secret = r.path().join("outside-secret.txt");
    std::fs::write(&secret, "OUTSIDE-SENTINEL-794\n").unwrap();
    let rp = Replay::new(r.path(), ReplayLimits { wall_ms: 5_000, ..Default::default() }).unwrap();
    let ordinary = rp.run(&["sh".into(), "-c".into(), "printf loader-ok".into()]).unwrap();
    assert!(ordinary.ok(), "{ordinary:?}");
    assert_eq!(ordinary.output, "loader-ok");
    // cat receives a separate argv value, not shell-interpolated source.
    let denied = rp.run(&["cat".into(), secret.to_string_lossy().into_owned()]).unwrap();
    assert!(!denied.ok());
    assert!(!denied.output.contains("OUTSIDE-SENTINEL-794"));

    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let script = format!("printf network-probe > /dev/tcp/127.0.0.1/{}", listener.local_addr().unwrap().port());
    // Establish the identical harmless command can connect outside the sandbox. A missing
    // tool, unsupported shell feature or inaccessible listener cannot count as containment.
    let control = Command::new("bash").args(["-c", &script]).output().unwrap();
    assert!(control.status.success(), "{}", String::from_utf8_lossy(&control.stderr));
    drop(listener.accept().unwrap());
    let ready = rp.run(&["bash".into(), "-c".into(), "printf bash-ready".into()]).unwrap();
    assert!(ready.ok(), "{ready:?}");
    let net = rp.run(&["bash".into(), "-c".into(), script]).unwrap();
    assert!(!net.ok());
    assert!(!net.timed_out);
    assert_eq!(listener.accept().unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
}
