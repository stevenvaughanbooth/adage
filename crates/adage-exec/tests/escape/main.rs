//! L0-sec: the sandbox escape suite (Track C §6). Required: **0 escapes**.
//!
//! Every test runs its hostile payload under each sandbox backend this machine offers (Landlock
//! with seccomp; bubblewrap when installed). When none is available, each test instead asserts
//! that nothing runs at all (fail closed). Payloads are `/bin/sh` or `python3` scripts; paths are
//! interpolated into the script text, since the sandboxed environment is built from nothing.
//!
//! To make sure the *sandbox* is what stops each attempt (not ordinary file permissions), targets
//! are world-readable or world-writable wherever that matters.
//!
//! Run with `--nocapture` to see one `contained:` line per payload and backend.

#![cfg(unix)]

mod helpers;

use adage_exec::sandbox::Backend;
use helpers::*;
use std::net::TcpListener;
use std::os::unix::fs::symlink;
use std::path::Path;

#[test]
fn reports_capability() {
    let cap = adage_exec::sandbox::detect();
    eprintln!("sandbox capability: {}", cap.line());
    eprintln!(
        "  landlock abi: {:?}, user namespaces: {}, bwrap: {:?}",
        cap.landlock_abi, cap.user_namespaces, cap.bwrap
    );
    if !cap.available() {
        assert!(matches!(cap.backend, Backend::Unavailable { .. }));
    }
}

#[test]
fn sanity_the_sandbox_runs_ordinary_commands() {
    for_each_backend("sanity", |b| {
        let f = Fixture::new();
        std::fs::write(f.repo.path().join("hello.txt"), "hello from the repo\n").unwrap();
        let o = b.sh(&f, "cat hello.txt && echo scratch > \"$HOME/x\" && cat \"$HOME/x\" && echo \"$TMPDIR\" | grep -q adage-exec && echo ok", 10_000);
        assert_eq!(o.exit, Some(0), "stderr: {}", String::from_utf8_lossy(&o.stderr));
        assert_eq!(String::from_utf8_lossy(&o.stdout), "hello from the repo\nscratch\nok\n");
    });
}

#[test]
fn write_outside_scratch() {
    for_each_backend("write outside scratch", |b| {
        let f = Fixture::new();
        let out = f.outside.path();
        let repo = f.repo.path();
        let tmp_target = format!("/tmp/adage-escape-{}-{}", std::process::id(), b.name);
        let script = format!(
            "echo pwned > {o}/pwned; echo pwned > {r}/pwned; echo pwned > {t}; mkdir {o}/d; touch {r}/.git-pwned; \
             rm -f {r}/keep.txt; echo done",
            o = out.display(),
            r = repo.display(),
            t = tmp_target
        );
        std::fs::write(repo.join("keep.txt"), "keep").unwrap();
        let o = b.sh(&f, &script, 10_000);
        assert_eq!(String::from_utf8_lossy(&o.stdout), "done\n");
        for p in [out.join("pwned"), repo.join("pwned"), out.join("d"), repo.join(".git-pwned")] {
            assert!(!p.exists(), "ESCAPE: {} was written", p.display());
        }
        assert!(!Path::new(&tmp_target).exists(), "ESCAPE: {tmp_target} was written");
        assert!(repo.join("keep.txt").exists(), "ESCAPE: a repository file was deleted");
    });
}

#[test]
fn connect_to_loopback_and_internet() {
    if !has("python3") {
        eprintln!("skipped: python3 not installed");
        return;
    }
    for_each_backend("network", |b| {
        let f = Fixture::new();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let port = listener.local_addr().unwrap().port();
        let script = format!(
            "import socket\n\
             for host, port in [('127.0.0.1', {port}), ('1.1.1.1', 443), ('1.1.1.1', 53)]:\n\
             \x20   try:\n\
             \x20       s = socket.create_connection((host, port), timeout=2)\n\
             \x20       print('CONNECTED', host)\n\
             \x20   except Exception as e:\n\
             \x20       print('blocked', host, type(e).__name__)\n\
             for fam in (socket.AF_INET6, socket.AF_NETLINK, socket.AF_UNIX):\n\
             \x20   try:\n\
             \x20       socket.socket(fam, socket.SOCK_DGRAM)\n\
             \x20       print('SOCKET', fam)\n\
             \x20   except Exception as e:\n\
             \x20       print('blocked', fam)\n"
        );
        let o = b.py(&f, &script, 15_000);
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(!out.contains("CONNECTED"), "ESCAPE: {out}");
        let blocked = out.matches("blocked").count();
        if b.name.starts_with("landlock") {
            assert_eq!(blocked, 6, "seccomp denies every socket: {out}\n{}", String::from_utf8_lossy(&o.stderr));
        } else {
            assert!(blocked >= 3, "{out}\n{}", String::from_utf8_lossy(&o.stderr));
        }
        assert!(listener.accept().is_err(), "ESCAPE: the loopback listener received a connection");
    });
}

#[test]
fn read_ssh_keys_in_a_fake_home() {
    for_each_backend("read ~/.ssh", |b| {
        let f = Fixture::new();
        let ssh = f.fake_home.path().join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        std::fs::write(ssh.join("id_ed25519"), "PRIVATE-KEY-SECRET-1234\n").unwrap();
        world_readable(f.fake_home.path());
        let script = format!(
            "cat {k}; cat ~/.ssh/id_ed25519; ls {h}; cat /root/.ssh/id_* /home/*/.ssh/id_* /etc/shadow; echo done",
            k = ssh.join("id_ed25519").display(),
            h = f.fake_home.path().display()
        );
        let o = b.sh(&f, &script, 10_000);
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(!out.contains("PRIVATE-KEY-SECRET"), "ESCAPE: {out}");
        assert!(!out.contains(".ssh"), "ESCAPE: listed the fake home: {out}");
        assert!(!out.contains("root:"), "ESCAPE: read /etc/shadow: {out}");
        assert!(out.ends_with("done\n"));
    });
}

#[test]
fn ptrace_and_proc_memory() {
    if !has("python3") {
        eprintln!("skipped: python3 not installed");
        return;
    }
    for_each_backend("ptrace", |b| {
        let f = Fixture::new();
        let me = std::process::id();
        let script = format!(
            "import ctypes, os\n\
             libc = ctypes.CDLL(None, use_errno=True)\n\
             print('attach', libc.ptrace(16, {me}, 0, 0))\n\
             print('traceme', libc.ptrace(0, 0, 0, 0))\n\
             for p in ['/proc/{me}/environ', '/proc/{me}/mem', '/proc/{me}/cmdline', '/proc/1/environ']:\n\
             \x20   try:\n\
             \x20       data = open(p, 'rb').read(4096)\n\
             \x20       print('READ', p, data.replace(bytes(1), b' ').decode(errors='replace'))\n\
             \x20   except Exception as e:\n\
             \x20       print('blocked', p)\n"
        );
        let o = b.py(&f, &script, 10_000);
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains("attach -1"), "ESCAPE: ptrace attach succeeded: {out}");
        if b.name.starts_with("landlock") {
            assert!(out.contains("traceme -1"), "ptrace is denied by seccomp: {out}");
        }
        assert!(!out.contains(&format!("READ /proc/{me}/")), "ESCAPE: {out}");
        if b.name.starts_with("landlock") {
            assert!(!out.contains("READ /proc/1/"), "ESCAPE: {out}");
        } else {
            // Under bwrap, /proc is the sandbox's own pid namespace: its pid 1 is bwrap's init,
            // whose environment is the scrubbed one, never the host's or this test's.
            assert!(!out.contains("CARGO_") && !out.contains("CLAUDECODE"), "ESCAPE: {out}");
        }
    });
}

#[test]
fn fork_bomb_is_bounded() {
    if !has("python3") {
        eprintln!("skipped: python3 not installed");
        return;
    }
    for_each_backend("fork bomb", |b| {
        let f = Fixture::new();
        let marker = format!("forkbomb{}{}", std::process::id(), b.name);
        let script = format!(
            "import os, sys, time\n\
             tag = '{marker}'\n\
             n = 0\n\
             try:\n\
             \x20   while n < 5000:\n\
             \x20       pid = os.fork()\n\
             \x20       if pid == 0:\n\
             \x20           try:\n\
             \x20               os.setsid()\n\
             \x20           except OSError:\n\
             \x20               pass\n\
             \x20           time.sleep(30)\n\
             \x20           os._exit(0)\n\
             \x20       n += 1\n\
             except OSError as e:\n\
             \x20   pass\n\
             print('forks', n, flush=True)\n\
             time.sleep(30)\n"
        );
        let o = b.py(&f, &script, 3_000);
        let out = String::from_utf8_lossy(&o.stdout);
        let n: usize = out.split_whitespace().nth(1).and_then(|s| s.parse().ok()).unwrap_or(0);
        assert!(n < 300, "ESCAPE: {n} forks (RLIMIT_NPROC is 256): {out}");
        assert!(o.timed_out, "the payload sleeps; the wall timeout must end it");
        std::thread::sleep(std::time::Duration::from_millis(300));
        let left = processes_with(&marker);
        assert!(left.is_empty(), "ESCAPE: {} processes survived the run: {left:?}", left.len());
    });
}

#[test]
fn ten_gigabytes_of_output_is_capped() {
    for_each_backend("10 GB output", |b| {
        let f = Fixture::new();
        let t = std::time::Instant::now();
        let o = b.sh(&f, "head -c 10737418240 /dev/zero", 20_000);
        assert!(o.truncated, "the capture cap was not hit");
        assert!(o.stdout.len() as u64 <= b.capture, "kept {} bytes", o.stdout.len());
        assert!(t.elapsed().as_secs() < 10, "took {:?}", t.elapsed());
        // Writing a huge file in scratch stops at RLIMIT_FSIZE (64 MiB).
        let o = b.sh(&f, "head -c 100000000 /dev/zero > \"$HOME/big\"; wc -c < \"$HOME/big\"", 20_000);
        let n: u64 = String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(u64::MAX);
        assert!(n <= 64 << 20, "ESCAPE: wrote {n} bytes, over RLIMIT_FSIZE");
    });
}

#[test]
fn symlink_escapes() {
    for_each_backend("symlinks", |b| {
        let f = Fixture::new();
        let secret = f.outside.path().join("secret.txt");
        std::fs::write(&secret, "OUTSIDE-SECRET\n").unwrap();
        world_readable(f.outside.path());
        world_writable(f.outside.path());
        symlink(&secret, f.repo.path().join("link")).unwrap();
        symlink(f.outside.path(), f.repo.path().join("dirlink")).unwrap();
        let script = format!(
            "cat link; cat dirlink/secret.txt; echo x > dirlink/pwned; \
             ln -s {s} \"$HOME/s\"; cat \"$HOME/s\"; ln {s} \"$HOME/hard\" && cat \"$HOME/hard\"; \
             ln -s / \"$HOME/root\"; cat \"$HOME/root{s}\"; echo done",
            s = secret.display()
        );
        let o = b.sh(&f, &script, 10_000);
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(!out.contains("OUTSIDE-SECRET"), "ESCAPE: {out}");
        assert!(!f.outside.path().join("pwned").exists(), "ESCAPE: wrote through a symlink");
        assert!(out.ends_with("done\n"), "{out}");
    });
    // Typed projection refuses a repo-path that is a symlink out of the repository.
    let f = Fixture::new();
    std::fs::write(f.outside.path().join("x"), "x").unwrap();
    symlink(f.outside.path().join("x"), f.repo.path().join("out")).unwrap();
    assert!(adage_exec::project::repo_path_ok(f.repo.path(), "out").is_err());
}

#[test]
fn proc_self_root_tricks() {
    for_each_backend("/proc/self/root", |b| {
        let f = Fixture::new();
        let secret = f.outside.path().join("secret.txt");
        std::fs::write(&secret, "PROC-ROOT-SECRET\n").unwrap();
        world_readable(f.outside.path());
        let s = secret.display();
        let script = format!(
            "cat /proc/self/root{s}; cat /proc/$$/root{s}; cat /proc/1/root{s}; cat /proc/self/cwd/../../..{s}; \
             cat /proc/self/root/etc/shadow; ls /proc/1/root/; echo done"
        );
        let o = b.sh(&f, &script, 10_000);
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(!out.contains("PROC-ROOT-SECRET"), "ESCAPE: {out}");
        assert!(!out.contains("root:"), "ESCAPE: {out}");
        assert!(out.ends_with("done\n"), "{out}");
    });
}

#[test]
fn hostile_git_config_runs_nothing_outside() {
    if !has("git") {
        eprintln!("skipped: git not installed");
        return;
    }
    for_each_backend("git fsmonitor/textconv", |b| {
        let f = Fixture::new();
        let marker = f.outside.path().join("hook-ran");
        world_writable(f.outside.path());
        let hook = f.repo.path().join("hook.sh");
        std::fs::write(
            &hook,
            format!("#!/bin/sh\necho ran > {}\necho ran > /tmp/adage-hook-ran\ncat\n", marker.display()),
        )
        .unwrap();
        std::fs::set_permissions(&hook, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        git_init(f.repo.path());
        std::fs::write(f.repo.path().join(".gitattributes"), "*.txt diff=evil\n").unwrap();
        std::fs::write(f.repo.path().join("a.txt"), "one\n").unwrap();
        git(f.repo.path(), &["add", "."]);
        git(f.repo.path(), &["commit", "-qm", "one"]);
        // The hostile config goes in last: the setup above runs git outside any sandbox.
        git(f.repo.path(), &["config", "core.fsmonitor", hook.to_str().unwrap()]);
        git(f.repo.path(), &["config", "diff.evil.textconv", hook.to_str().unwrap()]);
        std::fs::write(f.repo.path().join("a.txt"), "two\n").unwrap();
        world_readable(f.repo.path());

        let hazards = adage_exec::git::hazards(f.repo.path());
        assert!(
            hazards.contains(&"core.fsmonitor".to_string()) && hazards.contains(&"diff.evil.textconv".to_string()),
            "{hazards:?}"
        );
        let table = adage_core::CommandTable::load();
        let argv: Vec<String> = vec!["git".into(), "status".into()];
        assert!(
            adage_exec::classify::inert(&argv, table, &hazards).is_err(),
            "a hostile repository keeps git's inert status"
        );

        let root = f.repo.path().canonicalize().unwrap();
        let safe = format!("safe.directory={}", root.display());
        // Hardened, as adage-exec runs git: the fsmonitor hook is disabled outright.
        let hardened = adage_exec::git::harden(&["git".into(), "status".into(), "--porcelain".into()], Some(&root));
        let o = b.run(&f, &hardened, 10_000);
        assert_eq!(String::from_utf8_lossy(&o.stdout), " M a.txt\n", "{}", String::from_utf8_lossy(&o.stderr));
        // Unhardened (as if a person approved it anyway): whatever the hooks run is still inside
        // the sandbox, and cannot write outside scratch.
        for args in [vec!["status"], vec!["diff"], vec!["log", "-p", "-1"], vec!["show", "HEAD"]] {
            let mut argv: Vec<String> = vec!["git".into(), "-c".into(), safe.clone()];
            argv.extend(args.iter().map(|s| s.to_string()));
            let _ = b.run(&f, &argv, 10_000);
        }
        assert!(!marker.exists(), "ESCAPE: a repository-configured hook wrote outside the sandbox");
        assert!(!Path::new("/tmp/adage-hook-ran").exists(), "ESCAPE: a hook wrote to /tmp");
    });
}

#[test]
fn ripgrep_config_smuggling() {
    if !has("rg") {
        eprintln!("skipped: rg not installed");
        return;
    }
    for_each_backend("RIPGREP_CONFIG_PATH", |b| {
        let f = Fixture::new();
        let marker = f.outside.path().join("rg-pre-ran");
        world_writable(f.outside.path());
        let evil = f.repo.path().join("evil.sh");
        std::fs::write(&evil, format!("#!/bin/sh\necho ran > {}\necho SMUGGLED\n", marker.display())).unwrap();
        std::fs::set_permissions(&evil, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let rc = f.repo.path().join(".ripgreprc");
        std::fs::write(&rc, format!("--pre={}\n", evil.display())).unwrap();
        std::fs::write(f.repo.path().join("notes.txt"), "needle\n").unwrap();
        world_readable(f.repo.path());
        let smuggled = [("RIPGREP_CONFIG_PATH", rc.to_str().unwrap())];
        // The environment is built from nothing: the variable never arrives.
        let env = b.env_for(&f, &smuggled);
        assert!(!env.contains_key("RIPGREP_CONFIG_PATH"));
        let o = b.run_with(&f, &["rg".into(), "--no-messages".into(), "needle".into()], 10_000, &smuggled, false);
        let out = String::from_utf8_lossy(&o.stdout);
        assert!(out.contains("needle") && !out.contains("SMUGGLED"), "ESCAPE: {out}");
        // Defence in depth: even if it did arrive, the preprocessor runs inside the sandbox.
        let _ = b.run_with(&f, &["rg".into(), "needle".into()], 10_000, &smuggled, true);
        assert!(!marker.exists(), "ESCAPE: the smuggled preprocessor wrote outside the sandbox");
    });
}

#[test]
fn fails_closed_without_a_sandbox() {
    let f = Fixture::new();
    let b = Backendish::unavailable();
    let r = b.try_run(&f, &["/bin/sh".into(), "-c".into(), "echo ran".into()], 5_000);
    let err = r.expect_err("ran with no sandbox");
    assert_eq!(err.stage, "sandbox");
}
