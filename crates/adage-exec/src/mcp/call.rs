//! One `tools/call` (Track C §3.5): the arguments checked against the tool's contracts and
//! `plain_token`; the route planned again from the index **now** (so the approval key, L-ref
//! script hashes included, is recomputed at call time and compared by `run::execute` with the
//! approvals file read again now); a run slot; the sandboxed run at E2's limits (10 s, 256 KiB
//! captured, 16 KiB returned); the typed projection; the safety oracle over the block; and the
//! result in the same `<adage-data>` envelope as E1, with the projected fields as
//! `structuredContent`.
//!
//! Any failure is a tool error (`isError: true`) naming the stage and a reason that never
//! contains output text. Each call appends one record to `$ADAGE_HOME/exec/mcp.spool`: the run's
//! `{ level, exit, bytes, sha256, truncated, duration_ms, auto }`, or the stage that withheld it;
//! never the output, never the argument values.

use adage_core::CommandTable;
use adage_eval::safety::check_data_block;
use adage_store::Paths;
use adage_tracks::Index;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;

use super::catalog::{self, Tool};
use super::limit;
use super::schema::{InputKind, MAX_INPUT};
use crate::approvals::{self, Store};
use crate::request::{plain_token, OutputMode};
use crate::run::{self, Context, Refusal};
use crate::{policy, sandbox};

/// A tool error: `isError` with one fixed-form text block.
pub fn tool_error(stage: &str, reason: &str) -> Value {
    json!({
        "content": [{ "type": "text", "text": format!("Not run ({stage}): {reason}. Nothing from the command's output is returned.") }],
        "isError": true,
    })
}

fn refused(stage: &'static str, reason: impl Into<String>) -> Refusal {
    Refusal { stage, reason: reason.into() }
}

/// The arguments as bound values: exactly the tool's inputs, each of the right JSON type and
/// within its contract.
pub fn arguments(tool: &Tool, args: Option<&Value>) -> Result<BTreeMap<String, String>, Refusal> {
    let empty = Map::new();
    let obj = match args {
        None | Some(Value::Null) => &empty,
        Some(Value::Object(m)) => m,
        Some(_) => return Err(refused("arguments", "arguments must be an object")),
    };
    if let Some(k) = obj.keys().find(|k| !tool.inputs.iter().any(|(n, _)| n == *k)) {
        let shown: String =
            k.chars().take(40).filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-').collect();
        return Err(refused("arguments", format!("unknown argument {shown:?}")));
    }
    let mut out = BTreeMap::new();
    for (name, kind) in &tool.inputs {
        let v = obj.get(name).ok_or_else(|| refused("arguments", format!("argument {name} is required")))?;
        let s = match (kind, v) {
            (InputKind::Int { max }, Value::Number(n)) => match n.as_i64() {
                Some(i) if (0..=*max).contains(&i) => i.to_string(),
                _ => return Err(refused("arguments", format!("argument {name} must be an integer from 0 to {max}"))),
            },
            (_, Value::String(s)) => s.clone(),
            _ => return Err(refused("arguments", format!("argument {name} has the wrong type"))),
        };
        if s.len() > MAX_INPUT || !plain_token(&s) {
            return Err(refused("arguments", format!("argument {name} is not a plain token")));
        }
        if let InputKind::Enum { values } = kind {
            if !values.contains(&s) {
                return Err(refused("arguments", format!("argument {name} is not one of the allowed values")));
            }
        }
        out.insert(name.clone(), s);
    }
    Ok(out)
}

/// Everything a call needs from the server.
pub struct Call<'a> {
    pub home: &'a Path,
    pub root: &'a Path,
    pub tool: &'a Tool,
    pub args: Option<&'a Value>,
    /// Whether the client understands `structuredContent` (protocol 2025-06-18 and later).
    pub structured: bool,
    pub session: &'a str,
    /// Canaries for the oracle (tests).
    pub canaries: Vec<String>,
}

/// Run a call; returns the `CallToolResult` and appends the spool record.
pub fn run(c: &Call) -> Value {
    let (result, record) = match attempt(c) {
        Ok((result, rec)) => (result, rec),
        Err(r) => (tool_error(r.stage, &r.reason), json!({ "withheld": r.stage })),
    };
    let mut rec = json!({
        "ts": crate::time::now(),
        "session": c.session,
        "tool": c.tool.name,
        "route": c.tool.label,
        "level": "e2",
    });
    if let (Some(o), Some(r)) = (rec.as_object_mut(), record.as_object()) {
        for (k, v) in r {
            o.insert(k.clone(), v.clone());
        }
    }
    let _ = adage_store::spool::append(&c.home.join("exec").join("mcp.spool"), &rec);
    result
}

fn attempt(c: &Call) -> Result<(Value, Value), Refusal> {
    let inputs = arguments(c.tool, c.args)?;
    let paths = Paths::at(c.home);
    let index = Index::load(&paths.index());
    let key =
        catalog::repo_key(&index, c.root, &paths).ok_or_else(|| refused("route", "no routes for this repository"))?;
    let ir = index
        .routes
        .iter()
        .find(|r| r.repo == key && format!("{}/{}", r.track, r.route.id) == c.tool.label)
        .ok_or_else(|| refused("route", "the route is no longer in the index"))?;
    let store =
        Store::load(&approvals::path(c.home)).map_err(|_| refused("approval", "the approvals file is unreadable"))?;
    let table = CommandTable::load();
    let (plan, _, _) = catalog::plan(ir, &index, c.root, &paths, &store, &table).map_err(|e| refused("route", e))?;
    let mut req = plan.request;
    req.inputs = inputs;
    for name in &plan.repo_paths {
        crate::project::repo_path_ok(c.root, &req.inputs[name]).map_err(|_| {
            refused("arguments", format!("argument {name} is not a path that exists in the repository"))
        })?;
    }
    let pol = policy::load(c.home).map_err(|_| refused("policy", "the policy file is unreadable"))?;
    let cap = sandbox::detect();
    let ctx = Context {
        inherited_env: std::env::vars().collect(),
        policy: &pol,
        store: &store,
        capability: &cap,
        table: &table,
        repo_key: key,
        cas: Some(c.home.join("cas")).filter(|p| p.is_dir()),
        home: adage_store::home_dir(),
        canaries: c.canaries.clone(),
    };
    // Every gate short of running first, so a refused call never waits for a slot.
    run::check(&req, &ctx).map_err(|(r, _)| r)?;
    let _slot = limit::acquire(&c.home.join("exec").join("mcp-slots"), limit::CONCURRENCY, limit::SLOT_WAIT)
        .ok_or_else(|| refused("limits", "too many Adage tools are running on this machine; try again shortly"))?;
    let delivered = run::execute(&req, &ctx)?;
    let fields: Vec<(String, adage_eval::safety::DataType)> =
        req.output.fields.iter().map(|(k, f)| (k.clone(), crate::hook::data_type(f))).collect();
    let typed = req.output.mode == OutputMode::Typed;
    let decl = typed.then_some(fields.as_slice());
    if !check_data_block(&req.labels.route, &delivered.block, decl, Some(c.root), &ctx.canaries).is_empty() {
        return Err(refused("oracle", "the block failed the safety check"));
    }
    let mut result = json!({ "content": [{ "type": "text", "text": delivered.block }], "isError": false });
    if typed && c.structured {
        let payload = delivered
            .block
            .strip_suffix("\n</adage-data>")
            .and_then(|b| b.rsplit('\n').next())
            .and_then(|p| serde_json::from_str::<Value>(p).ok())
            .filter(Value::is_object)
            .ok_or_else(|| refused("render", "the block has no typed payload"))?;
        result["structuredContent"] = payload;
    }
    let record = json!({ "exec": delivered.record });
    Ok((result, record))
}
