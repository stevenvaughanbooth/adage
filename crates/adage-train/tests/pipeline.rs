//! The whole pipeline against the fake harness: plan → run (exploration, mining, validation) →
//! report, on a committed fixture repository (the acme sandbox) and a fixture SKILL.md whose test
//! command is stale and whose regenerate step is right.

use adage_train::fake::{Fake, FakeWorld};
use adage_train::harness::Arm;
use adage_train::report::Verdict;
use adage_train::{plan, report, run};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
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
        .args([
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "-c",
            "init.defaultBranch=main",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("HOME", dir)
        .output()
        .unwrap();
    assert!(ok.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&ok.stderr));
}

/// The acme sandbox as a git repository with an origin (so its key differs from a clone's).
fn repo(tmp: &Path) -> PathBuf {
    let r = tmp.join("acme");
    copy_dir(&fixtures().join("acme"), &r);
    git(&r, &["init", "-q"]);
    git(&r, &["add", "."]);
    git(&r, &["commit", "-q", "-m", "fixture"]);
    git(&r, &["remote", "add", "origin", "https://example.com/acme/billing.git"]);
    r
}

fn world() -> FakeWorld {
    serde_json::from_str(&std::fs::read_to_string(fixtures().join("world.json")).unwrap()).unwrap()
}

/// An operator environment with nothing real in it.
fn base(tmp: &Path) -> BTreeMap<String, String> {
    [("PATH", std::env::var("PATH").unwrap_or_default()), ("HOME", tmp.join("operator").display().to_string())]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect()
}

fn make_plan(r: &Path, sessions: usize) -> plan::Plan {
    plan::build(&plan::PlanOptions {
        from: fixtures().join("SKILL.md"),
        repo: Some(r.to_path_buf()),
        seed: 7,
        sessions,
        model: "claude-haiku-4-5".into(),
    })
    .unwrap()
}

fn options(home: &Path, id: &str, validate: usize) -> run::RunOptions {
    let mut o = run::RunOptions::defaults(home.to_path_buf());
    o.harness = "fake".into();
    o.model = "claude-haiku-4-5".into();
    o.budget_usd = 100.0;
    o.concurrency = 3;
    o.validate = validate;
    o.run_id = Some(id.into());
    o.execute = true;
    o
}

#[test]
fn plan_extracts_typed_claims_and_balanced_tasks() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let p = make_plan(&r, 24);
    let kinds: Vec<(&str, bool)> = p.claims.iter().map(|c| (c.kind.label(), c.is_unsafe())).collect();
    assert_eq!(
        kinds,
        vec![
            ("verify", false),
            ("regenerate_after", false),
            ("lookup", false),
            ("regenerate_after", false),
            ("environment", true)
        ]
    );
    assert_eq!(p.source.name, "acme-billing");
    assert!(p.not_exercised.iter().any(|(id, why)| id == "c4" && why.contains("proto/*.proto")));
    assert_eq!(p.tasks.len(), 24);
    let controls = p.tasks.iter().filter(|t| t.control).count();
    assert!(controls * 4 >= 24, "{controls}");
    assert!(p.tasks.iter().all(|t| t.claim.as_deref() != Some("c5") && t.claim.as_deref() != Some("c4")));
    assert!(p.estimate.usd > 0.0);
    assert_eq!(p.repo.key, "example.com/acme/billing");
    // Deterministic, and round-trips through its file.
    let again = make_plan(&r, 24);
    assert_eq!(serde_json::to_string(&p).unwrap(), serde_json::to_string(&again).unwrap());
    let f = tmp.path().join("plan.json");
    std::fs::write(&f, serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(plan::load(&f).unwrap().tasks, p.tasks);
    let text = plan::render(&p);
    assert!(text.contains("UNSAFE") && text.contains("budget estimate: 24 sessions × $0.15/session"), "{text}");
}

#[test]
fn plan_reads_makefile_and_workflow_through_declared_facts() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let mk = plan::build(&plan::PlanOptions {
        from: r.join("Makefile"),
        repo: None,
        seed: 1,
        sessions: 8,
        model: "x".into(),
    })
    .unwrap();
    let got: Vec<String> = mk.claims.iter().map(|c| c.kind.describe()).collect();
    assert!(got.contains(&"regenerate_after(schema/*.json): `make gen`".to_string()), "{got:?}");
    assert!(got.contains(&"verify: `make test`".to_string()), "{got:?}");
    let wf = tmp.path().join("ci.yml");
    std::fs::write(&wf, "jobs:\n  t:\n    steps:\n      - run: make test\n      - run: git push origin main\n")
        .unwrap();
    let ci =
        plan::build(&plan::PlanOptions { from: wf, repo: Some(r), seed: 1, sessions: 8, model: "x".into() }).unwrap();
    let got: Vec<(String, bool)> = ci.claims.iter().map(|c| (c.kind.describe(), c.is_unsafe())).collect();
    assert_eq!(
        got,
        vec![("verify: `make test`".to_string(), false), ("environment: `git push origin main`".to_string(), true)]
    );
}

#[test]
fn fake_end_to_end_compiles_the_right_claim_and_flags_the_stale_one() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let home = tmp.path().join("adage-home");
    let p = make_plan(&r, 24);
    let fake = Fake { world: world() };
    let (dir, state) = run::execute(&p, &options(&home, "e2e", 9), &fake, &base(tmp.path()), &|_| {}).unwrap();
    assert_eq!(dir, home.join("train/e2e"));
    assert!(state.jobs.iter().all(|j| j.status == "done"), "{:#?}", state.jobs);
    assert_eq!(state.jobs.iter().filter(|j| j.arm == Arm::Explore).count(), 24);
    assert_eq!(state.jobs.iter().filter(|j| j.phase == "validate").count(), 27);
    // Transcripts live under the run, never in a Claude Code projects directory.
    assert_eq!(std::fs::read_dir(dir.join("transcripts/explore")).unwrap().count(), 24);
    assert!(!tmp.path().join("operator/.claude").exists());
    // Per-session clones and homes are gone; the origin-less clone key differs from the repo key.
    assert!(!dir.join("homes").read_dir().is_ok_and(|mut d| d.next().is_some()));
    assert_eq!(state.clone_key.as_deref().map(|k| k.starts_with("acme@")), Some(true));

    // Staged, under the person's repository key, with the synthetic provenance.
    let staged = adage_tracks::io::read_all(&dir.join("staged/tracks")).unwrap();
    assert!(!staged.is_empty());
    let prov = format!("synthetic:skill:{}@e2e", p.source.sha256);
    for t in &staged {
        assert_eq!(t.manifest.scope.repo, "example.com/acme/billing");
        assert_eq!(t.manifest.track.provenance, prov);
        assert!(t.routes.iter().all(|r| r.evidence.provenance == prov));
        // The synthetic cap: never fire, never beyond forming.
        assert!(t
            .routes
            .iter()
            .all(|r| r.strength != adage_tracks::Strength::Fire && r.state != adage_tracks::RouteState::Firm));
    }
    let side: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("staged/provenance.json")).unwrap()).unwrap();
    assert_eq!(side["provenance"], prov.as_str());
    // Nothing was installed where hooks read.
    assert!(!home.join("index.json").exists() && !home.join("tracks").exists());

    let rep = report::build(&dir, &p, &state).unwrap();
    let verdict = |id: &str| rep.claims.iter().find(|c| c.id == id).unwrap().verdict.clone();
    match verdict("c1") {
        Verdict::Contradicted { route, instead, failed_sessions, ran_sessions } => {
            assert!(route.starts_with("dead-end-"), "{route}");
            assert_eq!(instead, "make test");
            assert_eq!(failed_sessions, ran_sessions);
        }
        v => panic!("c1: {v:?}"),
    }
    assert!(matches!(verdict("c2"), Verdict::Compiled { route } if route.starts_with("finish-line-")));
    assert!(matches!(verdict("c3"), Verdict::Compiled { route } if route == "lookup-work-item"));
    assert!(matches!(verdict("c4"), Verdict::NeverExercised { .. }));
    assert!(matches!(verdict("c5"), Verdict::Unsafe { .. }));
    // Acceptance: every compiled or contradicted claim with held-out sessions passes.
    for a in &rep.acceptance {
        match a.claim.as_str() {
            "c1" | "c2" | "c3" => assert_eq!(a.result, "accepted", "{a:#?}"),
            other => panic!("{other} has no held-out tasks: {a:#?}"),
        }
    }
    let c1 = rep.acceptance.iter().find(|a| a.claim == "c1").unwrap();
    assert_eq!(c1.arms["skill"].rate, Some(0.0));
    assert_eq!(c1.arms["reflex"].rate, Some(1.0));
    assert!(c1.arms["reflex"].mean_turns < c1.arms["skill"].mean_turns);
    assert!((rep.cost_usd - state.spent_usd).abs() < 1e-9 && rep.cost_usd > 0.0);
    let text = report::render(&rep);
    assert!(text.contains("your skill is stale") && text.contains("compiled → route finish-line-"), "{text}");
    eprintln!("{text}");

    // Deterministic: another run of the same plan stages the same routes.
    let (_, again) = run::execute(&p, &options(&home, "e2e-2", 0), &fake, &base(tmp.path()), &|_| {}).unwrap();
    let ids = |d: &Path| -> Vec<String> {
        let mut v: Vec<String> = adage_tracks::io::read_all(&d.join("staged/tracks"))
            .unwrap()
            .into_iter()
            .flat_map(|t| t.routes.into_iter().map(|r| format!("{:?}{:?}{:?}", r.id, r.trigger, r.step)))
            .collect();
        v.sort();
        v
    };
    assert_eq!(ids(&dir), ids(&home.join("train/e2e-2")));
    assert!(again.jobs.iter().all(|j| j.phase == "explore"));
    // A run id is never reused.
    assert!(run::execute(&p, &options(&home, "e2e", 0), &fake, &base(tmp.path()), &|_| {}).is_err());
}

#[test]
fn a_plain_verify_claim_the_sessions_follow_compiles() {
    // "Run `make test` before you finish": the exploration sessions edit, then run it. The real
    // run in evals/corpus (finding 15) staged nothing for such a claim.
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let skill = tmp.path().join("SKILL.md");
    std::fs::write(
        &skill,
        "---\nname: acme-billing\ndescription: Working in acme billing.\n---\n\n# Acme billing\n\n\
         Always run `make test` before you finish.\n\nAfter editing `schema/*.json`, always run `make gen`.\n",
    )
    .unwrap();
    let p = plan::build(&plan::PlanOptions {
        from: skill,
        repo: Some(r.clone()),
        seed: 3,
        sessions: 24,
        model: "claude-haiku-4-5".into(),
    })
    .unwrap();
    let c = p.claims.iter().find(|c| c.kind.describe() == "verify: `make test`").expect("the verify claim");
    let home = tmp.path().join("adage-home");
    let (dir, state) =
        run::execute(&p, &options(&home, "verify", 0), &Fake { world: world() }, &base(tmp.path()), &|_| {}).unwrap();
    let staged = adage_tracks::io::read_all(&dir.join("staged/tracks")).unwrap();
    let route = staged
        .iter()
        .flat_map(|t| t.routes.iter())
        .find(|r| r.id.starts_with("finish-check-"))
        .expect("the claimed check is staged");
    assert_eq!(route.step, adage_tracks::Step::Run { command: "make test".into() });
    assert!(matches!(&route.trigger, adage_tracks::Trigger::AfterEdit { .. }));
    assert_eq!(route.state, adage_tracks::RouteState::Forming, "the synthetic cap applies");
    let rep = report::build(&dir, &p, &state).unwrap();
    let v = rep.claims.iter().find(|x| x.id == c.id).unwrap().verdict.clone();
    assert!(matches!(&v, Verdict::Compiled { route } if route.starts_with("finish-check-")), "{v:?}");
}

#[test]
fn the_budget_stops_new_sessions() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let p = make_plan(&r, 12);
    let mut o = options(&tmp.path().join("h"), "b", 0);
    o.budget_usd = 0.2;
    o.concurrency = 1;
    let (_, state) = run::execute(&p, &o, &Fake { world: world() }, &base(tmp.path()), &|_| {}).unwrap();
    let done: Vec<_> = state.jobs.iter().filter(|j| j.status == "done").collect();
    let skipped = state.jobs.iter().filter(|j| j.status == "skipped").count();
    assert!(!done.is_empty() && skipped > 0, "{done:?}");
    let max_one = done.iter().map(|j| j.outcome.cost_usd).fold(0.0, f64::max);
    assert!(state.spent_usd >= 0.2 && state.spent_usd < 0.2 + max_one + 1e-9, "{}", state.spent_usd);
}

#[test]
fn the_dry_run_writes_nothing_and_shows_exact_commands() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let p = make_plan(&r, 8);
    let home = tmp.path().join("h");
    let mut o = options(&home, "dry", 2);
    o.execute = false;
    let h = adage_train::harness::ClaudeCode::default();
    let mut env = base(tmp.path());
    env.insert("ANTHROPIC_API_KEY".into(), "sk-test".into());
    env.insert("GITHUB_TOKEN".into(), "nope".into());
    let d = run::describe(&p, &o, &h, &env);
    assert!(!home.exists());
    assert_eq!(d["sessions"], 8 + 2 * 3);
    let text = run::render_describe(&d);
    assert!(text.starts_with("DRY RUN"), "{text}");
    let explore = d["examples"]["explore"]["command"].as_array().unwrap();
    let args: Vec<&str> = explore.iter().map(|a| a.as_str().unwrap()).collect();
    for want in [
        "claude",
        "-p",
        "--max-turns",
        "--output-format",
        "stream-json",
        "--append-system-prompt",
        "--disallowedTools",
        "WebFetch",
        "Task",
    ] {
        assert!(args.contains(&want), "{want}: {args:?}");
    }
    // The model sees exactly the allowed tools (no TaskGet/TaskList from the harness).
    let at = args.iter().position(|a| *a == "--tools").expect("--tools");
    assert_eq!(args[at + 1], adage_train::harness::ALLOWED_TOOLS.join(","));
    let reflex: Vec<&str> =
        d["examples"]["reflex"]["command"].as_array().unwrap().iter().map(|a| a.as_str().unwrap()).collect();
    assert!(!reflex.contains(&"--append-system-prompt"));
    assert_eq!(d["examples"]["reflex"]["adage_env"]["ADAGE_CONTROL"], "0");
    assert_eq!(d["examples"]["explore"]["adage_env"]["ADAGE_DISABLE"], "1");
    assert!(!text.contains("sk-test") && !text.contains("nope"));
    assert_eq!(d["examples"]["explore"]["settings"]["sandbox"]["enabled"], true);
}

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_adage-train")
}

#[test]
fn the_cli_refuses_to_execute_inside_an_agent_session() {
    let tmp = tempfile::tempdir().unwrap();
    let r = repo(tmp.path());
    let out = tmp.path().join("plan.json");
    let st = Command::new(bin())
        .args(["plan", "--from"])
        .arg(fixtures().join("SKILL.md"))
        .arg("--repo")
        .arg(&r)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    assert!(String::from_utf8_lossy(&st.stdout).contains("nothing was executed"));
    for (k, v) in [("CLAUDECODE", "1"), ("ADAGE_TRAIN_SESSION", "x/y")] {
        let st = Command::new(bin())
            .args(["--plan"])
            .arg(&out)
            .args(["--budget", "1", "--execute"])
            .env("ADAGE_HOME", tmp.path().join("h"))
            .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
            .env_remove("CLAUDECODE")
            .env_remove("ADAGE_ARM")
            .env(k, v)
            .output()
            .unwrap();
        assert!(!st.status.success());
        let err = String::from_utf8_lossy(&st.stderr);
        assert!(err.contains("inside an agent session") && err.contains(k), "{err}");
    }
    assert!(!tmp.path().join("h").exists());
    // Without a terminal (and without the CI switch): refused too.
    let st = Command::new(bin())
        .args(["--plan"])
        .arg(&out)
        .args(["--budget", "1", "--execute"])
        .env_remove("CLAUDECODE")
        .env_remove("ADAGE_ARM")
        .env_remove("ADAGE_TRAIN_SESSION")
        .env_remove("ADAGE_ALLOW_NONINTERACTIVE")
        .env("ADAGE_HOME", tmp.path().join("h"))
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&st.stderr).contains("needs a person at a terminal"));
    // The fake harness runs end to end from the CLI, and `report` reads the run back.
    let st = Command::new(bin())
        .args(["--plan"])
        .arg(&out)
        .args(["--harness", "fake", "--fake-world"])
        .arg(fixtures().join("world.json"))
        .args(["--run-id", "cli", "--budget", "10", "--execute"])
        .env("ADAGE_HOME", tmp.path().join("h"))
        .output()
        .unwrap();
    assert!(st.status.success(), "{}", String::from_utf8_lossy(&st.stderr));
    let st =
        Command::new(bin()).args(["report", "cli", "--json"]).env("ADAGE_HOME", tmp.path().join("h")).output().unwrap();
    let rep: serde_json::Value = serde_json::from_slice(&st.stdout).unwrap();
    assert_eq!(rep["claims"][0]["verdict"], "contradicted");
    assert_eq!(rep["claims"][1]["verdict"], "compiled");
}
