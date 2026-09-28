//! Smoke test against the real Claude Code CLI, when it is installed (gate G2's Claude Code leg):
//! `claude plugin validate` accepts the adage-exec plugin (its `.mcp.json` included), and
//! `claude mcp list` / `claude mcp get` connect to `adage-exec mcp` (the initialize handshake,
//! through the plugin's `scripts/mcp` and through a user-scope registration). No model API call is
//! made: only `claude plugin` and `claude mcp` subcommands run, in a throwaway `HOME`, with API
//! credentials removed and non-essential traffic off. Skipped when no `claude` is found
//! (`CLAUDE_BIN`, else `PATH`, else `/opt/node22/bin/claude`).

#![cfg(unix)]

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

fn claude() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("CLAUDE_BIN").map(PathBuf::from) {
        return p.is_file().then_some(p);
    }
    let on_path = std::env::var_os("PATH")
        .into_iter()
        .flat_map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .map(|d| d.join("claude"))
        .find(|p| p.is_file());
    on_path.or_else(|| Some(PathBuf::from("/opt/node22/bin/claude")).filter(|p| p.is_file()))
}

/// Run `claude <args>` in `cwd` with `home` as HOME, for at most 90 s; (exit code, stdout+stderr).
fn run(claude: &Path, home: &Path, cwd: &Path, args: &[&str]) -> (Option<i32>, String) {
    let exec_dir = Path::new(env!("CARGO_BIN_EXE_adage-exec")).parent().unwrap().to_path_buf();
    let path = std::env::join_paths(
        std::iter::once(exec_dir)
            .chain(claude.parent().map(Path::to_path_buf))
            .chain(std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())),
    )
    .unwrap();
    let out = home.join("out.txt");
    let f = std::fs::File::create(&out).unwrap();
    let mut c = Command::new(claude);
    c.args(args)
        .current_dir(cwd)
        .env("HOME", home)
        .env("PATH", path)
        .env("ADAGE_HOME", home.join("adage"))
        .env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1")
        .env("DISABLE_AUTOUPDATER", "1")
        .stdin(Stdio::null())
        .stdout(f.try_clone().unwrap())
        .stderr(f);
    for v in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN", "CLAUDECODE", "XDG_CONFIG_HOME"] {
        c.env_remove(v);
    }
    let mut child = c.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(90);
    let code = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s.code();
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    (code, std::fs::read_to_string(&out).unwrap_or_default())
}

#[test]
fn claude_code_parses_the_plugin_and_connects_to_the_server() {
    let Some(claude) = claude() else {
        eprintln!("skip: no claude CLI");
        return;
    };
    let home = tempfile::tempdir().unwrap();
    let home = home.path().canonicalize().unwrap();
    let proj = home.join("proj");
    std::fs::create_dir_all(&proj).unwrap();
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/claude-code-exec").canonicalize().unwrap();

    let (code, out) = run(&claude, &home, &proj, &["plugin", "validate", plugin.to_str().unwrap()]);
    if code.is_none() {
        eprintln!("skip: claude did not answer in time: {out}");
        return;
    }
    assert_eq!(code, Some(0), "claude plugin validate:\n{out}");

    let (code, out) = run(&claude, &home, &proj, &["--plugin-dir", plugin.to_str().unwrap(), "mcp", "list"]);
    eprintln!("{out}");
    assert_eq!(code, Some(0), "{out}");
    let line = out.lines().find(|l| l.contains("plugin:adage-exec:adage")).unwrap_or_else(|| panic!("{out}"));
    assert!(line.contains("Connected"), "the plugin's server did not connect: {line}");

    let bin = env!("CARGO_BIN_EXE_adage-exec");
    let cfg = serde_json::json!({ "type": "stdio", "command": bin, "args": ["mcp"] }).to_string();
    let (code, out) = run(&claude, &home, &proj, &["mcp", "add-json", "--scope", "user", "adage", &cfg]);
    assert_eq!(code, Some(0), "{out}");
    let (code, out) = run(&claude, &home, &proj, &["mcp", "get", "adage"]);
    eprintln!("{out}");
    assert_eq!(code, Some(0), "{out}");
    assert!(out.contains("Connected"), "{out}");
}
