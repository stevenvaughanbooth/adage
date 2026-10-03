//! The receipt beside a `PreToolUse` test run: a passing learned verification in this session
//! that already ran every test the call selects ([`covering_receipt`]), and the result the mod may
//! answer the call with ([`receipt_result`]).

use adage_hook::{PreCall, PreNote};
use adage_runtime::render;
use adage_tracks::Step;
use std::time::Instant;

use super::pytest::{
    covers, in_targets, pytest_calls, pytest_command, test_function, valid_nodeid, PytestCall, MAX_LISTED_TESTS,
    PYTEST_NODEIDS,
};
use super::{executes_at, label};
use crate::approvals::{self, Store};
use crate::request::Level;
use crate::route;
use adage_core::CommandTable;

/// The most a receipt check may take of a `PreToolUse` hook's 5 s: past it, no note (the
/// interpreter rewrite beside it must still be delivered in time).
const RECEIPT_CHECK_MS: u64 = 1_500;

/// The receipt of a completed, successful learned verification in this session that already ran
/// every test `command` selects ([`PreNote`]), or `None` when there is none. It is **fresh** when
/// the call is made at the repository root (relative targets resolve as the receipt's did) and the
/// run's inputs are byte-identical now (the executor's own snapshot, recomputed here within
/// [`RECEIPT_CHECK_MS`]); only a fresh receipt carries the note for the model and the result the
/// mod may answer the call with. A receipt that is not fresh names only the command that ran. A
/// fact, never a refusal: the call still runs if it is made (review F5, F6).
pub(super) fn covering_receipt(call: &PreCall<'_>) -> Option<PreNote> {
    let deadline = Instant::now() + std::time::Duration::from_millis(RECEIPT_CHECK_MS);
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let asked = pytest_calls(call.command, call.root)?;
    let rows = crate::journal::list(&call.paths.root).ok()?;
    let r = rows.iter().rev().find(|r| {
        r.repo == call.repo_key
            && r.session == call.session
            && r.phase == crate::journal::Phase::Completed
            && matches!(r.outcome, crate::stage::Outcome::Applied { .. })
    })?;
    let ir = call.index.for_repo(call.repo_key).find(|x| label(&x.track, &x.route.id) == r.route)?;
    let Step::Run { command: template } = &ir.route.step else { return None };
    if ir.route.effect != adage_core::Effect::Verify || template.contains('{') {
        return None;
    }
    let store = Store::load(&approvals::path(&call.paths.root)).ok()?;
    let a = store.approvals.iter().find(|a| a.route == r.route && executes_at(a, call.repo_key, Level::E2))?;
    let done = pytest_command(template)?;
    if !asked.iter().all(|c| covers(&done, c)) {
        return None;
    }
    let elsewhere = asked.iter().any(|c| c.interpreter != done.interpreter);
    // A bare `python` is the project's interpreter only when no other environment is active.
    let project_env =
        std::path::Path::new(&done.interpreter).parent().and_then(|bin| bin.parent()).map(|e| call.root.join(e));
    let active = std::env::var_os("VIRTUAL_ENV").filter(|v| !v.is_empty()).map(std::path::PathBuf::from);
    if elsewhere
        && active
            .is_some_and(|a| Some(a.canonicalize().unwrap_or(a)) != project_env.map(|p| p.canonicalize().unwrap_or(p)))
    {
        return None;
    }
    let operation: String = render::sanitise(&r.id).chars().take(12).collect();
    let stale = PreNote {
        text: String::new(),
        fresh: false,
        result: None,
        covering: template.clone(),
        card: None,
        operation: operation.clone(),
    };
    if canon(call.cwd) != canon(call.root) {
        return Some(stale);
    }
    let unchanged = || -> Option<bool> {
        let plan = route::plan(
            ir,
            call.index,
            call.root,
            &call.paths.tracks(),
            Level::E2,
            a.terms.as_ref(),
            &CommandTable::load(),
        )
        .ok()?;
        let action = plan.request.action.as_ref()?;
        let repo = plan.request.repo_root.canonicalize().ok()?;
        let now = crate::stage::input_snapshot_until(&repo, action, Some(deadline)).ok()?;
        Some(crate::sha256_hex(crate::canonical_json(&now).as_bytes()) == r.input_digest)
    };
    if unchanged() != Some(true) {
        return Some(stale);
    }
    let text = format!(
        "<adage-receipt operation=\"{}\">\nAdage's run of `{}` in this session passed on files identical to the current ones, and it included every test this command selects{}.\n</adage-receipt>",
        operation,
        render::sanitise(template),
        if elsewhere { " (in the project's environment)" } else { "" }
    );
    let (result, tests, total) = receipt_result(call, r, template, &asked, &operation);
    let card = adage_hook::ReceiptCard {
        ago: crate::time::parse(&r.ts).map(|t| crate::time::ago(crate::time::now_secs(), t)).unwrap_or_default(),
        ran_at: render::sanitise(&r.ts),
        summary: format!(
            "passed on files identical to the current ones{}",
            match total {
                0 => String::new(),
                1 => " · 1 test this command selects".into(),
                n => format!(" · {n} tests this command selects"),
            }
        ),
        more: total - tests.len(),
        tests,
        operation: operation.clone(),
    };
    Some(PreNote { text, fresh: true, result: Some(result), covering: template.clone(), card: Some(card), operation })
}

/// The most a receipt's result names of the tests it ran.
const MAX_RESULT_BYTES: usize = 2_000;

/// A fresh receipt as the answered call's output reads (the Claude Code mod's architecture §3.3):
/// shorter than the runner's own, it says Adage answered from its own run, which command ran and
/// when, and the tests it ran that the call selects (the session's own new tests first), from the
/// run's pytest record when the journal binds it (the file now in the worktree is the run's
/// output). Every interpolated value is sanitised; at most [`MAX_RESULT_BYTES`].
fn receipt_result(
    call: &PreCall<'_>,
    r: &crate::journal::Receipt,
    template: &str,
    asked: &[PytestCall],
    operation: &str,
) -> (String, Vec<String>, usize) {
    let mut text = format!(
        "Adage answered this test run from its own run of `{}` in this session ({}{}): it passed on files identical to the current ones, and it ran every test this command selects. The command itself did not run again.",
        render::sanitise(template),
        render::sanitise(&r.ts),
        if operation.is_empty() { String::new() } else { format!(", operation {operation}") }
    );
    let bound = match r.outputs.get(PYTEST_NODEIDS) {
        Some(crate::stage::FileState::File { sha256, .. }) => std::fs::read(call.root.join(PYTEST_NODEIDS))
            .ok()
            .filter(|b| crate::sha256_hex(b) == *sha256)
            .and_then(|b| serde_json::from_slice::<Vec<String>>(&b).ok()),
        _ => None,
    };
    let targets: Vec<String> = asked.iter().flat_map(|c| c.targets.iter().cloned()).collect();
    let mut ids: Vec<String> =
        bound.unwrap_or_default().into_iter().filter(|id| valid_nodeid(id) && in_targets(id, &targets)).collect();
    if ids.is_empty() {
        return (text, Vec::new(), 0);
    }
    // The session's own tests first (the runtime's names of test functions its edits added).
    let mine: Vec<String> =
        adage_store::state::try_read::<adage_runtime::SessionState>(&call.paths.state(call.session))
            .ok()
            .flatten()
            .map(|st| st.new_tests)
            .unwrap_or_default();
    ids.sort_by_key(|id| (!mine.iter().any(|n| n == test_function(id)), id.clone()));
    let total = ids.len();
    let mut listed = Vec::new();
    text.push_str("\nTests it ran:");
    for id in &ids {
        let id = render::sanitise(id);
        let line = format!("\n- {id} passed");
        if listed.len() == MAX_LISTED_TESTS || text.len() + line.len() + 32 > MAX_RESULT_BYTES {
            break;
        }
        text.push_str(&line);
        listed.push(id);
    }
    if total > listed.len() {
        text.push_str(&format!("\n- and {} more", total - listed.len()));
    }
    (text, listed, total)
}

#[cfg(test)]
mod tests {
    use super::super::pytest::pytest_call;
    use super::super::testkit::*;
    use super::*;
    use adage_core::Effect;
    use adage_runtime::{Arm, SessionState};
    use adage_tracks::{Evidence, IndexedRoute, Route, RouteState, Strength, Trigger};
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    /// T-C3-01: a receipt is fresh, with its note, its result and the command that ran, for an
    /// applied run on identical inputs that covers the call; after one byte changes in a
    /// snapshotted file it is not fresh and has neither; a call made from elsewhere is not fresh;
    /// a call that is not a pytest run has no receipt at all.
    #[test]
    fn t_c3_01_pretool_context_needs_a_fresh_successful_receipt_in_this_session_covering_the_call() {
        let f = pytest_fixture();
        let call_from =
            |command: &'static str, session: &'static str, cwd: &std::path::Path| f.receipt_from(command, session, cwd);
        let call = |command: &'static str, session: &'static str| call_from(command, session, &f.root);
        let fresh = |command: &'static str| call(command, "fixture-session").filter(|n| n.fresh);
        let node = ".venv/bin/python -m pytest tests/test_x.py::test_one -xvs";
        let template = ".venv/bin/python -m pytest tests/test_x.py -q";
        assert!(call(node, "fixture-session").is_none(), "no receipt yet");
        let digest = current_digest(&f);
        record_receipt(&f, "fixture-session", &digest);
        let note = call(node, "fixture-session").unwrap();
        assert!(note.fresh && note.covering == template, "{note:?}");
        assert!(note.text.contains("run of `.venv/bin/python -m pytest tests/test_x.py -q` in this session passed on files identical to the current ones"), "{note:?}");
        assert!(!note.text.contains("project's environment"));
        let result = note.result.as_deref().unwrap();
        assert!(result.starts_with("Adage answered this test run from its own run of `.venv/bin/python -m pytest tests/test_x.py -q` in this session (fixture): "), "{result}");
        assert!(result.contains("The command itself did not run again.") && !result.contains('<'), "{result}");
        assert!(fresh("python -m pytest tests/test_x.py -v").unwrap().text.contains("(in the project's environment)"));
        assert!(call(node, "another-session").is_none(), "another session's receipt is not this session's evidence");
        assert!(call(".venv/bin/python -m pytest tests/", "fixture-session").is_none(), "wider scope");
        assert!(
            call(".venv/bin/python -m pytest tests/test_x.py -k one", "fixture-session").is_none(),
            "selection option"
        );
        assert!(call("git status", "fixture-session").is_none(), "not a test run: no receipt");
        // From a subdirectory, a relative target names another file: covered as written, never fresh.
        std::fs::create_dir_all(f.root.join("tests")).unwrap();
        let away = call_from(node, "fixture-session", &f.root.join("tests")).unwrap();
        assert_eq!((away.fresh, away.result.as_deref(), away.text.as_str()), (false, None, ""));
        assert_eq!(away.covering, template);
        // Declared caches do not make a receipt stale; any other repository file does.
        std::fs::create_dir_all(f.root.join(".pytest_cache")).unwrap();
        std::fs::write(f.root.join(".pytest_cache/nodeids"), "[]").unwrap();
        assert!(fresh(node).is_some(), "a declared cache does not make it stale");
        std::fs::remove_dir_all(f.root.join(".pytest_cache")).unwrap();
        std::fs::write(f.root.join("tests/test_x.py"), "def test_one():\n    assert False\n").unwrap();
        let stale = call(node, "fixture-session").unwrap();
        assert_eq!((stale.fresh, stale.result, stale.text.as_str()), (false, None, ""), "a later edit makes it stale");
        assert_eq!(stale.covering, template);
    }

    /// A fresh receipt's result names the tests its run's record holds that the call selects, the
    /// session's own new tests first, when the journal binds the record (its digest is the file's).
    #[test]
    fn a_fresh_receipts_result_names_the_tests_it_ran_the_sessions_own_first() {
        let f = pytest_fixture();
        let record = f.root.join(PYTEST_NODEIDS);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        let ids = ["tests/test_x.py::test_alpha", "tests/test_x.py::test_zeta", "tests/other.py::t", "bad id"];
        std::fs::write(&record, serde_json::to_vec(&ids).unwrap()).unwrap();
        let mut outputs = BTreeMap::new();
        outputs.insert(
            PYTEST_NODEIDS.to_string(),
            crate::stage::FileState::File { sha256: crate::sha256_hex(&std::fs::read(&record).unwrap()), exec: false },
        );
        journal(
            &f,
            &crate::journal::Receipt {
                ts: "2026-10-03T09:00:00Z".into(),
                outputs,
                ..completed("fixture-session", applied(), &current_digest(&f))
            },
        );
        adage_store::state::update::<SessionState, _>(&f.paths.state("fixture-session"), |s| {
            s.new_tests = vec!["test_zeta".into()]
        })
        .unwrap();
        let note = f.receipt(".venv/bin/python -m pytest tests/test_x.py -q").unwrap();
        let result = note.result.unwrap();
        assert!(result.contains(" in this session (2026-10-03T09:00:00Z): it passed"), "{result}");
        assert!(
            result.ends_with(
                "\nTests it ran:\n- tests/test_x.py::test_zeta passed\n- tests/test_x.py::test_alpha passed"
            ),
            "the session's own test first, only the call's targets, only well-formed ids: {result}"
        );
        // The receipt card's data: the same tests, in the same order, and when the run was.
        let card = note.card.expect("a fresh receipt has a card");
        assert_eq!(card.tests, ["tests/test_x.py::test_zeta", "tests/test_x.py::test_alpha"]);
        assert_eq!((card.ran_at.as_str(), card.more, card.operation.as_str()), ("2026-10-03T09:00:00Z", 0, ""));
        assert_eq!(card.summary, "passed on files identical to the current ones · 2 tests this command selects");
        assert!(!card.ago.is_empty(), "the time since the run is said");
    }

    #[test]
    fn failed_or_refused_receipts_never_cover_a_call() {
        let f = pytest_fixture();
        let digest = current_digest(&f);
        for outcome in [
            crate::stage::Outcome::Failed { exit: Some(1), signal: None },
            crate::stage::Outcome::Refused { stage: "verify".into(), reason: "x".into(), counts: true },
        ] {
            journal(&f, &completed("fixture-session", outcome, &digest));
            let covered = f.receipt(".venv/bin/python -m pytest tests/test_x.py -q");
            assert!(covered.is_none());
        }
    }

    /// Review F5: a receipt never covers a call whose command line changes what pytest sees
    /// (another directory, an exported or assigned variable, any command that is not passive), or
    /// a call made from a subdirectory. T-C3-01: a call made outside the root is not fresh.
    #[test]
    fn t_c3_01_a_receipt_never_covers_a_call_from_elsewhere() {
        let root = std::path::Path::new("/repo");
        let done = pytest_call(&[".venv/bin/python", "-m", "pytest", "tests/test_x.py", "-q"]).unwrap();
        let covered = |c: &str| pytest_calls(c, root).is_some_and(|v| v.iter().all(|a| covers(&done, a)));
        for no in [
            "cd other && .venv/bin/python -m pytest tests/test_x.py -q",
            "pushd other && .venv/bin/python -m pytest tests/test_x.py -q",
            "popd && .venv/bin/python -m pytest tests/test_x.py -q",
            "export PYTHONPATH=src && .venv/bin/python -m pytest tests/test_x.py -q",
            "PYTHONPATH=src .venv/bin/python -m pytest tests/test_x.py -q",
            "git stash && .venv/bin/python -m pytest tests/test_x.py -q",
            "make gen && .venv/bin/python -m pytest tests/test_x.py -q",
        ] {
            assert!(!covered(no), "{no}");
        }
        assert!(covered(".venv/bin/python -m pytest tests/test_x.py -q 2>&1 | tail -20 && echo done"));
        let f = pytest_fixture();
        record_receipt(&f, "fixture-session", &current_digest(&f));
        let call = |cwd: &std::path::Path| {
            f.receipt_from(".venv/bin/python -m pytest tests/test_x.py -q", "fixture-session", cwd)
        };
        assert!(call(&f.root).is_some_and(|n| n.fresh), "at the root it is covered");
        assert!(
            call(&f.root.join("tests")).is_some_and(|n| !n.fresh && n.text.is_empty()),
            "from a subdirectory the same words name other files: never fresh, never said"
        );
    }

    /// Review F6: the receipt check's repository hash stops at its deadline instead of holding up
    /// the PreToolUse hook; the note is then skipped.
    #[test]
    fn the_input_snapshot_stops_at_its_deadline() {
        let f = pytest_fixture();
        let req = f.request();
        let action = req.action.as_ref().unwrap();
        let late = crate::stage::input_snapshot_until(&f.root, action, Some(Instant::now())).unwrap_err();
        assert_eq!(late.stage, "time");
        let later = Instant::now() + std::time::Duration::from_secs(60);
        assert_eq!(
            crate::stage::input_snapshot_until(&f.root, action, Some(later)).unwrap(),
            crate::stage::input_snapshot(&f.root, action).unwrap()
        );
    }

    /// T-C3-02, T-C3-03 through the hook pipeline with this extension: a journaled passing run
    /// covers the agent's repeated test run. The command hook's output is today's context; the
    /// mod's dry run carries the receipt with `fresh`, `result` and `covering`; its verdict
    /// `answered: receipt` answers the call, spools one `pretool` record with `answered: receipt`,
    /// and meets the test finish line, so the Stop gate asks for no run; the command hook that
    /// follows for the same id prints nothing.
    #[test]
    fn t_c3_02_03_a_fresh_receipt_answers_the_repeated_test_run_and_meets_the_finish_line() {
        let mut f = pytest_fixture();
        f.index.routes.push(IndexedRoute {
            repo: "fixture".into(),
            track: "fixture/finish".into(),
            version: "0.1.0".into(),
            exec: None,
            route: Route {
                id: "tests-after-edit".into(),
                summary: String::new(),
                trigger: Trigger::AfterEdit { paths: "tests/*.py".into() },
                step: Step::Run { command: ".venv/bin/python -m pytest tests/test_x.py -q".into() },
                slots: vec![],
                effect: Effect::Verify,
                strength: Strength::Fire,
                state: RouteState::Firm,
                exit: vec![],
                authored: false,
                evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
            },
        });
        f.index.write_atomic(&f.paths.index()).unwrap();
        std::fs::write(f.paths.root.join("config.toml"), "[learn]\nauto = false\n").unwrap();
        treated(&f, Arm::Treated);
        let edit = edit(f.root.join("tests/test_x.py"));
        let armed = hook(&f, "PostToolBatch", json!({ "tool_calls": [edit] })).expect("the finish line");
        assert!(armed.to_string().contains("pytest tests/test_x.py -q"), "{armed}");
        // Adage's approved run of the check passed on the files as they are now.
        record_receipt(&f, "fixture-session", &current_digest(&f));
        let test = ".venv/bin/python -m pytest tests/test_x.py::test_one -xvs";
        let pre = |id: &str, extra: Value| {
            let mut p = json!({ "hook_event_name": "PreToolUse", "tool_name": "Bash", "tool_use_id": id,
                                "tool_input": { "command": test } });
            for (k, v) in extra.as_object().unwrap() {
                p[k] = v.clone();
            }
            hook(&f, "PreToolUse", p)
        };
        // The floor: the command hook's payload gets the receipt as context, as before the mod.
        let floor = pre("toolu_a", json!({})).expect("the receipt beside the run");
        let context = floor["hookSpecificOutput"]["additionalContext"].as_str().unwrap();
        assert!(context.starts_with("<adage-receipt operation=") && context.ends_with("</adage-receipt>"), "{floor}");
        assert_eq!(floor.as_object().unwrap().len(), 1, "context only: {floor}");
        // The mod's dry run.
        let plan = pre("toolu_b", json!({ "dry_run": true })).expect("a plan");
        let r = &plan["plan"]["receipt"];
        assert_eq!(
            (r["fresh"].as_bool(), r["covering"].as_str()),
            (Some(true), Some(".venv/bin/python -m pytest tests/test_x.py -q"))
        );
        assert_eq!(r["note"].as_str(), Some(context));
        assert!(r["result"].as_str().unwrap().starts_with("Adage answered this test run"), "{plan}");
        // The plan names the run, fresh or not, for the line the mod draws beside a run that
        // happens anyway: the same id the note carries (absent when the journal row has none).
        let operation = r["operation"].as_str().unwrap_or("").to_string();
        assert!(context.contains(&format!("operation=\"{operation}\"")), "{plan}");
        let before = adage_store::spool::read_all::<Value>(&f.paths.spool("fixture-session")).unwrap().len();
        let out = pre("toolu_b", json!({ "verdict": { "answered": "receipt" } })).expect("answered");
        assert_eq!(out["receipt"]["result"], r["result"]);
        assert_eq!(out["receipt"]["note"].as_str(), Some(context));
        let recs = adage_store::spool::read_all::<Value>(&f.paths.spool("fixture-session")).unwrap();
        assert_eq!(recs.len(), before + 1, "one record");
        let rec = &recs[before];
        assert_eq!((rec["kind"].as_str(), rec["answered"].as_str()), (Some("pretool"), Some("receipt")));
        assert_eq!(rec["decision"]["judgements"], json!([{ "kind": "obligation_met", "route": "tests-after-edit" }]));
        assert!(pre("toolu_b", json!({})).is_none(), "the command hook's repeat of the id");
        let stop = hook(&f, "Stop", json!({ "stop_hook_active": false }));
        assert!(stop.is_none(), "the finish line is met: {stop:?}");
        // A later edit makes the receipt stale: the plan says so, and the call is not answered.
        std::fs::write(f.root.join("tests/test_x.py"), "def test_one():\n    assert True\n").unwrap();
        let plan = pre("toolu_c", json!({ "dry_run": true })).expect("a plan");
        let mut stale = json!({ "fresh": false, "covering": ".venv/bin/python -m pytest tests/test_x.py -q" });
        if !operation.is_empty() {
            stale["operation"] = json!(operation);
        }
        assert_eq!(plan["plan"]["receipt"], stale);
        assert!(pre("toolu_c", json!({ "verdict": { "answered": "receipt" } })).is_none());
        assert!(pre("toolu_d", json!({})).is_none(), "and the command hook says nothing of a stale receipt");
    }
}
