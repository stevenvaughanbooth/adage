//! What the hook's unit tests share: the worlds they run events in (a temporary data directory and
//! repository with a hand-written index, built with the workspace test kit) and the stand-in
//! extensions.

use crate::{handle, Delivery, Extension, Options, PreCall, PreNote, ReceiptCard, StopCall, StopGate, Turn};
use adage_core::repo;
use adage_runtime::{Arm, Directive, SessionState};
use adage_store::Paths;
use adage_tracks::testkit::{self, kinds, IndexBuilder, LearnedHome, TempWorld};
use adage_tracks::{Origin, Step, Strength, Trigger};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::time::Instant;

/// A stand-in for adage-exec: its finish-line gate answers from a script (one answer per Stop,
/// in order), and it counts how often it was asked.
pub(crate) struct Scripted {
    answers: std::sync::Mutex<Vec<StopGate>>,
    pub(crate) asked: std::sync::atomic::AtomicUsize,
}

impl Extension for Scripted {
    fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
        Delivery::default()
    }
    fn stop_gate(&self, _: &StopCall<'_>) -> Option<StopGate> {
        self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let mut a = self.answers.lock().unwrap();
        (!a.is_empty()).then(|| a.remove(0))
    }
}

pub(crate) fn scripted(answers: Vec<StopGate>) -> &'static Scripted {
    Box::leak(Box::new(Scripted { answers: std::sync::Mutex::new(answers), asked: Default::default() }))
}

/// A treated session in a repository whose one learned reflex is a finish-line obligation:
/// after an edit to `registry/*.csv`, run `make gen`.
pub(crate) struct ObligationWorld {
    _world: TempWorld,
    pub(crate) root: PathBuf,
    /// The data directory (`ADAGE_HOME`), which seeds and reads session `s`.
    pub(crate) home: LearnedHome,
    /// `home.paths`, by the name the tests use.
    pub(crate) paths: Paths,
}

impl ObligationWorld {
    pub(crate) fn new() -> Self {
        let world = TempWorld::new(&["acme/registry"]);
        let root = world.repo("acme");
        let home = world.home("home").without_auto_learn();
        IndexBuilder::new("obligation-world")
            .route(
                "acme",
                "acme/finish-lines",
                kinds::finish_line("regenerate-registry", "registry/*.csv", "make gen").support(5, 5).days(3),
            )
            .write(&home.paths.index());
        home.seed::<SessionState>("s", |st| st.arm = Some(Arm::Treated));
        let paths = home.paths.clone();
        ObligationWorld { _world: world, root, home, paths }
    }

    /// Session `s` in `arm`.
    fn arm(&self, arm: Arm) {
        self.home.seed::<SessionState>("s", |st| st.arm = Some(arm));
    }

    pub(crate) fn event(&self, ext: &'static dyn Extension, event: &str, extra: Value) -> Option<Value> {
        let mut p = json!({ "session_id": "s", "cwd": self.root });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let opts = Options { learner: None, extension: Some(ext) };
        let out = handle("claude", event, &p.to_string(), opts, &self.paths, Instant::now()).unwrap();
        out.map(|o| serde_json::from_str(&o).unwrap())
    }

    pub(crate) fn edit(&self, ext: &'static dyn Extension) -> Option<Value> {
        let call = json!({ "tool_name": "Edit", "tool_use_id": "t1", "tool_response": "ok",
                           "tool_input": { "file_path": self.root.join("registry/currencies.csv") } });
        self.event(ext, "PostToolBatch", json!({ "tool_calls": [call] }))
    }

    pub(crate) fn stop(&self, ext: &'static dyn Extension, active: bool) -> Option<Value> {
        self.event(ext, "Stop", json!({ "stop_hook_active": active }))
    }

    pub(crate) fn state(&self) -> SessionState {
        self.home.state::<SessionState>("s")
    }
}

pub(crate) fn spooled(w: &ObligationWorld) -> Vec<Value> {
    w.home.spooled("s")
}

/// A repository whose sessions learned that `python3 -m pytest` fails and `make test` works.
pub(crate) fn dead_end_world(arm: Arm) -> ObligationWorld {
    let w = ObligationWorld::new();
    let avoid = "python3 -m pytest / python -m pytest";
    IndexBuilder::new("dead-end-world")
        .route(
            "acme",
            "acme/dead-ends",
            kinds::dead_end("dead-end-pytest", avoid, "make test", "No module named pytest").support(5, 5).days(3),
        )
        .write(&w.paths.index());
    w.arm(arm);
    w
}

pub(crate) struct Plain;
impl Extension for Plain {
    fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
        Delivery::default()
    }
}
pub(crate) static NO_EXTENSION: Plain = Plain;

/// A treated (or control) session in a checkout whose project environment is bound for U11
/// (`.venv/bin/python`, pytest installed), with no rule in the person's settings that a rewrite
/// would change.
pub(crate) fn u11_world(arm: Arm) -> ObligationWorld {
    let w = ObligationWorld::new();
    let key = repo::repo_key_cached(&w.root, &w.paths.repo_keys());
    let origin = Origin::Instinct {
        pack: "core".into(),
        version: "0.1.0".into(),
        instinct: "U11".into(),
        ecosystem: None,
        declared: None,
    };
    let trigger = Trigger::AfterFailure { command: "exec python -m pytest".into(), signature: "No module".into() };
    let route = testkit::route("python-env", trigger, Step::Run { command: ".venv/bin/python -m pytest".into() })
        .strength(Strength::Prime)
        .support(1, 1)
        .origin(origin);
    IndexBuilder::new("u11-world").route(&key, "acme/instincts", route).write(&w.paths.index());
    std::fs::create_dir_all(w.root.join(".venv/bin")).unwrap();
    std::fs::write(w.root.join(".venv/bin/python"), "").unwrap();
    std::fs::write(w.root.join(".venv/pyvenv.cfg"), "version = 3.12.4\n").unwrap();
    let binding = adage_pack::binding::binding_path(&w.paths.tracks(), &key);
    std::fs::create_dir_all(binding.parent().unwrap()).unwrap();
    let b = json!({ "root": w.root, "declared": "", "env": { "interpreter": ".venv/bin/python", "pytest": true, "modules": [] }, "packs": "" });
    std::fs::write(binding, b.to_string()).unwrap();
    w.arm(arm);
    w
}

/// A Bash call's `PreToolUse` in session `s`, with this tool-use id (none when `id` is empty).
pub(crate) fn bash_call(
    w: &ObligationWorld,
    ext: &'static dyn Extension,
    id: &str,
    command: &str,
    extra: Value,
) -> Option<Value> {
    let mut p = json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_input": { "command": command } });
    if !id.is_empty() {
        p["tool_use_id"] = json!(id);
    }
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    w.event(ext, "PreToolUse", p)
}

pub(crate) fn pretool_records(w: &ObligationWorld) -> Vec<Value> {
    w.home.spooled_kind("s", "pretool")
}

pub(crate) const PYTEST: &str = "python -m pytest tests -q";

/// A world whose finish line is a test run: after an edit of `tests/*.py`, `python -m pytest
/// tests -q`.
pub(crate) fn receipt_world(arm: Arm) -> ObligationWorld {
    let w = ObligationWorld::new();
    std::fs::create_dir_all(w.root.join("tests")).unwrap();
    // A test run, not a regeneration: a verification the turn owes after the edit.
    let route = kinds::finish_line("tests-after-edit", "tests/*.py", PYTEST)
        .effect(adage_core::Effect::Verify)
        .support(5, 5)
        .days(3);
    IndexBuilder::new("receipt-world").route("acme", "acme/finish-lines", route).write(&w.paths.index());
    w.arm(arm);
    w
}

pub(crate) struct FreshReceipts;
impl Extension for FreshReceipts {
    fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
        Delivery::default()
    }
    fn pretool_context(&self, call: &PreCall<'_>) -> Option<PreNote> {
        call.command.contains("pytest").then(|| PreNote {
            text: "<adage-receipt operation=\"op\">\nAdage's run passed.\n</adage-receipt>".into(),
            fresh: !call.command.contains("stale"),
            result: (!call.command.contains("stale")).then(|| "Adage answered this test run.".into()),
            covering: PYTEST.into(),
            card: (!call.command.contains("stale")).then(|| ReceiptCard {
                operation: "op".into(),
                ran_at: "2026-10-03T10:00:00Z".into(),
                ago: "4 min ago".into(),
                summary: "passed on files identical to the current ones".into(),
                tests: vec!["tests/test_x.py::t".into()],
                more: 0,
            }),
            operation: "op".into(),
        })
    }
}
pub(crate) static FRESH_RECEIPTS: FreshReceipts = FreshReceipts;
