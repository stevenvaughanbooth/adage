//! Which of this repository's grants hold here, as the hook asks: the session contract (and a
//! grant that stopped matching, for the person), the exec-only reflexes it may direct, the
//! learned verifications an E2 grant covers, and the reflexes a `TurnStep` may predict.

use adage_core::CommandTable;
use adage_hook::SessionTexts;
use adage_runtime::render;
use adage_tracks::index::IndexedRoute;
use adage_tracks::{Step, Trigger};

use super::label;
use crate::approvals::{self, Approval, Store};
use crate::request::Level;
use crate::{policy, route};

/// The session contract and the lapse notice ([`adage_hook::Extension::session_texts`]).
pub(super) fn session_texts(
    repo_key: &str,
    root: &std::path::Path,
    index: &adage_tracks::Index,
    paths: &adage_store::Paths,
) -> SessionTexts {
    let (Ok(pol), Ok(store)) = (policy::load(&paths.root), Store::load(&approvals::path(&paths.root))) else {
        return SessionTexts::default();
    };
    let planned = route::plan_grants(repo_key, root, index, &paths.tracks(), &store, CommandTable::load());
    // A grant that could run under this policy but no longer matches: said, never silent.
    let lapsed: Vec<route::Lapse> =
        route::lapses(&planned, &store).into_iter().filter(|l| l.level <= pol.max_level).collect();
    let notice = lapsed.first().map(|l| match lapsed.len() {
        1 => l.notice(),
        n => format!("{} ({} more no longer match.)", l.notice(), n - 1),
    });
    if pol.max_level < Level::E2 {
        return SessionTexts { context: None, notice };
    }
    // Only a grant whose key still equals the route's current one, derived as act() derives
    // it: a lapsed grant would be refused, so it promises no receipt. A short factual
    // description of what Adage runs, when, and where results appear (not instructions).
    let lines: Vec<String> = current_verifications(&planned, &store)
        .into_iter()
        .take(2)
        .map(|(ir, command, _)| {
            let pattern = edit_pattern(ir);
            let when = if pattern.is_empty() { "when your edits match its learned pattern".to_string() } else { format!("after edits to {pattern}") };
            format!("In this repository Adage runs `{}` (approved by a person, sandboxed) {when}, and again when you finish if files changed since.", render::sanitise(&command))
        })
        .collect();
    let context = (!lines.is_empty()).then(|| execution_contract(&lines));
    SessionTexts { context, notice }
}

/// The session contract's reflex block around its lines (one per verification Adage runs).
fn execution_contract(lines: &[String]) -> String {
    format!("<adage-reflex track=\"adage/execution@1\" route=\"session-contract\" strength=\"prime\" kind=\"execution-contract\">\n{}\nIts results appear with your tool results, with the tests it ran; a failure comes back to you with the failing tests.\n</adage-reflex>", lines.join("\n"))
}

/// Exec-only reflexes approved here for execution at any level
/// ([`adage_hook::Extension::exec_only`]).
pub(super) fn exec_only(
    repo_key: &str,
    index: &adage_tracks::Index,
    paths: &adage_store::Paths,
) -> Vec<(String, String)> {
    let Ok(store) = Store::load(&approvals::path(&paths.root)) else { return Vec::new() };
    index
        .for_repo(repo_key)
        .filter(|r| r.exec_only())
        .filter(|r| {
            let label = label(&r.track, &r.route.id);
            store.approvals.iter().any(|a| a.key.repo == repo_key && a.route == label && a.key.mode.is_execute())
        })
        .map(|r| (r.track.clone(), r.route.id.clone()))
        .collect()
}

/// The marker `adage delegate predict off` writes: nothing is predicted on this machine, whatever
/// is granted. Read at every `TurnStep`, so it takes effect at the next step of a running session.
pub fn predict_off(paths: &adage_store::Paths) -> std::path::PathBuf {
    paths.root.join("exec").join("predict-off")
}

/// The `(track, route)` pairs a `TurnStep` may predict in the checkout at `root` (plan §2.2 item
/// 3): each `predict` grant for `repo_key` whose route is in the index, whose key equals the one
/// planned now ([`route::plan_predict`]: a changed command, contract or script lapses it), that
/// policy permits, and whose route holds no execute grant (adage-exec already acts on that route;
/// two paths would make the same call). Nothing while `exec/predict-off` exists.
pub fn predict_grants(
    repo_key: &str,
    root: &std::path::Path,
    index: &adage_tracks::Index,
    paths: &adage_store::Paths,
) -> Vec<(String, String)> {
    if predict_off(paths).exists() {
        return Vec::new();
    }
    let Ok(store) = Store::load(&approvals::path(&paths.root)) else { return Vec::new() };
    let mine = |a: &&Approval| a.key.repo == repo_key && a.key.mode == approvals::GrantMode::Predict;
    if !store.approvals.iter().any(|a| mine(&a)) {
        return Vec::new();
    }
    let Ok(pol) = policy::load(&paths.root) else { return Vec::new() };
    let table = CommandTable::load();
    let mut out: Vec<(String, String)> = Vec::new();
    for a in store.approvals.iter().filter(mine) {
        let executed =
            store.approvals.iter().any(|x| x.key.repo == repo_key && x.route == a.route && x.key.mode.is_execute());
        let Some((track, id)) = a.route.rsplit_once('/') else { continue };
        let Some(ir) = route::find(index, repo_key, track, id) else { continue };
        if executed || out.iter().any(|(t, r)| t == &ir.track && r == &ir.route.id) {
            continue;
        }
        let Ok(plan) = route::plan_predict(ir, root, &paths.tracks()) else { continue };
        let req = plan.request;
        if store.find(&approvals::ApprovalKey::for_request(&req, repo_key)).is_none() {
            continue;
        }
        if pol.permits(&req, route::predict_classified(&req, table)).is_err() {
            continue;
        }
        out.push((ir.track.clone(), ir.route.id.clone()));
    }
    out
}

/// The edit globs of a context trigger, in order ("tests/*.py then CHANGELOG.rst").
fn edit_pattern(ir: &IndexedRoute) -> String {
    let Trigger::Context { items, .. } = &ir.route.trigger else { return String::new() };
    let globs: Vec<String> = items
        .iter()
        .filter_map(|i| match i {
            adage_tracks::CtxItem::Edit { glob } => Some(render::sanitise(glob)),
            _ => None,
        })
        .collect();
    globs.join(" then ")
}

/// This repository's learned context verifications with a current E2 grant: the route, its fixed
/// command, and the grant (most supported first).
pub(super) fn verifications<'a>(
    repo_key: &'a str,
    root: &std::path::Path,
    index: &'a adage_tracks::Index,
    paths: &adage_store::Paths,
    store: &'a Store,
) -> Vec<(&'a IndexedRoute, String, &'a Approval)> {
    current_verifications(
        &route::plan_grants(repo_key, root, index, &paths.tracks(), store, CommandTable::load()),
        store,
    )
}

/// [`verifications`], from grants already planned.
fn current_verifications<'a>(
    planned: &[route::Planned<'a>],
    store: &Store,
) -> Vec<(&'a IndexedRoute, String, &'a Approval)> {
    let mut out: Vec<(&IndexedRoute, String, &Approval)> = Vec::new();
    for p in planned {
        let (r, a) = (p.route, p.approval);
        let Step::Run { command } = &r.route.step else { continue };
        if r.route.effect != adage_core::Effect::Verify
            || command.contains('{')
            || !a.key.mode.is_execute()
            || a.key.level != Level::E2
            || a.key.effect != adage_core::Effect::Verify
            || !p.current(store)
            || out.iter().any(|(x, _, _)| std::ptr::eq(*x, r))
        {
            continue;
        }
        out.push((r, command.clone(), a));
    }
    out.sort_by_key(|(r, _, _)| std::cmp::Reverse(r.route.evidence.support));
    out
}

/// The person's line for a grant `a` whose key no longer matches (`changed`: its fields, or why).
fn lapse_notice(a: &Approval, changed: Vec<String>) -> String {
    route::Lapse {
        route: a.route.clone(),
        level: a.key.level,
        mode: a.key.mode,
        changed,
        reapprove: route::reapprove_command(a),
    }
    .notice()
}

/// [`lapse_notice`] for a grant whose key differs from the one `key` planned now: the fields
/// that changed.
pub(super) fn key_lapse_notice(a: &Approval, key: &approvals::ApprovalKey) -> String {
    lapse_notice(a, a.key.diff(key).into_iter().map(|c| c.field.to_string()).collect())
}

/// [`lapse_notice`] for a grant whose route cannot be planned now (`why`).
pub(super) fn unplanned_notice(a: &Approval, why: &str) -> String {
    lapse_notice(a, vec![format!("it cannot be planned now: {}", crate::reason_text(why))])
}

#[cfg(test)]
mod tests {
    use super::super::testkit::*;
    use super::super::PREFETCH;
    use super::*;
    use adage_hook::Extension;
    use adage_runtime::Arm;
    use serde_json::{json, Value};

    /// Where the extension is asked: the fixture's checkout, index and home, under `repo_key`.
    fn site<'a>(f: &'a Fixture, repo_key: &'a str) -> adage_hook::Site<'a> {
        adage_hook::Site { repo_key, root: &f.root, index: &f.index, paths: &f.paths }
    }

    #[test]
    fn session_contract_requires_scoped_e2_verification_and_policy() {
        let f = Fixture::new();
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_none());
        f.approve();
        let text = PREFETCH.session_context(&site(&f, "fixture")).unwrap();
        assert!(text.contains("<adage-reflex") && text.contains("session-contract"));
        assert!(
            text.contains("Adage runs `sh tools/check.sh` (approved by a person, sandboxed) after edits to src/*.rs"),
            "{text}"
        );
        assert!(text.contains("again when you finish") && text.contains("a failure comes back to you"), "{text}");
        assert!(!text.contains("Use a current receipt") && text.len() < 500, "factual and short: {text}");
        assert!(PREFETCH.session_context(&site(&f, "other-repository")).is_none());
        std::fs::write(f.paths.root.join("policy.toml"), "[exec]\nmax_level = \"E1\"\n").unwrap();
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_none());
    }

    #[test]
    fn session_contract_ignores_lapsed_and_revoked_verification() {
        let f = Fixture::new();
        f.approve();
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_some());
        let store_path = approvals::path(&f.paths.root);
        let script = f.root.join("tools/check.sh");
        let original = std::fs::read(&script).unwrap();
        std::fs::write(&script, "#!/bin/sh\ntest -f src/other.rs\n").unwrap();
        let store = Store::load(&store_path).unwrap();
        let req = f.request();
        let key = approvals::ApprovalKey::for_request(&req, "fixture");
        assert!(store.find(&key).is_none());
        assert_eq!(store.lapsed(&key, &store.approvals[0].route)[0].1[0].field, "route_exec_digest");
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_none(), "lapsed grant");
        std::fs::write(&script, original).unwrap();
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_some(), "matches again");
        let mut store = Store::load(&store_path).unwrap();
        assert_eq!(store.revoke("fixture", Some(CHECK), None), 1);
        store.save(&store_path).unwrap();
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_none(), "revoked grant");
        f.approve();
        let mut store = Store::load(&store_path).unwrap();
        assert!(store.revoke_automatically("fixture", CHECK, Level::E2, approvals::GrantMode::Execute, "refused"));
        store.save(&store_path).unwrap();
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_none(), "auto-revoked grant");
    }

    /// dev-v8b, end to end: bytecode an ordinary Python run writes into the runtime's
    /// `__pycache__` (or the environment's) leaves the grant, the session contract and the receipt
    /// promise in force; a bare injected `.pyc` is code, and lapses the grant, which the person is
    /// told about: which field changed and the command that approves it again.
    #[cfg(unix)]
    #[test]
    fn pycache_writes_keep_the_grant_and_a_real_change_is_told_to_the_person() {
        let f = pytest_fixture();
        let top = f.root.parent().unwrap().to_path_buf();
        assert!(
            Store::load(&approvals::path(&f.paths.root)).unwrap().approvals[0].terms.is_some(),
            "approved with its terms"
        );
        let texts = || PREFETCH.session_texts(&site(&f, "fixture"));
        let before = texts();
        assert!(before.context.is_some() && before.notice.is_none(), "{before:?}");
        for cache in [
            "runtime/lib/python3.12/__pycache__/os.cpython-312.pyc",
            "runtime/lib/python3.12/encodings/__pycache__/__init__.cpython-312.pyc",
            "fixture/.venv/lib/python3.12/site-packages/__pycache__/x.cpython-312.pyc",
        ] {
            let file = top.join(cache);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, b"bytecode from an ordinary run").unwrap();
        }
        assert_eq!(texts(), before, "bytecode caches change nothing");
        std::fs::write(top.join("runtime/lib/python3.12/injected.pyc"), b"unapproved bytecode").unwrap();
        let after = texts();
        assert!(after.context.is_none(), "a lapsed grant promises nothing to the agent");
        let notice = after.notice.expect("the person is told");
        assert!(notice.contains("E2 approval for fixture/context/predict-check no longer matches"), "{notice}");
        assert!(notice.contains("(route_exec_digest)"), "{notice}");
        assert!(
            notice.contains("adage review delegate fixture/context predict-check --level e2 --effect verify --cache '.pytest_cache/**'"),
            "{notice}"
        );
    }

    /// T-C2-03 (the hook half): `predict_grants` names a granted reflex only while its key matches
    /// what the call would run now, policy permits it, the machine switch is on, the step is asked
    /// for this repository, and no execute grant already acts on the route.
    #[test]
    fn t_c2_03_predict_grants_name_only_current_permitted_switched_on_grants() {
        let mut f = Fixture::new();
        with_obligation(&mut f);
        assert!(predict_grants("fixture", &f.root, &f.index, &f.paths).is_empty(), "nothing granted");
        grant_predict(&f, 1);
        let want = vec![("fixture/finish".to_string(), "regenerate-registry".to_string())];
        assert_eq!(predict_grants("fixture", &f.root, &f.index, &f.paths), want);
        assert_eq!(PREFETCH.predict_grants(&site(&f, "fixture")), want, "through the extension");
        assert!(PREFETCH.predict_grants(&site(&f, "other")).is_empty(), "another repository's key");
        // The machine switch.
        std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
        std::fs::write(predict_off(&f.paths), "x").unwrap();
        assert!(PREFETCH.predict_grants(&site(&f, "fixture")).is_empty());
        assert!(predict_grants("fixture", &f.root, &f.index, &f.paths).is_empty());
        std::fs::remove_file(predict_off(&f.paths)).unwrap();
        assert_eq!(PREFETCH.predict_grants(&site(&f, "fixture")), want);
        // Policy.
        let policy = f.paths.root.join("policy.toml");
        std::fs::write(&policy, "[exec]\nmax_level = \"E1\"\n").unwrap();
        assert!(predict_grants("fixture", &f.root, &f.index, &f.paths).is_empty(), "E1 caps it");
        std::fs::write(&policy, "[exec]\nmax_level = \"E2\"\n[effects]\ndeny = [\"regenerate\"]\n").unwrap();
        assert!(predict_grants("fixture", &f.root, &f.index, &f.paths).is_empty(), "a denied effect");
        std::fs::write(&policy, "[exec]\nmax_level = \"E2\"\n").unwrap();
        // A changed command lapses the grant.
        let mut changed = f.index.clone();
        changed.routes[1].route.step = Step::Run { command: "make generate".into() };
        assert!(predict_grants("fixture", &f.root, &changed, &f.paths).is_empty(), "lapsed");
        let texts = PREFETCH.session_texts(&adage_hook::Site {
            repo_key: "fixture",
            root: &f.root,
            index: &changed,
            paths: &f.paths,
        });
        let notice = texts.notice.expect("a lapsed predict grant is told to the person");
        assert!(notice.contains("the predict approval for fixture/finish/regenerate-registry"), "{notice}");
        assert!(notice.contains("`adage review delegate fixture/finish regenerate-registry --predict`"), "{notice}");
        // A route adage-exec already acts on is never predicted too.
        grant_finish_line(&f);
        assert!(predict_grants("fixture", &f.root, &f.index, &f.paths).is_empty(), "two paths would make the call");
    }

    /// T-C1-10 under adage-exec: the hook pipeline with this extension and a person's predict
    /// grant on the finish line prints the `{"predict": …}` line at the step that ends the turn,
    /// and runs nothing itself; with the machine switch off it prints nothing. T-C2-08 (the
    /// hook half): `exec/predict-off` stops `adage-exec hook claude TurnStep` from predicting.
    #[test]
    fn t_c1_10_t_c2_08_turn_step_under_adage_exec_predicts_the_granted_finish_line() {
        let mut f = Fixture::new();
        with_obligation(&mut f);
        treated(&f, Arm::Treated);
        grant_predict(&f, 1);
        let step = |index: u32, ending: bool| {
            hook(
                &f,
                "TurnStep",
                json!({ "hook_event_name": "TurnStep", "turn_id": "t1", "index": index, "ending": ending }),
            )
        };
        assert!(step(1, true).is_none(), "nothing directed yet");
        let edit = edit(f.root.join("registry/currencies.csv"));
        let armed = hook(&f, "PostToolBatch", json!({ "tool_calls": [edit] })).expect("the obligation's directive");
        assert!(armed.to_string().contains("make gen"), "{armed}");
        std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
        std::fs::write(predict_off(&f.paths), "x").unwrap();
        assert!(step(2, true).is_none(), "predictions are off on this machine");
        std::fs::remove_file(predict_off(&f.paths)).unwrap();
        let out = step(2, true).expect("the finish line, in the agent's place");
        assert_eq!(
            out,
            json!({ "predict": { "kind": "obligation", "route": "regenerate-registry", "track": "fixture/finish",
                                  "calls": [{ "tool": "Bash", "input": { "command": "make gen" } }] } })
        );
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty(), "adage-exec ran nothing");
        let spooled = adage_store::spool::read_all::<Value>(&f.paths.spool("fixture-session")).unwrap();
        assert!(spooled.iter().all(|r| r.get("exec").is_none()), "no execution was attempted: {spooled:?}");
    }

    /// A predict grant alone never runs anything in the hook: not the learned action, not a
    /// prefetch, not the finish-line gate, and it does not bring an exec-only reflex into play.
    #[test]
    fn a_predict_grant_alone_runs_nothing() {
        let f = Fixture::new();
        let key = predict_key_by_hand(&f);
        seed(&f, CHECK, key, None);
        let (event, dirs) = f.selected("src/feature.rs");
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(d.spool.is_none() && d.met.is_empty(), "nothing attempted");
        assert!(d.texts.unwrap().iter().all(String::is_empty), "as with no grant at all");
        assert!(PREFETCH.exec_only(&site(&f, "fixture")).is_empty());
        assert!(PREFETCH.session_context(&site(&f, "fixture")).is_none());
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "stale");
        assert!(stop(&f, false).is_none());
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1, "only the seeded receipt");
        assert!(predict_grants("fixture", &f.root, &f.index, &f.paths).is_empty(), "an exec-only reflex");
    }

    /// T-C2-07: a route holding both grants keeps every execute path: the E2 action at the batch
    /// runs under the execute key, the session contract and the receipt still find it, and
    /// neither grant reads as lapsed. The predict grant is listed first, so a lookup that ignored
    /// the mode would pick it.
    #[test]
    fn t_c2_07_both_grants_on_one_route_keep_the_execute_paths() {
        let mut f = Fixture::new();
        with_obligation(&mut f);
        std::fs::write(
            f.root.join("Makefile"),
            "gen:\n\tmkdir -p gen && cp registry/currencies.csv gen/currencies.txt\n",
        )
        .unwrap();
        grant_finish_line(&f);
        grant_predict(&f, 1);
        let store = Store::load(&approvals::path(&f.paths.root)).unwrap();
        assert_eq!(store.approvals.len(), 2);
        assert_eq!(store.approvals[0].key.mode, approvals::GrantMode::Predict);
        let texts = PREFETCH.session_texts(&site(&f, "fixture"));
        assert!(texts.notice.is_none(), "neither grant lapsed: {:?}", texts.notice);
        if staging() {
            treated(&f, Arm::Treated);
            let edit = edit(f.root.join("registry/currencies.csv"));
            let out = hook(&f, "PostToolBatch", json!({ "tool_calls": [edit] })).expect("the action's result");
            assert!(out.to_string().contains("Adage ran `make gen`"), "{out}");
            assert_eq!(std::fs::read_to_string(f.root.join("gen/currencies.txt")).unwrap(), "EUR\n");
            assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1);
        }
        // The receipt and the session contract, on the learned check with both grants.
        let g = pytest_fixture();
        seed(&g, CHECK, predict_key_by_hand(&g), None);
        assert!(PREFETCH.session_context(&site(&g, "fixture")).is_some(), "the contract");
        let digest = current_digest(&g);
        record_receipt(&g, "fixture-session", &digest);
        let covered = g.receipt(".venv/bin/python -m pytest tests/test_x.py -q");
        assert!(covered.is_some(), "covering_receipt finds the execute grant");
    }
}
