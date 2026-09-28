//! Resolution of a pack against one repository (Track C §2.3), at index compile time (`adage
//! add`, and the index compiles that follow), never in the hook.
//!
//! Every route becomes a [`BoundRoute`]: **live** with the highest level it may ever be approved
//! at on this machine, or **inert** with a reason. Only signature, schema or closure failures
//! refuse a whole pack (at `add`); everything here makes individual routes inert, and the pack
//! stays installed. In order:
//!
//! 1. **Applicability** ([`applies`]): `repo` compares the target's keys (its `repo_key`, and its
//!    `<name>@<root commit>` form) with the pack's; `fingerprint` evaluates the predicates over
//!    `adage_instincts::declared::extract(root)` (bounded, never executes); `detector` runs the
//!    compiled detector over the same facts. A pack that does not apply binds nothing; one whose
//!    detector or catalogue this build does not know binds every route inert.
//! 2. **`requires.os`**, then the **tools** each route runs, by PATH lookup only (nothing is run).
//! 3. **Models and templates** the route names must be known to this build.
//! 4. **L-ref scripts**: the SHA-256 of `root/<path>` (a regular file, no symlink on the way,
//!    inside the root, at most 1 MiB) must equal the pin; `deps` globs must resolve inside the
//!    root, and their hashes join the binding. **L-vendor** scripts must be in the cas.
//! 5. **A runtime form**: E0 delivers today's route model (`adage_tracks::Route`). A route whose
//!    shape has no such form (several steps, a sequence, a read of a step's output) is inert:
//!    "shape not yet supported at runtime". A route that runs a vendored script is live but
//!    exec-only: only `adage-exec` runs it, and E0 never directs it.
//!
//! The result is a pure function of the pack, the repository's files and the environment
//! ([`BindEnv`]), so the same repository gives a byte-identical binding.

use crate::model::*;
use crate::types;
use crate::upgrade::hash_repo_file;
use crate::validate::{program, Program};
use crate::Pack;
use adage_core::{CommandTable, Effect};
use adage_tracks::{
    io, Contract, Evidence, Exit, Gauge, Manifest, Origin, Route, RouteState, Scope, Slot, SlotSource, Step, Track,
    TrackInfo, Trigger,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// The reason a route whose shape the runtime cannot deliver yet is inert.
pub const SHAPE_NOT_SUPPORTED: &str = "shape not yet supported at runtime";

/// Where a pack is bound: a repository key, and its checkout on this machine if there is one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// The key hooks look routes up by (`repo_key(root)` when there is a checkout).
    pub repo: String,
    pub root: Option<PathBuf>,
}

impl Target {
    /// A checkout: its `repo_key`.
    pub fn checkout(root: &Path) -> Target {
        Target { repo: adage_core::repo::repo_key(root), root: Some(root.to_path_buf()) }
    }
}

/// What a binding depends on besides the pack and the repository's files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindEnv {
    pub os: Os,
    /// The `PATH` tools are looked up in (`None`: no tool is found).
    pub path: Option<OsString>,
    /// `$ADAGE_HOME/cas`, where vendored scripts are installed (`None`: none are).
    pub cas: Option<PathBuf>,
}

impl BindEnv {
    /// This machine: its OS and `PATH`, and the cas at `cas`.
    pub fn current(cas: Option<PathBuf>) -> BindEnv {
        let os = if cfg!(target_os = "macos") {
            Os::Macos
        } else if cfg!(windows) {
            Os::Windows
        } else {
            Os::Linux
        };
        BindEnv { os, path: std::env::var_os("PATH"), cas }
    }
}

/// A route's standing on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Status {
    /// Its closure is satisfied here; it may be approved at most at `max_level`.
    Live { max_level: Level },
    /// Its closure is not satisfied here, and why. Nothing of it is delivered.
    Inert { reason: String },
}

impl Status {
    pub fn is_live(&self) -> bool {
        matches!(self, Status::Live { .. })
    }
    pub fn reason(&self) -> Option<&str> {
        match self {
            Status::Inert { reason } => Some(reason),
            Status::Live { .. } => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BoundRoute {
    pub id: String,
    #[serde(flatten)]
    pub status: Status,
    /// The route exec digest (Track C §1.5); empty for a route that was never part of the pack.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub route_exec_digest: String,
    /// Resolved scripts and their SHA-256: repository-relative (L-ref and its deps) or
    /// `cas/<hex>` (L-vendor).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scripts: Vec<(String, String)>,
    /// The runtime form of a live route (at probation state rules: an environment step is a
    /// draft).
    #[serde(skip)]
    pub route: Option<Route>,
}

impl BoundRoute {
    /// The index's view of this binding (live routes only).
    pub fn exec(&self) -> Option<adage_tracks::index::ExecBinding> {
        match self.status {
            Status::Live { max_level } => Some(adage_tracks::index::ExecBinding {
                route_exec_digest: self.route_exec_digest.clone(),
                scripts: self.scripts.clone(),
                max_level,
            }),
            Status::Inert { .. } => None,
        }
    }
}

/// Whether a pack applies to a target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Applies {
    Yes,
    /// It does not apply here (nothing is bound), and why.
    No(String),
    /// It cannot be evaluated by this build: every route binds inert with this reason.
    Unknown(String),
}

/// A pack bound to one target.
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    pub pack: String,
    pub version: String,
    pub repo: String,
    pub applies: Applies,
    /// Sorted by id; empty when the pack does not apply.
    pub routes: Vec<BoundRoute>,
}

impl Binding {
    pub fn live(&self) -> impl Iterator<Item = &BoundRoute> {
        self.routes.iter().filter(|r| r.status.is_live())
    }
    pub fn inert(&self) -> impl Iterator<Item = &BoundRoute> {
        self.routes.iter().filter(|r| !r.status.is_live())
    }
}

/// The keys a checkout answers to: its `repo_key`, its origin form (if any), and its
/// `<directory name>@<root commit>` form.
pub fn local_keys(root: &Path) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    keys.insert(adage_core::repo::repo_key(root));
    if let Some(o) = adage_core::repo::origin_key(root) {
        keys.insert(o);
    }
    if let Some(c) = adage_core::gitobj::Repo::open(root).and_then(|mut r| r.root_commit()) {
        keys.insert(format!("{}@{}", adage_core::repo::dir_name(root), &adage_core::gitobj::hex(&c)[..12]));
    }
    keys
}

/// The catalogue this build compiles in (`core@0.1.0`).
pub fn catalogue_id() -> String {
    let c = adage_instincts::catalogue();
    format!("{}@{}", c.pack.name, c.pack.version)
}

/// Does `pack` apply to `target`?
pub fn applies(pack: &Pack, target: &Target) -> Applies {
    match &pack.manifest.applicability {
        Applicability::Any => Applies::Yes,
        Applicability::Repo { repo_keys } => {
            let mine = match &target.root {
                Some(root) => local_keys(root),
                None => [target.repo.clone()].into_iter().collect(),
            };
            if repo_keys.iter().any(|k| mine.contains(k)) {
                Applies::Yes
            } else {
                Applies::No(format!("{} is not one of the pack's repositories ({})", target.repo, repo_keys.join(", ")))
            }
        }
        Applicability::Fingerprint { fingerprint } => {
            let Some(root) = &target.root else {
                return Applies::No(format!("no checkout of {} on this machine to read its files", target.repo));
            };
            let facts = adage_instincts::declared::extract(root);
            let mut walked: Option<Vec<String>> = None;
            for f in fingerprint {
                let ok = match f {
                    Fingerprint::MakefileTarget(t) => {
                        facts.entry(adage_instincts::declared::Source::Makefile, t).is_some()
                    }
                    Fingerprint::PackageScript(s) => {
                        facts.entry(adage_instincts::declared::Source::PackageJson, s).is_some()
                    }
                    Fingerprint::Lockfile(l) => facts.lockfiles.iter().any(|(p, _)| p == l),
                    Fingerprint::PathExists(g) => {
                        let files = walked.get_or_insert_with(|| {
                            adage_instincts::files::RepoFiles::open(root).map(|f| f.walk()).unwrap_or_default()
                        });
                        glob::Pattern::new(g).is_ok_and(|p| files.iter().any(|f| p.matches(f)))
                    }
                };
                if !ok {
                    return Applies::No(format!("the repository does not have {}", describe_fingerprint(f)));
                }
            }
            Applies::Yes
        }
        Applicability::Detector { detector, catalogue } => {
            let ours = catalogue_id();
            if *catalogue != ours {
                return Applies::Unknown(format!("catalogue {catalogue} is not this build's ({ours})"));
            }
            let cat = adage_instincts::catalogue();
            let ids: Vec<&str> = if detector == "*" {
                cat.instincts.iter().filter(|i| i.enabled).map(|i| i.id.as_str()).collect()
            } else if cat.get(detector).is_some() {
                vec![detector.as_str()]
            } else {
                return Applies::Unknown(format!("detector {detector} is not compiled into this build"));
            };
            let Some(root) = &target.root else {
                return Applies::No(format!("no checkout of {} on this machine to run detectors over", target.repo));
            };
            let facts = adage_instincts::declared::extract(root);
            let table = CommandTable::load();
            let ctx = adage_instincts::detect::Ctx { facts: &facts, table: &table, cat };
            let fires = ids.iter().any(|id| {
                let d = adage_instincts::detect::detect(id, &ctx);
                !d.routes.is_empty() || !d.advisories.is_empty()
            });
            if fires {
                Applies::Yes
            } else {
                Applies::No(format!("detector {detector} finds nothing in this repository"))
            }
        }
    }
}

fn describe_fingerprint(f: &Fingerprint) -> String {
    match f {
        Fingerprint::MakefileTarget(t) => format!("the Makefile target {t}"),
        Fingerprint::PackageScript(s) => format!("the package.json script {s}"),
        Fingerprint::PathExists(g) => format!("a file matching {g}"),
        Fingerprint::Lockfile(l) => format!("the lockfile {l}"),
    }
}

/// Is `name` an executable file in a directory of `path`? Relative entries are ignored. Nothing
/// is run.
pub fn on_path(name: &str, path: Option<&OsString>) -> bool {
    let Some(path) = path else { return false };
    std::env::split_paths(path).filter(|d| d.is_absolute()).any(|d| {
        let names: Vec<String> = if cfg!(windows) {
            vec![name.to_string(), format!("{name}.exe"), format!("{name}.cmd")]
        } else {
            vec![name.to_string()]
        };
        names.iter().any(|n| executable(&d.join(n)))
    })
}

fn executable(p: &Path) -> bool {
    let Ok(m) = std::fs::metadata(p) else { return false };
    if !m.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        m.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The templates this build renders.
pub fn known_template(t: &str) -> bool {
    let shapes = ["run", "avoid", "after", "read", "advise", "obligation", "sequence"];
    if let Some(name) = t.strip_suffix("@1") {
        if shapes.contains(&name) {
            return true;
        }
        if let Some(a) = name.strip_prefix("advise:") {
            return adage_tracks::advice::TEMPLATES.iter().any(|(id, _, _)| *id == a);
        }
    }
    false
}

/// The embedding models this build can run.
pub fn known_model(m: &str) -> bool {
    m == adage_intent::model::pinned_id()
}

fn os_name(o: Os) -> &'static str {
    match o {
        Os::Linux => "linux",
        Os::Macos => "macos",
        Os::Windows => "windows",
    }
}

fn short(h: &str) -> &str {
    &h[..h.len().min(12)]
}

/// Bind `pack` to `target`.
pub fn bind(pack: &Pack, target: &Target, env: &BindEnv) -> Binding {
    let m = &pack.manifest;
    let mut b = Binding {
        pack: m.pack.name.clone(),
        version: m.pack.version.clone(),
        repo: target.repo.clone(),
        applies: applies(pack, target),
        routes: Vec::new(),
    };
    let everything_inert = match &b.applies {
        Applies::No(_) => return b,
        Applies::Unknown(why) => Some(why.clone()),
        Applies::Yes => None,
    };
    let os_problem = (!m.requires.os.is_empty() && !m.requires.os.contains(&env.os)).then(|| {
        format!(
            "the pack requires {}; this machine runs {}",
            m.requires.os.iter().map(|o| os_name(*o)).collect::<Vec<_>>().join(" or "),
            os_name(env.os)
        )
    });
    let table = CommandTable::load();
    for r in &pack.routes {
        let digest = crate::digest::route_exec_digest(r, &pack.types);
        let mut scripts: Vec<(String, String)> = Vec::new();
        let status = match everything_inert.clone().or_else(|| os_problem.clone()) {
            Some(reason) => Err(reason),
            None => check_route(r, target, env, &table, &mut scripts),
        };
        let (status, route) = match status.and_then(|()| to_runtime(r, pack, &table).map_err(|e| e.to_string())) {
            Ok(route) => {
                let mut max_level = r.exec.as_ref().map(|x| x.level).unwrap_or_default().min(m.exec.requested);
                if env.os == Os::Windows {
                    max_level = Level::E0; // Track C D10: E0 only on Windows.
                }
                if vendored(r) && max_level == Level::E0 {
                    let reason = "it runs a vendored script, which only adage-exec runs, at no execution level here";
                    (Status::Inert { reason: reason.into() }, None)
                } else {
                    (Status::Live { max_level }, Some(route))
                }
            }
            Err(reason) => (Status::Inert { reason }, None),
        };
        scripts.sort();
        scripts.dedup();
        b.routes.push(BoundRoute { id: r.id.clone(), status, route_exec_digest: digest, scripts, route });
    }
    b.routes.sort_by(|a, b| a.id.cmp(&b.id));
    b
}

/// Steps 2 to 4 for one route; `scripts` collects what its scripts resolved to.
fn check_route(
    r: &RouteV2,
    target: &Target,
    env: &BindEnv,
    table: &CommandTable,
    scripts: &mut Vec<(String, String)>,
) -> Result<(), String> {
    // Templates and models.
    let mut templates = vec![r.shape.template()];
    templates.extend(r.steps.iter().filter_map(|s| s.advise.as_ref().map(|a| format!("advise:{}@1", a.template))));
    if let Some(t) = templates.iter().find(|t| !known_template(t)) {
        return Err(format!("template {t} is not known to this build"));
    }
    if let TriggerV2::Intent { model, .. } = &r.trigger {
        if !known_model(model) {
            return Err(format!("embedding model {model} is not the one this build runs"));
        }
    }
    // Tools, then scripts, step by step.
    for s in &r.steps {
        let Some(run) = &s.run else { continue };
        let prog = program(&run.argv, table).map_err(|e| e.to_string())?;
        let tool = match &prog {
            Program::Tool(t) => Some(t.clone()),
            Program::Script { interpreter, .. } => interpreter.clone(),
        };
        if let Some(t) = tool {
            if !on_path(&t, env.path.as_ref()) {
                return Err(format!("the tool {t} is not on PATH"));
            }
        }
        let Some(sc) = &s.script else { continue };
        match sc.closure {
            ClosureLevel::Ref => {
                let Some(root) = &target.root else {
                    return Err(format!(
                        "runs the repository script {}; there is no checkout of {} on this machine to check it against",
                        sc.path, target.repo
                    ));
                };
                let got =
                    hash_repo_file(root, &sc.path).map_err(|e| format!("repository script {}: {e:#}", sc.path))?;
                if got != sc.sha256 {
                    return Err(format!(
                        "repository script {} has changed: sha256 {} is not the pinned {}",
                        sc.path,
                        short(&got),
                        short(&sc.sha256)
                    ));
                }
                scripts.push((sc.path.clone(), got));
                for dep in deps(root, &sc.deps).map_err(|e| format!("script {} deps: {e}", sc.path))? {
                    scripts.push(dep);
                }
            }
            ClosureLevel::Vendor => {
                let cas = env.cas.as_ref().map(|c| c.join(&sc.sha256));
                let ok = cas
                    .as_ref()
                    .is_some_and(|p| std::fs::read(p).is_ok_and(|b| crate::digest::sha256_hex(&b) == sc.sha256));
                if !ok {
                    return Err(format!("vendored script {} is not in the cas", short(&sc.sha256)));
                }
                scripts.push((format!("cas/{}", sc.sha256), sc.sha256.clone()));
            }
        }
    }
    Ok(())
}

/// The files `globs` match under `root` (bounded walk, no symlinks), with their SHA-256. Each
/// glob must match something.
fn deps(root: &Path, globs: &[String]) -> Result<Vec<(String, String)>, String> {
    if globs.is_empty() {
        return Ok(Vec::new());
    }
    let files = adage_instincts::files::RepoFiles::open(root).map(|f| f.walk()).unwrap_or_default();
    let mut out = Vec::new();
    for g in globs {
        let p = glob::Pattern::new(g).map_err(|e| format!("{g}: {e}"))?;
        let hits: Vec<&String> = files.iter().filter(|f| p.matches(f)).collect();
        if hits.is_empty() {
            return Err(format!("{g} matches no file"));
        }
        for h in hits {
            let sha = hash_repo_file(root, h).map_err(|e| format!("{h}: {e:#}"))?;
            out.push((h.clone(), sha));
        }
    }
    Ok(out)
}

/// A route's runtime form: the E0 route model, if its shape has one, validated as a track route.
pub fn to_runtime(r: &RouteV2, pack: &Pack, table: &CommandTable) -> Result<Route, String> {
    let route = to_v1(r, pack, table)?;
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: pack.manifest.pack.name.clone(),
                version: pack.manifest.pack.version.clone(),
                description: String::new(),
                provenance: "imported:pack".into(),
            },
            scope: Scope { repo: "pack".into() },
            gauge: Gauge::default(),
        },
        routes: vec![route.clone()],
    };
    io::validate(&t).map_err(|e| format!("its runtime form fails track validation: {e:#}"))?;
    Ok(route)
}

fn rank(e: Effect) -> u8 {
    crate::validate::rank(e)
}

/// Route v2 → the runtime route model. `Err` names why there is none.
fn to_v1(r: &RouteV2, pack: &Pack, table: &CommandTable) -> Result<Route, String> {
    if r.steps.len() != 1 || r.shape == Shape::Sequence {
        return Err(SHAPE_NOT_SUPPORTED.into());
    }
    let s = &r.steps[0];
    let trigger = match &r.trigger {
        TriggerV2::Prompt { span } => Trigger::Prompt { span: *span },
        TriggerV2::AfterCommand { command } => Trigger::AfterCommand { command: command.clone() },
        TriggerV2::AfterFailure { command, signature } => {
            Trigger::AfterFailure { command: command.clone(), signature: signature.clone() }
        }
        TriggerV2::AfterEdit { paths } => Trigger::AfterEdit { paths: paths.clone() },
        TriggerV2::SessionStart => Trigger::SessionStart,
        TriggerV2::Intent { family, model, centroid, threshold } => Trigger::Intent {
            family: family.clone(),
            model: model.clone(),
            centroid: centroid.clone(),
            threshold: *threshold,
        },
    };
    let mut slots = Vec::new();
    for (name, i) in &r.inputs {
        let contract = contract(&i.ty, &pack.types).ok_or_else(|| {
            format!("{SHAPE_NOT_SUPPORTED}: input {name} has type {}, which has no runtime contract", i.ty)
        })?;
        let source = match (&i.from.prompt, &i.from.result) {
            (Some(p), None) => match &p.vtype {
                Some(vtype) => SlotSource::PromptValue { vtype: vtype.clone(), mentions: p.mentions.clone() },
                None => SlotSource::Prompt { span: p.span },
            },
            (None, Some(f)) => SlotSource::ResultField { field: f.field.clone() },
            _ => return Err(format!("input {name} has no single source")),
        };
        slots.push(Slot { name: name.clone(), source, contract });
    }
    let mut effect = s.effect;
    let step = match s.kind().ok_or("a step has no single kind")? {
        StepKind::Run(run) => {
            // A vendored script runs only under adage-exec: the route is exec-only (the index
            // keeps it for E1 and E2, and E0 never directs it; `IndexedRoute::exec_only`).
            let command = run.argv.join(" ");
            // The command table's view wins where it is stronger (as track validation requires).
            let classified = io::classify(&command, table);
            if rank(classified) > rank(effect) {
                effect = classified;
            }
            Step::Run { command }
        }
        StepKind::Read(rd) => {
            if rd.from.contains('.') || !r.inputs.contains_key(&rd.from) {
                return Err(format!("{SHAPE_NOT_SUPPORTED}: it reads a step's output"));
            }
            Step::ReadBatch { slot: rd.from.clone(), max: rd.max as usize }
        }
        StepKind::Avoid(a) => Step::Avoid {
            avoid: a.runners.iter().map(|x| x.join(" ")).collect::<Vec<_>>().join(" / "),
            instead: a.instead.join(" "),
            signature: a.signature.clone(),
        },
        StepKind::Advise(a) => Step::Advise { template: a.template.clone(), slots: a.slots.clone() },
    };
    let exit = r
        .exit
        .iter()
        .filter_map(|e| match e {
            ExitV2::IntentChanges => Some(Exit::IntentChanges),
            ExitV2::ExploratoryChange => Some(Exit::ExploratoryChange),
            // Evaluated on typed outputs, which only execution produces.
            ExitV2::ResultEquals { .. } => None,
        })
        .collect();
    let e = r.evidence.clone().unwrap_or_default();
    let evidence = Evidence {
        opportunities: e.opportunities,
        support: e.support,
        days: e.days,
        successes: e.successes,
        deviations_failed: e.deviations_failed,
        rate: e.rate,
        lower_bound: e.lower_bound,
        median_position: e.median_position,
        turns_saved: e.turns_saved,
        provenance: e.provenance,
        models: e.models,
        exposed: e.exposed,
        stale_since: None,
        origin: e.origin.map(|o| match o {
            RouteOrigin::Learned => Origin::Learned,
            RouteOrigin::Instinct { pack, version, instinct, ecosystem, declared } => {
                Origin::Instinct { pack, version, instinct, ecosystem, declared }
            }
            RouteOrigin::Imported => Origin::Imported,
            RouteOrigin::Authored => Origin::Authored,
            RouteOrigin::Synthetic => Origin::Synthetic,
        }),
        by_origin: e.by_origin,
        outcome_lift: None,
        effect_evidence: None,
    };
    Ok(Route {
        id: r.id.clone(),
        summary: r.summary.clone(),
        trigger,
        step,
        slots,
        effect,
        aspect: r.aspect,
        // A pack carries no per-machine state: an environment step waits for a person (O11). An
        // exec-only route (a vendored script) is never directed, so it waits for nothing here: its
        // execution approval is explicit, per machine, and never automatic.
        state: if effect == Effect::Environment && !vendored(r) { RouteState::Draft } else { RouteState::Probation },
        exit,
        evidence,
        authored: false,
    })
}

/// Whether a route runs a vendored script (and so is exec-only).
pub fn vendored(r: &RouteV2) -> bool {
    r.steps.iter().any(|s| s.script.as_ref().is_some_and(|sc| sc.closure == ClosureLevel::Vendor))
}

/// The runtime contract of a type, where one exists.
fn contract(ty: &str, user: &BTreeMap<String, TypeDef>) -> Option<Contract> {
    match ty {
        "repo-path" | "repo-path-list" => return Some(Contract::RepoPath),
        "git-rev" => return Some(Contract::Pattern { regex: types::GIT_REV.into() }),
        _ if types::is_builtin(ty) => return None,
        _ => {}
    }
    let t = user.get(ty)?;
    match t.base {
        Base::String => t.pattern.clone().map(|regex| Contract::Pattern { regex }),
        Base::List if t.item.as_deref() == Some("repo-path") => Some(Contract::RepoPath),
        _ => None,
    }
}
