//! `adage-exec hook <harness> <event>`: E1 prefetch and E2 actions (Track C §3.2, §3.4, §3.6).
//!
//! The same pipeline as `adage hook` (`adage-hook`: adapters, fast path, state lock, `decide()`,
//! spool, render), with one [`Extension`]. For each directive about to be delivered whose route
//! has an E1 approval for this repository, it derives the route's request ([`crate::route`]),
//! reads the bound values back from the directive, and runs it through [`crate::run::execute`]
//! (every gate: approval key equality, including the L-ref hash read again now; policy; a live
//! sandbox; limits; typed projection). Each block is re-checked by the safety oracle
//! (`adage_eval::safety::check_data_block`), the directive adapts ("That lookup has been run; its
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
use adage_eval::safety::{check_data_block, DataType};
use adage_hook::{Delivery, Extension, Turn};
use adage_runtime::{render, Directive};
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
    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery {
        let mut d = Delivery::default();
        let mut records: Vec<Value> = Vec::new();
        if !directives.is_empty() {
            match prefetch(turn, directives) {
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
        }
        if !records.is_empty() {
            d.spool = Some(Value::Array(records));
        }
        d
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
    if turn.harness == "opencode" {
        not_run(&mut out, "harness", "actions do not run under OpenCode, whose hook bridge waits one second");
        return out;
    }
    let table = CommandTable::load();
    let plan = match route::plan(ir, turn.index, turn.root, &turn.paths.tracks(), Level::E2, a.terms.as_ref(), &table) {
        Ok(p) => p,
        Err(e) => {
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
                let text = if s.context.is_some() {
                    render::action_predicted(&dir, s.command, &paths, ir.route.effect == adage_core::Effect::Verify)
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
fn prefetch(turn: &Turn<'_>, directives: &[Directive]) -> Result<Option<(Vec<String>, Vec<Value>)>, Vec<Value>> {
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
            Err(e) => return Err(fail(records, "route", &e)),
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
            let root = tmp.path().canonicalize().unwrap().join("repo");
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
                harness: "claude",
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
                index: &self.index,
                repo: "fixture".into(),
                root: &self.root,
                table: &table,
                embed: None,
                params: Default::default(),
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
            adage_runtime::with_exec_only(vec![("fixture/context".into(), "predict-check".into())], || {
                for ev in &events {
                    dirs = adage_runtime::decide(&ctx, &mut state, ev).directives;
                }
                assert!(
                    adage_runtime::decide(&ctx, &mut state, &events[2]).directives.is_empty(),
                    "repeat is not selected"
                );
            });
            assert!(state.obligations.is_empty(), "prediction creates no edit obligation");
            adage_store::state::update::<SessionState, _>(&self.paths.state("fixture-session"), |s| *s = state)
                .unwrap();
            (events[2].clone(), dirs)
        }
        fn approve(&self) {
            let terms = route::Terms { effect: Some(Effect::Verify), ..Default::default() };
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
            index: &f.index,
            repo: "fixture".into(),
            root: &f.root,
            table: &table,
            embed: None,
            params: Default::default(),
        };
        let mut state = SessionState { arm: Some(Arm::Treated), ..Default::default() };
        let mut last = None;
        adage_runtime::with_exec_only(vec![(ir.track.clone(), ir.route.id.clone())], || {
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
        });
        let (event, dirs) = last.expect("fresh related sequence predicts the learned command");
        assert!(state.obligations.is_empty());
        let delivery = PREFETCH.deliver(&f.turn(&event), &dirs);
        let records = delivery.spool.unwrap();
        assert_eq!(records[0]["outcome"], "applied", "{records}");
        assert_eq!(delivery.cpu_predictions, vec![ir.route.id]);
        assert!(delivery.texts.unwrap().join("\n").contains("verification succeeded"));
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
        assert!(text.contains("verification succeeded") && text.contains("exec=\"e2\""), "{text}");
        assert!(!text.contains("after your edit"));
        let repeat = PREFETCH.deliver(&turn, &dirs);
        assert_eq!(repeat.spool.unwrap()[0]["withheld"], "duplicate");
        assert_eq!(crate::journal::list(&f.paths.root).unwrap().len(), 1);
    }
}
