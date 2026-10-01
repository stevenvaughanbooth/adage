//! The closure of a pack (Track C §2.1): every route, type, script, template, model, tool and
//! repository file it references, the edges between them, and the canonical `CLOSURE` listing.
//!
//! Edges:
//!
//! 1. `inputs[*].type` and `outputs[*].type` → a built-in type or `types/<id>` (and a list type →
//!    its item type).
//! 2. `steps[*].script` → `scripts/sha256-<hex>` (vendor) or a `requires.repo_files` entry (ref).
//! 3. `steps[*].run.argv[0]` → an L-none program (`requires.tools`), or the path `script` covers;
//!    an argv naming a repository path without a `script` entry is refused
//!    ([`crate::validate::program`]). An interpreter before a script is a tool too. A program of
//!    the project's environment (`.venv/bin/python`) → `env:<path>`, which binding finds in the
//!    checkout (it is not a requirement a pack can list or satisfy).
//! 4. `links` → routes of the same pack.
//! 5. Implicit: route B triggered `after_command C` with `pattern::meets(C, key(A.steps[0].run))`
//!    gives A → B.
//! 6. An avoid step whose `instead` is another route's run command gives an edge to that route.
//! 7. A route → the template of its shape (`run@1`), an advise step → `advise:<template>@1`
//!    (`requires.templates`); an intent trigger → its model (`requires.models`).
//! 8. `read.from` names an earlier step's list-of-`repo-path` output (checked in validation).
//! 9. A template (an instinct pack's route, [`RouteV2::detector`]) → `detector:<id>`, compiled into
//!    Adage like the templates and types that are built in. Its command is a blank until the
//!    detector fills it, so it has no program edge (3) and takes part in no implicit edge (5, 6).
//!
//! Then: breadth-first search from every entry route; no dangling references; no unreferenced
//! bytes or requirements; no cycle through `after` edges and no `after` chain longer than
//! [`MAX_AFTER_CHAIN`] routes. The listing is sorted lines `kind id digest`, where edges are
//! `edge:<kind> <from>-><to> -`.

use crate::digest::{canonical_json, route_exec_digest, sha256_hex, type_json};
use crate::model::*;
use crate::types;
use crate::validate::{program, Parsed, Program};
use adage_core::{keys::template_command, pattern, CommandTable};
use anyhow::{bail, Result};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// At most this many routes in a chain of `after` edges (bounds multi-step executions).
pub const MAX_AFTER_CHAIN: usize = 4;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Edge {
    /// `type`, `item`, `script`, `repo-file`, `tool`, `env`, `link`, `after`, `instead`,
    /// `template`, `model`, `detector`.
    pub kind: &'static str,
    pub from: String,
    pub to: String,
}

/// A pack's closure.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Closure {
    /// `kind:id` → digest (`-` when the node has none).
    pub nodes: BTreeMap<String, String>,
    pub edges: BTreeSet<Edge>,
}

impl Closure {
    /// The canonical `CLOSURE` file.
    pub fn render(&self) -> String {
        let mut lines: Vec<String> = self
            .nodes
            .iter()
            .map(|(n, d)| {
                let (kind, id) = n.split_once(':').unwrap_or((n, ""));
                format!("{kind} {id} {d}")
            })
            .chain(self.edges.iter().map(|e| format!("edge:{} {}->{} -", e.kind, e.from, e.to)))
            .collect();
        lines.sort();
        lines.iter().map(|l| format!("{l}\n")).collect()
    }

    /// The implicit and explicit route-to-route edges (`after`, `instead`, `link`).
    pub fn route_edges(&self) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(|e| matches!(e.kind, "after" | "instead" | "link"))
    }
}

/// The canonical key of an argv, as trigger keys are written (`exec python3 scripts/x {id}`).
pub fn argv_key(argv: &[String]) -> String {
    format!("exec {}", template_command(argv).join(" "))
}

fn type_node(ty: &str) -> String {
    if types::is_builtin(ty) {
        format!("builtin:{ty}")
    } else {
        format!("type:{ty}")
    }
}

/// Compute and check the closure of a validated pack.
pub fn compute(p: &Parsed) -> Result<Closure> {
    let table = CommandTable::load();
    let q = &p.manifest.requires;
    let mut c = Closure::default();
    let edge = |c: &mut Closure, kind: &'static str, from: &str, to: String| {
        c.edges.insert(Edge { kind, from: from.to_string(), to });
    };
    let routes: BTreeMap<&str, &RouteV2> = p.routes.iter().map(|r| (r.id.as_str(), r)).collect();

    for r in &p.routes {
        let me = format!("route:{}", r.id);
        c.nodes.insert(me.clone(), route_exec_digest(r, &p.types));
        let tys =
            r.inputs.values().map(|i| &i.ty).chain(r.steps.iter().flat_map(|s| s.outputs.values().map(|o| &o.ty)));
        for ty in tys {
            if !types::is_builtin(ty) && !p.types.contains_key(ty) {
                bail!("route {}: type {ty:?} is not in the pack (dangling reference)", r.id);
            }
            edge(&mut c, "type", &me, type_node(ty));
        }
        edge(&mut c, "template", &me, format!("template:{}", r.shape.template()));
        if let TriggerV2::Intent { model, .. } = &r.trigger {
            edge(&mut c, "model", &me, format!("model:{model}"));
        }
        if let Some(d) = &r.detector {
            edge(&mut c, "detector", &me, format!("detector:{d}"));
        }
        for s in &r.steps {
            if let Some(a) = &s.advise {
                edge(&mut c, "template", &me, format!("template:advise:{}@1", a.template));
            }
            let Some(run) = &s.run else { continue };
            if r.is_template() {
                continue;
            }
            match program(&run.argv, &table)? {
                Program::Tool(t) => edge(&mut c, "tool", &me, format!("tool:{t}")),
                // The project's environment: found in the checkout at bind, never in `requires`.
                Program::Env { path, .. } => edge(&mut c, "env", &me, format!("env:{path}")),
                Program::Script { interpreter, .. } => {
                    match interpreter {
                        Some(i) if i.contains('/') => {
                            edge(&mut c, "env", &me, format!("env:{}", i.trim_start_matches("./")))
                        }
                        Some(i) => edge(&mut c, "tool", &me, format!("tool:{i}")),
                        None => {}
                    }
                    let Some(sc) = &s.script else {
                        bail!(
                            "route {}: step {} runs a script without a script entry (dangling reference)",
                            r.id,
                            s.id
                        );
                    };
                    match sc.closure {
                        ClosureLevel::Vendor => {
                            if !p.scripts.contains_key(&sc.sha256) {
                                bail!(
                                    "route {}: vendored script {} is not in the pack (dangling reference)",
                                    r.id,
                                    sc.path
                                );
                            }
                            edge(&mut c, "script", &me, format!("script:{}", sc.path));
                        }
                        ClosureLevel::Ref => {
                            let pinned = q.repo_files.iter().find(|f| f.path == sc.path);
                            match pinned {
                                Some(f) if f.sha256 == sc.sha256 && f.deps == sc.deps => {}
                                Some(_) => bail!(
                                    "route {}: script {} disagrees with its requires.repo_files entry (hash or deps)",
                                    r.id,
                                    sc.path
                                ),
                                None => bail!(
                                    "route {}: script {} has no requires.repo_files entry (dangling reference)",
                                    r.id,
                                    sc.path
                                ),
                            }
                            edge(&mut c, "repo-file", &me, format!("repo-file:{}", sc.path));
                        }
                    }
                }
            }
        }
        for l in &r.links {
            let t = l.trim_start_matches('#');
            if !routes.contains_key(t) {
                bail!("route {}: link {l:?} names no route in the pack (dangling reference)", r.id);
            }
            edge(&mut c, "link", &me, format!("route:{t}"));
        }
    }

    // Implicit edges: A's first run step meets B's after_command trigger. A template's command is
    // a blank, so it meets nothing.
    let fixed: Vec<&RouteV2> = p.routes.iter().filter(|r| !r.is_template()).collect();
    for b in &fixed {
        let TriggerV2::AfterCommand { command } = &b.trigger else { continue };
        for a in &fixed {
            if let Some(run) = a.steps.first().and_then(|s| s.run.as_ref()) {
                if pattern::meets(command, &argv_key(&run.argv)) {
                    edge(&mut c, "after", &format!("route:{}", a.id), format!("route:{}", b.id));
                }
            }
        }
    }
    // An avoid step's `instead` that another route runs.
    for r in &fixed {
        let Some(av) = r.steps.first().and_then(|s| s.avoid.as_ref()) else { continue };
        let instead = av.instead.join(" ");
        for a in fixed.iter().filter(|a| a.id != r.id) {
            if let Some(run) = a.steps.first().and_then(|s| s.run.as_ref()) {
                if pattern::same_command(&instead, &run.argv.join(" ")) {
                    edge(&mut c, "instead", &format!("route:{}", r.id), format!("route:{}", a.id));
                }
            }
        }
    }
    // List types → their item types.
    for t in p.types.values() {
        if let Some(item) = &t.item {
            edge(&mut c, "item", &format!("type:{}", t.id), type_node(item));
        }
    }

    // Every edge target exists (routes, types and scripts were checked above).
    let tools: BTreeSet<String> = q.tools.iter().map(|t| format!("tool:{}", t.name)).collect();
    let templates: BTreeSet<String> = q.templates.iter().map(|t| format!("template:{t}")).collect();
    let models: BTreeSet<String> = q.models.iter().map(|m| format!("model:{m}")).collect();
    for e in &c.edges {
        let (list, what) = match e.to.split_once(':').map(|x| x.0) {
            Some("tool") => (&tools, "requires.tools"),
            Some("template") => (&templates, "requires.templates"),
            Some("model") => (&models, "requires.models"),
            _ => continue,
        };
        if !list.contains(&e.to) {
            bail!("{} needs {}, which {what} does not list (dangling reference)", e.from, e.to);
        }
    }

    // Breadth-first search from every entry route.
    let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in &c.edges {
        adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
    }
    let mut reached: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<String> = p.routes.iter().filter(|r| r.entry).map(|r| format!("route:{}", r.id)).collect();
    while let Some(n) = queue.pop_front() {
        if !reached.insert(n.clone()) {
            continue;
        }
        for t in adj.get(n.as_str()).into_iter().flatten() {
            if !reached.contains(*t) {
                queue.push_back(t.to_string());
            }
        }
    }
    for r in &p.routes {
        if !reached.contains(&format!("route:{}", r.id)) {
            bail!("route {} is not an entry and no link reaches it", r.id);
        }
    }
    for id in p.types.keys() {
        if !reached.contains(&format!("type:{id}")) {
            bail!("types/{id}.yaml is referenced by no route (unreferenced bytes)");
        }
    }
    for h in p.scripts.keys() {
        if !reached.contains(&format!("script:scripts/sha256-{h}")) {
            bail!("scripts/sha256-{h} is referenced by no step (smuggled bytes)");
        }
    }
    for f in &q.repo_files {
        if !reached.contains(&format!("repo-file:{}", f.path)) {
            bail!("requires.repo_files: {} is run by no step", f.path);
        }
    }
    for n in tools.iter().chain(&templates).chain(&models) {
        if !reached.contains(n) {
            bail!("requires: {n} is used by no route");
        }
    }

    check_after_chains(&c)?;

    // Nodes.
    for (id, t) in &p.types {
        c.nodes.insert(format!("type:{id}"), sha256_hex(canonical_json(&type_json(t)).as_bytes()));
    }
    for e in c.edges.clone() {
        let digest = match e.to.split_once(':') {
            Some(("script", path)) => path.trim_start_matches("scripts/sha256-").to_string(),
            Some(("repo-file", path)) => {
                q.repo_files.iter().find(|f| f.path == path).map(|f| f.sha256.clone()).unwrap_or_default()
            }
            Some(("builtin" | "tool" | "template" | "model" | "env" | "detector", _)) => "-".to_string(),
            _ => continue,
        };
        c.nodes.insert(e.to.clone(), digest);
    }
    Ok(c)
}

/// No cycle through `after` edges, and no chain longer than [`MAX_AFTER_CHAIN`] routes.
fn check_after_chains(c: &Closure) -> Result<()> {
    let mut adj: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for e in c.edges.iter().filter(|e| e.kind == "after") {
        adj.entry(e.from.as_str()).or_default().push(e.to.as_str());
    }
    // Longest chain (in routes) starting at each node, with cycle detection.
    fn longest<'a>(
        n: &'a str,
        adj: &BTreeMap<&'a str, Vec<&'a str>>,
        memo: &mut BTreeMap<&'a str, usize>,
        stack: &mut Vec<&'a str>,
    ) -> Result<usize> {
        if let Some(&d) = memo.get(n) {
            return Ok(d);
        }
        if let Some(i) = stack.iter().position(|x| *x == n) {
            let cycle: Vec<&str> =
                stack[i..].iter().chain(std::iter::once(&n)).map(|x| x.trim_start_matches("route:")).collect();
            bail!("after chain cycle: {}", cycle.join(" -> "));
        }
        stack.push(n);
        let mut best = 0;
        for t in adj.get(n).into_iter().flatten() {
            best = best.max(longest(t, adj, memo, stack)?);
        }
        stack.pop();
        memo.insert(n, best + 1);
        Ok(best + 1)
    }
    let mut memo = BTreeMap::new();
    for n in adj.keys() {
        let d = longest(n, &adj, &mut memo, &mut Vec::new())?;
        if d > MAX_AFTER_CHAIN {
            bail!(
                "after chain from {} is {d} routes long; at most {MAX_AFTER_CHAIN} are allowed",
                n.trim_start_matches("route:")
            );
        }
    }
    Ok(())
}
