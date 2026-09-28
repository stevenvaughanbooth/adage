//! A compiled route as an execution request (Track C §3.2, §4.1): what `adage review allow-exec`
//! approves and what `adage-exec hook` runs are both derived here, from the same index entry, so
//! the approval key computed at the trigger equals the approved one exactly when nothing that
//! would run has changed.
//!
//! - **argv** is the route's command template (`python3 scripts/wi-lookup {id} --json`), split on
//!   whitespace as the runtime splits it; there is never a shell.
//! - **inputs** are its prompt slots, each with its anchored pattern contract. A slot bound from a
//!   command's result, or a repository-path contract, has no E1 form yet.
//! - **scripts**: a pack route's resolved scripts come from its binding (the pins bind checked);
//!   a local route's are the argv elements that name a file in the repository (L-ref), hashed
//!   **now**. A script that changed since its approval therefore lapses the approval here, and
//!   [`crate::run::execute`] re-reads and re-hashes it once more before running a private copy.
//! - **output**: a pack route's declared step outputs; else the fields a person declared at
//!   approval ([`Terms`]); else the result fields later routes read from this command's output
//!   (the implicit edge `lookup → read-linked`, §2.1 item 5), as repository-path lists.
//! - **exits**: a pack route's `result_equals` predicates, or those declared at approval.

use adage_core::{CommandTable, Effect};
use adage_tracks::index::IndexedRoute;
use adage_tracks::{Contract, Index, SlotSource, Step, Trigger};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::request::{ExecRequest, FieldSpec, FieldType, Labels, Level, Origin, OutputMode, OutputSpec, ScriptRef};
use crate::sandbox::Profile;

/// What a person may declare at approval, beside the route: the typed output fields, exit
/// predicates on them, and the wall timeout. Stored with the approval; the fields are part of the
/// approval key (through the exec digest), the timeout through the profile id.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Terms {
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub fields: BTreeMap<String, FieldSpec>,
    /// Field → value: when the projected field equals it, the route directs nothing further.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub exit_when: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub raw: bool,
}

/// An E1-ready request (inputs unbound) and the exit predicates to evaluate on its output.
#[derive(Debug, Clone)]
pub struct Plan {
    pub request: ExecRequest,
    pub exit_when: BTreeMap<String, String>,
    /// Slots whose contract is a repository path (E2 only): the value must also name something
    /// that exists inside the repository, checked at call time.
    pub repo_paths: std::collections::BTreeSet<String>,
}

/// The contract a repository-path slot is bound with at E2 (strict: it passes the hostile
/// probes); the value must also exist inside the repository ([`crate::project::repo_path_ok`]).
pub const REPO_PATH_CONTRACT: &str = r"^[A-Za-z0-9_][A-Za-z0-9_./@+-]{0,199}$";

/// The step label every derived request carries (`step="lookup"` in the data block).
pub const STEP: &str = "lookup";

fn is_script_path(a: &str, root: &Path) -> Option<String> {
    let rel = a.trim_start_matches("./");
    if !a.contains('/') || a.contains('{') || !adage_core::repo::is_safe_relative(rel) || a.starts_with('/') {
        return None;
    }
    let p = root.join(rel);
    let m = std::fs::symlink_metadata(&p).ok()?;
    m.file_type().is_file().then(|| a.to_string())
}

fn hash_file(p: &Path) -> Option<String> {
    let bytes = std::fs::read(p).ok()?;
    (bytes.len() <= 1 << 20).then(|| crate::sha256_hex(&bytes))
}

/// The output fields later routes read from this command's JSON output: for each route in the
/// repository triggered after this command with a result-field slot, a repository-path list.
pub fn implicit_outputs(ir: &IndexedRoute, index: &Index, table: &CommandTable) -> BTreeMap<String, FieldSpec> {
    let Step::Run { command } = &ir.route.step else { return BTreeMap::new() };
    // The key is value-free: every slot is filled with the same plain token.
    let sample = crate::request::slots(&command.split_whitespace().map(String::from).collect::<Vec<_>>())
        .iter()
        .fold(command.clone(), |c, s| c.replace(&format!("{{{s}}}"), "x1"));
    let key = adage_runtime::command_key(&sample, table);
    let mut out = BTreeMap::new();
    for r in index.routes.iter().filter(|r| r.repo == ir.repo) {
        let Trigger::AfterCommand { command: trig } = &r.route.trigger else { continue };
        if !adage_core::pattern::meets(trig, &key) {
            continue;
        }
        let max = match &r.route.step {
            Step::ReadBatch { max, .. } => Some(*max),
            _ => None,
        };
        for s in &r.route.slots {
            if let (SlotSource::ResultField { field }, Contract::RepoPath) = (&s.source, &s.contract) {
                if field.starts_with('*') || !crate::request::safe_id(&s.name) {
                    continue;
                }
                out.insert(
                    s.name.clone(),
                    FieldSpec {
                        ty: FieldType::RepoPathList,
                        json: (field != &s.name).then(|| field.clone()),
                        max: max.map(|m| m.clamp(1, crate::request::MAX_LIST)),
                        values: vec![],
                        regex: None,
                        min: None,
                        max_value: None,
                        optional: false,
                    },
                );
            }
        }
    }
    out
}

/// A pack route's typed outputs and exits, read from the installed pack (`tracks/@imported/<name>`).
/// `None` when the pack cannot be read or the route has no single run step.
pub fn pack_terms(tracks: &Path, ir: &IndexedRoute) -> Option<Terms> {
    let dir = tracks.join(adage_tracks::index::IMPORTED_DIR).join(&ir.track);
    let pack = adage_pack::Pack::load(&dir).ok()?;
    let r = pack.routes.iter().find(|r| r.id == ir.route.id)?;
    let step = r.steps.first()?;
    let mut fields = BTreeMap::new();
    for (name, o) in &step.outputs {
        let spec = field_of_type(&o.ty, o.max, o.values.clone(), &pack.types)?;
        fields.insert(name.clone(), FieldSpec { json: (o.from.json != *name).then(|| o.from.json.clone()), ..spec });
    }
    let exit_when = r
        .exit
        .iter()
        .filter_map(|e| match e {
            adage_pack::ExitV2::ResultEquals { step: s, field, value } if s == &step.id => {
                Some((field.clone(), value.clone()))
            }
            _ => None,
        })
        .collect();
    let (timeout_ms, raw) = match &r.exec {
        Some(x) => (Some(x.timeout_ms as u64), x.output == adage_pack::OutputMode::Raw),
        None => (None, false),
    };
    Some(Terms { fields, exit_when, timeout_ms, raw })
}

fn field_of_type(
    ty: &str,
    max: Option<u32>,
    values: Option<Vec<String>>,
    types: &BTreeMap<String, adage_pack::TypeDef>,
) -> Option<FieldSpec> {
    let base = FieldSpec {
        ty: FieldType::RepoPath,
        json: None,
        max: None,
        values: vec![],
        regex: None,
        min: None,
        max_value: None,
        optional: false,
    };
    Some(match ty {
        "repo-path" => base,
        "repo-path-list" => FieldSpec { ty: FieldType::RepoPathList, max: max.map(|m| m as usize), ..base },
        "git-rev" => FieldSpec { ty: FieldType::GitRev, ..base },
        "int" => FieldSpec { ty: FieldType::Int, ..base },
        "bool" => FieldSpec { ty: FieldType::Bool, ..base },
        "enum" => FieldSpec { ty: FieldType::Enum, values: values?, ..base },
        other => {
            let t = types.get(other)?;
            match t.base {
                adage_pack::Base::String => FieldSpec { ty: FieldType::Pattern, regex: t.pattern.clone(), ..base },
                adage_pack::Base::Int => FieldSpec {
                    ty: FieldType::Int,
                    min: t.range.map(|r| r[0]),
                    max_value: t.range.map(|r| r[1]),
                    ..base
                },
                adage_pack::Base::Enum => FieldSpec { ty: FieldType::Enum, values: t.values.clone()?, ..base },
                adage_pack::Base::List if t.item.as_deref() == Some("repo-path") => {
                    FieldSpec { ty: FieldType::RepoPathList, max: max.or(t.max).map(|m| m as usize), ..base }
                }
                _ => return None,
            }
        }
    })
}

/// Derive the execution request for `ir` at `level`, in the checkout at `root`. `terms` are what
/// a person declared at approval (used for a local route's output when given). `tracks` is the
/// tracks directory, where a pack route's pack is read.
pub fn plan(
    ir: &IndexedRoute,
    index: &Index,
    root: &Path,
    tracks: &Path,
    level: Level,
    terms: Option<&Terms>,
    table: &CommandTable,
) -> Result<Plan, String> {
    let Step::Run { command } = &ir.route.step else {
        return Err("only a route that runs a command can be prefetched".into());
    };
    if matches!(ir.route.trigger, Trigger::AfterEdit { .. }) {
        return Err("a finish-line step changes the repository; it never runs at E1 or E2".into());
    }
    let argv: Vec<String> = command.split_whitespace().map(String::from).collect();
    let used = crate::request::slots(&argv);
    let mut contracts = BTreeMap::new();
    let mut repo_paths = std::collections::BTreeSet::new();
    for name in &used {
        let s = ir.route.slots.iter().find(|s| &s.name == name).ok_or(format!("slot {{{name}}} is not declared"))?;
        match (&s.source, &s.contract, level) {
            (SlotSource::Prompt { .. }, Contract::Pattern { regex }, _) => {
                contracts.insert(name.clone(), regex.clone());
            }
            // At E2 the agent supplies every input as a tool argument, so where the route binds
            // it from at E0 does not matter; its contract does.
            (_, Contract::Pattern { regex }, Level::E2 | Level::E3) => {
                contracts.insert(name.clone(), regex.clone());
            }
            (_, Contract::RepoPath, Level::E2 | Level::E3) => {
                contracts.insert(name.clone(), REPO_PATH_CONTRACT.to_string());
                repo_paths.insert(name.clone());
            }
            (_, _, Level::E1) => {
                return Err(format!("slot {{{name}}} has no E1 form (a prompt value with a pattern contract)"))
            }
        }
    }
    let from_pack = ir.exec.as_ref().map(|_| pack_terms(tracks, ir));
    let (scripts, origin, digest, pack) = match (&ir.exec, from_pack) {
        (Some(b), Some(Some(t))) => {
            if b.max_level < adage_tracks::index::Level::E1 {
                return Err("the pack binds this route at E0 only".into());
            }
            let scripts: Vec<ScriptRef> = b
                .scripts
                .iter()
                .filter(|(p, _)| argv.iter().any(|a| a.trim_start_matches("./") == p.trim_start_matches("./")))
                .map(|(p, h)| ScriptRef { path: p.clone(), sha256: h.clone() })
                .collect();
            let origin = Origin::Pack { signer: None, trusted: false, probation_passed: false };
            (scripts, origin, Some(b.route_exec_digest.clone()), Some(t))
        }
        (Some(_), _) => return Err("the route's pack cannot be read".into()),
        (None, _) => {
            let mut scripts = Vec::new();
            for a in &argv {
                if let Some(p) = is_script_path(a, root) {
                    let h =
                        hash_file(&root.join(p.trim_start_matches("./"))).ok_or(format!("script {p} is unreadable"))?;
                    scripts.push(ScriptRef { path: p, sha256: h });
                }
            }
            (scripts, Origin::Local, None, None)
        }
    };
    let terms = pack.as_ref().or(terms);
    let fields = match terms {
        Some(t) if !t.fields.is_empty() => t.fields.clone(),
        _ => implicit_outputs(ir, index, table),
    };
    let mode = if terms.is_some_and(|t| t.raw) { OutputMode::Raw } else { OutputMode::Typed };
    if mode == OutputMode::Typed && fields.is_empty() {
        return Err("no typed output: declare fields at approval (--fields) or a later route that reads them".into());
    }
    let mut profile = Profile::for_level(level);
    if let Some(ms) = terms.and_then(|t| t.timeout_ms) {
        profile.limits = profile.limits.with_timeout(level, ms);
    }
    let exit_when = terms.map(|t| t.exit_when.clone()).unwrap_or_default();
    Ok(Plan {
        request: ExecRequest {
            repo_root: root.to_path_buf(),
            argv,
            inputs: BTreeMap::new(),
            contracts,
            effect: Effect::Observe,
            level,
            output: OutputSpec { mode, fields },
            scripts,
            profile,
            labels: Labels {
                track: format!("{}@{}", ir.track, ir.version),
                route: ir.route.id.clone(),
                step: STEP.into(),
            },
            origin,
            route_exec_digest: digest,
        },
        exit_when,
        repo_paths,
    })
}

/// The values a directive bound, read back from its text: the indented command line, aligned
/// with the template word by word. `None` unless every literal word matches and every slot word
/// yields a value.
pub fn bound_inputs(template: &[String], text: &str) -> Option<BTreeMap<String, String>> {
    let line = text.lines().find(|l| l.starts_with("  "))?;
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.len() != template.len() {
        return None;
    }
    let mut out = BTreeMap::new();
    for (t, w) in template.iter().zip(words) {
        let names = crate::request::slots(std::slice::from_ref(t));
        match names.as_slice() {
            [] if t == w => {}
            [name] => {
                let pat = format!("{{{name}}}");
                let (pre, post) = t.split_once(&pat)?;
                let v = w.strip_prefix(pre)?.strip_suffix(post)?;
                if v.is_empty() || out.get(name).is_some_and(|x: &String| x != v) {
                    return None;
                }
                out.insert(name.clone(), v.to_string());
            }
            _ => return None,
        }
    }
    Some(out)
}

/// The index entry for a directive, if the index holds it for this repository.
pub fn find<'a>(index: &'a Index, repo: &str, track: &str, route: &str) -> Option<&'a IndexedRoute> {
    index.routes.iter().find(|r| r.repo == repo && r.track == track && r.route.id == route)
}

/// Where the installed tracks live under a data directory.
pub fn tracks_dir(home: &Path) -> PathBuf {
    adage_store::Paths::at(home).tracks()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn bound_values_are_read_back_from_the_directive() {
        let t = v(&["python3", "scripts/wi-lookup", "{id}", "--json"]);
        let text = "In this repository, work item details are looked up with:\n  python3 scripts/wi-lookup W-2042 --json\nIgnore this.";
        assert_eq!(bound_inputs(&t, text).unwrap(), [("id".to_string(), "W-2042".to_string())].into());
        assert!(bound_inputs(&t, "x\n  python3 scripts/other W-2042 --json").is_none());
        assert!(bound_inputs(&t, "x\n  python3 scripts/wi-lookup W-2042").is_none());
        let t = v(&["tool", "--id={id}"]);
        assert_eq!(bound_inputs(&t, "x\n  tool --id=W-1").unwrap()["id"], "W-1");
    }
}
