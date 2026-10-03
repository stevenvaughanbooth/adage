//! Regression tests for the fresh-install first-user pass (evals/launch/fresh-install.md): each
//! test is named after the finding it guards (B2 … B11, L1 … L5).
//!
//! Every test runs the `adage` binary with an isolated `HOME`, `ADAGE_HOME`, `XDG_CONFIG_HOME`,
//! `CODEX_HOME` and `TMPDIR`, no agent-session variables, and `ADAGE_TEST_NO_SPAWN=record`: a hook
//! records the learner it would start instead of starting it.

use adage_core::Effect;
use adage_tracks::why::Why;
use adage_tracks::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

mod common;

struct World {
    _dir: tempfile::TempDir,
    root: PathBuf,
    /// `ADAGE_HOME`.
    data: PathBuf,
}

impl World {
    fn new() -> World {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        for d in ["home/.claude/projects", "xdg", "codex", "tmp", "bin"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let data = root.join("data");
        World { _dir: dir, root, data }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }
    /// A repository (a directory with `.git`) at `rel`.
    fn repo(&self, rel: &str) -> PathBuf {
        let at = self.p(rel);
        std::fs::create_dir_all(at.join(".git")).unwrap();
        at
    }
    fn cmd(&self, args: &[&str], cwd: &Path) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_adage"));
        common::scrub(&mut c);
        c.args(args)
            .current_dir(cwd)
            .env("HOME", self.p("home"))
            .env("ADAGE_HOME", &self.data)
            .env("XDG_CONFIG_HOME", self.p("xdg"))
            .env("CODEX_HOME", self.p("codex"))
            .env("TMPDIR", self.p("tmp"))
            .env("PATH", format!("{}:/usr/bin:/bin", self.p("bin").display()))
            .env("ADAGE_TEST_NO_SPAWN", "record")
            .env("ADAGE_ARM", "treated")
            .env("NO_COLOR", "1")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        c.env_remove("XDG_DATA_HOME");
        c
    }
    fn run_in(&self, cwd: &Path, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
        let mut c = self.cmd(args, cwd);
        for (k, v) in env {
            c.env(k, v);
        }
        let mut child = c.spawn().unwrap();
        let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
        child.wait_with_output().unwrap()
    }
    fn run(&self, args: &[&str]) -> Output {
        self.run_in(&self.root, args, "", &[])
    }
    /// One Claude Code hook event: exit 0 and nothing on stderr, always.
    fn hook(&self, event: &str, cwd: &Path, sid: &str) -> String {
        let p = json!({ "session_id": sid, "cwd": cwd, "hook_event_name": event, "source": "startup", "transcript_path": "/dev/null" });
        let o = self.run_in(&self.root, &["hook", "claude", event], &p.to_string(), &[]);
        assert!(o.status.success() && o.stderr.is_empty(), "{}", text(&o));
        String::from_utf8_lossy(&o.stdout).to_string()
    }
    fn spawned(&self) -> Vec<String> {
        std::fs::read_to_string(self.data.join("learn-queue/spawn-record.jsonl"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }
    fn doctor(&self, cwd: &Path) -> Value {
        let o = self.run_in(cwd, &["doctor", "--json"], "", &[]);
        serde_json::from_slice(&o.stdout).unwrap_or_else(|_| panic!("{}", text(&o)))
    }
    /// `n` Claude Code traces recorded in `cwd` (only their names are counted before learning).
    fn traces(&self, cwd: &Path, n: usize) {
        let dir = self.p("home/.claude/projects").join(adage_sim::claude::project_dir(&cwd.to_string_lossy()));
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..n {
            std::fs::write(dir.join(format!("0000000{i}-trace.jsonl")), "{}\n").unwrap();
        }
    }
}

fn text(o: &Output) -> String {
    format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr))
}

fn warnings(doctor: &Value) -> Vec<String> {
    doctor["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|c| c["status"] != "pass")
        .map(|c| format!("{}: {}", c["check"], c["detail"]))
        .collect()
}

fn route(id: &str, trigger: Trigger, step: Step, strength: Strength, support: u32, opportunities: u32) -> Route {
    Route {
        id: id.into(),
        summary: "s".into(),
        trigger,
        step,
        slots: vec![],
        effect: Effect::Verify,
        strength,
        state: RouteState::Firm,
        exit: vec![Exit::IntentChanges],
        evidence: Evidence {
            support,
            opportunities,
            rate: if opportunities > 0 { support as f64 / opportunities as f64 } else { 0.0 },
            lower_bound: -0.0004,
            provenance: "desire-path".into(),
            origin: Some(Origin::Learned),
            ..Default::default()
        },
        authored: false,
    }
}

fn track(repo: &str, routes: Vec<Route>) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: format!("{repo}/learned"),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: repo.into() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

/// B2: a session opened outside a repository (`~`, `/`, a directory since deleted) says nothing,
/// starts no learner, and leaves nothing for doctor to warn about; a request an older Adage queued
/// for such a directory is not a warning either, and its learner exits quietly.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn b2_outside_a_repository_nothing_is_learned_or_said() {
    let w = World::new();
    let gone = w.p("gone");
    std::fs::create_dir_all(&gone).unwrap();
    let gone_cwd = gone.clone();
    std::fs::remove_dir_all(&gone).unwrap();
    for (i, cwd) in [w.p("home"), PathBuf::from("/"), gone_cwd].iter().enumerate() {
        let sid = format!("s-out-{i}");
        assert_eq!(w.hook("SessionStart", cwd, &sid), "", "no notice in {}", cwd.display());
        assert_eq!(w.hook("Stop", cwd, &sid), "");
        assert_eq!(w.hook("SessionEnd", cwd, &sid), "");
    }
    assert!(w.spawned().is_empty(), "no learner outside a repository: {:?}", w.spawned());
    let queued = std::fs::read_dir(w.data.join("learn-queue")).into_iter().flatten().flatten().count();
    assert_eq!(queued, 0, "no learn request outside a repository");
    let v = w.doctor(&w.p("home"));
    assert!(warnings(&v).iter().all(|c| !c.contains("auto-learn")), "{:?}", warnings(&v));

    // A request queued by an older Adage for `~`, and the failure its learner logged: no warning.
    std::fs::create_dir_all(w.data.join("learn-queue")).unwrap();
    let home = w.p("home");
    let req = json!({ "repo": "home@path-f5d317b89775", "root": home, "harness": "claude", "requested_at": 1 });
    std::fs::write(w.data.join("learn-queue/home-path-f5d317b89775.json"), req.to_string()).unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
    let line = json!({ "ts": now, "kind": "condition_error", "trigger": "auto", "repo": "home@path-f5d317b89775", "class": "condition", "error": "conditioning failed" });
    std::fs::write(w.data.join("learn.log"), format!("{line}\n")).unwrap();
    let v = w.doctor(&w.p("home"));
    assert!(warnings(&v).iter().all(|c| !c.contains("auto-learn")), "{:?}", warnings(&v));
    // Its learner, started by hand as the hook would have: exits 0 and logs nothing more.
    let o = w.run(&["condition", "--auto", "--repo-root", &home.to_string_lossy()]);
    assert!(o.status.success(), "{}", text(&o));
    assert_eq!(std::fs::read_to_string(w.data.join("learn.log")).unwrap().lines().count(), 1);

    // In a repository the first session still learns and says so.
    let repo = w.repo("work/acme");
    let out = w.hook("SessionStart", &repo, "s-in");
    assert!(out.contains("adage: learning from this repo"), "{out}");
    assert_eq!(w.spawned().len(), 1);
}

/// QA first-run #8 and #5: a repository set up from a terminal (its reflexes indexed before any
/// session) says what is in effect at its first session, once; and `adage report` counts what was
/// withheld from control traces only, not every directive again.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn a_repository_set_up_from_a_terminal_says_so_at_its_first_session() {
    let w = World::new();
    let repo = w.repo("work/click");
    let key = adage_core::repo::repo_key(&repo);
    let start =
        route("tests-first", Trigger::SessionStart, Step::Run { command: "make test".into() }, Strength::Prime, 9, 10);
    let mut t = track("click", vec![start]);
    t.manifest.scope.repo = key;
    io::write(&w.data.join("tracks/click/learned"), &t).unwrap();
    let o = w.run_in(&repo, &["condition", "--instincts-only"], "", &[]);
    assert!(o.status.success(), "{}", text(&o));
    let first = w.hook("SessionStart", &repo, "s1");
    let msg: Value = serde_json::from_str(first.trim()).unwrap_or_else(|_| panic!("a notice: {first:?}"));
    assert_eq!(
        msg["systemMessage"],
        "adage: 1 reflex active in this repo; it keeps learning as you work. Nothing runs without you."
    );
    assert_eq!(w.hook("SessionStart", &repo, "s2"), "", "said once");
    // A treated session: its directive is sent, and nothing was withheld.
    let prompt = |sid: &str, arm: &str| {
        let p = json!({ "session_id": sid, "cwd": repo, "hook_event_name": "UserPromptSubmit", "prompt": "fix it", "transcript_path": "/dev/null" });
        let o = w.run_in(&w.root, &["hook", "claude", "UserPromptSubmit"], &p.to_string(), &[("ADAGE_ARM", arm)]);
        String::from_utf8_lossy(&o.stdout).to_string()
    };
    assert!(prompt("s1", "treated").contains("make test"));
    let t = text(&w.run(&["report"]));
    assert!(t.contains("directives  1 sent") && !t.contains("withheld"), "{t}");
    // A control session: what it would have said is withheld, and counted as such.
    assert_eq!(prompt("s3", "control"), "");
    let t = text(&w.run(&["report"]));
    assert!(t.contains("directives  1 sent · 1 withheld in control traces"), "{t}");
    let j: Value = serde_json::from_slice(&w.run(&["report", "--json"]).stdout).unwrap();
    assert_eq!((j["totals"]["directives"].as_u64(), j["totals"]["withheld"].as_u64()), (Some(1), Some(1)), "{j}");
}

/// B3: `adage status` and the `adage watch` header show the bundled model, not "no model", and do
/// not send the person to `adage models fetch`.
#[test]
fn b3_status_and_watch_show_the_bundled_model() {
    let w = World::new();
    let repo = w.repo("work/acme");
    let centroid = vec![1000i16; 8];
    let intent = route(
        "intent-tests",
        Trigger::Intent { family: "f".into(), model: "potion-base-8m-i8@x".into(), centroid, threshold: 0.8 },
        Step::Run { command: "make test".into() },
        Strength::Prime,
        5,
        6,
    );
    io::write(&w.data.join("tracks/acme/learned"), &track("acme", vec![intent])).unwrap();
    let o = w.run_in(&repo, &["status"], "", &[]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("intent model") && t.contains("potion-base-8m-i8"), "{t}");
    assert!(t.contains("(bundled in this binary)"), "{t}");
    assert!(!t.contains("not installed") && !t.contains("models fetch"), "{t}");

    let o = w.run_in(&repo, &["watch", "--once", "--size", "110x30"], "", &[]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("bundled model") && !t.contains("no model"), "{t}");
}

/// B4: switching the model hooks embed with says which reflexes stop matching prompts, and asks
/// for their repository to be conditioned again at once.
#[test]
#[cfg_attr(not(debug_assertions), ignore = "requires debug-only ADAGE_TEST_NO_SPAWN instrumentation")]
fn b4_switching_models_requeues_what_was_learned_with_the_old_one() {
    let w = World::new();
    let repo = w.repo("work/acme");
    let bundled = adage_intent::model::resolve(&w.data.join("models"), false).embedder.id();
    let card = route(
        "card-5c06f78810",
        Trigger::Intent { family: "f".into(), model: bundled.clone(), centroid: vec![1000i16; 8], threshold: 0.8 },
        Step::Run { command: "make test".into() },
        Strength::Prime,
        5,
        6,
    );
    let t = track("acme", vec![card]);
    Index::compile(std::slice::from_ref(&t)).write_atomic(&w.data.join("index.json")).unwrap();
    std::fs::create_dir_all(w.data.join("learn-queue")).unwrap();
    let req = json!({ "repo": "acme", "root": repo, "harness": "claude", "requested_at": 1 });
    std::fs::write(w.data.join("learn-queue/acme.json"), req.to_string()).unwrap();

    let o = w.run(&["models", "use", "hashed"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains(&format!("acme: 1 reflex that matches prompts learned with {bundled} does not fire")), "{t}");
    assert!(t.contains("conditioning it again in the background now"), "{t}");
    let spawned = w.spawned();
    assert_eq!(spawned.len(), 1, "{spawned:?}");
    assert!(spawned[0].contains(&*repo.to_string_lossy()), "{spawned:?}");

    // Back to the model it was learned with: nothing to say.
    let o = w.run(&["models", "use", "auto"]);
    assert!(o.status.success() && !text(&o).contains("does not fire"), "{}", text(&o));
}

/// B5: `adage install --write` shows the diff, keeps the previous file, asks at a terminal unless
/// `--yes`, and says so when there is nothing to change.
#[test]
fn b5_install_write_shows_the_diff_and_keeps_a_backup() {
    let w = World::new();
    let settings = w.p("home/.claude/settings.json");
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "{\n  \"model\": \"opus\"\n}\n").unwrap();
    let args = ["install", "claude-code", "--write", "--scope", "user", "--bin", "/opt/adage"];
    let ok = [("ADAGE_ALLOW_NONINTERACTIVE", "1")];
    let o = w.run_in(&w.root, &args, "", &ok);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("settings.json would change:") && t.contains("+ ") && t.contains("\"/opt/adage\""), "{t}");
    let backup = w.p("home/.claude/settings.json.adage-bak");
    assert_eq!(std::fs::read_to_string(&backup).unwrap(), "{\n  \"model\": \"opus\"\n}\n");
    assert!(t.contains("the previous version is in") && t.contains("wrote"), "{t}");
    let after = std::fs::read_to_string(&settings).unwrap();
    assert!(after.contains("\"model\": \"opus\"") && after.contains("SessionStart"), "{after}");
    // Again: nothing to change, nothing written.
    let o = w.run_in(&w.root, &args, "", &ok);
    assert!(text(&o).contains("nothing to write"), "{}", text(&o));

    // At a terminal (`script` gives it one): "n" writes nothing; `--yes` does not ask.
    #[cfg(target_os = "linux")]
    if Path::new("/usr/bin/script").exists() {
        std::fs::write(&settings, "{}\n").unwrap();
        let line = format!("{} install claude-code --write --scope user --bin /opt/adage", env!("CARGO_BIN_EXE_adage"));
        let at_terminal = |line: &str, answer: &str| {
            let mut c = w.cmd(&["-qec", line, "/dev/null"], &w.root);
            let mut s = Command::new("/usr/bin/script");
            s.args(["-qec", line, "/dev/null"]);
            for (k, v) in c.get_envs() {
                match v {
                    Some(v) => s.env(k, v),
                    None => s.env_remove(k),
                };
            }
            let mut child = s
                .current_dir(&w.root)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();
            let _ = child.stdin.take().unwrap().write_all(answer.as_bytes());
            let _ = &mut c;
            child.wait_with_output().unwrap()
        };
        let t = text(&at_terminal(&line, "n\n"));
        // Only where `script` really gave it a terminal.
        if t.contains("Write it? [y/N]") {
            assert!(t.contains("nothing written"), "{t}");
            assert_eq!(std::fs::read_to_string(&settings).unwrap(), "{}\n", "declined: unchanged");
            let t = text(&at_terminal(&format!("{line} --yes"), ""));
            assert!(!t.contains("Write it?") && t.contains("wrote"), "{t}");
            assert!(std::fs::read_to_string(&settings).unwrap().contains("SessionStart"));
        }
    }
}

/// B7: the evidence `adage status` shows is the evidence the strength was decided on: a finish
/// line that formed on its value says so (not `0/7 traces, lb -0.00`), and says it is enforced
/// at Stop.
#[test]
fn b7_status_shows_the_evidence_the_decision_used() {
    let w = World::new();
    let finish = route(
        "run-3e72af",
        Trigger::AfterEdit { paths: "src/click/*.py".into() },
        Step::Run { command: ".venv/bin/python -m pytest tests".into() },
        Strength::Prime,
        0,
        7,
    );
    let dir = w.data.join("tracks/click/learned");
    io::write(&dir, &track("click", vec![finish])).unwrap();
    let why = Why {
        shape: "run".into(),
        pattern: "[edit src/click/*.py] =tests_of=> [.venv/bin/python -m pytest {tests}]: A 2.10 calls (sd 0.50, P(A>0) 0.97); Q(∅) 5.00, Q(Sat) 2.90; 7 triggers, 0 ran it; 3 files mapped; valued".into(),
        backoff: 1,
        gain: 2.1,
        lift: 0.0,
        conf: 0.97,
        conf_lb: 0.62,
        support: 0,
        days: 0,
        members: vec![],
        examples: vec![],
        prior: None,
    };
    adage_tracks::why::save(&dir, &[("run-3e72af".to_string(), why)].into_iter().collect()).unwrap();
    let o = w.run(&["status"]);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    assert!(t.contains("value: saves ~2.10 calls when followed (P(saves) 0.97, 90% lower bound 0.62)"), "{t}");
    assert!(t.contains("7 triggers, 0 ran it") && t.contains("desire path"), "{t}");
    assert!(t.contains("finish line: after an edit to src/click/*.py") && t.contains("asks it once"), "{t}");
    assert!(!t.contains("-0.00") && !t.contains("0/7"), "{t}");
}

/// B10: a fresh install, hooks registered and nothing learned yet, is healthy: a green card, no
/// doctor warning, and the repository's past traces counted.
#[test]
fn b10_a_fresh_install_is_healthy_and_counts_past_traces() {
    let w = World::new();
    let repo = w.repo("work/click");
    w.traces(&repo, 10);
    let bin = env!("CARGO_BIN_EXE_adage");
    let o = w.run_in(
        &w.root,
        &["install", "claude-code", "--write", "--scope", "user", "--bin", bin],
        "",
        &[("ADAGE_ALLOW_NONINTERACTIVE", "1")],
    );
    assert!(o.status.success(), "{}", text(&o));
    let v = w.doctor(&repo);
    assert!(warnings(&v).is_empty(), "a fresh install has nothing to fix: {:?}", warnings(&v));
    let o = w.run_in(&repo, &[], "", &[]);
    let card = text(&o);
    assert!(card.lines().next().unwrap().contains("● healthy"), "{card}");
    assert!(!card.contains("0 of ~5"), "10 past traces are counted: {card}");
    assert!(card.contains("learning    after each session · first run at your next agent session here"), "{card}");
}

/// B11: a data directory that cannot be written: one clear notice at the first session start,
/// not one at every start, and commands name the directory and the fix, not a bare OS error.
#[test]
fn b11_an_unwritable_data_directory_is_said_once_and_clearly() {
    let mut w = World::new();
    // Under a regular file, so it cannot be created whoever runs the tests (root included).
    std::fs::write(w.p("blocker"), "").unwrap();
    w.data = w.p("blocker/adage");
    let repo = w.repo("work/acme");
    let first = w.hook("SessionStart", &repo, "s1");
    let msg: Value = serde_json::from_str(first.trim()).unwrap_or_else(|_| panic!("a notice: {first:?}"));
    let msg = msg["systemMessage"].as_str().unwrap();
    assert!(msg.contains("can't write its data directory") && msg.contains("ADAGE_HOME"), "{msg}");
    assert!(!msg.contains("learning from"), "it cannot learn: {msg}");
    for sid in ["s2", "s3"] {
        assert_eq!(w.hook("SessionStart", &repo, sid), "", "said once");
    }
    let o = w.run_in(&repo, &["condition", "--instincts-only"], "", &[]);
    let t = text(&o);
    assert!(!o.status.success(), "{t}");
    assert!(
        t.contains("cannot create the data directory") && t.contains("set ADAGE_HOME to a writable directory"),
        "{t}"
    );
    assert!(!t.contains("os error"), "{t}");
    let o = w.run_in(
        &w.root,
        &["install", "claude-code", "--write", "--scope", "user", "--bin", "/opt/adage"],
        "",
        &[("ADAGE_ALLOW_NONINTERACTIVE", "1"), ("HOME", &w.p("blocker/home").to_string_lossy())],
    );
    let t = text(&o);
    // It names the file it could not write, not only the OS error.
    assert!(!o.status.success() && t.contains("blocker/home/.claude"), "{t}");
}

/// L2: `adage graph --serve` is gone, with no alias: refused, not silently ignored, and the
/// refusal names what replaced it (a usage error, exit 2).
#[test]
fn l2_graph_refuses_unknown_options() {
    let w = World::new();
    let o = w.run(&["graph", "--serve"]);
    assert_eq!(o.status.code(), Some(2), "{}", text(&o));
    assert!(text(&o).contains("`adage graph --serve` is now `adage map`"), "{}", text(&o));
    let o = w.run(&["graph", "--bogus"]);
    assert!(!o.status.success() && text(&o).contains("unexpected argument '--bogus'"), "{}", text(&o));
    let o = w.run(&["graph", "--json", "--events", "5"]);
    assert!(o.status.success(), "{}", text(&o));
}

/// L5: the Claude Code plugin and a settings registration at once run every hook twice: doctor
/// says so.
#[test]
fn l5_doctor_flags_plugin_and_settings_registered_together() {
    let w = World::new();
    let repo = w.repo("work/acme");
    let bin = env!("CARGO_BIN_EXE_adage");
    let staged = w.p("home/.claude/plugins/data/adage-adage/bin");
    std::fs::create_dir_all(&staged).unwrap();
    std::fs::copy(bin, staged.join("adage")).unwrap();
    let v = w.doctor(&repo);
    assert!(warnings(&v).iter().all(|c| !c.contains("twice")), "{:?}", warnings(&v));
    // `install --write` does not add a second registration beside the plugin's (QA first-run #2).
    let person = [("ADAGE_ALLOW_NONINTERACTIVE", "1")];
    let o = w.run_in(&w.root, &["install", "claude-code", "--write", "--scope", "user", "--bin", bin], "", &person);
    let t = text(&o);
    assert!(o.status.success() && t.contains("already registered") && t.contains("the Adage plugin"), "{t}");
    assert!(t.contains("would run every hook twice"), "{t}");
    assert!(!w.p("home/.claude/settings.json").exists(), "nothing written");
    // One made otherwise (by hand, or by an older Adage): doctor says so, with the removal.
    let o = w.run_in(&w.root, &["install", "claude-code", "--print", "--bin", bin], "", &[]);
    std::fs::write(w.p("home/.claude/settings.json"), &o.stdout).unwrap();
    let v = w.doctor(&repo);
    let twice: Vec<&Value> =
        v["checks"].as_array().unwrap().iter().filter(|c| c["check"].as_str().unwrap().contains("twice")).collect();
    assert_eq!(twice.len(), 1, "{:?}", warnings(&v));
    let detail = twice[0]["detail"].as_str().unwrap();
    assert!(detail.contains("registered by the Adage plugin and in ~/.claude/settings.json"), "{detail}");
    let fix = twice[0]["fix"].as_str().unwrap();
    assert!(fix.contains("`adage install claude-code --remove --scope user`"), "{fix}");
    let o = w.run_in(&w.root, &["install", "claude-code", "--remove", "--scope", "user"], "", &person);
    assert!(o.status.success() && text(&o).contains("removed"), "{}", text(&o));
    assert!(warnings(&w.doctor(&repo)).iter().all(|c| !c.contains("twice")));
}

/// QA first-run #2: a user-scope registration, then the documented project-scope default: no
/// second copy is written; one made by hand is flagged by `doctor` and `delegate status`, each
/// naming the `--remove` that leaves one.
#[test]
fn a_second_registration_is_refused_and_flagged() {
    let w = World::new();
    let repo = w.repo("work/acme");
    let bin = env!("CARGO_BIN_EXE_adage");
    let person = [("ADAGE_ALLOW_NONINTERACTIVE", "1")];
    let o = w.run_in(&repo, &["install", "claude-code", "--write", "--scope", "user", "--bin", bin], "", &person);
    assert!(o.status.success(), "{}", text(&o));
    let local = repo.join(".claude/settings.local.json");
    let o = w.run_in(&repo, &["install", "claude-code", "--write", "--bin", bin], "", &person);
    let t = text(&o);
    assert!(o.status.success() && t.contains("already registered") && t.contains("~/.claude/settings.json"), "{t}");
    assert!(t.contains("adage install claude-code --remove --scope user"), "{t}");
    assert!(!local.exists(), "no second copy");
    // Written by hand: doctor warns, and so does `delegate status`.
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::copy(w.p("home/.claude/settings.json"), &local).unwrap();
    let v = w.doctor(&repo);
    let twice: Vec<&Value> =
        v["checks"].as_array().unwrap().iter().filter(|c| c["check"] == "hooks: claude-code (twice)").collect();
    assert_eq!(twice.len(), 1, "{v:#}");
    assert_eq!(twice[0]["status"], "warn");
    assert!(twice[0]["fix"].as_str().unwrap().contains("`adage install claude-code --remove --scope project`"));
    let o = w.run_in(&repo, &["delegate", "status"], "", &[]);
    let t = text(&o);
    assert!(
        t.contains("claude-code is registered twice")
            && t.contains("adage install claude-code --remove --scope project"),
        "{t}"
    );
    let j: Value = serde_json::from_slice(&w.run_in(&repo, &["delegate", "status", "--json"], "", &[]).stdout).unwrap();
    assert_eq!(j["duplicates"][0]["harness"], "claude-code", "{j:#}");
    // The fix it names leaves one.
    let o = w.run_in(&repo, &["install", "claude-code", "--remove", "--scope", "project"], "", &person);
    assert!(o.status.success(), "{}", text(&o));
    assert!(warnings(&w.doctor(&repo)).iter().all(|c| !c.contains("twice")));
}

/// QA first-run #12 and #10: a user-scope install keeps the person's settings as they wrote
/// them (key order, formatting, their own hooks), and `--remove` takes Adage's hooks out again,
/// leaving the file exactly as it was; both keep a backup.
#[test]
fn install_keeps_the_persons_settings_and_remove_restores_them() {
    let w = World::new();
    let settings = w.p("home/.claude/settings.json");
    let mine = "{\n  \"model\": \"sonnet\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"echo mine\"\n          }\n        ]\n      }\n    ]\n  },\n  \"env\": {\"A\": \"1\"}\n}\n";
    std::fs::write(&settings, mine).unwrap();
    let person = [("ADAGE_ALLOW_NONINTERACTIVE", "1")];
    let args = ["install", "claude-code", "--write", "--scope", "user", "--bin", "/opt/adage"];
    let o = w.run_in(&w.root, &args, "", &person);
    let t = text(&o);
    assert!(o.status.success(), "{t}");
    // The diff only adds: nothing of the person's is shown as removed and re-added.
    assert!(!t.lines().any(|l| l.trim_start().starts_with("- ")), "{t}");
    let after = std::fs::read_to_string(&settings).unwrap();
    assert!(after.starts_with("{\n  \"model\": \"sonnet\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"echo mine\""), "{after}");
    assert!(after.ends_with("  },\n  \"env\": {\"A\": \"1\"}\n}\n"), "{after}");
    let v: Value = serde_json::from_str(&after).unwrap();
    assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2, "theirs kept, ours added");
    assert_eq!(std::fs::read_to_string(w.p("home/.claude/settings.json.adage-bak")).unwrap(), mine);
    // Removed: exactly what the person had, and a backup of what was there.
    let o = w.run_in(&w.root, &["install", "claude-code", "--remove", "--scope", "user"], "", &person);
    let t = text(&o);
    assert!(o.status.success() && t.contains("removed") && t.contains("settings.json.adage-bak"), "{t}");
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), mine);
    assert_eq!(std::fs::read_to_string(w.p("home/.claude/settings.json.adage-bak")).unwrap(), after);
    // Again: nothing to remove.
    let o = w.run_in(&w.root, &["install", "claude-code", "--remove", "--scope", "user"], "", &person);
    assert!(o.status.success() && text(&o).contains("nothing to remove"), "{}", text(&o));
    // Inside an agent session, it refuses, as --write does.
    let o = w.run_in(&w.root, &["install", "claude-code", "--remove", "--scope", "user"], "", &[("CLAUDECODE", "1")]);
    assert!(!o.status.success() && text(&o).contains("will not run inside one"), "{}", text(&o));
    // OpenCode: Adage's plugin file is removed, with a backup.
    let plugin = w.p("xdg/opencode/plugins/adage.js");
    let o =
        w.run_in(&w.root, &["install", "opencode", "--write", "--scope", "user", "--bin", "/opt/adage"], "", &person);
    assert!(o.status.success() && plugin.exists(), "{}", text(&o));
    let o = w.run_in(&w.root, &["install", "opencode", "--remove", "--scope", "user"], "", &person);
    assert!(o.status.success() && !plugin.exists(), "{}", text(&o));
    assert!(w.p("xdg/opencode/plugins/adage.js.adage-bak").exists());
}
