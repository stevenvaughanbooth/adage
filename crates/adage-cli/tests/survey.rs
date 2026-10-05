//! `adage survey`, through the binary, over synthetic Claude Code history in an isolated `HOME`
//! (`~/.claude/projects`) and an isolated `ADAGE_HOME`: the report, its redaction, its filters,
//! and that it writes nothing.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct Tx {
    lines: Vec<Value>,
    sid: String,
    cwd: String,
    n: usize,
}

impl Tx {
    fn new(sid: &str, cwd: &str, day: &str, prompt: &str) -> Tx {
        let mut t = Tx { lines: vec![], sid: sid.into(), cwd: cwd.into(), n: 0 };
        t.lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":format!("{day}T10:00:00Z"),"message":{"role":"user","content":prompt}}));
        t
    }
    fn call(&mut self, name: &str, input: Value, output: &str, is_error: bool) -> &mut Self {
        self.n += 1;
        let id = format!("toolu_{}", self.n);
        self.lines.push(json!({"type":"assistant","sessionId":self.sid,"cwd":self.cwd,"message":{"id":format!("msg_{}", self.n),"model":"claude-test","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
        self.lines.push(json!({"type":"user","sessionId":self.sid,"cwd":self.cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":output,"is_error":is_error}]}}));
        self
    }
    fn bash(&mut self, cmd: &str, out: &str, err: bool) -> &mut Self {
        self.call("Bash", json!({"command": cmd}), out, err)
    }
    fn prompt(&mut self, text: &str) -> &mut Self {
        self.lines
            .push(json!({"type":"user","sessionId":self.sid,"cwd":self.cwd,"message":{"role":"user","content":text}}));
        self
    }
    fn write(&self, dir: &Path) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let p = dir.join(format!("{}.jsonl", self.sid));
        std::fs::write(&p, self.lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        p
    }
}

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join("home")).unwrap();
        World { _dir: dir, root }
    }
    fn home(&self) -> PathBuf {
        self.root.join("home")
    }
    fn run(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_adage"))
            .args(args)
            .current_dir(&self.root)
            .env("ADAGE_HOME", self.root.join("data"))
            .env("HOME", self.home())
            .env("CODEX_HOME", self.home().join(".codex"))
            .env("CLAUDE_CONFIG_DIR", self.home().join(".claude"))
            .env_remove("CLAUDECODE")
            .env_remove("OPENCODE")
            .env_remove("CODEX_SANDBOX")
            .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }
    fn ok(&self, args: &[&str]) -> String {
        let o = self.run(args);
        assert!(
            o.status.success(),
            "adage {args:?} failed:\n{}{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        String::from_utf8_lossy(&o.stdout).to_string()
    }
}

fn today() -> String {
    let days = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() / 86_400;
    adage_tracks::review::civil_date(days as i64)
}

/// Six recent sessions in one repository (a lookup, a schema edit then `make gen`, a failing test
/// runner then `make test`), and one old session in another.
fn history(w: &World) {
    let projects = w.home().join(".claude/projects");
    let cwd = "/fixture/acme-billing";
    let day = today();
    let secret = format!("ghp_{}", "A1b2C3d4E5".repeat(4));
    for i in 0..6 {
        let id = format!("W-10{:02}", 40 + i);
        let prompt = if i == 5 {
            format!("Pick up {id}; notes are in {}/notes.md and the token is {secret}", w.home().display())
        } else {
            format!("Pick up {id} and get it done.")
        };
        let mut t = Tx::new(&format!("s{i}"), cwd, &day, &prompt);
        t.bash("ls", "README.md\nMakefile", false)
            .bash(
                &format!("python3 scripts/wi-lookup {id}"),
                &format!("ID: {id}\nLinked files: acme/billing.py"),
                false,
            )
            .call("Read", json!({"file_path": format!("{cwd}/acme/billing.py")}), "code", false)
            .call("Edit", json!({"file_path": format!("{cwd}/schema/invoice.json")}), "ok", false)
            .bash("make gen", "generated", false)
            .bash("python3 -m pytest tests", "Traceback\nRuntimeError: ACME_ENV is not set", true)
            .bash("make test", "OK", false)
            .prompt("thanks, also update the changelog");
        t.write(&projects.join("-fixture-acme-billing"));
    }
    let mut old = Tx::new("old0", "/fixture/other", "2020-01-02", "Tidy up the README");
    old.bash("ls", "README.md", false);
    old.write(&projects.join("-fixture-other"));
}

#[test]
fn surveys_history_per_repository_and_writes_nothing() {
    let w = World::new();
    history(&w);
    let out = w.ok(&["survey", "--demo"]);
    for s in [
        "Read-only",
        "7 session(s) in 2 repository(ies)",
        "acme-billing — 6 sessions, 12 prompts",
        "turns before first edit   median 3",
        "exec python3 scripts/wi-lookup",
        "a work item in 6 sessions (W-####)",
        "dead-end candidates",
        "exec python3 -m pytest tests   then worked: exec make test",
        "6 of 6 sessions editing schema/*.json then ran exec make gen",
        "candidates only, nothing is written",
        "new       work-items/lookup-work-item",
        "finish-line/finish-line-",
        "dead-ends/dead-end-",
        "intent families           skipped",
    ] {
        assert!(out.contains(s), "report lacks {s:?}:\n{out}");
    }
    // Redaction: never the home path or the token, and prompts only as short snippets.
    assert!(!out.contains(&w.home().display().to_string()), "{out}");
    assert!(!out.contains("ghp_"), "{out}");
    assert!(out.contains("notes are in ~/notes.md and the token is [redacted]"), "{out}");
    assert!(!w.root.join("data").exists(), "the survey writes nothing");

    // JSON for tools, with the same content.
    let v: Value = serde_json::from_str(&w.ok(&["survey", "--json", "--demo"])).unwrap();
    let repos = v["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2);
    let acme = repos.iter().find(|r| r["repo"] == "acme-billing").unwrap();
    assert_eq!(acme["sessions"], 6);
    assert_eq!(acme["harnesses"], json!(["claude-code"]));
    assert_eq!(acme["turns_before_first_edit"]["median"], 3);
    let kinds: Vec<&str> =
        acme["candidates"].as_array().unwrap().iter().map(|c| c["track"].as_str().unwrap()).collect();
    for k in ["work-items", "finish-line", "dead-ends"] {
        assert!(kinds.contains(&k), "{kinds:?}");
    }
    assert!(!v.to_string().contains("ghp_"));

    // Filters: one repository; a recent window drops the old session.
    let v: Value = serde_json::from_str(&w.ok(&["survey", "--json", "--repo", "other"])).unwrap();
    assert_eq!(v["repos"].as_array().unwrap().len(), 1);
    assert_eq!(v["repos"][0]["repo"], "other");
    let v: Value = serde_json::from_str(&w.ok(&["survey", "--json", "--since", "30"])).unwrap();
    assert_eq!(v["sessions"], 6);
    assert!(w.run(&["survey", "--since", "soon"]).status.code() == Some(1));
}

#[test]
fn candidates_are_compared_with_installed_tracks() {
    let w = World::new();
    history(&w);
    // After `adage learn`, the survey shows the same routes as installed, not new.
    let files: Vec<String> = std::fs::read_dir(w.home().join(".claude/projects/-fixture-acme-billing"))
        .unwrap()
        .map(|e| e.unwrap().path().display().to_string())
        .collect();
    let mut args = vec!["learn", "--demo", "--transcripts"];
    args.extend(files.iter().map(String::as_str));
    w.ok(&args);
    let before = std::fs::read(w.root.join("data/index.json")).unwrap();
    let v: Value = serde_json::from_str(&w.ok(&["survey", "--json", "--demo", "--repo", "acme-billing"])).unwrap();
    let statuses: Vec<&str> =
        v["repos"][0]["candidates"].as_array().unwrap().iter().map(|c| c["status"].as_str().unwrap()).collect();
    assert!(!statuses.is_empty() && statuses.iter().all(|s| *s == "installed"), "{statuses:?}");
    assert_eq!(std::fs::read(w.root.join("data/index.json")).unwrap(), before, "nothing rewritten");
}

#[test]
fn an_empty_history_says_how_to_point_it_elsewhere() {
    let w = World::new();
    let out = w.ok(&["survey"]);
    assert!(out.contains("No sessions to survey"), "{out}");
}
