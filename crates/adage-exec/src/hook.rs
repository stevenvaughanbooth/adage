//! `adage-exec hook <harness> <event>`: E1 prefetch (Track C §3.2, §3.4).
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
//! **On any failure** (no sandbox, a lapsed approval, a timeout, output that fails its type, an
//! oracle violation, a panic, text past the harness's cap) the output is exactly the E0 output.
//! With nothing approved, nothing is loaded beyond the approvals file, and the output equals
//! `adage hook`'s byte for byte.
//!
//! The spool record gains `exec`: per attempted route, `{ route, level, exit, bytes, sha256,
//! truncated, duration_ms, auto }` when delivered, `{ route, level, withheld }` when not. The
//! output itself is never recorded.

use adage_core::CommandTable;
use adage_eval::safety::{check_data_block, DataType};
use adage_hook::{Delivery, Extension, Turn};
use adage_runtime::{render, Directive};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::time::Instant;

use crate::approvals::{self, Approval, Store};
use crate::request::{FieldSpec, FieldType, Level, OutputMode, DEFAULT_LIST, MAX_LIST};
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
        match prefetch(turn, directives) {
            Ok(None) => Delivery::default(),
            Ok(Some((texts, records))) => Delivery { texts: Some(texts), spool: Some(Value::Array(records)) },
            Err(records) => Delivery { texts: None, spool: Some(Value::Array(records)) },
        }
    }
}

fn withheld(route: &str, stage: &str) -> Value {
    json!({ "route": route, "level": "e1", "withheld": stage })
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
        Err(_) => return Err(vec![withheld("*", "approvals")]),
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
    let pol = policy::load(home).map_err(|_| vec![withheld("*", "policy")])?;
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
        let fail = |mut records: Vec<Value>, stage: &str| {
            records.push(withheld(&name, stage));
            records
        };
        let Some(ir) = route::find(turn.index, turn.repo_key, &d.track, &d.route) else {
            return Err(fail(records, "route"));
        };
        let plan = match route::plan(ir, turn.index, turn.root, &tracks, Level::E1, a.terms.as_ref(), &table) {
            Ok(p) => p,
            Err(_) => return Err(fail(records, "route")),
        };
        let mut req = plan.request;
        match route::bound_inputs(&req.argv, &d.text) {
            Some(inputs) => req.inputs = inputs,
            None => return Err(fail(records, "bind")),
        }
        // All E1 runs of one event together stay within E1's longest wall timeout.
        let left = limits::E1_MAX_WALL_MS.saturating_sub(started.elapsed().as_millis() as u64);
        if left < req.profile.limits.wall_ms {
            return Err(fail(records, "limits"));
        }
        let delivered = match run::execute(&req, &ctx) {
            Ok(x) => x,
            Err(r) => return Err(fail(records, r.stage)),
        };
        let fields: Vec<(String, DataType)> =
            req.output.fields.iter().map(|(k, f)| (k.clone(), data_type(f))).collect();
        let decl = (req.output.mode == OutputMode::Typed).then_some(fields.as_slice());
        if !check_data_block(&d.route, &delivered.block, decl, Some(turn.root), &ctx.canaries).is_empty() {
            return Err(fail(records, "oracle"));
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
        let Some(text) = render::wrap_directive_e1(d, &delivered.block, note.as_deref()) else {
            return Err(fail(records, "render"));
        };
        texts.push(text);
        let mut rec = serde_json::to_value(&delivered.record).unwrap_or(Value::Null);
        rec["route"] = json!(name);
        records.push(rec);
    }
    if texts.join("\n\n").len() > turn.max_context {
        return Err({
            records.push(withheld("*", "size"));
            records
        });
    }
    Ok(Some((texts, records)))
}
