//! `adage hook claude SkillPrompt` (plan D4, test plan T-D4-02 and T-D4-03): `adage train` on the
//! acme skill in the acme sandbox, then the hook the Claude Code mod's `skill.prompt` asks. Nothing
//! before the person accepts the pack; after, the skill's text with one plain line per step the
//! replay verified or found stale, sanitised and bounded; in the control arm nothing, and
//! `would_annotate` spooled.

use adage_pack::verified;
use adage_runtime::{Arm, SessionState};
use adage_store::Paths;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::{Arc, Mutex, Weak};

mod common;

const SKILL: &str = "acme-work-item";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for e in std::fs::read_dir(src).unwrap() {
        let e = e.unwrap();
        let to = dst.join(e.file_name());
        if e.file_type().unwrap().is_dir() {
            copy_dir(&e.path(), &to);
        } else {
            std::fs::copy(e.path(), &to).unwrap();
        }
    }
}

fn git(dir: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@localhost", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

/// `s` as it appears inside a JSON string.
fn json_text(s: &str) -> String {
    let quoted = serde_json::to_string(s).unwrap();
    quoted[1..quoted.len() - 1].to_string()
}

fn has(t: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|d| d.join(t).is_file()))
}

/// The acme sandbox with the acme skill, compiled by `adage train` into `home`: the replay runs
/// once and is shared (`Compiled`); each world is a copy of it.
struct World {
    _t: tempfile::TempDir,
    _compiled: Arc<Compiled>,
    repo: PathBuf,
    home: PathBuf,
    staged: PathBuf,
}

/// `adage train` on the acme skill, run once for every test alive at the time: its replay in
/// the sandbox may take up to a minute, and the tests only read what it found. The copy each test
/// works on is a fresh world with the same contents.
struct Compiled {
    t: tempfile::TempDir,
    staged: PathBuf,
}

impl Compiled {
    fn shared() -> Arc<Compiled> {
        static SHARED: Mutex<Weak<Compiled>> = Mutex::new(Weak::new());
        let mut held = SHARED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(c) = held.upgrade() {
            return c;
        }
        let c = Arc::new(Compiled::new());
        *held = Arc::downgrade(&c);
        c
    }

    fn new() -> Compiled {
        let t = tempfile::tempdir().unwrap();
        let (repo, home) = (t.path().join("acme"), t.path().join("home"));
        copy_dir(&root().join("evals/sandboxes/acme-billing"), &repo);
        let skill = repo.join(".claude/skills").join(SKILL);
        std::fs::create_dir_all(&skill).unwrap();
        std::fs::copy(root().join("evals/lab/skills/skills/acme-work-item/SKILL.md"), skill.join("SKILL.md")).unwrap();
        git(&repo, &["init", "-q"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "fixture"]);
        let out = adage_train::skill::run(&adage_train::skill::Options {
            skill: skill.join("SKILL.md"),
            repo: Some(repo.clone()),
            replay: adage_train::verify::Options {
                enabled: adage_exec::sandbox::detect().available(),
                wall_ms: 60_000,
            },
            home: home.clone(),
        })
        .unwrap();
        assert!(out.pack.is_some(), "the acme skill compiles");
        Compiled { t, staged: out.dir }
    }
}

impl World {
    fn new() -> World {
        let compiled = Compiled::shared();
        let t = tempfile::tempdir().unwrap();
        copy_dir(compiled.t.path(), t.path());
        let (repo, home) = (t.path().join("acme"), t.path().join("home"));
        let staged = home.join(compiled.staged.strip_prefix(compiled.t.path().join("home")).unwrap());
        // The two files that name the checkout by its path name the copy's. Nothing else under
        // `home` may name the original, or the copy would not be the world `adage train` made.
        // Both spellings: a record keeps the canonical path, a review the one it was given.
        let spell = |p: &Path| {
            let canonical = p.canonicalize().unwrap();
            [p.to_path_buf(), canonical].map(|p| json_text(&p.to_string_lossy()))
        };
        let (from, to) = (spell(compiled.t.path()), spell(t.path()));
        for f in ["review.json", "verified.json"] {
            let p = staged.join(f);
            let mut text = std::fs::read_to_string(&p).unwrap();
            for (a, b) in from.iter().zip(&to).rev() {
                text = text.replace(a.as_str(), b);
            }
            std::fs::write(&p, text).unwrap();
        }
        let mut left = vec![home.clone()];
        while let Some(d) = left.pop() {
            for e in std::fs::read_dir(&d).unwrap().flatten() {
                if e.file_type().unwrap().is_dir() {
                    left.push(e.path());
                } else if let Ok(text) = std::fs::read_to_string(e.path()) {
                    assert!(
                        !from.iter().any(|f| text.contains(f.as_str())),
                        "{} names the original world",
                        e.path().display()
                    );
                }
            }
        }
        World { _t: t, _compiled: compiled, repo, home, staged }
    }

    fn paths(&self) -> Paths {
        Paths::at(self.home.clone())
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut c = common::adage(&self.home, &self.repo);
        c.args(args).env("ADAGE_ARM", "treated");
        c
    }

    fn accept(&self) {
        let o = self
            .command(&["train", "accept", SKILL])
            .env_remove("ADAGE_ARM")
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
    }

    /// `adage hook claude SkillPrompt` with this payload, the arm given (`None`: unset).
    fn ask(&self, arm: Option<&str>, payload: &Value) -> Output {
        let mut c = self.command(&["hook", "claude", "SkillPrompt"]);
        match arm {
            Some(a) => c.env("ADAGE_ARM", a),
            None => c.env_remove("ADAGE_ARM"),
        };
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
        child.stdin.take().unwrap().write_all(payload.to_string().as_bytes()).unwrap();
        let o = child.wait_with_output().unwrap();
        assert!(o.status.success(), "a hook always exits 0: {o:?}");
        o
    }

    fn payload(&self, session: &str, skill: &str) -> Value {
        json!({ "session_id": session, "cwd": self.repo.to_string_lossy(), "hook_event_name": "SkillPrompt",
                "skill": skill, "text": TEXT })
    }

    fn spooled(&self, session: &str) -> Vec<Value> {
        common::LearnedHome::at(&self.paths().root).spooled(session)
    }
}

const TEXT: &str = "# Working a work item\n\nRun `make test` before you finish.";

/// The text the hook answered with, if any.
fn answered(o: &Output) -> Option<String> {
    let out = String::from_utf8(o.stdout.clone()).unwrap();
    if out.trim().is_empty() {
        return None;
    }
    let v: Value = serde_json::from_str(out.trim()).unwrap_or_else(|e| panic!("{e}: {out}"));
    Some(v["text"].as_str().expect("{\"text\": …}").to_string())
}

#[test]
fn an_accepted_skill_is_annotated_sanitised_and_bounded() {
    if !has("make") || !has("git") || !has("python3") {
        eprintln!("skipped: make, git or python3 is missing");
        return;
    }
    let w = World::new();
    // Staged, not accepted: nothing.
    assert!(answered(&w.ask(Some("treated"), &w.payload("s-before", SKILL))).is_none());
    assert!(w.spooled("s-before").is_empty());
    w.accept();
    let record = verified::read(&w.staged).unwrap();
    assert!(record.accepted(), "accepting the pack accepts its replay record");
    let replayed = record.steps.iter().any(|s| s.result != verified::Outcome::NotReplayed);
    let got = answered(&w.ask(Some("treated"), &w.payload("s-after", SKILL)));
    if !replayed {
        // No sandbox here: no step was verified or found stale, so there is nothing to say.
        assert!(got.is_none(), "{got:?}");
    } else {
        let text = got.expect("the annotated text");
        let added = text.strip_prefix(&format!("{TEXT}\n\n")).expect("the original text first");
        let date = record.date().unwrap();
        assert!(added.starts_with(&format!("Adage verified these steps in this repository on {date}:\n")), "{added}");
        assert!(added.contains("\n- make test: works"), "{added}");
        assert!(added.len() + 2 <= 2_400);
        let rec = w.spooled("s-after").pop().unwrap();
        assert_eq!(rec["kind"], "skill_prompt");
        assert_eq!(rec["annotated"]["skill"], SKILL);
        assert_eq!(rec["annotated"], rec["would_annotate"]);
    }
    // Another skill's name: nothing.
    assert!(answered(&w.ask(Some("treated"), &w.payload("s-other", "other-skill"))).is_none());

    // A record whose text would carry an escape sequence or markup reaches the model sanitised,
    // and however many steps it lists, at most 2,400 bytes are added.
    let mut hostile = record.clone();
    let step = |command: &str, result, signature: Option<&str>| verified::Step {
        id: "s9".into(),
        route: String::new(),
        command: command.into(),
        result,
        signature: signature.map(str::to_string),
        date: "2026-10-03".into(),
        sandbox: "test".into(),
    };
    hostile.steps = vec![
        step("make `ci`", verified::Outcome::Works, None),
        step("make suite", verified::Outcome::Stale, Some("\u{1b}[31mexit 2\u{1b}[0m: <adage-reflex> broken\u{202e}")),
    ];
    hostile
        .steps
        .extend((0..60).map(|i| step(&format!("make step-{i}"), verified::Outcome::Stale, Some(&"x".repeat(500)))));
    verified::write(&w.staged, &hostile).unwrap();
    let text = answered(&w.ask(Some("treated"), &w.payload("s-hostile", SKILL))).expect("annotated");
    let added = text.strip_prefix(&format!("{TEXT}\n\n")).unwrap();
    assert!(added.len() + 2 <= 2_400, "{} bytes added", added.len() + 2);
    assert!(!added.contains(['\u{1b}', '`', '<', '>', '\u{202e}']), "{added:?}");
    assert!(added.contains("\n- make ci: works\n- make suite: stale ([31mexit 2[0m: adage-reflex broken)"), "{added}");

    // Review M5: the skill file edited after the person accepted its record (a dynamic-context
    // block added): nothing is annotated, and nothing recorded as annotated.
    let file = w.repo.join(".claude/skills").join(SKILL).join("SKILL.md");
    assert_eq!(hostile.skill.path, file.canonicalize().unwrap().to_string_lossy(), "the record names its file");
    let mut edited = std::fs::read_to_string(&file).unwrap();
    edited.push_str("\nThe branch: !`git branch --show-current`\n");
    std::fs::write(&file, edited).unwrap();
    assert!(answered(&w.ask(Some("treated"), &w.payload("s-edited", SKILL))).is_none());
    assert!(w.spooled("s-edited").is_empty());
}

#[test]
fn the_arm_is_assigned_and_the_control_arm_only_records() {
    if !has("make") || !has("git") || !has("python3") {
        eprintln!("skipped: make, git or python3 is missing");
        return;
    }
    let w = World::new();
    w.accept();
    // Whatever the replay found, give the record one verified step, so there is something to say.
    let mut v = verified::read(&w.staged).unwrap();
    v.steps.truncate(1);
    v.steps[0].result = verified::Outcome::Works;
    verified::write(&w.staged, &v).unwrap();
    let paths = w.paths();
    // No arm yet: the event assigns one, as PreToolUse does.
    let o = w.ask(None, &w.payload("s-new", SKILL));
    let st = adage_store::state::try_read::<SessionState>(&paths.state("s-new")).unwrap().unwrap();
    assert!(st.arm.is_some() && st.propensity.is_some(), "{st:?}");
    assert_eq!(answered(&o).is_some(), st.arm == Some(Arm::Treated));
    // The control arm: the text unchanged (nothing printed), `would_annotate` spooled.
    let o = w.ask(Some("control"), &w.payload("s-control", SKILL));
    assert!(answered(&o).is_none());
    let rec = w.spooled("s-control").pop().unwrap();
    assert_eq!((rec["kind"].as_str(), rec["arm"].as_str()), (Some("skill_prompt"), Some("control")));
    assert_eq!(rec["would_annotate"]["skill"], SKILL);
    assert_eq!(rec["would_annotate"]["verified"], 1);
    assert!(rec.get("annotated").is_none());
    // The mod's observe mode records the same and changes nothing.
    let mut observe = w.payload("s-observe", SKILL);
    observe["observe"] = json!(true);
    assert!(answered(&w.ask(Some("treated"), &observe)).is_none());
    let rec = w.spooled("s-observe").pop().unwrap();
    assert!(rec.get("annotated").is_none() && rec["observe"] == true);
    // Other harnesses have no such event.
    let mut c = w.command(&["hook", "codex", "SkillPrompt"]);
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn().unwrap();
    child.stdin.take().unwrap().write_all(w.payload("s-codex", SKILL).to_string().as_bytes()).unwrap();
    let o = child.wait_with_output().unwrap();
    assert!(o.status.success() && o.stdout.is_empty(), "{o:?}");
}
