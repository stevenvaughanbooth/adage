//! What the hook's unit tests share: the worlds they run events in (a temporary data directory and
//! repository with a hand-written index) and the stand-in extensions.

use crate::{handle, Delivery, Extension, Options, PreCall, PreNote, ReceiptCard, StopCall, StopGate, Turn};
use adage_core::repo;
use adage_runtime::{Arm, Directive, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
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
    _temp: tempfile::TempDir,
    pub(crate) root: PathBuf,
    pub(crate) paths: Paths,
}

impl ObligationWorld {
    pub(crate) fn new() -> Self {
        use adage_tracks::{index::IndexedRoute, Evidence, Route, RouteState, Step, Strength, Trigger};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap().join("acme");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::create_dir_all(root.join("registry")).unwrap();
        let paths = Paths::at(temp.path().join("home"));
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(paths.root.join("config.toml"), "[learn]\nauto = false\n").unwrap();
        let route = Route {
            id: "regenerate-registry".into(),
            summary: String::new(),
            trigger: Trigger::AfterEdit { paths: "registry/*.csv".into() },
            step: Step::Run { command: "make gen".into() },
            slots: vec![],
            effect: adage_core::Effect::Regenerate,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
            authored: false,
        };
        let index = Index {
            generation: "obligation-world".into(),
            routes: vec![IndexedRoute {
                repo: "acme".into(),
                track: "acme/finish-lines".into(),
                version: "0.1.0".into(),
                exec: None,
                route,
            }],
            ..Default::default()
        };
        index.write_atomic(&paths.index()).unwrap();
        state::update::<SessionState, _>(&paths.state("s"), |st| st.arm = Some(Arm::Treated)).unwrap();
        ObligationWorld { _temp: temp, root, paths }
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
        state::try_read::<SessionState>(&self.paths.state("s")).unwrap().unwrap()
    }
}

pub(crate) fn spooled(w: &ObligationWorld) -> Vec<Value> {
    spool::read_all(&w.paths.spool("s")).unwrap()
}

/// A repository whose sessions learned that `python3 -m pytest` fails and `make test` works.
pub(crate) fn dead_end_world(arm: Arm) -> ObligationWorld {
    use adage_tracks::{index::IndexedRoute, Evidence, Route, RouteState, Step, Strength, Trigger};
    let w = ObligationWorld::new();
    let route = Route {
        id: "dead-end-pytest".into(),
        summary: String::new(),
        trigger: Trigger::SessionStart,
        step: Step::Avoid {
            avoid: "python3 -m pytest / python -m pytest".into(),
            instead: "make test".into(),
            signature: "No module named pytest".into(),
        },
        slots: vec![],
        effect: adage_core::Effect::Verify,
        strength: Strength::Inhibit,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
        authored: false,
    };
    let index = Index {
        generation: "dead-end-world".into(),
        routes: vec![IndexedRoute {
            repo: "acme".into(),
            track: "acme/dead-ends".into(),
            version: "0.1.0".into(),
            exec: None,
            route,
        }],
        ..Default::default()
    };
    index.write_atomic(&w.paths.index()).unwrap();
    state::update::<SessionState, _>(&w.paths.state("s"), |st| st.arm = Some(arm)).unwrap();
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
    use adage_tracks::{index::IndexedRoute, Evidence, Origin, Route, RouteState, Step, Strength, Trigger};
    let w = ObligationWorld::new();
    let key = repo::repo_key_cached(&w.root, &w.paths.repo_keys());
    let origin = Origin::Instinct {
        pack: "core".into(),
        version: "0.1.0".into(),
        instinct: "U11".into(),
        ecosystem: None,
        declared: None,
    };
    let route = Route {
        id: "python-env".into(),
        summary: String::new(),
        trigger: Trigger::AfterFailure { command: "exec python -m pytest".into(), signature: "No module".into() },
        step: Step::Run { command: ".venv/bin/python -m pytest".into() },
        slots: vec![],
        effect: adage_core::Effect::Verify,
        strength: Strength::Prime,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence { support: 1, opportunities: 1, origin: Some(origin), ..Default::default() },
        authored: false,
    };
    let index = Index {
        generation: "u11-world".into(),
        routes: vec![IndexedRoute {
            repo: key.clone(),
            track: "acme/instincts".into(),
            version: "0.1.0".into(),
            exec: None,
            route,
        }],
        ..Default::default()
    };
    index.write_atomic(&w.paths.index()).unwrap();
    std::fs::create_dir_all(w.root.join(".venv/bin")).unwrap();
    std::fs::write(w.root.join(".venv/bin/python"), "").unwrap();
    std::fs::write(w.root.join(".venv/pyvenv.cfg"), "version = 3.12.4\n").unwrap();
    let binding = adage_pack::binding::binding_path(&w.paths.tracks(), &key);
    std::fs::create_dir_all(binding.parent().unwrap()).unwrap();
    let b = json!({ "root": w.root, "declared": "", "env": { "interpreter": ".venv/bin/python", "pytest": true, "modules": [] }, "packs": "" });
    std::fs::write(binding, b.to_string()).unwrap();
    state::update::<SessionState, _>(&w.paths.state("s"), |st| st.arm = Some(arm)).unwrap();
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
    spool::read_all::<Value>(&w.paths.spool("s"))
        .unwrap_or_default()
        .into_iter()
        .filter(|r| r["kind"] == "pretool")
        .collect()
}

pub(crate) const PYTEST: &str = "python -m pytest tests -q";

/// A world whose finish line is a test run: after an edit of `tests/*.py`, `python -m pytest
/// tests -q`.
pub(crate) fn receipt_world(arm: Arm) -> ObligationWorld {
    use adage_tracks::{index::IndexedRoute, Evidence, Route, RouteState, Step, Strength, Trigger};
    let w = ObligationWorld::new();
    std::fs::create_dir_all(w.root.join("tests")).unwrap();
    let route = Route {
        id: "tests-after-edit".into(),
        summary: String::new(),
        trigger: Trigger::AfterEdit { paths: "tests/*.py".into() },
        step: Step::Run { command: PYTEST.into() },
        slots: vec![],
        effect: adage_core::Effect::Verify,
        strength: Strength::Fire,
        state: RouteState::Firm,
        exit: vec![],
        evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
        authored: false,
    };
    let index = Index {
        generation: "receipt-world".into(),
        routes: vec![IndexedRoute {
            repo: "acme".into(),
            track: "acme/finish-lines".into(),
            version: "0.1.0".into(),
            exec: None,
            route,
        }],
        ..Default::default()
    };
    index.write_atomic(&w.paths.index()).unwrap();
    state::update::<SessionState, _>(&w.paths.state("s"), |st| st.arm = Some(arm)).unwrap();
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
