//! The press (mod architecture §3.12; plan A7): a token the binary issues to the Claude Code mod in
//! the window before the agent's first act, and that a person-only verb accepts on stdin, at the
//! listed sites only. End to end through the `adage` binary (and `adage-exec` where built), with
//! an isolated `ADAGE_HOME`. Test-plan cases T-A7-01 … T-A7-09 and T-X-13's L2 part.

use adage_hook::press;
use adage_store::{state, Paths};
use adage_tracks::testkit::{self, kinds};
use adage_tracks::*;
use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

const TRACK: &str = "acme-billing/work-items";
const ROUTE: &str = "lookup-work-item";
const PERSON_ONLY: &str = "will not run inside an agent session";

/// Every harness's agent marker the `adage` binary knows (`adage_app::person::AGENT_ENV`).
const AGENT: [(&str, &str); 4] =
    [("CLAUDECODE", "1"), ("OPENCODE", "1"), ("CODEX_SANDBOX", "seatbelt"), ("CODEX_SANDBOX_NETWORK_DISABLED", "1")];

mod common;
use common::text;

struct World {
    w: common::TempWorld,
}

fn track(repo: &str) -> Track {
    let lookup = kinds::lookup(ROUTE, "python3 scripts/wi-lookup {id} --json")
        .until_intent_changes()
        .measured(9, 10)
        .days(3)
        .provenance("desire-path")
        // This suite's fixed bound, not the Wilson bound of the counts.
        .with(|r| r.evidence.lower_bound = 0.6);
    testkit::track(TRACK, repo).version("0.4.0").route(lookup).build()
}

impl World {
    /// A checkout `acme-billing` (a `.git` directory, no history) with one installed learned
    /// track and its index.
    fn new() -> World {
        let w = World { w: common::TempWorld::new(&["home", "adage", "acme-billing/.git"]) };
        w.w.write("acme-billing/scripts/wi-lookup", "#!/usr/bin/env python3\nprint('{}')\n");
        let t = track("acme-billing");
        let home = w.home();
        home.install(TRACK, &t);
        home.compile_root(&[t]);
        w
    }
    /// A checkout with no reflexes at all (an empty `ADAGE_HOME`).
    fn bare() -> World {
        World { w: common::TempWorld::new(&["home", "adage", "acme-billing/.git"]) }
    }
    fn p(&self, rel: &str) -> PathBuf {
        self.w.p(rel)
    }
    fn repo(&self) -> PathBuf {
        self.p("acme-billing")
    }
    fn paths(&self) -> Paths {
        self.home().paths
    }
    fn home(&self) -> common::LearnedHome {
        common::LearnedHome::at(self.p("adage"))
    }
    fn base(&self, bin: &str, args: &[&str]) -> Command {
        let mut c = common::hermetic(bin, &self.p("adage"), &self.repo());
        c.args(args)
            .env("ADAGE_POLICY", self.p("adage/managed.toml"))
            .env("HOME", self.p("home"))
            .env("ADAGE_ARM", "treated");
        c
    }
    /// `adage hook claude <event>` with `payload` on stdin: its stdout, trimmed.
    fn hook(&self, event: &str, payload: Value) -> String {
        self.hook_with(event, payload, &[])
    }
    fn hook_with(&self, event: &str, payload: Value, env: &[(&str, &str)]) -> String {
        let mut c = self.base(env!("CARGO_BIN_EXE_adage"), &["hook", "claude", event]);
        for (k, v) in env {
            c.env(k, v);
        }
        let o = feed(c, &payload.to_string());
        assert!(o.status.success(), "a hook always exits 0");
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }
    /// A start as the engine sends it (naming the session's transcript), in this checkout.
    fn start(&self, session: &str, source: &str) {
        self.start_in(&self.repo(), session, source);
    }
    fn start_in(&self, cwd: &Path, session: &str, source: &str) {
        self.hook("SessionStart", start_payload(cwd, session, source));
    }
    /// The mod's `Press` ask: the token, or `None` when nothing was printed.
    fn press(&self, session: &str, source: &str) -> Option<String> {
        let out = self.hook(
            "Press",
            json!({ "session_id": session, "cwd": self.repo(), "hook_event_name": "Press", "source": source }),
        );
        (!out.is_empty()).then(|| {
            let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("not JSON ({e}): {out}"));
            v["press"].as_str().unwrap().to_string()
        })
    }
    fn prompt(&self, session: &str, text: &str) -> String {
        self.hook(
            "UserPromptSubmit",
            json!({ "session_id": session, "cwd": self.repo(), "hook_event_name": "UserPromptSubmit", "prompt": text }),
        )
    }
    /// An `adage` verb as the mod runs it: every agent marker set, stdin a pipe carrying `stdin`,
    /// `ADAGE_ALLOW_NONINTERACTIVE` unset unless `env` sets it.
    fn verb(&self, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
        self.verb_with(env!("CARGO_BIN_EXE_adage"), args, stdin, env)
    }
    fn verb_with(&self, bin: &str, args: &[&str], stdin: &str, env: &[(&str, &str)]) -> Output {
        let mut c = self.base(bin, args);
        for (k, v) in AGENT.iter().chain(env) {
            c.env(k, v);
        }
        feed(c, stdin)
    }
    /// A verb at a terminal (CI's form: no agent marker, the override set).
    fn person(&self, args: &[&str]) -> Output {
        let mut c = self.base(env!("CARGO_BIN_EXE_adage"), args);
        c.env("ADAGE_ALLOW_NONINTERACTIVE", "1").env_remove("ADAGE_ARM").stdin(Stdio::null());
        c.output().unwrap()
    }
    fn spool(&self, session: &str) -> Vec<Value> {
        self.home().spooled(session)
    }
    fn record(&self, session: &str) -> press::Record {
        state::read(&press::record_path(&self.paths(), session)).unwrap()
    }
    fn review_yaml(&self) -> String {
        std::fs::read_to_string(self.paths().tracks().join(TRACK).join(review::FILE)).unwrap_or_default()
    }
}

fn start_payload(cwd: &Path, session: &str, source: &str) -> Value {
    let transcript = format!("/home/dev/.claude/projects/acme/{session}.jsonl");
    json!({ "session_id": session, "cwd": cwd, "hook_event_name": "SessionStart", "source": source,
            "transcript_path": transcript })
}

fn feed(mut c: Command, stdin: &str) -> Output {
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    // A verb that refuses before it reads stdin closes the pipe first: that is not a failure here.
    let _ = child.stdin.take().unwrap().write_all(stdin.as_bytes());
    child.wait_with_output().unwrap()
}

fn pressed(token: &str, session: &str) -> String {
    json!({ "press": token, "session": session }).to_string()
}

/// The next token a pressed verb printed as its first line.
fn next(o: &Output) -> String {
    let out = String::from_utf8_lossy(&o.stdout);
    let first = out.lines().next().unwrap_or_else(|| panic!("no output:\n{}", text(o)));
    let v: Value =
        serde_json::from_str(first).unwrap_or_else(|e| panic!("first line is not the next token ({e}):\n{}", text(o)));
    let t = v["next"].as_str().unwrap().to_string();
    assert_eq!(t.len(), 64);
    t
}

fn refused(o: &Output) {
    assert!(!o.status.success(), "accepted:\n{}", text(o));
    assert!(text(o).contains(PERSON_ONLY) || text(o).contains("needs a person at a terminal"), "{}", text(o));
    assert!(!String::from_utf8_lossy(&o.stdout).contains("\"next\""), "a refusal prints no token:\n{}", text(o));
}

/// A session whose window is open, and its first token, as the mod holds them after
/// `classic.SessionStart`.
fn session(w: &World, id: &str) -> String {
    w.start(id, "startup");
    w.press(id, "startup").expect("a token in the window")
}

/// T-A7-01: issued once, in the window; the state and the spool hold the hash, never the token.
#[test]
fn issued_once_in_the_window() {
    let w = World::new();
    let t = session(&w, "s1");
    assert!(t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()), "{t}");
    assert_eq!(w.press("s1", "startup"), None, "a second press");
    let rec = w.record("s1");
    assert_eq!(rec.hash.as_deref(), Some(press::hash(&t).as_str()));
    assert_eq!(rec.hash.as_ref().unwrap().len(), 64);
    let issued: Vec<Value> = w.spool("s1").into_iter().filter(|r| r["kind"] == "press_issued").collect();
    assert_eq!(issued.len(), 1);
    for e in std::fs::read_dir(w.paths().sessions()).unwrap().flatten() {
        let bytes = std::fs::read(e.path()).unwrap();
        assert!(!String::from_utf8_lossy(&bytes).contains(&t), "{} holds the token", e.path().display());
    }
}

/// T-A7-02: the first invocation closes the window, whether or not anything is recorded: a
/// prompt in a repository with no reflexes, a prompt that matches nothing, a `PreToolUse` with no
/// Python word, a `PostToolBatch` with no match.
#[test]
fn the_window_closes_on_the_first_invocation_not_the_first_record() {
    let bare = World::bare();
    bare.start("s", "startup");
    assert_eq!(bare.prompt("s", "hello"), "");
    assert_eq!(bare.press("s", "startup"), None, "no routes: the prompt closed it");

    let w = World::new();
    let quiet = |id: &str, event: &str, payload: Value| {
        w.start(id, "startup");
        assert_eq!(w.hook(event, payload), "", "{event} says nothing here");
        assert_eq!(w.press(id, "startup"), None, "{event} closed the window");
    };
    let repo = w.repo();
    quiet("p", "UserPromptSubmit", json!({ "session_id": "p", "cwd": repo, "prompt": "tidy the README" }));
    quiet(
        "t",
        "PreToolUse",
        json!({ "session_id": "t", "cwd": repo, "tool_name": "Bash", "tool_input": { "command": "ls -la" }, "tool_use_id": "toolu_1" }),
    );
    quiet(
        "b",
        "PostToolBatch",
        json!({ "session_id": "b", "cwd": repo, "tool_calls": [{ "tool_name": "Bash", "tool_input": { "command": "ls" }, "tool_response": "", "tool_use_id": "toolu_2" }] }),
    );
}

/// T-A7-03: compact never issues; resume issues once and replaces the hash; clear (a new id)
/// issues; a second resume issues nothing.
#[test]
fn sources() {
    let w = World::new();
    let first = session(&w, "s");
    w.prompt("s", "hello");
    w.start("s", "compact");
    assert_eq!(w.press("s", "compact"), None);
    assert_eq!(w.press("s", "startup"), None);
    w.start("s", "resume");
    let resumed = w.press("s", "resume").expect("a resume reissues once");
    assert_ne!(first, resumed);
    assert_eq!(w.record("s").hash.as_deref(), Some(press::hash(&resumed).as_str()));
    w.prompt("s", "again");
    w.start("s", "resume");
    assert_eq!(w.press("s", "resume"), None, "a second resume");
    w.start("s2", "clear");
    assert!(w.press("s2", "clear").is_some(), "clear starts a new session id");
}

/// T-A7-04 and T-A7-05: a valid press is a person at each listed site, and records `via: press`;
/// every way a press can be invalid refuses exactly as with none, records nothing, and spends
/// nothing; the chain moves one link per verb.
#[test]
fn a_press_is_a_person_at_the_listed_sites() {
    let w = World::new();
    let mut t = session(&w, "s");
    w.prompt("s", "hello");
    // The six review verbs, each with the next token. `dismiss` passes the check and then finds
    // no proposed change to dismiss.
    for verb in ["weaken", "strengthen", "pin", "extinguish", "approve"] {
        let o = w.verb(&["review", verb, TRACK, ROUTE], &pressed(&t, "s"), &[]);
        assert!(o.status.success(), "review {verb}:\n{}", text(&o));
        let n = next(&o);
        assert_ne!(n, t);
        // The spent token is refused from now on.
        refused(&w.verb(&["review", verb, TRACK, ROUTE], &pressed(&t, "s"), &[]));
        t = n;
        assert!(w.review_yaml().contains("via: press"), "{}", w.review_yaml());
    }
    let o = w.verb(&["review", "dismiss", TRACK, ROUTE], &pressed(&t, "s"), &[]);
    assert!(text(&o).contains("no proposed change to dismiss"), "{}", text(&o));
    t = next(&o);

    // The execution grants and switches.
    let o = w.verb(&["delegate", "policy", "--max-level", "e2"], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(std::fs::read_to_string(w.p("adage/policy.toml")).unwrap().contains("E2"));
    t = next(&o);
    let o = w.verb(&["review", "delegate", TRACK, ROUTE, "--predict"], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
    let store: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(store["approvals"][0]["via"], "press");
    assert_eq!(store["approvals"][0]["key"]["mode"], "predict");
    t = next(&o);
    let o = w.verb(&["review", "revoke", TRACK, ROUTE, "--predict"], &pressed(&t, "s"), &[]);
    assert!(text(&o).contains("1 delegation"), "{}", text(&o));
    t = next(&o);
    assert!(w.person(&["delegate", "predict", "off"]).status.success());
    let o = w.verb(&["delegate", "predict", "on"], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!w.p("adage/exec/predict-off").exists());
    t = next(&o);
    // `delegate on` under a press writes the plugin's marker only.
    let o = w.verb(&["delegate", "on"], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
    let marker: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/exec/enabled")).unwrap()).unwrap();
    assert_eq!(marker["via"], "press");
    assert!(text(&o).contains("takes effect in new agent sessions"), "{}", text(&o));
    t = next(&o);
    // `on` (A6).
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::write(w.repo().join(".adage/disabled"), "").unwrap();
    let o = w.verb(&["on"], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
    assert!(!w.repo().join(".adage/disabled").exists());
    t = next(&o);

    // Every way a press is not valid: refused as with none, nothing recorded, nothing spent.
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::write(w.repo().join(".adage/disabled"), "").unwrap();
    let mut wrong = t.clone();
    let last = if wrong.ends_with('0') { '1' } else { '0' };
    wrong.pop();
    wrong.push(last);
    w.start("other", "startup");
    let theirs = w.press("other", "startup").unwrap();
    for (why, stdin, env) in [
        ("last character changed", pressed(&wrong, "s"), vec![]),
        ("another session's token", pressed(&theirs, "s"), vec![]),
        ("no press at all", String::new(), vec![]),
        ("with the CI override", pressed(&t, "s"), vec![("ADAGE_ALLOW_NONINTERACTIVE", "1")]),
    ] {
        let o = w.verb(&["on"], &stdin, &env);
        assert!(!o.status.success(), "{why}: accepted\n{}", text(&o));
        assert!(text(&o).contains(PERSON_ONLY), "{why}: {}", text(&o));
        assert!(w.repo().join(".adage/disabled").exists(), "{why}: nothing changed");
    }
    // Without any agent marker (CI: the override set) stdin is not read at all (review L6): the
    // override decides as a terminal, and the press is neither honoured nor spent.
    let mut c = w.base(env!("CARGO_BIN_EXE_adage"), &["on"]);
    c.env("ADAGE_ALLOW_NONINTERACTIVE", "1");
    let o = feed(c, &pressed(&t, "s"));
    assert!(o.status.success() && !text(&o).contains("\"next\""), "{}", text(&o));
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::write(w.repo().join(".adage/disabled"), "").unwrap();
    // 25 hours old.
    state::update::<press::Record, _>(&press::record_path(&w.paths(), "s"), |r| r.since -= 25 * 3600 * 1000).unwrap();
    refused(&w.verb(&["on"], &pressed(&t, "s"), &[]));
    state::update::<press::Record, _>(&press::record_path(&w.paths(), "s"), |r| r.since += 25 * 3600 * 1000).unwrap();
    // Still valid after all that: nothing above spent it.
    let o = w.verb(&["on"], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
    t = next(&o);

    // Turn-downs need no press, with or without one.
    for args in [
        &["delegate", "off"][..],
        &["delegate", "policy", "--max-level", "e1"],
        &["delegate", "predict", "off"],
        &["off"],
    ] {
        assert!(w.verb(args, "", &[]).status.success(), "{args:?}");
        assert!(w.verb(args, &pressed(&t, "s"), &[]).status.success(), "{args:?} with a press");
    }

    // SessionEnd clears the hash: every token is refused after it.
    w.hook("SessionEnd", json!({ "session_id": "s", "cwd": w.repo(), "hook_event_name": "SessionEnd" }));
    assert_eq!(w.record("s").hash, None);
    refused(&w.verb(&["on"], &pressed(&t, "s"), &[]));
}

/// T-A7-04's other half: every person-only verb that is not a listed site refuses a valid press,
/// and the press is not spent by it.
#[test]
fn a_press_is_refused_everywhere_else() {
    let w = World::new();
    let t = session(&w, "s");
    w.prompt("s", "hello");
    let p = pressed(&t, "s");
    for args in [
        &["install", "claude-code", "--write", "--scope", "project"][..],
        &["config", "set", "learn.repos", "all"],
        &["share", TRACK, "--out", "x"],
        &["train", "accept", "x"],
        &["packs", "on", "x"],
        &["init", "--yes"],
        &["add", "../nowhere"],
        &["add", "../nowhere", "--trust"],
    ] {
        let o = w.verb(args, &p, &[]);
        assert!(!o.status.success(), "{args:?} accepted a press:\n{}", text(&o));
        // Each refuses with its own person-only words, naming the agent marker it saw.
        assert!(text(&o).contains("will not run inside") && text(&o).contains(" is set"), "{args:?}: {}", text(&o));
    }
    if let Some(exec) = adage_exec() {
        let exec = exec.to_string_lossy().into_owned();
        let more = [("ADAGE_ARM", "treated")];
        for args in [&["approve"][..], &["revoke", "--repo", "."], &["recover", "--id", "x"]] {
            let o = w.verb_with(&exec, args, &p, &more);
            assert!(!o.status.success(), "adage-exec {args:?} accepted a press:\n{}", text(&o));
            assert!(text(&o).contains(PERSON_ONLY), "adage-exec {args:?}: {}", text(&o));
        }
    }
    // None of them spent it.
    let o = w.verb(&["review", "pin", TRACK, ROUTE], &p, &[]);
    assert!(o.status.success(), "{}", text(&o));
    next(&o);
}

/// T-A7-06: stage, then confirm. An E1 stage answers argv, the input contracts, the output mode
/// and the proposed fields, and records nothing.
#[test]
fn an_e1_stage_answers_the_proposed_grant_and_records_nothing() {
    let w = World::new();
    let o = w.person(&["review", "delegate", TRACK, ROUTE, "--level", "e1", "--stage", "--json"]);
    assert!(o.status.success(), "{}", text(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap_or_else(|e| panic!("{e}: {}", text(&o)));
    assert_eq!(v["facts"]["argv"], json!(["python3", "scripts/wi-lookup", "{id}", "--json"]));
    assert_eq!(v["facts"]["inputs"][0]["contract"], r"^W-\d{4}$");
    assert_eq!(v["facts"]["output"]["mode"], "typed");
    assert!(v["facts"]["output"]["fields"].as_object().is_some_and(|f| !f.is_empty()), "{v}");
    assert_eq!(v["facts"]["scripts"][0]["path"], "scripts/wi-lookup");
    assert!(v["facts"]["scripts"][0]["text"].as_str().unwrap().contains("print"));
    assert_eq!((v["recorded"].as_bool(), v["ok"].as_bool()), (Some(false), Some(true)));
    assert!(!w.p("adage/approvals.json").exists());
    // Confirmed against its digest, it records the grant.
    let digest = v["digest"].as_str().unwrap().to_string();
    let mut c =
        w.base(env!("CARGO_BIN_EXE_adage"), &["review", "delegate", TRACK, ROUTE, "--level", "e1", "--confirm"]);
    c.env("ADAGE_ALLOW_NONINTERACTIVE", "1").env_remove("ADAGE_ARM");
    let o = feed(c, &json!({ "expect": digest }).to_string());
    assert!(o.status.success(), "{}", text(&o));
    let v: Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(v["recorded"], true);
    let store: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(
        (store["approvals"][0]["key"]["level"].as_str(), store["approvals"][0]["via"].as_str()),
        (Some("e1"), Some("terminal"))
    );
}

fn adage_exec() -> Option<PathBuf> {
    let p = Path::new(env!("CARGO_BIN_EXE_adage")).with_file_name("adage-exec");
    p.is_file().then_some(p)
}

/// `chmod -R a+rX`: as root, sandboxed commands run as nobody.
#[cfg(unix)]
fn open_up(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    let m = std::fs::symlink_metadata(p).unwrap();
    if m.file_type().is_symlink() {
        return;
    }
    let mode = m.permissions().mode();
    let add = if m.is_dir() || mode & 0o100 != 0 { 0o555 } else { 0o444 };
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | add)).unwrap();
    if m.is_dir() {
        for e in std::fs::read_dir(p).unwrap().flatten() {
            open_up(&e.path());
        }
    }
}

/// T-A7-06 for an action: the stage answers the proposed terms, the facts and the digest, and
/// records nothing; a confirm records the grant, and on a machine at E1 the policy and the marker
/// with it; a changed script refuses naming it; a moved repository answers the fresh diff and
/// records nothing until confirmed against the new digest. A later grant writes the grant alone.
#[cfg(unix)]
#[test]
fn an_action_is_staged_then_confirmed() {
    if !adage_exec::stage::backend(&adage_exec::sandbox::detect()).available() {
        eprintln!("skipped: no staging backend here");
        return;
    }
    let w = World::bare();
    let repo = w.repo();
    std::fs::remove_dir_all(repo.join(".git")).unwrap();
    for d in ["registry", "gen", "scripts"] {
        std::fs::create_dir_all(repo.join(d)).unwrap();
    }
    std::fs::write(repo.join("registry/items.csv"), "one\n").unwrap();
    std::fs::write(repo.join("gen/items.txt"), "").unwrap();
    std::fs::write(repo.join("scripts/gen"), "cat registry/items.csv > gen/items.txt\n").unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("git").arg("-C").arg(&repo).args(args).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    git(&["-c", "user.name=F", "-c", "user.email=f@example.com", "commit", "-qm", "fixture"]);
    open_up(&w.w.root);
    let key = adage_core::repo::repo_key(&repo);
    let route = |id: &str| {
        kinds::finish_line(id, "registry/*.csv", "sh scripts/gen")
            .exit(vec![Exit::ExploratoryChange])
            .support(7, 8)
            .days(3)
    };
    let mut t = track(&key);
    t.manifest.track.name = "acme/finish".into();
    t.routes = vec![route("regenerate-items").build(), route("regenerate-items-again").build()];
    w.home().compile(&[t]);

    // Staging changes the action's output in the sandbox only.
    std::fs::write(repo.join("registry/items.csv"), "one\ntwo\n").unwrap();
    let id = "s";
    let mut tok = session(&w, id);
    w.prompt(id, "regenerate");
    let stage = |tok: &str, route: &str| -> (Value, String) {
        let o = w.verb(
            &["review", "delegate", "acme/finish", route, "--level", "e2", "--stage", "--json"],
            &pressed(tok, id),
            &[],
        );
        assert!(o.status.success(), "{}", text(&o));
        let n = next(&o);
        let line = String::from_utf8_lossy(&o.stdout).lines().nth(1).unwrap().to_string();
        (serde_json::from_str(&line).unwrap(), n)
    };
    let confirm = |tok: &str, route: &str, digest: &str| -> (Output, String) {
        let stdin = json!({ "expect": digest, "press": tok, "session": id }).to_string();
        let o = w.verb(&["review", "delegate", "acme/finish", route, "--level", "e2", "--confirm"], &stdin, &[]);
        let n = next(&o);
        (o, n)
    };
    let (v, n) = stage(&tok, "regenerate-items");
    tok = n;
    assert_eq!(v["proposed"], true);
    assert_eq!(v["terms"]["outputs"], json!(["gen/**"]), "{v}");
    assert_eq!(v["first"], true, "the machine is at E1: the first action turns execution on");
    assert_eq!(v["facts"]["argv"], json!(["sh", "scripts/gen"]));
    assert!(v["facts"]["scripts"][0]["text"].as_str().unwrap().contains("cat registry"));
    let diff = &v["facts"]["staged"]["diff"][0];
    assert_eq!(diff["path"], "gen/items.txt");
    assert!(diff["before_sha256"].is_string() && diff["after_sha256"].is_string());
    assert!(diff["diff"].as_str().unwrap().contains("two"), "{diff}");
    assert_eq!(std::fs::read_to_string(repo.join("gen/items.txt")).unwrap(), "", "nothing is applied");
    assert!(!w.p("adage/approvals.json").exists() && !w.p("adage/policy.toml").exists());
    let digest = v["digest"].as_str().unwrap().to_string();

    // The agent kept working: a confirm answers the fresh diff and records nothing.
    std::fs::write(repo.join("registry/items.csv"), "one\ntwo\nthree\n").unwrap();
    let (o, n) = confirm(&tok, "regenerate-items", &digest);
    tok = n;
    assert!(o.status.success(), "{}", text(&o));
    let fresh: Value = serde_json::from_str(String::from_utf8_lossy(&o.stdout).lines().nth(1).unwrap()).unwrap();
    assert_eq!((fresh["recorded"].as_bool(), fresh["changed"].as_str()), (Some(false), Some("snapshot")));
    assert!(fresh["facts"]["staged"]["diff"][0]["diff"].as_str().unwrap().contains("three"));
    assert!(!w.p("adage/approvals.json").exists());

    // Confirmed against the new digest: policy, marker and grant together.
    let (o, n) = confirm(&tok, "regenerate-items", fresh["digest"].as_str().unwrap());
    tok = n;
    assert!(o.status.success(), "{}", text(&o));
    let done: Value = serde_json::from_str(String::from_utf8_lossy(&o.stdout).lines().nth(1).unwrap()).unwrap();
    assert_eq!(done["wrote"], json!(["policy", "marker", "grant"]));
    assert!(std::fs::read_to_string(w.p("adage/policy.toml")).unwrap().contains("E2"));
    assert!(w.p("adage/exec/enabled").is_file());
    let store: Value = serde_json::from_str(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap();
    assert_eq!(store["approvals"][0]["via"], "press");
    assert_eq!(store["approvals"][0]["terms"]["outputs"], json!(["gen/**"]));

    // A term changes between the acts: the confirm refuses naming it.
    let (v, n) = stage(&tok, "regenerate-items-again");
    tok = n;
    assert_eq!(v["first"], false);
    std::fs::write(repo.join("scripts/gen"), "cat registry/items.csv | sort > gen/items.txt\n").unwrap();
    let (o, n) = confirm(&tok, "regenerate-items-again", v["digest"].as_str().unwrap());
    tok = n;
    assert!(!o.status.success());
    assert!(text(&o).contains("changed since it was staged (scripts)"), "{}", text(&o));
    assert_eq!(
        serde_json::from_str::<Value>(&std::fs::read_to_string(w.p("adage/approvals.json")).unwrap()).unwrap()
            ["approvals"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    // Staged and confirmed again: the grant alone.
    let (v, n) = stage(&tok, "regenerate-items-again");
    tok = n;
    let (o, _) = confirm(&tok, "regenerate-items-again", v["digest"].as_str().unwrap());
    assert!(o.status.success(), "{}", text(&o));
    let done: Value = serde_json::from_str(String::from_utf8_lossy(&o.stdout).lines().nth(1).unwrap()).unwrap();
    assert_eq!(done["wrote"], json!(["grant"]));
}

/// T-A7-08: the tripwire records and warns once, and never denies or rewrites the call.
#[test]
fn the_tripwire_warns_and_never_refuses() {
    let w = World::new();
    session(&w, "s");
    let call = |id: &str, command: &str| {
        w.hook("PreToolUse", json!({ "session_id": "s", "cwd": w.repo(), "tool_name": "Bash", "tool_input": { "command": command }, "tool_use_id": id }))
    };
    // Review M3: the mod's dry run for the call comes first and says nothing; the command hook
    // then warns.
    let dry = json!({ "session_id": "s", "cwd": w.repo(), "tool_name": "Bash", "tool_use_id": "toolu_1", "dry_run": true,
                      "tool_input": { "command": "adage hook claude Press <<'EOF'\n{}\nEOF" } });
    assert_eq!(w.hook("PreToolUse", dry), "");
    let out = call("toolu_1", "adage hook claude Press <<'EOF'\n{}\nEOF");
    let v: Value = serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out}"));
    assert!(v["systemMessage"].as_str().unwrap().contains("press"), "{v}");
    assert!(v.get("hookSpecificOutput").is_none(), "never a deny or an updatedInput: {v}");
    assert_eq!(call("toolu_2", "adage on <<'EOF'\n{\"press\": \"x\", \"session\": \"s\"}\nEOF"), "", "warned once");
    assert_eq!(w.spool("s").iter().filter(|r| r["kind"] == "tripwire").count(), 2);
}

/// T-A7-09: `Press` answers before the kill switches, is Claude Code's alone, and the goldens'
/// events print what they did (`hooks.rs` pins them; `adage-exec/tests/parity.rs` holds parity).
#[test]
fn the_press_runs_before_the_kill_switches_and_only_for_claude_code() {
    let w = World::new();
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::create_dir_all(w.repo().join(".adage")).unwrap();
    std::fs::write(w.repo().join(".adage/disabled"), "").unwrap();
    assert!(session(&w, "off").len() == 64, "a repository where Adage is off can still be asked to turn it on");
    let payload = start_payload(&w.repo(), "env", "startup");
    w.hook_with("SessionStart", payload.clone(), &[("ADAGE_DISABLE", "1")]);
    assert!(!w.hook_with("Press", payload.clone(), &[("ADAGE_DISABLE", "1")]).is_empty());
    // Not a Codex or OpenCode event.
    let mut c = w.base(env!("CARGO_BIN_EXE_adage"), &["hook", "codex", "Press"]);
    c.env("ADAGE_ARM", "treated");
    let o = feed(c, &json!({ "session_id": "cx", "cwd": w.repo(), "source": "startup" }).to_string());
    assert!(o.stdout.is_empty());
}

/// T-X-13 (L2): the model cannot press. After any agent-side invocation no source issues but one
/// resume; a person-only verb refuses without a token and records nothing; a start written by
/// hand for a new session id yields a token this repository refuses; a second issue for one start
/// is visible in the spool.
#[test]
fn the_model_cannot_press() {
    let w = World::new();
    let real = session(&w, "real");
    w.prompt("real", "hello");
    for source in ["startup", "clear", "compact"] {
        w.start("real", source);
        assert_eq!(w.press("real", source), None, "{source}");
    }
    for verb in ["approve", "strengthen", "weaken", "extinguish", "pin", "dismiss"] {
        refused(&w.verb(&["review", verb, TRACK, ROUTE], "", &[]));
    }
    refused(&w.verb(&["review", "delegate", TRACK, ROUTE, "--predict"], "", &[]));
    assert_eq!(w.review_yaml(), "", "nothing recorded");
    assert!(!w.p("adage/approvals.json").exists());
    // A start written by hand while the real session is at work.
    w.start("forged", "startup");
    let forged = w.press("forged", "startup").expect("the forged start's window is open");
    refused(&w.verb(&["review", "approve", TRACK, ROUTE], &pressed(&forged, "forged"), &[]));
    assert_eq!(w.review_yaml(), "");
    // Review M4: acting in the forged session does not make it live.
    w.prompt("forged", "hello");
    refused(&w.verb(&["review", "approve", TRACK, ROUTE], &pressed(&forged, "forged"), &[]));
    assert_eq!(w.review_yaml(), "");
    // Review M4: nor does a fresh `git init` elsewhere, for a machine-wide verb.
    let x = w.p("x");
    std::fs::create_dir_all(x.join(".git")).unwrap();
    w.start_in(&x, "x", "startup");
    let t = w.hook("Press", json!({ "session_id": "x", "cwd": x, "hook_event_name": "Press", "source": "startup" }));
    let t: Value = serde_json::from_str(&t).unwrap();
    let mut on = w.base(env!("CARGO_BIN_EXE_adage"), &["delegate", "on"]);
    on.current_dir(&x);
    for (k, v) in AGENT {
        on.env(k, v);
    }
    refused(&feed(on, &pressed(t["press"].as_str().unwrap(), "x")));
    assert!(!w.p("adage/exec/enabled").exists());
    // A start written without the session's transcript opens no window at all.
    w.hook("SessionStart", json!({ "session_id": "bare", "cwd": w.repo(), "source": "startup" }));
    assert_eq!(w.press("bare", "startup"), None);
    // The one resume the design allows, and the second issue it leaves in the spool.
    w.start("real", "resume");
    assert!(w.press("real", "resume").is_some());
    assert_eq!(w.spool("real").iter().filter(|r| r["kind"] == "press_issued").count(), 2);
    // The real session's first token was replaced by the resume.
    refused(&w.verb(&["review", "approve", TRACK, ROUTE], &pressed(&real, "real"), &[]));
}

/// T-A7-04's last site: `add <pack> --trust` takes a press only for a pack directory committed in
/// the repository, unchanged since; a pack the agent could have written (uncommitted, or changed
/// since its commit) needs a terminal.
#[test]
fn a_committed_pack_can_be_trusted_with_a_press() {
    let w = World::new();
    let repo = w.repo();
    std::fs::remove_dir_all(repo.join(".git")).unwrap();
    std::fs::create_dir_all(repo.join("packs/team")).unwrap();
    std::fs::write(repo.join("packs/team").join(adage_pack::validate::MANIFEST), "[pack]\nname = \"team\"\n").unwrap();
    let git = |args: &[&str]| {
        let o = Command::new("git").arg("-C").arg(&repo).args(args).env("GIT_CONFIG_NOSYSTEM", "1").output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
    };
    git(&["init", "-q"]);
    git(&["add", "."]);
    // Committed a minute before the session opens (a commit's time is whole seconds, so one made
    // now could share the window's second); the files' on-disk times are older than the window
    // whatever the commit says.
    let past = format!("@{} +0000", adage_core::time::now_ms() / 1000 - 60);
    let o = Command::new("git")
        .arg("-C")
        .arg(&repo)
        .args(["-c", "user.name=F", "-c", "user.email=f@example.com", "commit", "-qm", "team pack"])
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_DATE", &past)
        .env("GIT_COMMITTER_DATE", &past)
        .output()
        .unwrap();
    assert!(o.status.success(), "git commit: {}", String::from_utf8_lossy(&o.stderr));
    let t = session(&w, "s");
    w.prompt("s", "hello");
    // Review M8: a pack the agent writes and commits during the session is offered (committed)
    // but not trusted with the press: a terminal's decision, and the press is not spent.
    std::fs::create_dir_all(repo.join("packs/mine")).unwrap();
    std::fs::write(repo.join("packs/mine").join(adage_pack::validate::MANIFEST), "[pack]\nname = \"mine\"\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=F",
        "-c",
        "user.email=f@example.com",
        "commit",
        "-qm",
        "mine",
        "--date",
        "2025-01-01T00:00:00Z",
    ]);
    assert!(adage_app::trust::committed_pack(&repo.join("packs/mine"), &repo));
    refused(&w.verb(&["add", "packs/mine", "--trust"], &pressed(&t, "s"), &[]));
    // Committed: the press is a person, and `add` goes on to read the pack (which this one is not).
    let o = w.verb(&["add", "packs/team", "--trust"], &pressed(&t, "s"), &[]);
    let t = next(&o);
    assert!(!text(&o).contains(PERSON_ONLY), "{}", text(&o));
    // Changed since its commit: a terminal's decision, and the press is not spent.
    std::fs::write(repo.join("packs/team/extra.toml"), "x").unwrap();
    let o = w.verb(&["add", "packs/team", "--trust"], &pressed(&t, "s"), &[]);
    refused(&o);
    let o = w.verb(&["review", "pin", TRACK, ROUTE], &pressed(&t, "s"), &[]);
    assert!(o.status.success(), "{}", text(&o));
}

/// Review L6: outside an agent session (CI: no agent marker, the override set) a person-only verb
/// reads nothing from stdin, so a pipe it inherited and nobody closes does not hold it up.
#[test]
fn a_verb_outside_a_session_does_not_wait_on_stdin() {
    let w = World::new();
    let mut c = w.base(env!("CARGO_BIN_EXE_adage"), &["review", "pin", TRACK, ROUTE]);
    c.env("ADAGE_ALLOW_NONINTERACTIVE", "1").env_remove("ADAGE_ARM");
    let started = std::time::Instant::now();
    let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().unwrap();
    let open = child.stdin.take();
    let o = child.wait_with_output().unwrap();
    drop(open);
    assert!(o.status.success(), "{}", text(&o));
    assert!(started.elapsed() < std::time::Duration::from_millis(2500), "waited {:?}", started.elapsed());
}
