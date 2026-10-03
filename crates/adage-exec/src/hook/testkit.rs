//! The hook's test world: one repository with one learned context verification
//! (`fixture/context/predict-check`: after `git status` and an edit to `src/*.rs`, run
//! `sh tools/check.sh`), a home beside it, and the grants, receipts and hook events the tests in
//! each module seed.

use adage_core::{CommandTable, Effect};
use adage_hook::{Extension, PreCall, PreNote, StopCall, StopGate, Turn};
use adage_runtime::{Arm, Call, Ctx, Directive, Event, SessionState};
use adage_store::Paths;
use adage_tracks::{
    CtxItem, Evidence, ExecutionEvidence, Index, IndexedRoute, Route, RouteState, Step, Strength, Trigger,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

use super::PREFETCH;
use crate::approvals::{self, Approval, Store};
use crate::journal::{Phase, Receipt, RunOrigin};
use crate::request::{ExecRequest, Level};
use crate::stage::Outcome;
use crate::{route, sandbox};

/// The fixture's route as grants and receipts name it.
pub(super) const CHECK: &str = "fixture/context/predict-check";

pub(super) struct Fixture {
    pub(super) _tmp: tempfile::TempDir,
    pub(super) root: std::path::PathBuf,
    pub(super) paths: Paths,
    pub(super) index: Index,
}

impl Fixture {
    pub(super) fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        // Named for its repository key: a hook resolves a checkout without a remote by its name.
        let root = tmp.path().canonicalize().unwrap().join("fixture");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("tools")).unwrap();
        std::fs::write(root.join("tools/check.sh"), "#!/bin/sh\ntest -f src/feature.rs\n").unwrap();
        std::fs::write(root.join("src/feature.rs"), "fixture\n").unwrap();
        let paths = Paths::at(tmp.path().join("adage"));
        let index = Index {
            generation: "fixture".into(),
            routes: vec![IndexedRoute {
                repo: "fixture".into(),
                track: "fixture/context".into(),
                version: "0.1.0".into(),
                exec: None,
                route: Route {
                    id: "predict-check".into(),
                    summary: "verify after inspection and source edit".into(),
                    trigger: Trigger::Context {
                        items: vec![
                            CtxItem::Act { keys: vec!["exec git status".into()] },
                            CtxItem::Edit { glob: "src/*.rs".into() },
                        ],
                        within: 3,
                    },
                    step: Step::Run { command: "sh tools/check.sh".into() },
                    slots: vec![],
                    effect: Effect::Verify,
                    strength: Strength::Fire,
                    state: RouteState::Firm,
                    exit: vec![],
                    authored: false,
                    evidence: Evidence {
                        execution: Some(ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 }),
                        ..Default::default()
                    },
                },
            }],
            ..Default::default()
        };
        Self { _tmp: tmp, root, paths, index }
    }

    pub(super) fn turn<'a>(&'a self, ev: &'a Event) -> Turn<'a> {
        Turn {
            harness: adage_hook::harness::Harness::ClaudeCode,
            event: "PostToolUse",
            event_identity: Some("edit-1".into()),
            ev,
            session: "fixture-session",
            index: &self.index,
            repo_key: "fixture",
            root: &self.root,
            cwd: &self.root,
            paths: &self.paths,
            max_context: 16000,
            arm: Some(Arm::Treated),
            stimuli: &[],
        }
    }

    /// The runtime's context for this fixture, with `exec_only` the one route only adage-exec runs.
    pub(super) fn ctx<'a>(&'a self, table: &'a CommandTable, exec_only: (String, String)) -> Ctx<'a> {
        Ctx {
            checkout: &adage_runtime::checkout::Disk,
            index: &self.index,
            repo: "fixture".into(),
            root: &self.root,
            table,
            embed: None,
            params: Default::default(),
            policy: adage_runtime::Policy { exec_only: vec![exec_only], ..Default::default() },
        }
    }

    pub(super) fn selected(&self, path: &str) -> (Event, Vec<Directive>) {
        let table = CommandTable::load();
        let ctx = self.ctx(&table, ("fixture/context".into(), "predict-check".into()));
        let mut state = SessionState { arm: Some(Arm::Treated), ..Default::default() };
        let events = [
            batch("Bash", json!({"command":"git status --short"}), ""),
            batch("Read", json!({"file_path": self.root.join("README.md")}), ""),
            batch("Edit", json!({"file_path": self.root.join(path)}), ""),
        ];
        let mut dirs = vec![];
        {
            for ev in &events {
                dirs = adage_runtime::decide(&ctx, &mut state, ev).directives;
            }
            assert!(
                adage_runtime::decide(&ctx, &mut state, &events[2]).directives.is_empty(),
                "repeat is not selected"
            );
        };
        assert!(state.obligations.is_empty(), "prediction creates no edit obligation");
        adage_store::state::update::<SessionState, _>(&self.paths.state("fixture-session"), |s| *s = state).unwrap();
        (events[2].clone(), dirs)
    }

    pub(super) fn approve(&self) {
        self.approve_with(vec![]);
    }

    pub(super) fn approve_with(&self, cache: Vec<String>) {
        let terms = route::Terms { effect: Some(Effect::Verify), cache, ..Default::default() };
        let req = route::plan(
            &self.index.routes[0],
            &self.index,
            &self.root,
            &self.paths.tracks(),
            Level::E2,
            Some(&terms),
            &CommandTable::load(),
        )
        .unwrap()
        .request;
        let mut store = Store::default();
        store.approve(Approval {
            key: approvals::ApprovalKey::for_request(&req, "fixture"),
            route: approvals::route_label(&req),
            approved_on: "fixture-only".into(),
            by: None,
            basis: Value::Null,
            auto: false,
            terms: Some(terms),
            via: None,
        });
        store.save(&approvals::path(&self.paths.root)).unwrap();
        std::fs::write(self.paths.root.join("policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
    }

    /// The fixture route's E2 request under the first grant's terms.
    pub(super) fn request(&self) -> ExecRequest {
        route::plan(
            &self.index.routes[0],
            &self.index,
            &self.root,
            &self.paths.tracks(),
            Level::E2,
            Store::load(&approvals::path(&self.paths.root)).unwrap().approvals[0].terms.as_ref(),
            &CommandTable::load(),
        )
        .unwrap()
        .request
    }

    /// The receipt check for `command` in `session`, made from `cwd`.
    pub(super) fn receipt_from(&self, command: &str, session: &str, cwd: &std::path::Path) -> Option<PreNote> {
        PREFETCH.pretool_context(&PreCall {
            repo_key: "fixture",
            root: &self.root,
            cwd,
            session,
            paths: &self.paths,
            index: &self.index,
            command,
        })
    }

    /// The receipt check for `command` in the fixture's session, made at the repository root.
    pub(super) fn receipt(&self, command: &str) -> Option<PreNote> {
        self.receipt_from(command, "fixture-session", &self.root)
    }
}

/// One agent call as a batch.
pub(super) fn batch(tool: &str, input: Value, output: &str) -> Event {
    Event::Batch {
        calls: vec![Call {
            tool: tool.into(),
            input,
            output: output.into(),
            failed: false,
            by: adage_runtime::By::Agent,
            tool_use_id: None,
            agent: None,
        }],
    }
}

/// A completed journal row for the fixture's route in `session`, on `digest`.
pub(super) fn completed(session: &str, outcome: Outcome, digest: &str) -> Receipt {
    Receipt {
        origin: None,
        id: String::new(),
        ts: "fixture".into(),
        session: session.into(),
        repo: "fixture".into(),
        worktree: String::new(),
        route: CHECK.into(),
        approved_on: "fixture-only".into(),
        level: "e2".into(),
        phase: Phase::Completed,
        outcome,
        input_digest: digest.into(),
        outputs: BTreeMap::new(),
    }
}

/// An applied run, as the journal records one.
pub(super) fn applied() -> Outcome {
    Outcome::Applied { changed: vec![], discarded: 0 }
}

/// Appends `r` to the fixture's journal.
pub(super) fn journal(f: &Fixture, r: &Receipt) {
    std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
    adage_store::spool::append(&crate::stage::spool_path(&f.paths.root), r).unwrap();
}

/// A completed, applied receipt for the fixture's route in `session`, on `digest`.
pub(super) fn record_receipt(f: &Fixture, session: &str, digest: &str) {
    journal(f, &Receipt { origin: Some(RunOrigin::Context), ..completed(session, applied(), digest) });
}

/// The fixture as a project-Python repository with one pytest verification route (the layout
/// `project_python` accepts: a venv whose runtime lives outside the repository).
pub(super) fn pytest_fixture() -> Fixture {
    let mut f = Fixture::new();
    let top = f.root.parent().unwrap().to_path_buf();
    for d in [".venv/bin", ".venv/lib/python3.12/site-packages", "tests"] {
        std::fs::create_dir_all(f.root.join(d)).unwrap();
    }
    for d in ["runtime/bin", "runtime/lib/python3.12/encodings"] {
        std::fs::create_dir_all(top.join(d)).unwrap();
    }
    std::fs::write(top.join("runtime/bin/python3.12"), b"\x7fELFfixture-only").unwrap();
    std::fs::write(top.join("runtime/lib/python3.12/os.py"), "# fixture\n").unwrap();
    std::fs::write(top.join("runtime/lib/python3.12/encodings/__init__.py"), "# fixture\n").unwrap();
    std::fs::write(
        f.root.join(".venv/pyvenv.cfg"),
        format!("home = {}/runtime/bin\nversion_info = 3.12\ninclude-system-site-packages = false\n", top.display()),
    )
    .unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(top.join("runtime/bin/python3.12"), f.root.join(".venv/bin/python")).unwrap();
    std::fs::write(f.root.join("tests/test_x.py"), "def test_one():\n    pass\n").unwrap();
    f.index.routes[0].route.step = Step::Run { command: ".venv/bin/python -m pytest tests/test_x.py -q".into() };
    f.approve_with(vec![".pytest_cache/**".into()]);
    f
}

/// The digest of the fixture route's inputs as they are now.
pub(super) fn current_digest(f: &Fixture) -> String {
    let req = f.request();
    let now = crate::stage::input_snapshot(&f.root.canonicalize().unwrap(), req.action.as_ref().unwrap()).unwrap();
    crate::sha256_hex(crate::canonical_json(&now).as_bytes())
}

pub(super) fn stop(f: &Fixture, active: bool) -> Option<StopGate> {
    stop_by(f, active, Instant::now() + std::time::Duration::from_secs(28))
}

pub(super) fn stop_by(f: &Fixture, active: bool, deadline: Instant) -> Option<StopGate> {
    PREFETCH.stop_gate(&StopCall {
        harness: adage_hook::harness::Harness::ClaudeCode,
        repo_key: "fixture",
        root: &f.root,
        session: "fixture-session",
        paths: &f.paths,
        index: &f.index,
        stop_hook_active: active,
        deadline,
    })
}

pub(super) fn treated(f: &Fixture, arm: Arm) {
    adage_store::state::update::<SessionState, _>(&f.paths.state("fixture-session"), |s| s.arm = Some(arm)).unwrap();
}

/// Whether this machine can stage a run (tests that need one return early without it).
pub(super) fn staging() -> bool {
    crate::stage::backend(&sandbox::detect()).available()
}

/// The fixture as the hook pipeline serves it (`adage_hook::handle` with this extension): a
/// learned finish-line obligation beside the learned verification (after an edit to
/// `registry/*.csv`, run `make gen`; no grant for it), and no learner.
pub(super) fn with_obligation(f: &mut Fixture) {
    f.index.routes.push(IndexedRoute {
        repo: "fixture".into(),
        track: "fixture/finish".into(),
        version: "0.1.0".into(),
        exec: None,
        route: Route {
            id: "regenerate-registry".into(),
            summary: String::new(),
            trigger: Trigger::AfterEdit { paths: "registry/*.csv".into() },
            step: Step::Run { command: "make gen".into() },
            slots: vec![],
            effect: Effect::Regenerate,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![],
            authored: false,
            evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
        },
    });
    f.index.write_atomic(&f.paths.index()).unwrap();
    std::fs::write(f.paths.root.join("config.toml"), "[learn]\nauto = false\n").unwrap();
    std::fs::create_dir_all(f.root.join("registry")).unwrap();
    std::fs::write(f.root.join("registry/currencies.csv"), "EUR\n").unwrap();
}

/// One hook event for the fixture's session, through the whole pipeline.
pub(super) fn hook(f: &Fixture, event: &str, extra: Value) -> Option<Value> {
    let mut p = json!({ "session_id": "fixture-session", "cwd": f.root });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    let opts = adage_hook::Options { learner: None, extension: Some(&PREFETCH) };
    let out = adage_hook::handle("claude", event, &p.to_string(), opts, &f.paths, Instant::now()).unwrap();
    out.map(|o| serde_json::from_str(&o).unwrap())
}

/// The agent's edit of `file` as one `PostToolBatch` call.
pub(super) fn edit(file: std::path::PathBuf) -> Value {
    json!({ "tool_name": "Edit", "tool_use_id": "e1", "tool_response": "ok", "tool_input": { "file_path": file } })
}

/// The fixture route's directive as the runtime delivers it.
pub(super) fn check_directive() -> Directive {
    Directive {
        route: "predict-check".into(),
        track: "fixture/context".into(),
        version: "0.1.0".into(),
        strength: Strength::Fire,
        text: "x".into(),
        kind: "run".into(),
        origin: None,
    }
}

/// The fixture's grants as a person writes them: `review delegate --predict` on the route at
/// `at`, keyed on its own path, added to whatever the store holds, with policy at E2.
pub(super) fn grant_predict(f: &Fixture, at: usize) {
    let ir = &f.index.routes[at];
    let req = route::plan_predict(ir, &f.root, &f.paths.tracks()).unwrap().request;
    seed(f, &format!("{}/{}", ir.track, ir.route.id), approvals::ApprovalKey::for_request(&req, "fixture"), None);
}

/// An E2 execute grant on the finish line (`make gen`, writing `gen/**`), first in the store.
pub(super) fn grant_finish_line(f: &Fixture) {
    let terms = route::Terms { outputs: vec!["gen/**".into()], ..Default::default() };
    let ir = &f.index.routes[1];
    let req = route::plan(ir, &f.index, &f.root, &f.paths.tracks(), Level::E2, Some(&terms), &CommandTable::load())
        .unwrap()
        .request;
    seed(f, "fixture/finish/regenerate-registry", approvals::ApprovalKey::for_request(&req, "fixture"), Some(terms));
}

/// A predict key for the learned context check, written by hand: the CLI refuses this grant
/// (only adage-exec can carry the route out), so it stands for a grant that should never act.
pub(super) fn predict_key_by_hand(f: &Fixture) -> approvals::ApprovalKey {
    let terms = route::Terms { effect: Some(Effect::Verify), ..Default::default() };
    let ir = &f.index.routes[0];
    let mut req = route::plan(ir, &f.index, &f.root, &f.paths.tracks(), Level::E2, Some(&terms), &CommandTable::load())
        .unwrap()
        .request;
    req.mode = approvals::GrantMode::Predict;
    req.action = None;
    approvals::ApprovalKey::for_request(&req, "fixture")
}

/// Put `key` first in the store (so a lookup that ignored the mode would find it first).
pub(super) fn seed(f: &Fixture, label: &str, key: approvals::ApprovalKey, terms: Option<route::Terms>) {
    let path = approvals::path(&f.paths.root);
    let mut store = Store::load(&path).unwrap();
    let a = Approval {
        key,
        route: label.into(),
        approved_on: "fixture-only".into(),
        by: None,
        basis: Value::Null,
        auto: false,
        terms,
        via: None,
    };
    store.approvals.retain(|x| !(x.route == a.route && x.key.mode == a.key.mode && x.key.level == a.key.level));
    store.approvals.insert(0, a);
    store.save(&path).unwrap();
    std::fs::write(f.paths.root.join("policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
}

/// A failed staged pytest run whose record the plan carries (`after`), from the worktree's
/// record (`before`), for [`super::pytest::failure_record`].
pub(super) fn failed_run(
    f: &Fixture,
    before: Option<&str>,
    after: Option<&str>,
    rewritten: bool,
) -> crate::stage::Staged {
    use super::pytest::PYTEST_LASTFAILED;
    let req = f.request();
    let record = f.root.join(PYTEST_LASTFAILED);
    std::fs::create_dir_all(record.parent().unwrap()).unwrap();
    match before {
        Some(b) => std::fs::write(&record, b).unwrap(),
        None => drop(std::fs::remove_file(&record)),
    }
    let apply: Vec<crate::stage::Change> = after
        .map(|a| crate::stage::Change {
            path: PYTEST_LASTFAILED.into(),
            kind: crate::stage::ChangeKind::Modified,
            after: crate::stage::After::File { bytes: a.as_bytes().to_vec(), exec: false },
            before_file: before.is_some(),
        })
        .into_iter()
        .collect();
    crate::stage::Staged {
        repo: f.root.clone(),
        action: req.action.unwrap(),
        exit: Some(1),
        signal: None,
        duration_ms: 5,
        plan: crate::stage::Plan { apply, ..Default::default() },
        pre: Default::default(),
        inputs: BTreeMap::new(),
        fields: None,
        backend: crate::stage::StageBackend::Copy,
        stderr: vec![],
        rewritten: if rewritten { vec![PYTEST_LASTFAILED.into()] } else { vec![] },
    }
}
