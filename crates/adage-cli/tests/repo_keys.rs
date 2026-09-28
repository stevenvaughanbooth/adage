//! Repository keys end to end: two unrelated repositories without `origin` that share a directory
//! name. `adage condition` keys one's routes by `<name>@<root commit>`, its hooks draw them, and the
//! other's do not; routes learned under the bare name before the change keep working until a
//! repository of that name is re-keyed. Needs git to build the repositories (skipped without it);
//! Adage itself never runs git.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let o = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "a")
        .env("GIT_AUTHOR_EMAIL", "a@b")
        .env("GIT_COMMITTER_NAME", "a")
        .env("GIT_COMMITTER_EMAIL", "a@b")
        .output()
        .ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

/// A repository at `dir` with one commit of `content`; its root commit.
fn repo(dir: &Path, content: &str) -> Option<String> {
    std::fs::create_dir_all(dir).unwrap();
    git(dir, &["init", "-q", "-b", "main"])?;
    std::fs::write(dir.join("README.md"), content).unwrap();
    git(dir, &["add", "README.md"])?;
    git(dir, &["commit", "-q", "-m", "init"])?;
    git(dir, &["rev-parse", "HEAD"])
}

/// A Claude Code transcript: a failing `python3 -m pytest`, then `make test` passing.
fn transcript(dir: &Path, sid: &str, cwd: &Path) -> PathBuf {
    let cwd = cwd.to_string_lossy();
    let mut lines = vec![
        json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":"Fix the rounding bug."}}),
    ];
    for (n, (cmd, out, err)) in
        [("python3 -m pytest tests", "/usr/bin/python3: No module named pytest", true), ("make test", "OK", false)]
            .iter()
            .enumerate()
    {
        let id = format!("toolu_{n}");
        lines.push(json!({"type":"assistant","sessionId":sid,"cwd":cwd,"message":{"id":format!("msg_{n}"),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":"Bash","input":{"command":cmd}}]}}));
        lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":out,"is_error":err}]}}));
    }
    let p = dir.join(format!("{sid}.jsonl"));
    std::fs::write(&p, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
    p
}

fn adage(home: &Path, args: &[&str], stdin: &str) -> String {
    let mut c = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(args)
        .env("ADAGE_HOME", home)
        .env("ADAGE_AUTO_LEARN", "0")
        .env("ADAGE_ARM", "treated")
        .env_remove("ADAGE_DISABLE")
        .env_remove("CLAUDECODE")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin.take().unwrap().write_all(stdin.as_bytes()).unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    String::from_utf8(o.stdout).unwrap()
}

fn prompt(home: &Path, cwd: &Path, sid: &str) -> String {
    let p = json!({ "session_id": sid, "cwd": cwd, "prompt": "Fix the rounding bug." });
    adage(home, &["hook", "claude", "UserPromptSubmit"], &p.to_string())
}

#[test]
fn unrelated_repositories_with_one_name_do_not_share_routes() {
    let d = tempfile::tempdir().unwrap();
    let root = d.path().canonicalize().unwrap();
    let (a, b) = (root.join("x/app"), root.join("y/app"));
    let Some(root_a) = repo(&a, "one") else {
        eprintln!("git not available: skipping");
        return;
    };
    let root_b = repo(&b, "two").unwrap();
    assert_ne!(root_a, root_b);

    let home = root.join("home");
    let files: Vec<String> =
        (0..3).map(|i| transcript(&root, &format!("s{i}"), &a).to_string_lossy().to_string()).collect();
    let mut args = vec!["condition", "--demo", "--json", "--transcripts"];
    args.extend(files.iter().map(String::as_str));
    let report: Value = serde_json::from_str(&adage(&home, &args, "")).unwrap();
    let key = format!("app@{}", &root_a[..12]);
    assert!(report["repos"].get(&key).is_some(), "learned under {key}: {report}");
    assert_eq!(std::fs::read_dir(home.join("repo-keys")).unwrap().count(), 1, "learn leaves the root for hooks");

    assert!(prompt(&home, &a, "a1").contains("dead-end-"), "the repository the routes were learned in");
    assert_eq!(prompt(&home, &b, "b1"), "", "an unrelated repository with the same name");

    // An index from before the change: routes keyed by the bare name still reach the repository
    // (either one: the bare name cannot tell them apart) ...
    let index = std::fs::read_to_string(home.join("index.json")).unwrap();
    std::fs::write(home.join("index.json"), index.replace(&format!("\"{key}\""), "\"app\"")).unwrap();
    assert!(prompt(&home, &b, "b2").contains("dead-end-"), "legacy key");
    // ... until a repository of that name is re-keyed: then a bare name is ambiguous, and ignored.
    let legacy = std::fs::read_to_string(home.join("index.json")).unwrap();
    let mut v: Value = serde_json::from_str(&legacy).unwrap();
    let mut extra = v["routes"][0].clone();
    extra["repo"] = json!(key);
    v["routes"].as_array_mut().unwrap().push(extra);
    std::fs::write(home.join("index.json"), v.to_string()).unwrap();
    assert_eq!(prompt(&home, &b, "b3"), "", "legacy key superseded");
    assert!(prompt(&home, &a, "a2").contains("dead-end-"));
}
