//! Which routes are tools, for one repository (Track C §3.5): every route in the index for the
//! repository that is live and approved at E2 on this machine, with the approval key recomputed
//! **now** (so an L-ref script changed since approval, a changed route, or a changed profile
//! drops the tool), plus, when policy turns automatic approval on, the routes in the
//! inert-observe class (§4.2; off by default until gate G3).
//!
//! Execution must be on (`$ADAGE_HOME/exec/enabled`, written only by `adage exec enable`): the
//! Claude Code plugin cannot unregister its `.mcp.json`, so with execution off the server lists
//! nothing.

use adage_core::{repo, CommandTable};
use adage_store::Paths;
use adage_tracks::index::IndexedRoute;
use adage_tracks::Index;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use super::schema::{self, InputKind};
use crate::approvals::{self, ApprovalKey, AutoContext, Store};
use crate::request::Level;
use crate::route::{self, Plan};
use crate::{git, policy, sandbox};

/// The marker `adage exec enable` writes.
pub fn marker(home: &Path) -> PathBuf {
    home.join("exec").join("enabled")
}

/// One tool: a route, the plan it runs, and how it is presented.
#[derive(Debug, Clone)]
pub struct Tool {
    pub name: String,
    /// `track/route` (the approval's label).
    pub label: String,
    pub track: String,
    pub route_id: String,
    pub inputs: Vec<(String, InputKind)>,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Option<Value>,
    /// Listed through automatic approval (inert-observe), not a stored approval.
    pub auto: bool,
    /// L-ref scripts (repository-relative), whose change must refresh the list.
    pub scripts: Vec<String>,
}

impl Tool {
    /// The `tools/list` entry for a client speaking `version` (fields newer than it are left out).
    pub fn definition(&self, version: &str) -> Value {
        let mut t = json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema,
        });
        if version >= "2025-06-18" {
            t["title"] = json!(schema::title(&self.route_id));
            if let Some(o) = &self.output_schema {
                t["outputSchema"] = o.clone();
            }
        }
        if version >= "2025-03-26" {
            t["annotations"] = json!({
                "title": schema::title(&self.route_id),
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false,
            });
        }
        t
    }
}

/// The tools for one repository, and why there are none when there are none.
#[derive(Debug, Clone, Default)]
pub struct Catalog {
    pub root: Option<PathBuf>,
    pub repo_key: Option<String>,
    pub tools: Vec<Tool>,
    /// Why the list is empty (for `adage-exec mcp --list` and the log), never shown to the agent.
    pub note: String,
}

impl Catalog {
    pub fn find(&self, name: &str) -> Option<&Tool> {
        self.tools.iter().find(|t| t.name == name)
    }

    /// A digest of everything a client sees, to decide whether `list_changed` is due.
    pub fn fingerprint(&self) -> String {
        let v: Vec<Value> = self.tools.iter().map(|t| t.definition("9999-99-99")).collect();
        crate::sha256_hex(crate::canonical_json(&v).as_bytes())
    }
}

/// The repository root for a starting directory.
pub fn repo_root(start: &Path) -> Option<PathBuf> {
    repo::find_root(&start.canonicalize().ok()?)
}

/// The index's key for `root`, if it holds routes for it.
pub fn repo_key(index: &Index, root: &Path, paths: &Paths) -> Option<String> {
    let keys: BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    repo::lookup_key(root, Some(&paths.repo_keys()), &keys)
}

/// The E2 plan for a route, with the approval's terms, and the key it would be approved under.
pub fn plan(
    ir: &IndexedRoute,
    index: &Index,
    root: &Path,
    paths: &Paths,
    store: &Store,
    table: &CommandTable,
) -> Result<(Plan, ApprovalKey, Option<approvals::Approval>), String> {
    if ir.exec.as_ref().is_some_and(|b| b.max_level < adage_tracks::index::Level::E2) {
        return Err("the pack binds this route below E2".into());
    }
    let label = format!("{}/{}", ir.track, ir.route.id);
    let approval =
        store.approvals.iter().find(|a| a.route == label && a.key.repo == ir.repo && a.key.level == Level::E2).cloned();
    let terms = approval.as_ref().and_then(|a| a.terms.as_ref());
    let p = route::plan(ir, index, root, &paths.tracks(), Level::E2, terms, table)?;
    let key = ApprovalKey::for_request(&p.request, &ir.repo);
    Ok((p, key, approval))
}

/// Build the catalog for the repository containing `start`.
pub fn build(home: &Path, start: Option<&Path>) -> Catalog {
    let mut c = Catalog::default();
    if !marker(home).is_file() {
        c.note = "execution is off (adage exec enable)".into();
        return c;
    }
    let Some(root) = start.and_then(repo_root) else {
        c.note = "not inside a git repository".into();
        return c;
    };
    c.root = Some(root.clone());
    let paths = Paths::at(home);
    let index = Index::load(&paths.index());
    let Some(key) = repo_key(&index, &root, &paths) else {
        c.note = "the index holds no routes for this repository".into();
        return c;
    };
    c.repo_key = Some(key.clone());
    let store = match Store::load(&approvals::path(home)) {
        Ok(s) => s,
        Err(e) => {
            c.note = format!("approvals unreadable: {e:#}");
            return c;
        }
    };
    let pol = match policy::load(home) {
        Ok(p) => p,
        Err(e) => {
            c.note = format!("policy unreadable: {e:#}");
            return c;
        }
    };
    if pol.max_level < Level::E2 {
        c.note = "policy caps execution below E2".into();
        return c;
    }
    let table = CommandTable::load();
    let mut auto_ctx: Option<(Vec<String>, bool)> = None;
    let mut found = Vec::new();
    for ir in index.routes.iter().filter(|r| r.repo == key) {
        let Ok((p, k, approval)) = plan(ir, &index, &root, &paths, &store, &table) else { continue };
        let approved = approval.as_ref().is_some_and(|a| a.key == k);
        let auto = !approved && pol.auto_approve && {
            let (hazards, enforced) =
                auto_ctx.get_or_insert_with(|| (git::hazards(&root), sandbox::detect().available()));
            let actx =
                AutoContext { table: &table, git_hazards: hazards, sandbox_enforced: *enforced, policy_auto: true };
            approvals::auto_eligible(&p.request, &actx).is_ok()
        };
        if !(approved || auto) || pol.permits(&p.request, crate::classify::effect(&p.request.argv, &table)).is_err() {
            continue;
        }
        let used = crate::request::slots(&p.request.argv);
        let inputs: Vec<(String, InputKind)> = used
            .iter()
            .map(|n| (n.clone(), schema::input_kind(&p.request.contracts[n], p.repo_paths.contains(n))))
            .collect();
        if p.request.output.validate().is_err() {
            continue;
        }
        let Some(description) = schema::description(&p.request.argv, &p.request.output, ir.route.evidence.support)
        else {
            continue;
        };
        found.push(Tool {
            name: String::new(),
            label: format!("{}/{}", ir.track, ir.route.id),
            track: ir.track.clone(),
            route_id: ir.route.id.clone(),
            input_schema: schema::input_schema(&inputs),
            output_schema: schema::output_schema(&p.request.output),
            inputs,
            description,
            auto,
            scripts: p.request.scripts.iter().map(|s| s.path.trim_start_matches("./").to_string()).collect(),
        });
    }
    found.sort_by(|a, b| a.label.cmp(&b.label));
    let labels: Vec<(String, String)> = found.iter().map(|t| (t.label.clone(), t.route_id.clone())).collect();
    for (t, n) in found.iter_mut().zip(schema::assign_names(&labels)) {
        t.name = n;
    }
    if found.is_empty() {
        c.note = "no route here is approved at E2 (adage review allow-exec … --level e2)".into();
    }
    c.tools = found;
    c
}

/// What the catalog depends on, cheaply: sizes and modification times of the index, the
/// approvals, the marker, the policy files, and each listed tool's scripts.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Stamp(Vec<Option<(u64, SystemTime)>>);

fn stat(p: &Path) -> Option<(u64, SystemTime)> {
    let m = std::fs::metadata(p).ok()?;
    Some((m.len(), m.modified().ok()?))
}

pub fn stamp(home: &Path, catalog: &Catalog) -> Stamp {
    let paths = Paths::at(home);
    let mut v = vec![
        stat(&paths.index()),
        stat(&approvals::path(home)),
        stat(&marker(home)),
        stat(&home.join("policy.toml")),
        stat(&policy::managed_path()),
    ];
    if let Some(root) = &catalog.root {
        for t in &catalog.tools {
            for s in &t.scripts {
                v.push(stat(&root.join(s)));
            }
        }
    }
    Stamp(v)
}
