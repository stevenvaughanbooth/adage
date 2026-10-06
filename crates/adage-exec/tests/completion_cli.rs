//! The binary boundary for read-only completion evidence and person-only requirement changes.
//! These tests execute no verification commands and require no platform sandbox.

use adage_core::Effect;
use adage_exec::completion::{AttemptGuard, Requirements};
use adage_store::Paths;
use adage_tracks::{Evidence, Index, IndexedRoute, Route, RouteState, Step, Strength, Trigger};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

const CHECK: &str = "fixture/checks/verify";

struct World {
    _tmp: tempfile::TempDir,
    repo: PathBuf,
    key: String,
    paths: Paths,
}

impl World {
    fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("fixture");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        // The CLI resolves --repo before deriving its path-based fallback key.
        let repo = repo.canonicalize().unwrap();
        let key = adage_core::repo::repo_key(&repo);
        let paths = Paths::at(tmp.path().join("adage"));
        std::fs::create_dir_all(&paths.root).unwrap();
        Index {
            generation: "completion-cli-fixture".into(),
            routes: vec![IndexedRoute {
                repo: key.clone(),
                track: "fixture/checks".into(),
                version: "0.1.0".into(),
                exec: None,
                route: Route {
                    id: "verify".into(),
                    summary: "A fixture verification command, with no execution approval".into(),
                    trigger: Trigger::AfterEdit { paths: "src/*.rs".into() },
                    step: Step::Run { command: "make check".into() },
                    slots: vec![],
                    effect: Effect::Verify,
                    strength: Strength::Prime,
                    state: RouteState::Firm,
                    exit: vec![],
                    evidence: Evidence::default(),
                    authored: false,
                },
            }],
            ..Default::default()
        }
        .write_atomic(&paths.index())
        .unwrap();
        Self { _tmp: tmp, repo, key, paths }
    }

    fn run(&self, verb: &str, flags: &[&str], environment: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_adage-exec"));
        cmd.arg(verb)
            .arg("--repo")
            .arg(&self.repo)
            .args(flags)
            .env("ADAGE_HOME", &self.paths.root)
            .env("ADAGE_POLICY", self.paths.root.join("managed.toml"))
            .env("HOME", &self.paths.root)
            .stdin(Stdio::null());
        for marker in adage_exec::person::AGENT_ENV {
            cmd.env_remove(marker);
        }
        cmd.env_remove("ADAGE_ALLOW_NONINTERACTIVE");
        for (name, value) in environment {
            cmd.env(name, value);
        }
        cmd.output().unwrap()
    }

    fn configure(&self) {
        let mut requirements = Requirements::default();
        requirements.repositories.insert(self.key.clone(), vec![CHECK.into()]);
        requirements.save(&self.paths.root).unwrap();
    }
}

fn report(out: Output, code: i32) -> Value {
    assert_eq!(out.status.code(), Some(code), "{}", String::from_utf8_lossy(&out.stderr));
    assert!(out.stderr.is_empty(), "a structured readiness result is not a usage error");
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn readiness_json_and_exit_status_distinguish_unconfigured_and_unapproved_checks() {
    let w = World::new();
    let unconfigured = report(w.run("readiness", &[], &[]), 3);
    assert_eq!(unconfigured["schema"], 1);
    assert_eq!(unconfigured["repo"], w.key);
    assert_eq!(unconfigured["configured"], false);
    assert_eq!(unconfigured["ready"], false);
    assert_eq!(unconfigured["checks"], json!([]));
    assert!(!w.paths.root.join("completion.json").exists(), "inspection does not create requirements");
    w.configure();
    let unapproved = report(w.run("readiness", &["--session", "fresh-agent-session"], &[("CLAUDECODE", "1")]), 3);
    assert_eq!(unapproved["configured"], true);
    assert_eq!(unapproved["ready"], false);
    assert_eq!(unapproved["checks"][0]["route"], CHECK);
    assert_eq!(unapproved["checks"][0]["readiness"]["status"], "unknown");
    assert!(unapproved["checks"][0]["readiness"]["reason"].as_str().unwrap().contains("no current E2 approval"));
    assert!(!w.paths.root.join("approvals.json").exists(), "requirements cannot grant execution");
    assert!(adage_exec::journal::list(&w.paths.root).unwrap().is_empty(), "readiness never runs the check");
}

#[test]
fn reading_completion_configuration_is_allowed_inside_an_agent_without_approval() {
    let w = World::new();
    assert_eq!(report(w.run("completion", &[], &[("CLAUDECODE", "1")]), 0), json!([]));
    w.configure();
    let before = std::fs::read(w.paths.root.join("completion.json")).unwrap();
    assert_eq!(report(w.run("completion", &[], &[("CLAUDECODE", "1")]), 0), json!([CHECK]));
    assert_eq!(std::fs::read(w.paths.root.join("completion.json")).unwrap(), before);
    assert!(!w.paths.root.join("approvals.json").exists());
}

#[test]
fn completion_and_readiness_reject_unknown_missing_and_mixed_flags_without_mutation() {
    let w = World::new();
    w.configure();
    let before = std::fs::read(w.paths.root.join("completion.json")).unwrap();
    for (verb, flags, reason) in [
        ("completion", vec!["--requrie", CHECK], "unknown argument"),
        ("readiness", vec!["--clear"], "unknown argument"),
        ("readiness", vec!["--session"], "needs a value"),
        ("completion", vec!["--require"], "needs a value"),
        ("completion", vec!["--clear", "--acknowledge"], "choose --require"),
        ("completion", vec!["--require", CHECK, "--clear"], "choose --require"),
        ("completion", vec!["--require", CHECK, "--acknowledge"], "choose --require"),
    ] {
        let out = w.run(verb, &flags, &[("ADAGE_ALLOW_NONINTERACTIVE", "1")]);
        assert_eq!(out.status.code(), Some(2), "{verb} {flags:?}");
        assert!(String::from_utf8_lossy(&out.stderr).contains(reason), "{}", String::from_utf8_lossy(&out.stderr));
        assert!(out.stdout.is_empty());
        assert_eq!(std::fs::read(w.paths.root.join("completion.json")).unwrap(), before);
    }
    assert!(!w.paths.root.join("approvals.json").exists());
}

#[test]
fn agents_cannot_require_clear_or_acknowledge_even_with_noninteractive_override() {
    let w = World::new();
    w.configure();
    let before = std::fs::read(w.paths.root.join("completion.json")).unwrap();
    let mut reservation = AttemptGuard::open(&w.paths.root, &w.repo).unwrap();
    reservation.begin("interrupted-session", CHECK, None).unwrap();
    drop(reservation);
    for marker in adage_exec::person::AGENT_ENV {
        for flags in [vec!["--require", CHECK], vec!["--clear"], vec!["--acknowledge"]] {
            let out = w.run("completion", &flags, &[(marker, "1"), ("ADAGE_ALLOW_NONINTERACTIVE", "1")]);
            assert_eq!(out.status.code(), Some(2), "{marker}, {flags:?}");
            let error = String::from_utf8_lossy(&out.stderr);
            assert!(error.contains("inside an agent session") && error.contains(marker), "{error}");
            assert!(out.stdout.is_empty());
            assert_eq!(std::fs::read(w.paths.root.join("completion.json")).unwrap(), before);
            let unchanged = AttemptGuard::open(&w.paths.root, &w.repo).unwrap();
            assert!(unchanged.attempts.pending(), "an agent cannot acknowledge interrupted work");
            assert_eq!(unchanged.attempts.count("interrupted-session", CHECK), 1);
        }
    }
    assert!(!w.paths.root.join("approvals.json").exists());
    assert!(adage_exec::journal::list(&w.paths.root).unwrap().is_empty());
}
