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
//! approval for this repository, the action runs here ([`act`]): at most one per event, never in
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
use adage_tracks::Step;
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
        if !turn.stimuli.is_empty() {
            let a = act(turn, directives);
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

/// E2: run at most one approved action for the obligations this event armed or enforces.
fn act(turn: &Turn<'_>, directives: &[Directive]) -> Acted {
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
    let approved = |s: &adage_hook::Stimulus| -> Option<(&Approval, &adage_tracks::index::IndexedRoute)> {
        store.approvals.iter().filter(|a| a.key.repo == turn.repo_key && a.key.level == Level::E2).find_map(|a| {
            let ir = turn.index.routes.iter().find(|r| {
                r.repo == turn.repo_key && r.route.id == s.route && a.route == format!("{}/{}", r.track, r.route.id)
            })?;
            Some((a, ir))
        })
    };
    // At most one action per event.
    let Some((s, a, ir)) = turn.stimuli.iter().find_map(|s| approved(s).map(|(a, ir)| (s, a, ir))) else { return out };
    let label = a.route.clone();
    let Some(dir) = directive_for(ir, directives, &s.command) else {
        out.records.push(json!({ "route": label, "level": "e2", "withheld": "render",
                                 "reason": "the action's directive could not be rendered" }));
        return out;
    };
    let not_run = |out: &mut Acted, stage: &str, reason: &str| {
        out.records
            .push(json!({ "route": label, "level": "e2", "withheld": stage, "reason": crate::reason_text(reason) }));
        if s.at_stop && ir.exec_only() {
            // The agent cannot run a vendored script itself: asking it to would be unfollowable.
            out.stop = Some(None);
        } else if s.at_stop {
            out.stop = Some(Some(render::stop_reason_not_run(&s.paths, &s.command, reason)));
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
    match route::bound_from_command(&req.argv, &s.command) {
        Some(inputs) => req.inputs = inputs,
        None => {
            not_run(&mut out, "bind", "its command could not be bound from the edit");
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
            not_run(&mut out, "state", "the obligation occurrence cannot be read safely");
            return out;
        }
    };
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
            out.met.push(s.route.clone());
            if s.at_stop {
                // The Stop hook cannot carry context: the request is withdrawn, the record kept.
                out.stop = Some(None);
            } else {
                let paths: Vec<String> = changed.iter().map(|l| l.path.clone()).collect();
                let text = render::action_applied(&dir, &s.edited, &s.command, &paths);
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
