//! `adage-exec hook <harness> <event>`: E1 prefetch and E2 actions (Track C §3.2, §3.4, §3.6).
//!
//! The same pipeline as `adage hook` (`adage-hook`: adapters, fast path, state lock, `decide()`,
//! spool, render), with one [`Extension`]. For each directive about to be delivered whose route
//! has an E1 approval for this repository, it derives the route's request ([`crate::route`]),
//! reads the bound values back from the directive, and runs it through [`crate::run::execute`]
//! (every gate: approval key equality, including the L-ref hash read again now; policy; a live
//! sandbox; limits; typed projection). Each block is re-checked by the safety oracle
//! (`adage_safety::check_data_block`), the directive adapts ("That lookup has been run; its
//! result is below."), and the block is placed beside it.
//!
//! E1 failures retain ordinary advice. E2 failures before application can return advice with a
//! reason; uncertain application, journal corruption and extension panics instead deliver a
//! recovery notice. Completed actions whose detail exceeds the harness cap deliver a bounded
//! completion notice, never an instruction to repeat the action. With no grants or pending
//! recovery, the output equals `adage hook`'s byte for byte.
//!
//! The spool record gains `exec`: per attempted route, `{ route, level, exit, bytes, sha256,
//! truncated, duration_ms, auto }` when delivered, `{ route, level, withheld }` when not. The
//! output itself is never recorded.
//!
//! **E2 actions.** When an edit arms a finish-line obligation (after editing `registry/*.csv`, run
//! `make gen`), or the obligation's Stop request is due, and the reflex holds an explicit E2
//! approval for this repository, the action runs here ([`act`]). A runtime-selected learned
//! context with execution evidence can also nominate its fixed command, without creating an
//! obligation. Both paths run at most one action per event, never in
//! the control arm, never under OpenCode (whose bridge waits one second), within 20 s. It is
//! staged, its change set verified against the declared outputs, and applied only if nothing it
//! touches changed meanwhile ([`crate::stage`]). Applied: the directive becomes a result block
//! ("Adage ran `make gen` after your edit to registry/currencies.csv: updated 1 file (…). Nothing
//! else changed."), the obligation is met, and no Stop request follows. Refused or failed: the
//! plain E0 directive and the reason. The outcome goes to `$ADAGE_HOME/exec/actions.spool` and to
//! this event's `exec` record; three refused change sets revoke the approval.

use adage_core::CommandTable;
use adage_hook::{Delivery, Extension, PreCall, SessionTexts, StopCall, StopGate, Turn};
use adage_runtime::{render, Directive};
use adage_safety::{check_data_block, DataType};
use adage_tracks::{Origin, Step, Trigger};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

use crate::approvals::{self, Approval, Store};
use crate::request::{FieldSpec, FieldType, Level, OutputMode, DEFAULT_LIST, DEFAULT_TEXT, MAX_LIST, MAX_TEXT};
use crate::run::{self, Context};
use crate::{limits, policy, route, sandbox};

/// The E1 prefetch extension.
pub struct Prefetch;

/// What `adage-exec hook` passes to the pipeline.
pub static PREFETCH: Prefetch = Prefetch;

/// The whole `hook` subcommand. Live learning starts the `adage` binary beside this one.
pub fn main(harness: String, event: String) -> ! {
    adage_hook::main(harness, event, adage_hook::Options { learner: Some(adage_beside), extension: Some(&PREFETCH) })
}

/// The `adage` binary installed beside `adage-exec` (live learning is `adage condition --auto`).
pub fn adage_beside() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "adage.exe" } else { "adage" };
    let p = exe.parent()?.join(name);
    p.is_file().then_some(p)
}

impl Extension for Prefetch {
    fn session_context(
        &self,
        repo_key: &str,
        root: &std::path::Path,
        index: &adage_tracks::Index,
        paths: &adage_store::Paths,
    ) -> Option<String> {
        self.session_texts(repo_key, root, index, paths).context
    }

    /// The contract and, for the person, a grant that stopped matching: one planning pass over
    /// this repository's grants serves both.
    fn session_texts(
        &self,
        repo_key: &str,
        root: &std::path::Path,
        index: &adage_tracks::Index,
        paths: &adage_store::Paths,
    ) -> SessionTexts {
        let (Ok(pol), Ok(store)) = (policy::load(&paths.root), Store::load(&approvals::path(&paths.root))) else {
            return SessionTexts::default();
        };
        let planned = route::plan_grants(repo_key, root, index, &paths.tracks(), &store, &CommandTable::load());
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
        let context = (!lines.is_empty()).then(|| format!("<adage-reflex track=\"adage/execution@1\" route=\"session-contract\" strength=\"prime\" kind=\"execution-contract\">\n{}\nIts results appear with your tool results, with the tests it ran; a failure comes back to you with the failing tests.\n</adage-reflex>", lines.join("\n")));
        SessionTexts { context, notice }
    }

    fn stop_gate(&self, call: &StopCall<'_>) -> Option<StopGate> {
        stop_gate(call)
    }

    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery {
        let mut d = Delivery::default();
        let mut records: Vec<Value> = Vec::new();
        if !directives.is_empty() {
            match prefetch(turn, directives, &mut d.notice) {
                Ok(None) => {}
                Ok(Some((texts, r))) => {
                    d.texts = Some(texts);
                    records.extend(r);
                }
                Err(r) => records.extend(r),
            }
        }
        let candidates = candidates(turn, directives);
        if !candidates.is_empty() {
            d.cpu_predictions =
                candidates.iter().filter(|c| c.context.is_some()).map(|c| c.route.to_string()).collect();
            // A prediction is execution-only. A missing or stale grant must not fall back to
            // asking the agent to perform it. Keep positions aligned for result replacement.
            if candidates.iter().any(|c| c.context.is_some()) {
                let texts = d.texts.get_or_insert_with(|| directives.iter().map(render::wrap_directive).collect());
                for (i, dir) in directives.iter().enumerate() {
                    if candidates
                        .iter()
                        .any(|c| c.context.is_some() && c.route == dir.route && c.track == Some(dir.track.as_str()))
                    {
                        texts[i].clear();
                    }
                }
            }
            let a = act(turn, directives, &candidates);
            records.extend(a.records);
            if !a.replace.is_empty() {
                let mut texts =
                    d.texts.take().unwrap_or_else(|| directives.iter().map(render::wrap_directive).collect());
                for (i, dir) in directives.iter().enumerate() {
                    if let Some(t) = a.replace.get(&dir.route) {
                        texts[i] = t.clone();
                    }
                }
                d.texts = Some(texts);
            }
            d.extra = a.extra;
            d.met = a.met;
            d.stop = a.stop;
            d.recovery = a.recovery;
            d.at_stop_action = a.at_stop_action;
            d.notice = d.notice.take().or(a.notice);
        }
        if !records.is_empty() {
            d.spool = Some(Value::Array(records));
        }
        d
    }

    fn pretool_context(&self, call: &PreCall<'_>) -> Option<String> {
        covering_receipt(call)
    }

    /// Exec-only reflexes (vendored scripts) approved here at E1 or E2: only then are they
    /// directed, and only because this binary carries them out.
    fn exec_only(
        &self,
        repo_key: &str,
        index: &adage_tracks::Index,
        paths: &adage_store::Paths,
    ) -> Vec<(String, String)> {
        let Ok(store) = Store::load(&approvals::path(&paths.root)) else { return Vec::new() };
        index
            .for_repo(repo_key)
            .filter(|r| r.exec_only())
            .filter(|r| {
                let label = format!("{}/{}", r.track, r.route.id);
                store.approvals.iter().any(|a| a.key.repo == repo_key && a.route == label)
            })
            .map(|r| (r.track.clone(), r.route.id.clone()))
            .collect()
    }
}

/// pytest's own record of collected node ids, a cache file the approval declares.
const PYTEST_NODEIDS: &str = ".pytest_cache/v/cache/nodeids";
/// pytest's record of the node ids that failed in its last run (also a declared cache file).
const PYTEST_LASTFAILED: &str = ".pytest_cache/v/cache/lastfailed";

/// The largest pytest cache record read.
const MAX_RECORD_BYTES: u64 = 4 << 20;

/// A pytest cache record's keys (node ids, or file ids for collection errors), or `None` when it
/// is not a JSON object within [`MAX_RECORD_BYTES`].
fn record_keys(bytes: &[u8]) -> Option<std::collections::BTreeSet<String>> {
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return None;
    }
    let map: serde_json::Map<String, Value> = serde_json::from_slice(bytes).ok()?;
    Some(map.into_iter().map(|(k, _)| k).collect())
}

/// What a failed staged run's declared pytest failure record says about *this* run
/// ([`render::FailureRecord`]), read from the staged change set and the worktree, never from
/// output; `None` when the command is not a pytest run whose approval declares the record.
///
/// pytest's `lastfailed` is cumulative: an entry stays until that test passes or is skipped, and
/// pytest rewrites the file only when its content changes. So the run's record counts only when
/// this run rewrote it (it is in the plan, or [`crate::stage::Staged::rewritten`]); then, within
/// the command's targets, entries it added are this run's failures (file ids, without `::`, are
/// collection errors), and entries it kept from before are failures it did not clear. Anything
/// else gives no names: never a stale list.
///
/// The listed failing tests are also described beyond their names ([`failing_cases`]): from their
/// own source and from `new_tests` (the runtime's names of test functions this session added).
fn failure_record(staged: &crate::stage::Staged, command: &str, new_tests: &[String]) -> Option<render::FailureRecord> {
    let call = pytest_call(&command.split_whitespace().collect::<Vec<_>>())?;
    if !crate::stage::matches_any(&staged.action.cache, PYTEST_LASTFAILED) {
        return None;
    }
    let unavailable = Some(render::FailureRecord::default());
    // The worktree's record is the one the run started from: a failed run applies nothing.
    let worktree = staged.repo.join(PYTEST_LASTFAILED);
    let read_worktree = || -> Option<Vec<u8>> {
        match std::fs::symlink_metadata(&worktree) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(b"{}".to_vec()),
            Ok(m) if m.is_file() && m.len() <= MAX_RECORD_BYTES => std::fs::read(&worktree).ok(),
            _ => None,
        }
    };
    let after = match staged.plan.apply.iter().find(|c| c.path == PYTEST_LASTFAILED) {
        Some(crate::stage::Change { after: crate::stage::After::File { bytes, .. }, .. }) => bytes.clone(),
        Some(_) => return unavailable,
        // Rewritten with the same bytes: refreshed, and identical to the worktree's.
        None if staged.rewritten.iter().any(|p| p == PYTEST_LASTFAILED) => match read_worktree() {
            Some(b) => b,
            None => return unavailable,
        },
        None => return unavailable,
    };
    let (Some(after), Some(before)) = (record_keys(&after), read_worktree().as_deref().and_then(record_keys)) else {
        return unavailable;
    };
    let scoped = |ids: &std::collections::BTreeSet<String>| -> Vec<String> {
        ids.iter()
            .filter(|id| (valid_nodeid(id) || valid_file_id(id)) && in_targets(id, &call.targets))
            .cloned()
            .collect()
    };
    let (after, before): (Vec<String>, std::collections::BTreeSet<String>) =
        (scoped(&after), scoped(&before).into_iter().collect());
    // A file that failed to collect ran none of its tests: its node ids say nothing about this run.
    let broken: Vec<&String> = after.iter().filter(|id| !id.contains("::")).collect();
    let ran = |id: &&String| !broken.iter().any(|f| id.starts_with(&format!("{f}::")));
    let mut failing = Vec::new();
    let mut collection = Vec::new();
    let mut uncleared = Vec::new();
    for id in after.iter().filter(ran) {
        match (before.contains(id), id.contains("::")) {
            (false, true) => failing.push(id.clone()),
            (false, false) => collection.push(id.clone()),
            (true, _) => uncleared.push(id.clone()),
        }
    }
    // One bound for all three lists, in the order they are read.
    let mut left = MAX_LISTED_TESTS;
    let mut listed = |mut ids: Vec<String>| {
        let n = ids.len().min(left);
        left -= n;
        let more = ids.len() - n;
        ids.truncate(n);
        render::Listed { names: ids, more }
    };
    let (failing, collection, uncleared) = (listed(failing), listed(collection), listed(uncleared));
    let cases = failing_cases(staged, failing.names.iter().chain(&uncleared.names), new_tests);
    Some(render::FailureRecord { refreshed: true, failing, collection, uncleared, cases })
}

/// The listed failing tests as a failed run's receipt describes them beyond their names
/// ([`render::FailingCase`]), never from the run's output: what each test's own source says
/// ([`adage_runtime::pytest_case::facts`]), read from the file the run used ([`run_source`]), and
/// whether this session's edits added its function (`new_tests`, by name, as a passing receipt
/// names them).
fn failing_cases<'a>(
    staged: &crate::stage::Staged,
    ids: impl Iterator<Item = &'a String>,
    new_tests: &[String],
) -> Vec<render::FailingCase> {
    let ids: Vec<&String> = ids.collect();
    if ids.is_empty() {
        return Vec::new();
    }
    let ignore = adage_core::gitignore::Ignore::new(&staged.repo);
    let base = staged.repo.canonicalize().ok();
    // Each test file is read and parsed once.
    let mut texts: BTreeMap<&str, Option<String>> = BTreeMap::new();
    for file in ids.iter().filter_map(|id| id.split_once("::").map(|(f, _)| f)) {
        texts.entry(file).or_insert_with(|| base.as_deref().and_then(|b| run_source(staged, b, file, &ignore)));
    }
    let sources: BTreeMap<&str, adage_runtime::pytest_case::TestSource> = texts
        .iter()
        .filter_map(|(f, t)| Some((*f, adage_runtime::pytest_case::TestSource::new(t.as_deref()?)?)))
        .collect();
    ids.into_iter()
        .filter(|id| id.contains("::"))
        .map(|id| render::FailingCase {
            id: id.clone(),
            facts: id.split_once("::").and_then(|(f, _)| sources.get(f)).and_then(|s| s.facts(id)),
            added: new_tests.iter().any(|n| n == test_function(id)),
        })
        .collect()
}

/// A test file's text as the run read it: through the orientation card's read gate
/// ([`adage_runtime::orient::readable`]: no dotfile, ignored, secret-holding, oversized or linked
/// file), and only when its bytes are the ones in the run's input snapshot.
fn run_source(
    staged: &crate::stage::Staged,
    base: &std::path::Path,
    rel: &str,
    ignore: &adage_core::gitignore::Ignore,
) -> Option<String> {
    let text = adage_runtime::orient::readable(base, rel, ignore)?;
    let (sha, _) = staged.inputs.get(rel)?.split_once(':')?;
    (crate::sha256_hex(text.as_bytes()) == sha).then_some(text)
}

/// A pytest file id as its failure record names a module that failed to collect.
fn valid_file_id(id: &str) -> bool {
    id.len() <= 200
        && !id.contains("::")
        && id.ends_with(".py")
        && !id.starts_with('/')
        && !id.split('/').any(|p| p == ".." || p.is_empty())
        && id.chars().all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
}

/// Whether a node or file id lies inside a pytest call's targets (every id, for a call that names
/// none: it collects the whole configured suite).
fn in_targets(id: &str, targets: &[String]) -> bool {
    let file = id.split_once("::").map_or(id, |(f, _)| f);
    targets.is_empty()
        || targets.iter().any(|t| file == t || file.starts_with(&format!("{t}/")) || id.starts_with(&format!("{t}::")))
}

/// The edit globs of a context trigger, in order ("tests/*.py then CHANGELOG.rst").
fn edit_pattern(ir: &adage_tracks::index::IndexedRoute) -> String {
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
fn verifications<'a>(
    repo_key: &'a str,
    root: &std::path::Path,
    index: &'a adage_tracks::Index,
    paths: &adage_store::Paths,
    store: &'a Store,
) -> Vec<(&'a adage_tracks::index::IndexedRoute, String, &'a Approval)> {
    current_verifications(
        &route::plan_grants(repo_key, root, index, &paths.tracks(), store, &CommandTable::load()),
        store,
    )
}

/// [`verifications`], from grants already planned.
fn current_verifications<'a>(
    planned: &[route::Planned<'a>],
    store: &Store,
) -> Vec<(&'a adage_tracks::index::IndexedRoute, String, &'a Approval)> {
    let mut out: Vec<(&adage_tracks::index::IndexedRoute, String, &Approval)> = Vec::new();
    for p in planned {
        let (r, a) = (p.route, p.approval);
        let Step::Run { command } = &r.route.step else { continue };
        if r.route.effect != adage_core::Effect::Verify
            || command.contains('{')
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

/// At most this many Stop-gate runs per session; only the first may block.
const MAX_STOP_GATES: u32 = 2;

/// Time a gate run needs beyond its command's own wall limit: the worktree copy, the change set,
/// the input snapshots and the apply (bounded in entries and bytes, not in time).
const GATE_OVERHEAD_MS: u64 = 6_000;

/// The finish-line gate. After a learned context verification ran in this treated session, a
/// Stop whose current inputs have no successful run of it runs it (same grant, policy, sandbox,
/// journal). Passed: the turn ends with a line for the person. Failed: the first time (and not
/// while another stop hook is already continuing), the agent continues with the failing tests;
/// otherwise the turn ends with a line for the person. Never more than [`MAX_STOP_GATES`] runs.
fn stop_gate(call: &StopCall<'_>) -> Option<StopGate> {
    if !call.harness.capabilities().actions {
        return None;
    }
    let state = adage_store::state::try_read::<adage_runtime::SessionState>(&call.paths.state(call.session)).ok()??;
    if state.arm != Some(adage_runtime::Arm::Treated) || state.stop_gates >= MAX_STOP_GATES {
        return None;
    }
    let home = &call.paths.root;
    let pol = policy::load(home).ok()?;
    if pol.max_level < Level::E2 {
        return None;
    }
    let store = Store::load(&approvals::path(home)).ok()?;
    let rows = crate::journal::list(home).ok()?;
    if crate::journal::pending(home, call.root).ok()?.is_some() {
        return None; // recovery comes first and is reported by the ordinary pipeline
    }
    let mine: Vec<&crate::journal::Receipt> =
        rows.iter().filter(|r| r.repo == call.repo_key && r.session == call.session).collect();
    // Only a verification this session's learned context already ran: the gate follows prediction.
    let (ir, command, a) =
        verifications(call.repo_key, call.root, call.index, call.paths, &store).into_iter().find(|(ir, _, _)| {
            matches!(ir.route.trigger, Trigger::Context { .. })
                && mine.iter().any(|r| r.route == format!("{}/{}", ir.track, ir.route.id))
        })?;
    let label = format!("{}/{}", ir.track, ir.route.id);
    let table = CommandTable::load();
    let mut req =
        route::plan(ir, call.index, call.root, &call.paths.tracks(), Level::E2, a.terms.as_ref(), &table).ok()?.request;
    req.inputs = route::bound_from_command(&req.argv, &command)?;
    let action = req.action.clone()?;
    let now =
        crate::stage::input_snapshot_until(&req.repo_root.canonicalize().ok()?, &action, Some(call.deadline)).ok()?;
    let digest = crate::sha256_hex(crate::canonical_json(&now).as_bytes());
    let fresh = mine.iter().any(|r| {
        r.route == label && r.input_digest == digest && matches!(r.outcome, crate::stage::Outcome::Applied { .. })
    });
    if fresh {
        return None;
    }
    // Start only a run that, staged, checked and applied, ends before the hook's deadline: a hook
    // killed between the journal's prepared and completed receipts leaves the worktree in
    // recovery (review F4). Not started, it waits for the next Stop, uncounted.
    let needed = std::time::Duration::from_millis(req.profile.limits.wall_ms.saturating_add(GATE_OVERHEAD_MS));
    if call.deadline.saturating_duration_since(Instant::now()) < needed {
        return None;
    }
    let cap = sandbox::detect();
    let ctx = Context {
        inherited_env: std::env::vars().collect(),
        policy: &pol,
        store: &store,
        capability: &cap,
        table: &table,
        repo_key: call.repo_key.to_string(),
        cas: Some(home.join("cas")).filter(|p| p.is_dir()),
        home: adage_store::home_dir(),
        canaries: Vec::new(),
    };
    let gates = adage_store::state::update::<adage_runtime::SessionState, _>(&call.paths.state(call.session), |st| {
        st.stop_gates += 1;
        st.stop_gates
    })
    .ok()?;
    let may_block = gates == 1 && !call.stop_hook_active;
    let staged = match crate::stage::stage(&req, &ctx) {
        Ok(s) => s,
        Err(r) => {
            return Some(StopGate::Notice(format!(
                "adage: could not run `{}` at finish: {}.",
                render::sanitise(&command),
                crate::reason_text(&r.reason)
            )))
        }
    };
    let completed = crate::journal::finish(
        &staged,
        &crate::journal::Commit {
            home,
            repo: call.repo_key,
            session: call.session,
            occurrence: &format!("stop-gate:{gates}:{digest}"),
            approved_on: &a.approved_on,
            request: &req,
            origin: crate::journal::RunOrigin::StopGate,
        },
    );
    Some(gate_answer(&completed.outcome, &command, &staged, &state.new_tests, may_block, call.stop_hook_active))
}

/// What a gate run's outcome tells the agent (a `Block`) or only the person (a `Notice`).
fn gate_answer(
    outcome: &crate::stage::Outcome,
    command: &str,
    staged: &crate::stage::Staged,
    new_tests: &[String],
    may_block: bool,
    stop_hook_active: bool,
) -> StopGate {
    match outcome {
        crate::stage::Outcome::Applied { .. } => {
            StopGate::Notice(render::stop_gate_notice(command, true, staged.exit, staged.duration_ms))
        }
        crate::stage::Outcome::Failed { exit, .. } if may_block => StopGate::Block(render::stop_gate_failed(
            command,
            *exit,
            failure_record(staged, command, new_tests).as_ref(),
        )),
        crate::stage::Outcome::Failed { exit, .. } => {
            StopGate::Notice(render::stop_gate_notice(command, false, *exit, staged.duration_ms))
        }
        // As for an action (`act`): completion unconfirmed, so the agent is told not to retry it
        // and to have a person inspect it (the journal's recovery block pauses later runs).
        crate::stage::Outcome::Unknown { reason } if !stop_hook_active => {
            StopGate::Block(render::stop_gate_unknown(command, &crate::reason_text(reason)))
        }
        crate::stage::Outcome::Refused { reason, .. } | crate::stage::Outcome::Unknown { reason } => {
            StopGate::Notice(format!(
                "adage: `{}` at finish did not complete: {}.",
                render::sanitise(command),
                crate::reason_text(reason)
            ))
        }
    }
}

const MAX_LISTED_TESTS: usize = 12;

/// The node ids in the worktree's pytest record, when this is a pytest verification whose approval
/// declares that cache file: `Some(empty)` when there is no record yet, `None` when not applicable
/// or unreadable. Only well-formed `path.py::name` ids inside the repository are kept.
fn pytest_nodeids(
    root: &std::path::Path,
    req: &crate::request::ExecRequest,
    command: &str,
) -> Option<std::collections::BTreeSet<String>> {
    let declared = req.action.as_ref().is_some_and(|a| crate::stage::matches_any(&a.cache, PYTEST_NODEIDS));
    if !declared || pytest_call(&command.split_whitespace().collect::<Vec<_>>()).is_none() {
        return None;
    }
    let path = root.join(PYTEST_NODEIDS);
    let meta = match std::fs::symlink_metadata(&path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Some(Default::default()),
        Err(_) => return None,
    };
    if !meta.is_file() || meta.len() > 4 << 20 {
        return None;
    }
    let ids: Vec<String> = serde_json::from_slice(&std::fs::read(&path).ok()?).ok()?;
    Some(ids.into_iter().filter(|id| valid_nodeid(id)).collect())
}

fn valid_nodeid(id: &str) -> bool {
    let Some((file, name)) = id.split_once("::") else { return false };
    id.len() <= 200
        && file.ends_with(".py")
        && !file.starts_with('/')
        && !file.split('/').any(|p| p == ".." || p.is_empty())
        && file.chars().all(|c| c.is_ascii_alphanumeric() || "_./-".contains(c))
        && !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || "_:.-[]".contains(c))
}

/// A node id's test function: its last part, without parameters (`a.py::C::test_x[1]` → `test_x`).
fn test_function(id: &str) -> &str {
    let last = id.rsplit("::").next().unwrap_or(id);
    last.split_once('[').map_or(last, |(name, _)| name)
}

/// The applied run's test evidence: its pytest record, bound to the journal (the file now in the
/// worktree must be the applied output the receipt recorded), restricted to the command's targets.
/// The tests this session's edits added (`edited`, by function name: the runtime's `new_tests`)
/// are named when the record has them; otherwise the ids new since the last recorded run.
fn test_evidence(
    turn: &Turn<'_>,
    req: &crate::request::ExecRequest,
    command: &str,
    operation: Option<&str>,
    before: Option<&std::collections::BTreeSet<String>>,
    edited: &[String],
) -> Option<render::TestEvidence> {
    let before = before?;
    let call = pytest_call(&command.split_whitespace().collect::<Vec<_>>())?;
    let receipt =
        crate::journal::list(&turn.paths.root).ok()?.into_iter().rev().find(|r| Some(r.id.as_str()) == operation)?;
    let crate::stage::FileState::File { sha256, .. } = receipt.outputs.get(PYTEST_NODEIDS)? else { return None };
    if crate::sha256_hex(&std::fs::read(turn.root.join(PYTEST_NODEIDS)).ok()?) != *sha256 {
        return None;
    }
    let after = pytest_nodeids(turn.root, req, command)?;
    let inside = |id: &String| in_targets(id, &call.targets);
    let collected = before.is_empty().then(|| after.iter().filter(|id| inside(id)).count());
    let yours: Vec<String> =
        after.iter().filter(|id| inside(id) && edited.iter().any(|n| n == test_function(id))).cloned().collect();
    let (new, yours) = if yours.is_empty() {
        (after.difference(before).filter(|id| inside(id)).cloned().collect::<Vec<String>>(), false)
    } else {
        (yours, true)
    };
    if collected.is_none() && new.is_empty() {
        return None;
    }
    let more_new = new.len().saturating_sub(MAX_LISTED_TESTS);
    Some(render::TestEvidence { collected, new: new.into_iter().take(MAX_LISTED_TESTS).collect(), more_new, yours })
}

/// pytest options that change only what is printed or when a passing run stops, never which
/// tests run (registered with the launch-gate scorer).
const OUTPUT_FLAGS: &[&str] =
    &["-q", "-qq", "-v", "-vv", "-s", "-x", "-rA", "-ra", "--no-header", "--disable-warnings", "--quiet", "--verbose"];

/// A pytest invocation: interpreter word, non-output options, sorted targets.
#[derive(Debug, PartialEq)]
struct PytestCall {
    interpreter: String,
    options: Vec<String>,
    targets: Vec<String>,
}

/// Commands a pytest call may share a command line with, because they only read or print (a
/// pipe to `tail`, an `echo` after): none changes the directory, the environment or a file.
const PASSIVE: &[&str] =
    &["tail", "head", "grep", "egrep", "fgrep", "echo", "printf", "true", "wc", "sort", "uniq", "cut"];

/// The pytest invocations in a plain command line (`&&`, `||`, `;` and `|` lists, with `2>&1`).
/// `None` for anything a word split cannot read faithfully (quotes, expansions, redirections,
/// globs), for no pytest at all, and for a line whose other commands could change what pytest
/// sees: `cd`, `pushd`, `popd`, `export`, a variable assignment, or any command not in
/// [`PASSIVE`] (review F5).
fn pytest_calls(command: &str, root: &std::path::Path) -> Option<Vec<PytestCall>> {
    if command.chars().any(|c| "'\"`$<(){}*?[]#\\\n".contains(c)) {
        return None;
    }
    let words: Vec<&str> = command.split_whitespace().filter(|w| *w != "2>&1").collect();
    if words.iter().any(|w| w.contains('>')) {
        return None;
    }
    let prefix = format!("{}/", root.to_string_lossy());
    let mut calls = Vec::new();
    for segment in words.split(|w| matches!(*w, "&&" | "||" | ";" | "|")) {
        let argv: Vec<&str> = segment.iter().map(|w| w.strip_prefix(prefix.as_str()).unwrap_or(w)).collect();
        if let Some(call) = pytest_call(&argv) {
            calls.push(call);
            continue;
        }
        let Some(first) = argv.first() else { continue };
        if first.contains('=') || !PASSIVE.contains(first) {
            return None;
        }
        if argv.iter().any(|w| w.contains("pytest") || w.ends_with(';') || w.contains('&')) {
            return None;
        }
    }
    (!calls.is_empty()).then_some(calls)
}

fn pytest_call(argv: &[&str]) -> Option<PytestCall> {
    let python = |w: &str| {
        let name = w.rsplit('/').next().unwrap_or(w);
        name == "python"
            || name
                .strip_prefix("python")
                .is_some_and(|v| !v.is_empty() && v.chars().all(|c| c.is_ascii_digit() || c == '.'))
    };
    let (interpreter, rest) = match argv {
        [i, "-m", "pytest", rest @ ..] if python(i) => (i.to_string(), rest),
        _ => return None,
    };
    let (mut options, mut targets) = (Vec::new(), Vec::new());
    let mut i = 0;
    while i < rest.len() {
        let a = rest[i];
        if a == "-p" && rest.get(i + 1) == Some(&"no:cacheprovider") {
            i += 2;
            continue;
        }
        let output = OUTPUT_FLAGS.contains(&a)
            || a.starts_with("--tb=")
            || a.starts_with("--color=")
            || (a.len() > 1
                && a.starts_with('-')
                && !a.starts_with("--")
                && a[1..].chars().all(|c| "qvsx".contains(c)));
        if !output {
            if a.starts_with('-') {
                options.push(a.to_string());
            } else {
                targets.push(a.trim_end_matches('/').to_string());
            }
        }
        i += 1;
    }
    targets.sort();
    targets.dedup();
    Some(PytestCall { interpreter, options, targets })
}

/// Whether `done` already ran every test `asked` selects: same options, and each requested
/// target is one of its targets or a node or path inside one. A different interpreter word
/// is accepted only for a bare system `python`, which the advice beside this points at the
/// project's environment; the note says so.
fn covers(done: &PytestCall, asked: &PytestCall) -> bool {
    let within = |t: &str| {
        done.targets.iter().any(|s| t == s || t.starts_with(&format!("{s}::")) || t.starts_with(&format!("{s}/")))
    };
    let interpreter = asked.interpreter == done.interpreter || !asked.interpreter.contains('/');
    interpreter
        && asked.options == done.options
        && !done.targets.is_empty()
        && !asked.targets.is_empty()
        && asked.targets.iter().all(|t| within(t))
}

/// The most a receipt check may take of a `PreToolUse` hook's 5 s: past it, no note (the
/// interpreter rewrite beside it must still be delivered in time).
const RECEIPT_CHECK_MS: u64 = 1_500;

/// A completed, successful learned verification in this session whose inputs are byte-identical
/// now (the executor's own snapshot, recomputed here) and which already ran every test `command`
/// selects. A fact for the agent, never a refusal: the call still runs if it is made. Only for a
/// call made at the repository root (relative targets resolve as the receipt's did), and only
/// when the check fits [`RECEIPT_CHECK_MS`] (review F5, F6).
fn covering_receipt(call: &PreCall<'_>) -> Option<String> {
    let deadline = Instant::now() + std::time::Duration::from_millis(RECEIPT_CHECK_MS);
    let canon = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if canon(call.cwd) != canon(call.root) {
        return None;
    }
    let asked = pytest_calls(call.command, call.root)?;
    let rows = crate::journal::list(&call.paths.root).ok()?;
    let r = rows.iter().rev().find(|r| {
        r.repo == call.repo_key
            && r.session == call.session
            && r.phase == crate::journal::Phase::Completed
            && matches!(r.outcome, crate::stage::Outcome::Applied { .. })
    })?;
    let ir = call.index.for_repo(call.repo_key).find(|x| format!("{}/{}", x.track, x.route.id) == r.route)?;
    let Step::Run { command: template } = &ir.route.step else { return None };
    if ir.route.effect != adage_core::Effect::Verify || template.contains('{') {
        return None;
    }
    let store = Store::load(&approvals::path(&call.paths.root)).ok()?;
    let a = store
        .approvals
        .iter()
        .find(|a| a.key.repo == call.repo_key && a.route == r.route && a.key.level == Level::E2)?;
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
    if crate::sha256_hex(crate::canonical_json(&now).as_bytes()) != r.input_digest {
        return None;
    }
    let done = pytest_call(&template.split_whitespace().collect::<Vec<_>>())?;
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
    Some(format!(
        "<adage-receipt operation=\"{}\">\nAdage's run of `{}` in this session passed on files identical to the current ones, and it included every test this command selects{}.\n</adage-receipt>",
        render::sanitise(&r.id).chars().take(12).collect::<String>(),
        render::sanitise(template),
        if elsewhere { " (in the project's environment)" } else { "" }
    ))
}

/// What the E2 step decided for one event.
#[derive(Default)]
struct Acted {
    /// Route → the text replacing its directive in this event.
    replace: BTreeMap<String, String>,
    extra: Vec<String>,
    met: Vec<String>,
    stop: Option<Option<String>>,
    records: Vec<Value>,
    recovery: Option<String>,
    /// An approved action was selected for a Stop request at this event ([`adage_hook::Delivery::at_stop_action`]).
    at_stop_action: bool,
    /// For the person: the selected grant no longer matches ([`adage_hook::Delivery::notice`]).
    notice: Option<String>,
}

/// The directive for a route: the one about to be delivered, else rebuilt from the index entry
/// (the obligation was announced earlier and re-armed now).
fn directive_for(ir: &adage_tracks::index::IndexedRoute, directives: &[Directive], command: &str) -> Option<Directive> {
    if let Some(d) = directives.iter().find(|d| d.route == ir.route.id && d.track == ir.track) {
        return Some(d.clone());
    }
    let Step::Run { command: template } = &ir.route.step else { return None };
    let bound: Vec<(String, String)> =
        route::bound_from_command(&template.split_whitespace().map(String::from).collect::<Vec<_>>(), command)?
            .into_iter()
            .collect();
    Some(Directive {
        route: ir.route.id.clone(),
        track: ir.track.clone(),
        version: ir.version.clone(),
        strength: ir.route.strength,
        text: render::route_text(&ir.route, &bound)?,
        kind: adage_tracks::shape(&ir.route).to_string(),
        origin: None,
    })
}

/// A runtime-selected action; a context prediction never creates an edit obligation.
struct Candidate<'a> {
    route: &'a str,
    track: Option<&'a str>,
    command: &'a str,
    paths: &'a str,
    edited: &'a str,
    at_stop: bool,
    context: Option<&'a Trigger>,
}

fn candidates<'a>(turn: &'a Turn<'_>, directives: &[Directive]) -> Vec<Candidate<'a>> {
    let mut out: Vec<_> = turn
        .stimuli
        .iter()
        .map(|s| Candidate {
            route: &s.route,
            track: None,
            command: &s.command,
            paths: &s.paths,
            edited: &s.edited,
            at_stop: s.at_stop,
            context: None,
        })
        .collect();
    // Only actual runtime selections qualify. Evidence is not authority: act() still requires
    // the exact local E2 grant, then every ordinary request/sandbox/journal gate.
    for d in directives {
        let Some(ir) = route::find(turn.index, turn.repo_key, &d.track, &d.route) else { continue };
        let r = &ir.route;
        if !matches!(r.trigger, Trigger::Context { .. })
            || r.origin() != Origin::Learned
            || r.evidence.execution.is_none()
            || !r.slots.is_empty()
            || !matches!(r.effect, adage_core::Effect::Verify | adage_core::Effect::Regenerate)
            || out.iter().any(|s| s.route == r.id)
        {
            continue;
        }
        let Step::Run { command } = &r.step else { continue };
        out.push(Candidate {
            route: &r.id,
            track: Some(&ir.track),
            command,
            paths: "",
            edited: "",
            at_stop: false,
            context: Some(&r.trigger),
        });
    }
    out
}

/// E2: run at most one approved obligation or selected learned-context action.
fn act(turn: &Turn<'_>, directives: &[Directive], candidates: &[Candidate<'_>]) -> Acted {
    let mut out = Acted::default();
    let home = &turn.paths.root;
    // Recovery precedes grant selection: a revoked/missing grant must not hide an earlier
    // possibly applied action and restore a directive asking the agent to repeat it.
    match crate::journal::pending(home, turn.root) {
        Ok(Some(receipt)) => {
            out.recovery = Some(format!("Adage paused actions in this worktree because operation {} needs recovery. Inspect its changes before acknowledging it; do not retry automatically.", receipt.id));
            out.records.push(
                json!({ "route": receipt.route, "level": "e2", "outcome": "unknown", "operation_id": receipt.id }),
            );
            return out;
        }
        Err(e) => {
            out.recovery = Some(format!("Adage cannot verify the execution journal: {}. Earlier changes may be present. Inspect the worktree and repair the journal; do not retry automatically.", crate::reason_text(&e.to_string())));
            out.records.push(json!({ "route": "*", "level": "e2", "outcome": "unknown", "withheld": "journal" }));
            return out;
        }
        Ok(None) => {}
    }
    if turn.arm == Some(adage_runtime::Arm::Control) {
        return out;
    }
    let Ok(store) = Store::load(&approvals::path(home)) else { return out };
    let approved = |s: &Candidate<'_>| -> Option<(&Approval, &adage_tracks::index::IndexedRoute)> {
        store.approvals.iter().filter(|a| a.key.repo == turn.repo_key && a.key.level == Level::E2).find_map(|a| {
            let ir = turn.index.routes.iter().find(|r| {
                r.repo == turn.repo_key
                    && r.route.id == s.route
                    && s.track.is_none_or(|t| t == r.track)
                    && a.route == format!("{}/{}", r.track, r.route.id)
            })?;
            Some((a, ir))
        })
    };
    // At most one action per event.
    let Some((s, a, ir)) = candidates.iter().find_map(|s| approved(s).map(|(a, ir)| (s, a, ir))) else { return out };
    let label = a.route.clone();
    let Some(dir) = directive_for(ir, directives, s.command) else {
        out.records.push(json!({ "route": label, "level": "e2", "withheld": "render",
                                 "reason": "the action's directive could not be rendered" }));
        return out;
    };
    let not_run = |out: &mut Acted, stage: &str, reason: &str| {
        out.records
            .push(json!({ "route": label, "level": "e2", "withheld": stage, "reason": crate::reason_text(reason), "context": s.context }));
        if s.context.is_some() {
            // The spool records the reason; an unexecuted prediction supplies no agent advice.
            out.replace.insert(dir.route.clone(), String::new());
        } else if s.at_stop && ir.exec_only() {
            // The agent cannot run a vendored script itself: asking it to would be unfollowable.
            out.stop = Some(None);
        } else if s.at_stop {
            out.stop = Some(Some(render::stop_reason_not_run(s.paths, s.command, reason)));
        } else if directives.iter().any(|d| d.route == dir.route) {
            out.replace.insert(dir.route.clone(), render::action_not_run(&dir, reason));
        } else {
            out.extra.push(render::action_not_run(&dir, reason));
        }
    };
    if !turn.harness.capabilities().actions {
        // Named from the harness: only a hook that can wait for a run carries actions out.
        let reason = format!("actions do not run under {}, whose hooks cannot wait for a run", turn.harness.display());
        not_run(&mut out, "harness", &reason);
        return out;
    }
    let table = CommandTable::load();
    let plan = match route::plan(ir, turn.index, turn.root, &turn.paths.tracks(), Level::E2, a.terms.as_ref(), &table) {
        Ok(p) => p,
        Err(e) => {
            out.notice = Some(lapse_notice(a, vec![format!("it cannot be planned now: {}", crate::reason_text(&e))]));
            not_run(&mut out, "route", &e);
            return out;
        }
    };
    let mut req = plan.request;
    match route::bound_from_command(&req.argv, s.command) {
        Some(inputs) => req.inputs = inputs,
        None => {
            not_run(&mut out, "bind", "its command could not be bound from the selected action");
            return out;
        }
    }
    // A grant that no longer matches what would run is refused below (`run::check`); the person
    // is told which field changed and how to approve it again, not only the spool.
    let key = approvals::ApprovalKey::for_request(&req, turn.repo_key);
    if store.find(&key).is_none() {
        out.notice = Some(lapse_notice(a, a.key.diff(&key).into_iter().map(|c| c.field.to_string()).collect()));
    }
    let Ok(pol) = policy::load(home) else {
        not_run(&mut out, "policy", "the policy file is unreadable");
        return out;
    };
    let cap = sandbox::detect();
    let ctx = Context {
        inherited_env: std::env::vars().collect(),
        policy: &pol,
        store: &store,
        capability: &cap,
        table: &table,
        repo_key: turn.repo_key.to_string(),
        cas: Some(home.join("cas")).filter(|p| p.is_dir()),
        home: adage_store::home_dir(),
        canaries: Vec::new(),
    };
    // Prefer native tool-call identity: a redelivery must not become a fresh apply just
    // because decide() incremented a batch counter. Counters remain a best-effort fallback
    // for Stop and older payloads without IDs; those cannot promise redelivery deduplication.
    let state = match adage_store::state::try_read::<adage_runtime::SessionState>(&turn.paths.state(turn.session)) {
        Ok(Some(s)) => s,
        _ => {
            not_run(&mut out, "state", "the action occurrence cannot be read safely");
            return out;
        }
    };
    // Runtime emits a context route once per session. Refuse replay before even spawning;
    // completed/failed receipts survive state reconstruction and native event redelivery.
    if s.context.is_some() {
        match crate::journal::list(home) {
            Ok(rows)
                if rows.iter().any(|r| r.repo == turn.repo_key && r.session == turn.session && r.route == label) =>
            {
                not_run(&mut out, "duplicate", "this context action was already attempted in this session");
                return out;
            }
            Err(_) => {
                not_run(&mut out, "journal", "the execution journal cannot be read safely");
                return out;
            }
            _ => {}
        }
    }
    let occurrence = match &turn.event_identity {
        Some(id) => format!("native:{id}:{}", s.route),
        None => format!("counter:{}:{}:{}:{}", turn.event, state.prompts, state.batches, s.route),
    };
    let nodeids_before = pytest_nodeids(turn.root, &req, s.command);
    // From here this Stop's time belongs to this action, whatever its outcome: no Stop gate too.
    out.at_stop_action = s.at_stop;
    let staged = match crate::stage::stage(&req, &ctx) {
        Ok(x) => x,
        Err(r) => {
            not_run(&mut out, r.stage, &r.reason);
            return out;
        }
    };
    let completed = crate::journal::finish(
        &staged,
        &crate::journal::Commit {
            home,
            repo: turn.repo_key,
            session: turn.session,
            occurrence: &occurrence,
            approved_on: &a.approved_on,
            request: &req,
            origin: if s.context.is_some() {
                crate::journal::RunOrigin::Context
            } else {
                crate::journal::RunOrigin::Obligation
            },
        },
    );
    let outcome = completed.outcome;
    let revoked = completed.revoked;
    let mut rec = json!({
        "route": label, "level": "e2", "outcome": outcome.name(), "exit": staged.exit,
        "duration_ms": staged.duration_ms,
        "operation_id": completed.id, "replayed_receipt": completed.replayed,
        "context": s.context,
    });
    if revoked {
        rec["revoked"] = json!(true);
    }
    match &outcome {
        crate::stage::Outcome::Unknown { reason } => {
            // Do not fall back to an instruction to repeat a potentially applied action.
            let text = format!("Adage cannot confirm this action's completion: {}. Do not retry it automatically; inspect the worktree and ask the person to acknowledge recovery.", crate::reason_text(reason));
            out.recovery = Some(text.clone());
            if s.at_stop {
                out.stop = Some(Some(text));
            } else {
                let block = render::wrap_origin(
                    &dir.track,
                    &dir.version,
                    &dir.route,
                    &dir.kind,
                    dir.strength,
                    &text,
                    dir.origin.as_ref(),
                );
                if directives.iter().any(|d| d.route == dir.route) {
                    out.replace.insert(dir.route.clone(), block);
                } else {
                    out.extra.push(block);
                }
            }
            out.records.push(rec);
        }
        crate::stage::Outcome::Applied { changed, .. } => {
            out.records.push(rec);
            if s.context.is_none() {
                out.met.push(s.route.to_string());
            }
            if s.at_stop {
                // The Stop hook cannot carry context: the request is withdrawn, the record kept.
                out.stop = Some(None);
            } else {
                let paths: Vec<String> = changed.iter().map(|l| l.path.clone()).collect();
                let tests = (ir.route.effect == adage_core::Effect::Verify)
                    .then(|| {
                        let edited = &state.new_tests;
                        test_evidence(turn, &req, s.command, completed.id.as_deref(), nodeids_before.as_ref(), edited)
                    })
                    .flatten();
                let text = if s.context.is_some() {
                    render::action_predicted(
                        &dir,
                        s.command,
                        &paths,
                        ir.route.effect == adage_core::Effect::Verify,
                        &render::CpuReceipt {
                            tests: tests.as_ref(),
                            operation: completed.id.as_deref().unwrap_or_default(),
                            input_digest: &crate::sha256_hex(crate::canonical_json(&staged.inputs).as_bytes()),
                            exit: staged.exit,
                            duration_ms: staged.duration_ms,
                        },
                    )
                } else {
                    render::action_applied(&dir, s.edited, s.command, &paths)
                };
                if directives.iter().any(|d| d.route == dir.route) {
                    out.replace.insert(dir.route.clone(), text);
                } else {
                    out.extra.push(text);
                }
            }
        }
        crate::stage::Outcome::Refused { reason, .. } => {
            let reason = if revoked { format!("{reason}; its approval is now revoked") } else { reason.clone() };
            let records = std::mem::take(&mut out.records);
            not_run(&mut out, "refused", &reason);
            out.records = records;
            out.records.push(rec);
        }
        crate::stage::Outcome::Failed { exit, .. }
            if s.context.is_some() && ir.route.effect == adage_core::Effect::Verify =>
        {
            // A failed run is a result the agent needs: what failed in this run, from the declared
            // record (never a stale list).
            let record = failure_record(&staged, s.command, &state.new_tests);
            out.records.push(rec);
            out.replace.insert(
                dir.route.clone(),
                render::action_failed(
                    &dir,
                    s.command,
                    *exit,
                    record.as_ref(),
                    completed.id.as_deref().unwrap_or_default(),
                ),
            );
        }
        crate::stage::Outcome::Failed { exit, .. } => {
            let reason = match exit {
                Some(e) => format!("it exited {e} in the staged copy, and nothing was applied"),
                None => "it was stopped in the staged copy, and nothing was applied".to_string(),
            };
            let records = std::mem::take(&mut out.records);
            not_run(&mut out, "failed", &reason);
            out.records = records;
            out.records.push(rec);
        }
    }
    out
}

/// The person's line for a grant `a` whose key no longer matches (`changed`: its fields, or why).
fn lapse_notice(a: &Approval, changed: Vec<String>) -> String {
    route::Lapse { route: a.route.clone(), level: a.key.level, changed, reapprove: route::reapprove_command(a) }
        .notice()
}

/// A withheld attempt's spool record: the stage that refused it and why (never output text), so
/// `adage-exec doctor` and `adage review` can say what went wrong ([`crate::last_withheld`]).
fn withheld(route: &str, stage: &str, reason: &str) -> Value {
    json!({ "route": route, "level": "e1", "withheld": stage, "reason": crate::reason_text(reason) })
}

/// The oracle's view of a declared field.
pub fn data_type(f: &FieldSpec) -> DataType {
    match f.ty {
        FieldType::RepoPath => DataType::RepoPath,
        FieldType::RepoPathList => DataType::RepoPathList { max: f.max.unwrap_or(DEFAULT_LIST).min(MAX_LIST) },
        FieldType::GitRev => DataType::GitRev,
        FieldType::Int => DataType::Int,
        FieldType::Bool => DataType::Bool,
        FieldType::Enum => DataType::Enum(f.values.clone()),
        FieldType::Pattern => DataType::Pattern(f.regex.clone().unwrap_or_default()),
        FieldType::Text if f.stdout => DataType::TextLines { max: f.max.unwrap_or(DEFAULT_TEXT).min(MAX_TEXT) },
        FieldType::Text => DataType::Text { max: f.max.unwrap_or(DEFAULT_TEXT).min(MAX_TEXT) },
    }
}

/// The payload line of a rendered block (the line before its closer).
fn payload(block: &str) -> Option<&str> {
    block.strip_suffix("\n</adage-data>")?.rsplit('\n').next()
}

/// `Ok(None)`: nothing approved here (E0, nothing recorded). `Ok(Some)`: every approved directive
/// delivered with its block. `Err`: something failed; E0 is delivered and the records say why.
#[allow(clippy::type_complexity)]
fn prefetch(
    turn: &Turn<'_>,
    directives: &[Directive],
    notice: &mut Option<String>,
) -> Result<Option<(Vec<String>, Vec<Value>)>, Vec<Value>> {
    let home = &turn.paths.root;
    let store = match Store::load(&approvals::path(home)) {
        Ok(s) => s,
        Err(e) => return Err(vec![withheld("*", "approvals", &format!("the approvals file is unreadable: {e:#}"))]),
    };
    let label = |d: &Directive| format!("{}/{}", d.track, d.route);
    let grant = |d: &Directive| -> Option<&Approval> {
        store.approvals.iter().find(|a| a.route == label(d) && a.key.repo == turn.repo_key && a.key.level == Level::E1)
    };
    if !directives.iter().any(|d| grant(d).is_some()) {
        return Ok(None);
    }
    let started = Instant::now();
    let mut records = Vec::new();
    let pol = policy::load(home)
        .map_err(|e| vec![withheld("*", "policy", &format!("the execution policy is unreadable: {e:#}"))])?;
    let cap = sandbox::detect();
    let table = CommandTable::load();
    let cas = Some(home.join("cas")).filter(|p| p.is_dir());
    let ctx = Context {
        inherited_env: std::env::vars().collect(),
        policy: &pol,
        store: &store,
        capability: &cap,
        table: &table,
        repo_key: turn.repo_key.to_string(),
        cas,
        home: adage_store::home_dir(),
        canaries: Vec::new(),
    };
    let tracks = turn.paths.tracks();
    let mut texts = Vec::new();
    for d in directives {
        let Some(a) = grant(d) else {
            texts.push(render::wrap_directive(d));
            continue;
        };
        let name = label(d);
        let fail = |mut records: Vec<Value>, stage: &str, reason: &str| {
            records.push(withheld(&name, stage, reason));
            records
        };
        let Some(ir) = route::find(turn.index, turn.repo_key, &d.track, &d.route) else {
            return Err(fail(records, "route", "the index holds no such route for this repository"));
        };
        let plan = match route::plan(ir, turn.index, turn.root, &tracks, Level::E1, a.terms.as_ref(), &table) {
            Ok(p) => p,
            Err(e) => {
                *notice = Some(lapse_notice(a, vec![format!("it cannot be planned now: {}", crate::reason_text(&e))]));
                return Err(fail(records, "route", &e));
            }
        };
        let mut req = plan.request;
        match route::bound_inputs(&req.argv, &d.text) {
            Some(inputs) => req.inputs = inputs,
            None => {
                return Err(fail(records, "bind", "the directive's command line does not match the route's template"))
            }
        }
        if let Err(reason) = route::unique_prompt_inputs(&ir.route, turn.ev, &req.inputs) {
            return Err(fail(records, "bind", &reason));
        }
        // As for an action: a grant that no longer matches is refused below, and said.
        // (An automatic approval is granted again while the route stays in its class.)
        let key = approvals::ApprovalKey::for_request(&req, turn.repo_key);
        if !a.auto && store.find(&key).is_none() {
            *notice = Some(lapse_notice(a, a.key.diff(&key).into_iter().map(|c| c.field.to_string()).collect()));
        }
        // All E1 runs of one event together stay within E1's longest wall timeout.
        let left = limits::E1_MAX_WALL_MS.saturating_sub(started.elapsed().as_millis() as u64);
        if left < req.profile.limits.wall_ms {
            let why = format!("{left} ms left of the event's {} ms for prefetches", limits::E1_MAX_WALL_MS);
            return Err(fail(records, "limits", &why));
        }
        let delivered = match run::execute(&req, &ctx) {
            Ok(x) => x,
            Err(r) => return Err(fail(records, r.stage, &r.reason)),
        };
        let fields: Vec<(String, DataType)> =
            req.output.fields.iter().map(|(k, f)| (k.clone(), data_type(f))).collect();
        let decl = (req.output.mode == OutputMode::Typed).then_some(fields.as_slice());
        let violations = check_data_block(&d.route, &delivered.block, decl, Some(turn.root), &ctx.canaries);
        if !violations.is_empty() {
            let why: Vec<String> = violations.iter().map(|v| v.reason.clone()).collect();
            return Err(fail(records, "oracle", &format!("the safety oracle refused the block: {}", why.join("; "))));
        }
        // Exit predicates, on the projected fields.
        let projected: BTreeMap<String, Value> = payload(&delivered.block)
            .filter(|_| req.output.mode == OutputMode::Typed)
            .and_then(|p| serde_json::from_str(p).ok())
            .unwrap_or_default();
        let note = plan
            .exit_when
            .iter()
            .find(|(f, v)| projected.get(f.as_str()).and_then(|x| x.as_str()) == Some(v.as_str()))
            .and_then(|(f, v)| render::e1_exit_note(f, v));
        // Only a complete stdout contract can say the lookup need not be repeated. A title or
        // an acceptance field alone does not establish that the task's description was retained.
        let whole = req.output.mode == OutputMode::Raw
            || req.output.fields.iter().any(|(k, f)| f.stdout && projected.get(k).is_some_and(|v| v.is_string()));
        let Some(text) = render::wrap_directive_e1(d, &delivered.block, note.as_deref(), whole) else {
            return Err(fail(records, "render", "the data block is not one whole <adage-data> block"));
        };
        texts.push(text);
        let mut rec = serde_json::to_value(&delivered.record).unwrap_or(Value::Null);
        rec["route"] = json!(name);
        records.push(rec);
    }
    let size = texts.join("\n\n").len();
    if size > turn.max_context {
        return Err({
            let why = format!("{size} bytes with the data blocks, over the harness's {} bytes", turn.max_context);
            records.push(withheld("*", "size", &why));
            records
        });
    }
    Ok(Some((texts, records)))
}

#[cfg(test)]
mod prediction_tests {
    use super::*;
    use adage_core::Effect;
    use adage_runtime::{Arm, Call, Ctx, Event, SessionState};
    use adage_store::Paths;
    use adage_tracks::{CtxItem, Evidence, ExecutionEvidence, Index, IndexedRoute, Route, RouteState, Strength};

    struct Fixture {
        _tmp: tempfile::TempDir,
        root: std::path::PathBuf,
        paths: Paths,
        index: Index,
    }
    impl Fixture {
        fn new() -> Self {
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
        fn turn<'a>(&'a self, ev: &'a Event) -> Turn<'a> {
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
        fn selected(&self, path: &str) -> (Event, Vec<Directive>) {
            let table = CommandTable::load();
            let ctx = Ctx {
                checkout: &adage_runtime::checkout::Disk,
                index: &self.index,
                repo: "fixture".into(),
                root: &self.root,
                table: &table,
                embed: None,
                params: Default::default(),
                policy: adage_runtime::Policy {
                    exec_only: vec![("fixture/context".into(), "predict-check".into())],
                    ..Default::default()
                },
            };
            let mut state = SessionState { arm: Some(Arm::Treated), ..Default::default() };
            let batch = |tool: &str, input| Event::Batch {
                calls: vec![Call { tool: tool.into(), input, output: String::new(), failed: false }],
            };
            let events = [
                batch("Bash", json!({"command":"git status --short"})),
                batch("Read", json!({"file_path": self.root.join("README.md")})),
                batch("Edit", json!({"file_path": self.root.join(path)})),
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
            adage_store::state::update::<SessionState, _>(&self.paths.state("fixture-session"), |s| *s = state)
                .unwrap();
            (events[2].clone(), dirs)
        }
        fn approve(&self) {
            self.approve_with(vec![]);
        }
        fn approve_with(&self, cache: Vec<String>) {
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
            });
            store.save(&approvals::path(&self.paths.root)).unwrap();
            std::fs::write(self.paths.root.join("policy.toml"), "[exec]\nmax_level = \"E2\"\n").unwrap();
        }
    }

    /// A completed, applied receipt for the fixture's route in `session`, on `digest`.
    fn record_receipt(f: &Fixture, session: &str, digest: &str) {
        let r = crate::journal::Receipt {
            origin: Some(crate::journal::RunOrigin::Context),
            id: String::new(),
            ts: "fixture".into(),
            session: session.into(),
            repo: "fixture".into(),
            worktree: String::new(),
            route: "fixture/context/predict-check".into(),
            approved_on: "fixture-only".into(),
            level: "e2".into(),
            phase: crate::journal::Phase::Completed,
            outcome: crate::stage::Outcome::Applied { changed: vec![], discarded: 0 },
            input_digest: digest.into(),
            outputs: BTreeMap::new(),
        };
        std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
        adage_store::spool::append(&crate::stage::spool_path(&f.paths.root), &r).unwrap();
    }

    /// The fixture as a project-Python repository with one pytest verification route (the layout
    /// `project_python` accepts: a venv whose runtime lives outside the repository).
    fn pytest_fixture() -> Fixture {
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
            format!(
                "home = {}/runtime/bin\nversion_info = 3.12\ninclude-system-site-packages = false\n",
                top.display()
            ),
        )
        .unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(top.join("runtime/bin/python3.12"), f.root.join(".venv/bin/python")).unwrap();
        std::fs::write(f.root.join("tests/test_x.py"), "def test_one():\n    pass\n").unwrap();
        f.index.routes[0].route.step = Step::Run { command: ".venv/bin/python -m pytest tests/test_x.py -q".into() };
        f.approve_with(vec![".pytest_cache/**".into()]);
        f
    }

    fn current_digest(f: &Fixture) -> String {
        let store = Store::load(&approvals::path(&f.paths.root)).unwrap();
        let plan = route::plan(
            &f.index.routes[0],
            &f.index,
            &f.root,
            &f.paths.tracks(),
            Level::E2,
            store.approvals[0].terms.as_ref(),
            &CommandTable::load(),
        )
        .unwrap();
        let now = crate::stage::input_snapshot(&f.root.canonicalize().unwrap(), plan.request.action.as_ref().unwrap())
            .unwrap();
        crate::sha256_hex(crate::canonical_json(&now).as_bytes())
    }

    #[test]
    fn pytest_scope_equivalence_is_narrow() {
        let root = std::path::Path::new("/repo");
        let done = pytest_call(&[".venv/bin/python", "-m", "pytest", "tests/test_x.py", "-q"]).unwrap();
        let covered = |c: &str| pytest_calls(c, root).is_some_and(|v| v.iter().all(|a| covers(&done, a)));
        for yes in [
            ".venv/bin/python -m pytest tests/test_x.py -q",
            ".venv/bin/python -m pytest tests/test_x.py::test_one -xvs",
            "/repo/.venv/bin/python -m pytest /repo/tests/test_x.py --tb=short -p no:cacheprovider 2>&1 | tail -20",
            "python -m pytest tests/test_x.py::test_one -v && .venv/bin/python -m pytest tests/test_x.py -q",
        ] {
            assert!(covered(yes), "{yes}");
        }
        for no in [
            ".venv/bin/python -m pytest tests/test_x.py -k one",
            ".venv/bin/python -m pytest tests/",
            ".venv/bin/python -m pytest",
            ".venv/bin/python -m pytest tests/test_y.py",
            "other/bin/python -m pytest tests/test_x.py",
            ".venv/bin/pytest tests/test_x.py",
            ".venv/bin/python -m pytest \"$T\"",
            ".venv/bin/python -m pytest tests/test_x.py > out.txt",
            "git status",
        ] {
            assert!(!covered(no), "{no}");
        }
    }

    #[test]
    fn pretool_context_needs_a_fresh_successful_receipt_in_this_session_covering_the_call() {
        let f = pytest_fixture();
        let call = |command: &'static str, session: &'static str| {
            PREFETCH.pretool_context(&PreCall {
                repo_key: "fixture",
                root: &f.root,
                cwd: &f.root,
                session,
                paths: &f.paths,
                index: &f.index,
                command,
            })
        };
        let node = ".venv/bin/python -m pytest tests/test_x.py::test_one -xvs";
        assert!(call(node, "fixture-session").is_none(), "no receipt yet");
        let digest = current_digest(&f);
        record_receipt(&f, "fixture-session", &digest);
        let text = call(node, "fixture-session").unwrap();
        assert!(text.contains("run of `.venv/bin/python -m pytest tests/test_x.py -q` in this session passed on files identical to the current ones"), "{text}");
        assert!(!text.contains("project's environment"));
        assert!(call("python -m pytest tests/test_x.py -v", "fixture-session")
            .unwrap()
            .contains("(in the project's environment)"));
        assert!(call(node, "another-session").is_none(), "another session's receipt is not this session's evidence");
        assert!(call(".venv/bin/python -m pytest tests/", "fixture-session").is_none(), "wider scope");
        assert!(
            call(".venv/bin/python -m pytest tests/test_x.py -k one", "fixture-session").is_none(),
            "selection option"
        );
        // Declared caches do not make a receipt stale; any other repository file does.
        std::fs::create_dir_all(f.root.join(".pytest_cache")).unwrap();
        std::fs::write(f.root.join(".pytest_cache/nodeids"), "[]").unwrap();
        assert!(call(node, "fixture-session").is_some(), "a declared cache does not make it stale");
        std::fs::remove_dir_all(f.root.join(".pytest_cache")).unwrap();
        std::fs::write(f.root.join("tests/test_x.py"), "def test_one():\n    assert False\n").unwrap();
        assert!(call(node, "fixture-session").is_none(), "a later edit makes the receipt stale");
    }

    #[test]
    fn receipt_lists_tests_from_the_declared_pytest_record_bound_to_the_journal() {
        let f = pytest_fixture();
        let req = route::plan(
            &f.index.routes[0],
            &f.index,
            &f.root,
            &f.paths.tracks(),
            Level::E2,
            Store::load(&approvals::path(&f.paths.root)).unwrap().approvals[0].terms.as_ref(),
            &CommandTable::load(),
        )
        .unwrap()
        .request;
        let command = ".venv/bin/python -m pytest tests/test_x.py -q";
        let record = f.root.join(PYTEST_NODEIDS);
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        let ids = |v: &[&str]| serde_json::to_vec(&v).unwrap();
        let before = pytest_nodeids(&f.root, &req, command).unwrap();
        assert!(before.is_empty(), "no record yet");
        std::fs::write(
            &record,
            ids(&[
                "tests/test_x.py::test_one",
                "tests/test_x.py::test_new[a-b]",
                "tests/other.py::t",
                "/etc/x.py::t",
                "bad id",
            ]),
        )
        .unwrap();
        let write_receipt = |sha: &str| {
            let mut outputs = BTreeMap::new();
            outputs
                .insert(PYTEST_NODEIDS.to_string(), crate::stage::FileState::File { sha256: sha.into(), exec: false });
            let r = crate::journal::Receipt {
                origin: None,
                id: String::new(),
                ts: "fixture".into(),
                session: "fixture-session".into(),
                repo: "fixture".into(),
                worktree: String::new(),
                route: "fixture/context/predict-check".into(),
                approved_on: "fixture-only".into(),
                level: "e2".into(),
                phase: crate::journal::Phase::Completed,
                outcome: crate::stage::Outcome::Applied { changed: vec![], discarded: 0 },
                input_digest: "x".into(),
                outputs,
            };
            std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
            adage_store::spool::append(&crate::stage::spool_path(&f.paths.root), &r).unwrap();
        };
        let event = Event::Batch { calls: vec![] };
        let turn = f.turn(&event);
        write_receipt("not-the-applied-output");
        assert!(
            test_evidence(&turn, &req, command, Some(""), Some(&before), &[]).is_none(),
            "unbound record is ignored"
        );
        write_receipt(&crate::sha256_hex(&std::fs::read(&record).unwrap()));
        let t = test_evidence(&turn, &req, command, Some(""), Some(&before), &[]).unwrap();
        assert_eq!(t.collected, Some(2), "only the command's targets, only well-formed ids");
        assert_eq!(t.new, vec!["tests/test_x.py::test_new[a-b]".to_string(), "tests/test_x.py::test_one".to_string()]);
        // With an earlier record, only the new ids are listed and no total is claimed.
        let earlier: std::collections::BTreeSet<String> = ["tests/test_x.py::test_one".to_string()].into();
        // The session's own new tests, by function name, are named instead (dev-v8: a fresh
        // home listed the first 12 of 69 ids, never the agent's own).
        let mine = test_evidence(&turn, &req, command, Some(""), Some(&before), &["test_one".to_string()]).unwrap();
        assert_eq!(
            (mine.collected, mine.new.clone(), mine.yours),
            (Some(2), vec!["tests/test_x.py::test_one".to_string()], true)
        );
        let param = test_evidence(&turn, &req, command, Some(""), Some(&before), &["test_new".to_string()]).unwrap();
        assert_eq!(param.new, vec!["tests/test_x.py::test_new[a-b]".to_string()], "parameters are the same test");
        let absent = test_evidence(&turn, &req, command, Some(""), Some(&before), &["test_gone".to_string()]).unwrap();
        assert!(!absent.yours && absent.new.len() == 2, "a name the record lacks falls back to the listing");
        assert_eq!(test_function("a.py::C::test_x[1-2]"), "test_x");
        let t = test_evidence(&turn, &req, command, Some(""), Some(&earlier), &[]).unwrap();
        assert_eq!((t.collected, t.new.len()), (None, 1));
        let text = render::action_predicted(
            &Directive {
                route: "predict-check".into(),
                track: "fixture/context".into(),
                version: "0.1.0".into(),
                strength: Strength::Fire,
                text: "x".into(),
                kind: "run".into(),
                origin: None,
            },
            command,
            &[],
            true,
            &render::CpuReceipt { tests: Some(&t), operation: "op", input_digest: "d", exit: Some(0), duration_ms: 5 },
        );
        assert!(text.contains("new since the last recorded run: tests/test_x.py::test_new[a-b]"), "{text}");
        assert!(text.contains("stays current until a file changes"), "{text}");
        assert!(!pytest_nodeids(&f.root, &req, "sh tools/check.sh").is_some(), "only pytest commands");
    }

    fn stop(f: &Fixture, active: bool) -> Option<StopGate> {
        stop_by(f, active, Instant::now() + std::time::Duration::from_secs(28))
    }

    fn stop_by(f: &Fixture, active: bool, deadline: Instant) -> Option<StopGate> {
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

    fn treated(f: &Fixture, arm: Arm) {
        adage_store::state::update::<SessionState, _>(&f.paths.state("fixture-session"), |s| s.arm = Some(arm))
            .unwrap();
    }

    #[test]
    fn stop_gate_needs_a_grant_a_treated_session_and_this_sessions_context_run() {
        let f = Fixture::new();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "stale");
        assert!(stop(&f, false).is_none(), "no grant: nothing runs");
        f.approve();
        treated(&f, Arm::Control);
        assert!(stop(&f, false).is_none(), "control arm: nothing runs");
        let g = Fixture::new();
        g.approve();
        treated(&g, Arm::Treated);
        assert!(stop(&g, false).is_none(), "the learned context never ran this session");
        record_receipt(&g, "another-session", "stale");
        assert!(stop(&g, false).is_none(), "another session's run does not arm this one");
        let digest = current_digest(&g);
        record_receipt(&g, "fixture-session", &digest);
        assert!(stop(&g, false).is_none(), "a passing run on the current inputs needs no rerun");
        assert_eq!(crate::journal::list(&g.paths.root).unwrap().len(), 2, "nothing ran");
    }

    #[test]
    fn stop_gate_runs_a_stale_check_and_blocks_once_on_failure() {
        if !crate::stage::backend(&sandbox::detect()).available() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        match stop(&f, false) {
            Some(StopGate::Notice(n)) => {
                assert!(n.contains("ran `sh tools/check.sh` at finish: passed (exit 0"), "{n}")
            }
            other => panic!("stale passing check: {other:?}"),
        }
        assert!(stop(&f, false).is_none(), "now current: no second run");
        // A later edit breaks the check: the first gate continues the agent, the next only reports.
        let f = Fixture::new();
        f.approve();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        std::fs::remove_file(f.root.join("src/feature.rs")).unwrap();
        match stop(&f, false) {
            Some(StopGate::Block(r)) => {
                assert!(r.contains("as you finished: it failed (exit 1)") && r.contains("runs it again"), "{r}")
            }
            other => panic!("failing check: {other:?}"),
        }
        match stop(&f, true) {
            Some(StopGate::Notice(n)) => assert!(n.contains("failed"), "{n}"),
            other => panic!("second gate never blocks: {other:?}"),
        }
        assert!(stop(&f, true).is_none(), "at most two gate runs per session");
        let runs = crate::journal::list(&f.paths.root).unwrap().len();
        assert_eq!(runs, 3, "the context run plus two gate runs, all journaled");
        // A first gate while another stop hook is already continuing reports instead of blocking.
        let g = Fixture::new();
        g.approve();
        treated(&g, Arm::Treated);
        record_receipt(&g, "fixture-session", "an-earlier-input-state");
        std::fs::remove_file(g.root.join("src/feature.rs")).unwrap();
        assert!(matches!(stop(&g, true), Some(StopGate::Notice(_))));
    }

    #[test]
    fn failed_or_refused_receipts_never_cover_a_call() {
        let f = pytest_fixture();
        let digest = current_digest(&f);
        for outcome in [
            crate::stage::Outcome::Failed { exit: Some(1), signal: None },
            crate::stage::Outcome::Refused { stage: "verify".into(), reason: "x".into(), counts: true },
        ] {
            let r = crate::journal::Receipt {
                origin: None,
                id: String::new(),
                ts: "fixture".into(),
                session: "fixture-session".into(),
                repo: "fixture".into(),
                worktree: String::new(),
                route: "fixture/context/predict-check".into(),
                approved_on: "fixture-only".into(),
                level: "e2".into(),
                phase: crate::journal::Phase::Completed,
                outcome,
                input_digest: digest.clone(),
                outputs: BTreeMap::new(),
            };
            std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
            adage_store::spool::append(&crate::stage::spool_path(&f.paths.root), &r).unwrap();
            let covered = PREFETCH.pretool_context(&PreCall {
                repo_key: "fixture",
                root: &f.root,
                cwd: &f.root,
                session: "fixture-session",
                paths: &f.paths,
                index: &f.index,
                command: ".venv/bin/python -m pytest tests/test_x.py -q",
            });
            assert!(covered.is_none());
        }
    }

    #[test]
    fn session_contract_requires_scoped_e2_verification_and_policy() {
        let f = Fixture::new();
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_none());
        f.approve();
        let text = PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).unwrap();
        assert!(text.contains("<adage-reflex") && text.contains("session-contract"));
        assert!(
            text.contains("Adage runs `sh tools/check.sh` (approved by a person, sandboxed) after edits to src/*.rs"),
            "{text}"
        );
        assert!(text.contains("again when you finish") && text.contains("a failure comes back to you"), "{text}");
        assert!(!text.contains("Use a current receipt") && text.len() < 500, "factual and short: {text}");
        assert!(PREFETCH.session_context("other-repository", &f.root, &f.index, &f.paths).is_none());
        std::fs::write(f.paths.root.join("policy.toml"), "[exec]\nmax_level = \"E1\"\n").unwrap();
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_none());
    }

    #[test]
    fn session_contract_ignores_lapsed_and_revoked_verification() {
        let f = Fixture::new();
        f.approve();
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_some());
        let store_path = approvals::path(&f.paths.root);
        let script = f.root.join("tools/check.sh");
        let original = std::fs::read(&script).unwrap();
        std::fs::write(&script, "#!/bin/sh\ntest -f src/other.rs\n").unwrap();
        let store = Store::load(&store_path).unwrap();
        let req = route::plan(
            &f.index.routes[0],
            &f.index,
            &f.root,
            &f.paths.tracks(),
            Level::E2,
            store.approvals[0].terms.as_ref(),
            &CommandTable::load(),
        )
        .unwrap()
        .request;
        let key = approvals::ApprovalKey::for_request(&req, "fixture");
        assert!(store.find(&key).is_none());
        assert_eq!(store.lapsed(&key, &store.approvals[0].route)[0].1[0].field, "route_exec_digest");
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_none(), "lapsed grant");
        std::fs::write(&script, original).unwrap();
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_some(), "matches again");
        let mut store = Store::load(&store_path).unwrap();
        assert_eq!(store.revoke("fixture", Some("fixture/context/predict-check")), 1);
        store.save(&store_path).unwrap();
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_none(), "revoked grant");
        f.approve();
        let mut store = Store::load(&store_path).unwrap();
        assert!(store.revoke_automatically("fixture", "fixture/context/predict-check", Level::E2, "refused"));
        store.save(&store_path).unwrap();
        assert!(PREFETCH.session_context("fixture", &f.root, &f.index, &f.paths).is_none(), "auto-revoked grant");
    }

    #[test]
    fn selected_context_uses_typed_command_across_related_paths_and_intervening_reads() {
        for path in ["src/feature.rs", "src/another.rs"] {
            let mut f = Fixture::new();
            let (event, mut dirs) = f.selected(path);
            assert_eq!(dirs.len(), 1);
            dirs[0].text = "untrusted prose suggesting another command".into();
            let turn = f.turn(&event);
            let c = candidates(&turn, &dirs);
            assert_eq!(c.len(), 1);
            assert_eq!(c[0].command, "sh tools/check.sh");
            assert!(c[0].context.is_some());
            assert!(candidates(&turn, &[]).is_empty(), "only actual selections nominate");
            f.index.routes[0].route.authored = true;
            assert!(candidates(&f.turn(&event), &dirs).is_empty(), "authored context is not learned");
            f.index.routes[0].route.authored = false;
            f.index.routes[0].route.evidence.execution = None;
            assert!(
                candidates(&f.turn(&event), &dirs).is_empty(),
                "advice without execution evidence is not nominated"
            );
        }
    }

    #[test]
    fn unapproved_context_does_not_execute_and_corrupt_journal_preserves_recovery() {
        let f = Fixture::new();
        let (event, dirs) = f.selected("src/feature.rs");
        let turn = f.turn(&event);
        let d = PREFETCH.deliver(&turn, &dirs);
        assert!(d.spool.is_none() && d.met.is_empty());
        assert!(d.texts.unwrap().iter().all(String::is_empty), "no unapproved fallback advice");
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty());
        std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
        std::fs::write(crate::stage::spool_path(&f.paths.root), b"corrupt journal").unwrap();
        let d = PREFETCH.deliver(&turn, &dirs);
        assert!(d.recovery.is_some(), "recovery precedes missing approval");
    }

    #[test]
    fn stale_context_grant_never_falls_back_to_agent_advice() {
        let f = Fixture::new();
        f.approve();
        std::fs::write(f.root.join("tools/check.sh"), "#!/bin/sh\necho changed\n").unwrap();
        let (event, dirs) = f.selected("src/feature.rs");
        let delivery = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(delivery.texts.unwrap().iter().all(String::is_empty));
        assert_eq!(delivery.cpu_predictions, vec!["predict-check"], "abstention is not ignored agent advice");
        assert!(delivery.spool.unwrap()[0].get("withheld").is_some());
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn prior_sessions_learn_a_context_that_executes_in_a_fresh_session() {
        if !crate::stage::backend(&sandbox::detect()).available() {
            return;
        }
        let mut f = Fixture::new();
        std::fs::write(f.root.join("Makefile"), "test:\n\t@test -f src/feature.rs\n").unwrap();
        let history = f._tmp.path().join("history");
        std::fs::create_dir(&history).unwrap();
        let mut transcripts = vec![];
        for i in 0..5 {
            let sid = format!("past-task-{i}");
            let mut lines = vec![
                json!({"type":"user","sessionId":sid,"cwd":f.root,"timestamp":"2026-09-20T10:00:00Z","message":{"role":"user","content":format!("Implement past feature {i}")}}),
            ];
            let calls = [
                ("Edit", json!({"file_path":f.root.join("src/feature.rs")})),
                ("Bash", json!({"command":if i % 2 == 0 { "git diff --stat" } else { "git diff --name-only" }})),
                ("Bash", json!({"command":"git status --short"})),
                ("Bash", json!({"command":"make test"})),
            ];
            for (n, (tool, input)) in calls.into_iter().enumerate() {
                let id = format!("tool-{n}");
                lines.push(json!({"type":"assistant","sessionId":sid,"cwd":f.root,"message":{"id":format!("turn-{n}"),"model":"fixture","content":[{"type":"tool_use","id":id,"name":tool,"input":input}]}}));
                lines.push(json!({"type":"user","sessionId":sid,"cwd":f.root,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,"content":"done","is_error":false}]}}));
            }
            let path = history.join(format!("{sid}.jsonl"));
            std::fs::write(&path, lines.iter().map(Value::to_string).collect::<Vec<_>>().join("\n")).unwrap();
            transcripts.push(path);
        }
        let inputs = adage_learn::Inputs {
            transcripts,
            outcomes: BTreeMap::new(),
            repo_override: Some("fixture".into()),
            models: None,
            cache: None,
            only_repo: None,
        };
        adage_learn::learn(&inputs, &f.paths.tracks(), &f.paths.index(), &Default::default()).unwrap();
        let learned = Index::load(&f.paths.index());
        let ir = learned
            .routes
            .iter()
            .find(|r| {
                matches!(r.route.trigger, Trigger::Context { .. })
                    && r.route.evidence.execution.is_some()
                    && matches!(&r.route.step, Step::Run { command } if command == "make test")
            })
            .expect("production learning must produce the context and certificate")
            .clone();
        assert_eq!(ir.route.evidence.execution.as_ref().unwrap().sessions, 5);
        assert!(!ir.route.authored);
        // Isolate the learned candidate under test without altering its trigger, command or evidence.
        f.index = Index { routes: vec![ir.clone()], ..learned };
        f.approve(); // A test-only grant in this temporary home, never a real user's approval.
        let table = CommandTable::load();
        let ctx = Ctx {
            checkout: &adage_runtime::checkout::Disk,
            index: &f.index,
            repo: "fixture".into(),
            root: &f.root,
            table: &table,
            embed: None,
            params: Default::default(),
            policy: adage_runtime::Policy {
                exec_only: vec![(ir.track.clone(), ir.route.id.clone())],
                ..Default::default()
            },
        };
        let mut state = SessionState { arm: Some(Arm::Treated), ..Default::default() };
        let mut last = None;
        {
            for (tool, input) in [
                ("Edit", json!({"file_path":f.root.join("src/feature.rs")})),
                ("Bash", json!({"command":"git diff --stat"})),
                ("Read", json!({"file_path":f.root.join("README.md")})),
                ("Bash", json!({"command":"git status --short"})),
            ] {
                let event = Event::Batch {
                    calls: vec![Call { tool: tool.into(), input, output: "done".into(), failed: false }],
                };
                let decision = adage_runtime::decide(&ctx, &mut state, &event);
                if !decision.directives.is_empty() {
                    last = Some((event, decision.directives));
                }
            }
        };
        let (event, dirs) = last.expect("fresh related sequence predicts the learned command");
        assert!(state.obligations.is_empty());
        let delivery = PREFETCH.deliver(&f.turn(&event), &dirs);
        let records = delivery.spool.unwrap();
        assert_eq!(records[0]["outcome"], "applied", "{records}");
        assert_eq!(delivery.cpu_predictions, vec![ir.route.id]);
        assert!(delivery.texts.unwrap().join("\n").contains("after your edits: passed (exit 0"));
        assert_eq!(std::fs::read_to_string(f.root.join("src/feature.rs")).unwrap(), "fixture\n");
    }

    #[cfg(unix)]
    #[test]
    fn approved_context_runs_on_cpu_and_duplicate_never_stages_again() {
        if !crate::stage::backend(&sandbox::detect()).available() {
            return;
        }
        let f = Fixture::new();
        f.approve();
        let (event, dirs) = f.selected("src/feature.rs");
        let mut turn = f.turn(&event);
        turn.arm = Some(Arm::Control);
        assert!(PREFETCH.deliver(&turn, &dirs).spool.is_none());
        turn.arm = Some(Arm::Treated);
        let d = PREFETCH.deliver(&turn, &dirs);
        let records = d.spool.as_ref().unwrap();
        assert_eq!(records[0]["outcome"], "applied", "{records}");
        assert_eq!(records[0]["context"]["on"], "context");
        assert!(d.met.is_empty(), "no invented obligation completion");
        let text = d.texts.unwrap().join("\n");
        assert!(text.contains("after your edits: passed (exit 0") && text.contains("exec=\"e2\""), "{text}");
        let receipt = crate::journal::list(&f.paths.root).unwrap().pop().unwrap();
        assert!(text.contains(&receipt.id[..12]) && !text.contains(&receipt.input_digest), "{text}");
        assert_eq!(receipt.origin, Some(crate::journal::RunOrigin::Context), "the journal says what started it");
        assert!(text.contains("stays current until a file changes"), "{text}");
        assert!(!text.contains("COMPLETED CPU ACTION") && !text.contains("not requested by the user"), "{text}");
        assert!(!text.contains("after your edit to"));
        let repeat = PREFETCH.deliver(&turn, &dirs);
        assert_eq!(repeat.spool.unwrap()[0]["withheld"], "duplicate");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1);
    }

    fn staging() -> bool {
        crate::stage::backend(&sandbox::detect()).available()
    }

    /// The fixture as the hook pipeline serves it (`adage_hook::handle` with this extension): a
    /// learned finish-line obligation beside the learned verification (after an edit to
    /// `registry/*.csv`, run `make gen`; no grant for it), and no learner.
    fn with_obligation(f: &mut Fixture) {
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
    fn hook(f: &Fixture, event: &str, extra: Value) -> Option<Value> {
        let mut p = json!({ "session_id": "fixture-session", "cwd": f.root });
        for (k, v) in extra.as_object().unwrap() {
            p[k] = v.clone();
        }
        let opts = adage_hook::Options { learner: None, extension: Some(&PREFETCH) };
        let out = adage_hook::handle("claude", event, &p.to_string(), opts, &f.paths, Instant::now()).unwrap();
        out.map(|o| serde_json::from_str(&o).unwrap())
    }

    /// Review F1, through the hook pipeline: the session armed `make gen` and its learned check
    /// ran earlier. At the first Stop the check fails and the obligation is due: both reach the
    /// agent in one continuation, recognisable as Adage's own text. The agent fixes the failure;
    /// at the next Stop the check passes, and the obligation was requested (once).
    #[test]
    fn a_failing_finish_check_keeps_the_learned_obligation_request() {
        if !staging() {
            return;
        }
        let mut f = Fixture::new();
        f.approve();
        with_obligation(&mut f);
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        std::fs::remove_file(f.root.join("src/feature.rs")).unwrap();
        let edit = json!({ "tool_name": "Edit", "tool_use_id": "e1", "tool_response": "ok",
                           "tool_input": { "file_path": f.root.join("registry/currencies.csv") } });
        let armed = hook(&f, "PostToolBatch", json!({ "tool_calls": [edit] })).expect("the obligation's directive");
        assert!(armed.to_string().contains("make gen"), "{armed}");
        let first = hook(&f, "Stop", json!({ "stop_hook_active": false })).expect("a continuation");
        assert_eq!(first["decision"], "block");
        let reason = first["reason"].as_str().unwrap();
        assert!(
            reason.contains("Adage ran `sh tools/check.sh` (person-approved) as you finished: it failed (exit 1)"),
            "{reason}"
        );
        assert!(reason.contains("requires running `make gen` after such changes"), "the obligation too: {reason}");
        assert!(adage_hook::harness::is_own_text(reason), "{reason}");
        std::fs::write(f.root.join("src/feature.rs"), "fixture\n").unwrap();
        let second = hook(&f, "Stop", json!({ "stop_hook_active": true })).expect("the person's line");
        let line = second["systemMessage"].as_str().unwrap();
        assert!(line.contains("ran `sh tools/check.sh` at finish: passed (exit 0"), "{second}");
        let st: SessionState = adage_store::state::try_read(&f.paths.state("fixture-session")).unwrap().unwrap();
        let o = st.obligations.iter().find(|o| o.route == "regenerate-registry").unwrap();
        assert!(o.stop_requested && st.stop_requests == 1, "{st:?}");
        assert_eq!(st.stop_gates, 2);
        let origins: Vec<_> = crate::journal::list(&f.paths.root).unwrap().iter().map(|r| r.origin).collect();
        // dev-v9: a gate run's pass must never be read as the context run's (it made a failed
        // control look unrealized), so each receipt records which one it was.
        use crate::journal::RunOrigin::{Context, StopGate};
        assert_eq!(origins, [Some(Context), Some(StopGate), Some(StopGate)], "the context run and two gate runs");
    }

    /// Review F4: the gate starts a run only when, staged and applied, it ends before the hook's
    /// deadline (the registered timeout less what the event already spent). Otherwise nothing is
    /// staged, journaled or counted, and it waits for the next Stop.
    #[test]
    fn stop_gate_never_starts_a_run_it_cannot_finish_in_time() {
        let f = Fixture::new();
        f.approve();
        treated(&f, Arm::Treated);
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        let soon = Instant::now() + std::time::Duration::from_millis(limits::E2_MAX_WALL_MS + GATE_OVERHEAD_MS - 1_000);
        assert!(stop_by(&f, false, soon).is_none(), "20 s of wall time and the overhead do not fit");
        assert!(stop_by(&f, false, Instant::now()).is_none(), "nothing left at all");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1, "nothing staged or journaled");
        let st: SessionState = adage_store::state::try_read(&f.paths.state("fixture-session")).unwrap().unwrap();
        assert_eq!(st.stop_gates, 0, "a run that never started is not counted");
        // The whole budget of a hook that has just started: it runs.
        if staging() {
            assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))));
        }
    }

    /// Review F4: a hook killed between the journal's prepared and completed receipts (the only
    /// window in which a kill matters) leaves its worktree in recovery. The next session runs
    /// nothing there, gate included, and tells the agent not to retry; once a person has
    /// inspected and acknowledged the operation, actions run again.
    #[test]
    fn an_interrupted_run_pauses_the_worktree_until_a_person_acknowledges_it() {
        let f = Fixture::new();
        f.approve();
        let id = "0123456789abcdef".repeat(4);
        let prepared = crate::journal::Receipt {
            origin: None,
            id: id.clone(),
            ts: "fixture".into(),
            session: "the-killed-session".into(),
            repo: "fixture".into(),
            worktree: f.root.canonicalize().unwrap().to_string_lossy().into_owned(),
            route: "fixture/context/predict-check".into(),
            approved_on: "fixture-only".into(),
            level: "e2".into(),
            phase: crate::journal::Phase::Prepared,
            outcome: crate::stage::Outcome::Unknown { reason: "prepared without a durable completion".into() },
            input_digest: "d".into(),
            outputs: BTreeMap::new(),
        };
        std::fs::create_dir_all(f.paths.root.join("exec")).unwrap();
        adage_store::spool::append(&crate::stage::spool_path(&f.paths.root), &prepared).unwrap();
        // The next session: its learned context is selected, and its check ran earlier.
        let (event, dirs) = f.selected("src/feature.rs");
        record_receipt(&f, "fixture-session", "an-earlier-input-state");
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        let warning = d.recovery.expect("recovery comes first");
        assert!(warning.contains(&id) && warning.contains("do not retry automatically"), "{warning}");
        assert!(stop(&f, false).is_none(), "no gate run while the worktree is in recovery");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 2, "nothing ran");
        // A person inspects the worktree and acknowledges it (`adage-exec recover --id <id> --acknowledge`).
        crate::journal::acknowledge(&f.paths.root, &id).unwrap();
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(d.recovery.is_none(), "recovered: no warning");
        if staging() {
            assert!(matches!(stop(&f, false), Some(StopGate::Notice(_))), "the finish-line check runs again");
            assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 3);
        }
    }

    /// A failed staged pytest run whose record the plan carries (`after`), from the worktree's
    /// record (`before`), for [`failure_record`].
    fn failed_run(f: &Fixture, before: Option<&str>, after: Option<&str>, rewritten: bool) -> crate::stage::Staged {
        let req = route::plan(
            &f.index.routes[0],
            &f.index,
            &f.root,
            &f.paths.tracks(),
            Level::E2,
            Store::load(&approvals::path(&f.paths.root)).unwrap().approvals[0].terms.as_ref(),
            &CommandTable::load(),
        )
        .unwrap()
        .request;
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

    /// Review F2: pytest's `lastfailed` is cumulative, so names come only from what this run
    /// changed in it, within the command's targets: new entries failed now, file ids are
    /// collection errors (their files' node ids say nothing), kept entries were not cleared, and
    /// another file's earlier failure is not this run's. A record this run did not rewrite gives
    /// no names at all.
    #[test]
    fn failing_tests_are_only_what_this_run_recorded_within_its_targets() {
        let f = pytest_fixture();
        let command = ".venv/bin/python -m pytest tests/test_b.py tests/test_c.py -q";
        let before = r#"{"tests/test_a.py::test_other": true, "tests/test_b.py::test_old": true, "tests/test_c.py::test_stale": true}"#;
        let after = r#"{"tests/test_a.py::test_other": true, "tests/test_b.py::test_old": true, "tests/test_b.py::test_new": true, "tests/test_c.py": true, "tests/test_c.py::test_stale": true, "/etc/x.py::t": true}"#;
        let r = failure_record(&failed_run(&f, Some(before), Some(after), false), command, &[]).unwrap();
        assert!(r.refreshed);
        assert_eq!(r.failing.names, ["tests/test_b.py::test_new"]);
        assert_eq!(r.collection.names, ["tests/test_c.py"]);
        assert_eq!(r.uncleared.names, ["tests/test_b.py::test_old"]);
        let text = render::stop_gate_failed(command, Some(1), Some(&r));
        assert!(text.contains("Failing tests: tests/test_b.py::test_new."), "{text}");
        assert!(text.contains("Collection errors (the file could not be imported) in: tests/test_c.py."), "{text}");
        assert!(!text.contains("test_other") && !text.contains("test_stale"), "{text}");
        // The agent already saw these failures: the run left the record byte-identical, so pytest
        // did not rewrite it. Nothing can be attributed to this run.
        let r = failure_record(&failed_run(&f, Some(before), None, false), command, &[]).unwrap();
        assert!(!r.refreshed && r.failing.is_empty() && r.uncleared.is_empty());
        let text = render::stop_gate_failed(command, Some(1), Some(&r));
        assert!(
            text.contains("not updated by this run, so no failing test names") && !text.contains("test_old"),
            "{text}"
        );
        // Rewritten with the same bytes (refreshed by this run): what it kept, it did not clear.
        let r = failure_record(&failed_run(&f, Some(before), None, true), command, &[]).unwrap();
        assert!(r.refreshed && r.failing.is_empty());
        assert_eq!(r.uncleared.names, ["tests/test_b.py::test_old", "tests/test_c.py::test_stale"]);
        // No earlier record: everything in it is this run's.
        let r =
            failure_record(&failed_run(&f, None, Some(r#"{"tests/test_b.py::test_new": true}"#), false), command, &[])
                .unwrap();
        assert_eq!(r.failing.names, ["tests/test_b.py::test_new"]);
        // Names are bounded across the lists.
        let many: serde_json::Map<String, Value> =
            (0..20).map(|i| (format!("tests/test_b.py::test_{i:02}"), json!(true))).collect();
        let r =
            failure_record(&failed_run(&f, None, Some(&Value::Object(many).to_string()), false), command, &[]).unwrap();
        assert_eq!((r.failing.names.len(), r.failing.more), (MAX_LISTED_TESTS, 20 - MAX_LISTED_TESTS));
        // Not a pytest run: nothing is said about test names.
        assert!(failure_record(&failed_run(&f, Some(before), Some(after), false), "sh tools/check.sh", &[]).is_none());
    }

    /// The held-out b08 shape (launch-gate-v8): a session's own parametrized test that declares a
    /// valid (latitude, longitude) pair invalid. The run's record names only the case's id.
    const B08_TESTS: &str = r#"import pytest

from marshmallow import validate
from marshmallow.exceptions import ValidationError


def test_and():
    validator = validate.And(validate.Range(min=0))
    assert validator(2)


@pytest.mark.parametrize(
    "valid_latlong",
    [
        (0, 0),
        (45, 90),
        (-45, -90),
        (90, 180),
        (-90, -180),
        (0.5, 0.5),
        (-45.5, 135.5),
        (45.0, -135.0),
    ],
)
def test_latlong_valid(valid_latlong):
    validator = validate.LatLong()
    assert validator(valid_latlong) == valid_latlong


@pytest.mark.parametrize(
    "invalid_latlong",
    [
        (91, 0),
        (-91, 0),
        (0, 181),
        (0, -181),
        (90.1, 0),
        (-90.1, 0),
        (0, 180.1),
        (0, -180.1),
        (100, 200),
        (45,),
        (45, 90, 45),
        [],
        [45, 90],
        "45, 90",
        45,
        None,
        (None, 0),
        (0, None),
        ("invalid", 0),
        (0, "invalid"),
    ],
)
def test_latlong_invalid(invalid_latlong):
    validator = validate.LatLong()
    with pytest.raises(ValidationError):
        validator(invalid_latlong)
"#;

    fn b08_directive() -> Directive {
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

    /// A failed run's receipt and the finish-line continuation describe a failing test beyond its
    /// id, from the test source the run used (never the run's output): the parametrized case with
    /// its value, and the test's one check. A test this session added is said to be the agent's
    /// own; one it did not add is not. A source that changed since the run is not described.
    #[test]
    fn a_failing_case_is_described_from_the_source_the_run_used() {
        let f = pytest_fixture();
        std::fs::write(f.root.join("tests/test_validate.py"), B08_TESTS).unwrap();
        let command = ".venv/bin/python -m pytest tests/test_validate.py -q";
        let id = "tests/test_validate.py::test_latlong_invalid[invalid_latlong12]";
        let mut staged = failed_run(&f, None, Some(&json!({ id: true }).to_string()), false);
        staged.inputs = crate::stage::input_snapshot(&f.root.canonicalize().unwrap(), &staged.action).unwrap();
        let added = ["test_latlong_valid".to_string(), "test_latlong_invalid".to_string()];
        let r = failure_record(&staged, command, &added).unwrap();
        assert_eq!(r.failing.names, [id]);
        let case = &r.cases[0];
        assert!(case.added, "{case:?}");
        let facts = case.facts.as_ref().expect("the source the run used is read");
        let line = |needle: &str| B08_TESTS.lines().position(|l| l.contains(needle)).unwrap() + 1;
        assert_eq!(facts.case, Some((line("        [45, 90],"), Some("invalid_latlong = [45, 90]".into()))));
        assert_eq!(facts.check, Some((line("pytest.raises"), "with pytest.raises(ValidationError):".into())));

        let detail = format!(
            "From the test source the run used (pytest's own failure messages are not passed on):\n\
             test_latlong_invalid[invalid_latlong12] is the case invalid_latlong = [45, 90] (tests/test_validate.py line {}); \
             the test's only check is line {}: with pytest.raises(ValidationError):\n\
             test_latlong_invalid[invalid_latlong12] was added in this session; check it against the request before changing the implementation.",
            line("        [45, 90],"),
            line("pytest.raises")
        );
        let receipt = render::action_failed(&b08_directive(), command, Some(1), Some(&r), "959cb36ea93c0123");
        let expected = format!(
            "Adage ran `{command}` after your edits: it failed (exit 1; receipt 959cb36ea93c). Nothing was changed.\n\
             Failing tests: {id}.\n{detail}\n</adage-reflex>"
        );
        assert!(receipt.ends_with(&expected), "{receipt}");
        assert!(receipt.len() < 1200, "{}", receipt.len());
        assert!(render::named(&receipt).commands == [command], "only the command reads as named: {receipt}");
        // The finish-line continuation says the same, in one paragraph.
        let failed = crate::stage::Outcome::Failed { exit: Some(1), signal: None };
        let StopGate::Block(gate) = gate_answer(&failed, command, &staged, &added, true, false) else { panic!() };
        assert!(gate.starts_with(render::FINISH_LINE) && gate.ends_with(render::NOT_FROM_USER), "{gate}");
        assert!(gate.contains(&format!("Failing tests: {id}. {}", detail.replace('\n', " "))), "{gate}");
        assert!(adage_hook::harness::is_own_text(&gate), "{gate}");
        assert_eq!(render::named(&gate).commands, [command], "{gate}");

        // A failing test this session did not add: described, not flagged.
        let r = failure_record(&staged, command, &["test_other".to_string()]).unwrap();
        assert!(!r.cases[0].added && r.cases[0].facts.is_some());
        let text = render::action_failed(&b08_directive(), command, Some(1), Some(&r), "959cb36ea93c0123");
        assert!(
            text.contains("is the case invalid_latlong = [45, 90]") && !text.contains("added in this session"),
            "{text}"
        );

        // The file changed after the run: its text is not the one that ran, so nothing is read.
        std::fs::write(f.root.join("tests/test_validate.py"), format!("{B08_TESTS}\n# edited\n")).unwrap();
        let r = failure_record(&staged, command, &added).unwrap();
        assert!(r.cases[0].facts.is_none() && r.cases[0].added, "{:?}", r.cases);
        let text = render::stop_gate_failed(command, Some(1), Some(&r));
        assert!(!text.contains("From the test source") && text.contains("was added in this session"), "{text}");
    }

    /// Review F8: a gate run whose completion cannot be confirmed tells the agent, like an action,
    /// not to retry it (once, never while another Stop hook is continuing).
    #[test]
    fn an_unconfirmed_gate_run_tells_the_agent_not_to_retry() {
        let f = pytest_fixture();
        let staged = failed_run(&f, None, None, false);
        let unknown = crate::stage::Outcome::Unknown { reason: "the completion could not be recorded".into() };
        match gate_answer(&unknown, "make test", &staged, &[], false, false) {
            StopGate::Block(r) => {
                assert!(r.contains("Do not retry it automatically") && r.contains("acknowledge recovery"), "{r}");
                assert!(adage_hook::harness::is_own_text(&r), "{r}");
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(gate_answer(&unknown, "make test", &staged, &[], false, true), StopGate::Notice(_)));
    }

    /// Review F5: a receipt never covers a call whose command line changes what pytest sees
    /// (another directory, an exported or assigned variable, any command that is not passive), or
    /// a call made from a subdirectory.
    #[test]
    fn a_receipt_never_covers_a_call_from_elsewhere() {
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
            PREFETCH.pretool_context(&PreCall {
                repo_key: "fixture",
                root: &f.root,
                cwd,
                session: "fixture-session",
                paths: &f.paths,
                index: &f.index,
                command: ".venv/bin/python -m pytest tests/test_x.py -q",
            })
        };
        assert!(call(&f.root).is_some(), "at the root it is covered");
        assert!(call(&f.root.join("tests")).is_none(), "from a subdirectory the same words name other files");
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
        let texts = || PREFETCH.session_texts("fixture", &f.root, &f.index, &f.paths);
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

    /// A lapsed grant at the moment its action would run: withheld as before (no agent advice),
    /// and now one line for the person with the field that changed and how to approve it again.
    #[test]
    fn a_lapsed_grant_is_told_to_the_person_when_its_action_is_withheld() {
        let f = Fixture::new();
        f.approve();
        let (event, dirs) = f.selected("src/feature.rs");
        std::fs::write(f.root.join("tools/check.sh"), "#!/bin/sh\ntest -f src/other.rs\n").unwrap();
        let d = PREFETCH.deliver(&f.turn(&event), &dirs);
        assert!(d.texts.unwrap().iter().all(String::is_empty), "no fallback advice");
        assert_eq!(d.spool.unwrap()[0]["withheld"], "approval");
        let notice = d.notice.expect("the person is told");
        assert!(notice.contains("fixture/context/predict-check") && notice.contains("route_exec_digest"), "{notice}");
        assert!(
            notice.contains("`adage review delegate fixture/context predict-check --level e2 --effect verify`"),
            "{notice}"
        );
        assert!(crate::journal::list(&f.paths.root).unwrap().is_empty(), "nothing ran");
    }

    /// Review F6: the receipt check's repository hash stops at its deadline instead of holding up
    /// the PreToolUse hook; the note is then skipped.
    #[test]
    fn the_input_snapshot_stops_at_its_deadline() {
        let f = pytest_fixture();
        let req = route::plan(
            &f.index.routes[0],
            &f.index,
            &f.root,
            &f.paths.tracks(),
            Level::E2,
            Store::load(&approvals::path(&f.paths.root)).unwrap().approvals[0].terms.as_ref(),
            &CommandTable::load(),
        )
        .unwrap()
        .request;
        let action = req.action.as_ref().unwrap();
        let late = crate::stage::input_snapshot_until(&f.root, action, Some(Instant::now())).unwrap_err();
        assert_eq!(late.stage, "time");
        let later = Instant::now() + std::time::Duration::from_secs(60);
        assert_eq!(
            crate::stage::input_snapshot_until(&f.root, action, Some(later)).unwrap(),
            crate::stage::input_snapshot(&f.root, action).unwrap()
        );
    }
}
