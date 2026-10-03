//! E1: each delivered directive whose route holds an E1 grant here is run before delivery, and its
//! data block, re-checked by the safety oracle, goes beside the adapted directive ([`prefetch`]).

use adage_core::CommandTable;
use adage_hook::Turn;
use adage_runtime::{render, Directive};
use adage_safety::{check_data_block, DataType};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

use super::grants::{key_lapse_notice, unplanned_notice};
use super::{executes_at, label, run_context};
use crate::approvals::{self, Approval, Store};
use crate::request::{FieldSpec, FieldType, Level, OutputMode, DEFAULT_LIST, DEFAULT_TEXT, MAX_LIST, MAX_TEXT};
use crate::{limits, policy, route, run, sandbox};

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

/// Every directive's text, with its block where a prefetch ran, and the spool records.
type Prefetched = (Vec<String>, Vec<Value>);

/// `Ok(None)`: nothing approved here (E0, nothing recorded). `Ok(Some)`: every approved directive
/// delivered with its block. `Err`: something failed; E0 is delivered and the records say why.
pub(super) fn prefetch(
    turn: &Turn<'_>,
    directives: &[Directive],
    notice: &mut Option<String>,
) -> Result<Option<Prefetched>, Vec<Value>> {
    let home = &turn.paths.root;
    let store = match Store::load(&approvals::path(home)) {
        Ok(s) => s,
        Err(e) => return Err(vec![withheld("*", "approvals", &format!("the approvals file is unreadable: {e:#}"))]),
    };
    let grant = |d: &Directive| -> Option<&Approval> {
        store
            .approvals
            .iter()
            .find(|a| a.route == label(&d.track, &d.route) && executes_at(a, turn.repo_key, Level::E1))
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
    let ctx = run_context(&pol, &store, &cap, &table, turn.repo_key, home);
    let tracks = turn.paths.tracks();
    let mut texts = Vec::new();
    for d in directives {
        let Some(a) = grant(d) else {
            texts.push(render::wrap_directive(d));
            continue;
        };
        let name = label(&d.track, &d.route);
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
                *notice = Some(unplanned_notice(a, &e));
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
            *notice = Some(key_lapse_notice(a, &key));
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
