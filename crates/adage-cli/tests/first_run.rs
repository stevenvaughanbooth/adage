//! `adage init` and `adage doctor`, through the binary, with an isolated `HOME`, `ADAGE_HOME`,
//! `XDG_CONFIG_HOME`, `CODEX_HOME`, and a `PATH` holding only what each test puts there.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home", "bin", "xdg", "work"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        // A git repository to run in.
        std::fs::create_dir_all(root.join("work/.git")).unwrap();
        World { _dir: dir, root }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        c.args(args)
            .current_dir(self.p("work"))
            .env("ADAGE_HOME", self.p("data"))
            .env("HOME", self.p("home"))
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("ADAGE_DISABLE")
            .env_remove("ADAGE_HOLDOUT")
            .env_remove("ADAGE_ARM")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .stdin(Stdio::null());
        for v in ["CLAUDECODE", "OPENCODE", "CODEX_SANDBOX", "CODEX_SANDBOX_NETWORK_DISABLED"] {
            c.env_remove(v);
        }
        c
    }
    fn run(&self, args: &[&str]) -> Output {
        self.cmd(args).output().unwrap()
    }
    #[cfg(unix)]
    fn fake_bin(&self, name: &str, script: &str) {
        let p = self.p("bin").join(name);
        std::fs::write(&p, format!("#!/bin/sh\n{script}\n")).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    /// Everything under the isolated configuration and data directories, to prove nothing changed.
    fn snapshot(&self) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        for top in ["home", "xdg", "data", "work", "codex"] {
            let mut stack = vec![self.p(top)];
            while let Some(d) = stack.pop() {
                for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
                    let p = e.path();
                    if p.is_dir() {
                        stack.push(p);
                    } else {
                        out.push((p.display().to_string(), std::fs::read(&p).unwrap_or_default()));
                    }
                }
            }
        }
        out.sort();
        out
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn doctor(w: &World) -> (Value, bool) {
    let o = w.run(&["doctor", "--json"]);
    (serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o))), o.status.success())
}

fn check<'a>(v: &'a Value, name: &str) -> &'a Value {
    v["checks"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["check"] == name)
        .unwrap_or_else(|| panic!("no check {name}: {v:#}"))
}

#[test]
fn init_detects_harnesses_prints_snippets_and_writes_nothing() {
    let w = World::new();
    std::fs::create_dir_all(w.p("home/.claude")).unwrap();
    std::fs::create_dir_all(w.p("xdg/opencode")).unwrap();
    // A fake `codex` on PATH is a shell script, which only Unix can run.
    #[cfg(unix)]
    w.fake_bin("codex", "exit 0");
    let before = w.snapshot();
    let o = w.run(&["init"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    let claude_found = format!("claude-code  found: ~{}.claude exists", std::path::MAIN_SEPARATOR);
    #[cfg(unix)]
    assert!(t.contains("codex        found: `codex` on PATH") && t.contains("hook codex"), "{t}");
    assert!(!t.contains(r"\\?\"), "no verbatim Windows prefix in output:\n{t}");
    for s in [
        "1. Harnesses",
        claude_found.as_str(),
        "opencode     found:",
        "adage install claude-code --write",
        "\"UserPromptSubmit\"",
        ".opencode/plugins/adage.js",
        "3. Embedding model",
        "`adage models fetch`",
        "4. Survey",
        "No traces to sense in that source",
    ] {
        assert!(t.contains(s), "init output lacks {s:?}:\n{t}");
    }
    assert!(!t.contains("// Adage for OpenCode"), "the OpenCode plugin source is not dumped");
    assert_eq!(w.snapshot(), before, "init without --yes writes nothing");
    // Nothing found: says so.
    let bare = World::new();
    let t = text(&bare.run(&["init"]));
    assert!(t.contains("No harness found"), "{t}");
}

#[test]
fn init_prints_inside_an_agent_session_but_yes_refuses_there() {
    let w = World::new();
    let o = w.cmd(&["init"]).env("CLAUDECODE", "1").output().unwrap();
    assert!(o.status.success() && text(&o).contains("printing only"), "{}", text(&o));
    let o = w.cmd(&["init", "--yes"]).env("CLAUDECODE", "1").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("agent session"), "{}", text(&o));
    let o = w.cmd(&["init", "--yes"]).env_remove("ADAGE_ALLOW_NONINTERACTIVE").output().unwrap();
    assert!(!o.status.success() && text(&o).contains("not a terminal"), "{}", text(&o));
    assert!(!w.p("data").exists());
}

#[test]
#[cfg(unix)] // fakes `curl` with a shell script
fn init_yes_fetches_the_model_and_survives_a_failed_download() {
    let w = World::new();
    // No network in tests: a `curl` that fails like an unreachable host.
    w.fake_bin("curl", "echo 'curl: (6) Could not resolve host' >&2; exit 6");
    let o = w.run(&["init", "--yes"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("fetching") && t.contains("could not fetch it") && t.contains("4. Survey"), "{t}");
    assert!(!w.p("data/models/potion-base-8m").exists());
}

#[test]
fn init_scopes_the_survey_to_a_repository() {
    let w = World::new();
    let o = w.run(&["init", "--repo", w.p("work").to_str().unwrap()]);
    assert!(o.status.success() && text(&o).contains("survey your existing agent history for work"), "{}", text(&o));
    let o = w.run(&["init", "--repo", w.p("home").to_str().unwrap()]);
    assert!(!o.status.success() && text(&o).contains("not inside a git repository"), "{}", text(&o));
}

#[test]
fn doctor_on_a_fresh_machine_warns_but_does_not_fail() {
    let w = World::new();
    let (v, ok) = doctor(&w);
    assert!(ok, "{v:#}");
    assert_eq!(check(&v, "binary")["status"], "pass");
    assert_eq!(check(&v, "data directory")["status"], "pass", "not created yet is not a problem");
    assert_eq!(check(&v, "index")["status"], "warn");
    assert_eq!(check(&v, "tracks")["status"], "warn");
    assert_eq!(check(&v, "embedding model")["status"], "pass");
    assert_eq!(check(&v, "hooks: claude-code")["detail"], "harness not installed; nothing to check");
    assert_eq!(check(&v, "kill switches")["status"], "pass");
    assert_eq!(check(&v, "fail-open errors")["detail"], "none recorded");
    for c in v["checks"].as_array().unwrap() {
        assert!(c["status"] == "pass" || c.get("fix").is_some(), "every non-pass check has a fix: {c}");
    }
    assert!(!w.p("data").exists(), "doctor creates nothing");
    let o = w.run(&["doctor"]);
    assert!(text(&o).contains("adage doctor") && text(&o).contains("0 failing"), "{}", text(&o));
}

#[test]
fn doctor_checks_hooks_index_tracks_and_switches() {
    let w = World::new();
    // Claude Code installed, with Adage registered in the repository, pointing at this binary.
    std::fs::create_dir_all(w.p("home/.claude")).unwrap();
    let o = w.run(&["install", "claude-code", "--write", "--repo", w.p("work").to_str().unwrap()]);
    assert!(o.status.success(), "{}", text(&o));
    // Codex's user hooks name a binary that no longer exists.
    std::fs::create_dir_all(w.p("codex")).unwrap();
    let gone = w.p("gone/adage");
    let codex = json!({ "hooks": { "Stop": [{ "hooks": [{ "type": "command", "command": format!("{} hook codex Stop", gone.display()) }] }] } });
    std::fs::write(w.p("codex/hooks.json"), codex.to_string()).unwrap();
    // OpenCode installed but not registered.
    std::fs::create_dir_all(w.p("xdg/opencode")).unwrap();
    // A valid index with no tracks, a track that fails validation, a recent fail-open error.
    std::fs::create_dir_all(w.p("data/tracks/acme/broken")).unwrap();
    std::fs::write(w.p("data/tracks/acme/broken/track.toml"), "not toml [").unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    std::fs::write(
        w.p("data/fail-open.log"),
        format!("{}\n", json!({ "ts": now, "harness": "claude", "event": "Stop", "error": "state file locked" })),
    )
    .unwrap();
    std::fs::create_dir_all(w.p("work/.adage")).unwrap();
    std::fs::write(w.p("work/.adage/disabled"), "").unwrap();

    let (v, ok) = doctor(&w);
    assert!(!ok, "a failing check exits non-zero");
    let cc = check(&v, "hooks: claude-code");
    assert_eq!(cc["status"], "pass", "{cc}");
    assert!(cc["detail"].as_str().unwrap().contains("(this binary)"), "{cc}");
    let cx = check(&v, "hooks: codex");
    assert_eq!(cx["status"], "fail", "{cx}");
    assert!(cx["detail"].as_str().unwrap().contains("does not exist"), "{cx}");
    let oc = check(&v, "hooks: opencode");
    assert_eq!(oc["status"], "warn", "{oc}");
    assert!(oc["fix"].as_str().unwrap().contains("adage install opencode --write"), "{oc}");
    assert_eq!(check(&v, "tracks")["status"], "fail");
    assert_eq!(check(&v, "data directory")["status"], "pass");
    assert_eq!(check(&v, "kill switches")["status"], "warn");
    let fo = check(&v, "fail-open errors");
    assert_eq!(fo["status"], "warn");
    assert!(fo["detail"].as_str().unwrap().contains("state file locked"), "{fo}");

    // A damaged index fails; a stale one warns.
    std::fs::remove_dir_all(w.p("data/tracks/acme/broken")).unwrap();
    std::fs::write(w.p("data/index.json"), json!({ "generation": "0000000000000000", "routes": [] }).to_string())
        .unwrap();
    let (v, _) = doctor(&w);
    assert_eq!(check(&v, "index")["status"], "fail", "{v:#}");
    std::fs::remove_file(w.p("codex/hooks.json")).unwrap();
    std::fs::remove_file(w.p("data/fail-open.log")).unwrap();
    let o = w.cmd(&["doctor", "--json"]).env("ADAGE_DISABLE", "1").output().unwrap();
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert!(check(&v, "kill switches")["detail"].as_str().unwrap().contains("ADAGE_DISABLE=1"));
}

#[test]
fn doctor_verifies_the_index_against_the_tracks() {
    let w = World::new();
    let learned = learn_fixture(&w);
    assert!(learned, "fixture learned");
    let (v, ok) = doctor(&w);
    assert!(ok, "{v:#}");
    assert_eq!(check(&v, "index")["status"], "pass", "{v:#}");
    assert_eq!(check(&v, "tracks")["status"], "pass", "{v:#}");
    // Editing a track without recompiling leaves the index stale.
    let dir = w.p("data/tracks/acme-billing/finish-line");
    let mut t = adage_tracks::io::read(&dir).unwrap();
    t.routes[0].summary = "edited by hand".into();
    adage_tracks::io::write(&dir, &t).unwrap();
    let (v, ok) = doctor(&w);
    assert!(ok);
    assert_eq!(check(&v, "index")["status"], "warn", "{v:#}");
}

/// Learn the golden corpus into the world's data directory.
fn learn_fixture(w: &World) -> bool {
    let cwd = "/fixture/acme-billing";
    let dir = w.p("tx");
    std::fs::create_dir_all(&dir).unwrap();
    let mut files = vec![];
    for i in 0..6 {
        let sid = format!("s{i}");
        let mut lines = vec![
            json!({"type":"user","sessionId":sid,"cwd":cwd,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":format!("Pick up W-10{:02}", 40 + i)}}),
        ];
        for (n, (name, input, out)) in [
            ("Edit", json!({"file_path": format!("{cwd}/schema/invoice.json")}), "ok"),
            ("Bash", json!({"command": "make gen"}), "generated"),
        ]
        .into_iter()
        .enumerate()
        {
            let id = format!("toolu_{n}");
            lines.push(json!({"type":"assistant","sessionId":sid,"cwd":cwd,"message":{"id":format!("msg_{n}"),"model":"m","content":[{"type":"tool_use","id":id,"name":name,"input":input}]}}));
            lines.push(json!({"type":"user","sessionId":sid,"cwd":cwd,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":out,"is_error":false}]}}));
        }
        let p = dir.join(format!("{sid}.jsonl"));
        std::fs::write(&p, lines.iter().map(|l| l.to_string()).collect::<Vec<_>>().join("\n")).unwrap();
        files.push(p.display().to_string());
    }
    let mut args = vec!["condition", "--demo", "--repo", "acme-billing", "--transcripts"];
    args.extend(files.iter().map(String::as_str));
    let o = w.run(&args);
    o.status.success() && Path::new(&w.p("data/index.json")).exists()
}
