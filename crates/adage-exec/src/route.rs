//! A compiled route as an execution request (Track C §3.2, §4.1): what `adage review delegate`
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
//! - **E2 actions** (Track C §3.6): a finish-line reflex (after editing X, run Y) or any other
//!   verify or regenerate step, planned with its own effect, a writable-overlay profile, and the
//!   action's declared outputs and cache globs (the pack's `exec`, or declared at approval).

use adage_core::{CommandTable, Effect};
use adage_tracks::index::IndexedRoute;
use adage_tracks::{Contract, Index, SlotSource, Step, Trigger};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::request::{
    ActionKind, ActionSpec, ExecRequest, FieldSpec, FieldType, Labels, Level, Origin, OutputMode, OutputSpec, ScriptRef,
};
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
    /// E2: the globs the action may change (regenerate), and cache globs.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache: Vec<String>,
    /// E2: the effect a person approved for a step that runs a script (classified environment),
    /// when lower; the sandbox and the change-set check enforce it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect: Option<Effect>,
}

/// An E1-ready request (inputs unbound) and the exit predicates to evaluate on its output.
#[derive(Debug, Clone)]
pub struct Plan {
    pub request: ExecRequest,
    pub exit_when: BTreeMap<String, String>,
    /// Slots whose contract is a repository path (E2 only): the value must also name something
    /// that exists inside the repository, checked at run time.
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
                if !crate::request::safe_id(&s.name) {
                    continue;
                }
                // `*paths`: the repository paths the command's text output mentions (the learner's
                // pseudo-field for a plain-text lookup), read from its lines.
                let text = field == PATHS_FIELD;
                if field.starts_with('*') && !text {
                    continue;
                }
                out.insert(
                    s.name.clone(),
                    FieldSpec {
                        ty: FieldType::RepoPathList,
                        json: (!text && field != &s.name).then(|| field.clone()),
                        max: max.map(|m| m.clamp(1, crate::request::MAX_LIST)),
                        values: vec![],
                        regex: None,
                        min: None,
                        max_value: None,
                        optional: false,
                        text,
                        stdout: false,
                    },
                );
            }
        }
    }
    out
}

/// The learner's pseudo-field for "repository paths mentioned in the command's text output"
/// (`adage_learn::mine::PATHS_FIELD`, spelled here: this crate does not depend on the learner).
pub const PATHS_FIELD: &str = "*paths";

/// The text fields a lookup's default output carries, when its output has them: a work item's (or
/// an issue's, a ticket's) title, description, state or status, and acceptance criteria. Read
/// from whichever shape the output has, a JSON object's field or a `Title: ...` line; each is
/// bounded plain text, scanned, and optional ([`FieldType::Text`]).
pub const LOOKUP_TEXT_FIELDS: [&str; 5] = ["title", "description", "state", "status", "acceptance"];

/// The typed output of a lookup (a prompt-triggered observe step) that no later route reads and
/// no person declared: the repository paths its output mentions, read from its lines (a
/// `repo-path-list` field `files`, [`FieldSpec::text`]), and its text fields
/// ([`LOOKUP_TEXT_FIELDS`]), so the prefetch replaces the agent's own lookup instead of only
/// pointing at files. Only existing repository paths and scanned, bounded text pass, whatever the
/// lookup's output shape; every field is optional, but a result with none is withheld. `None` for
/// any other route.
pub fn lookup_outputs(ir: &IndexedRoute) -> Option<BTreeMap<String, FieldSpec>> {
    let lookup = matches!(ir.route.trigger, Trigger::Prompt { .. })
        && matches!(ir.route.step, Step::Run { .. })
        && ir.route.effect == adage_core::Effect::Observe;
    lookup.then(|| {
        let base = FieldSpec {
            ty: FieldType::RepoPathList,
            json: None,
            max: Some(crate::request::DEFAULT_LIST),
            values: vec![],
            regex: None,
            min: None,
            max_value: None,
            optional: true,
            text: true,
            stdout: false,
        };
        let mut out: BTreeMap<String, FieldSpec> = [("files".to_string(), base.clone())].into();
        for name in LOOKUP_TEXT_FIELDS {
            let f = FieldSpec { ty: FieldType::Text, max: Some(crate::request::DEFAULT_TEXT), ..base.clone() };
            out.insert(name.to_string(), f);
        }
        out
    })
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
    let (timeout_ms, raw, outputs, cache) = match &r.exec {
        Some(x) => {
            (Some(x.timeout_ms as u64), x.output == adage_pack::OutputMode::Raw, x.outputs.clone(), x.cache.clone())
        }
        None => (None, false, vec![], vec![]),
    };
    Some(Terms { fields, exit_when, timeout_ms, raw, outputs, cache, effect: None })
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
        text: false,
        stdout: false,
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
    if matches!(ir.route.trigger, Trigger::AfterEdit { .. }) && level != Level::E2 {
        return Err("a finish-line step changes the repository; it runs only as an E2 action".into());
    }
    let argv: Vec<String> = command.split_whitespace().map(String::from).collect();
    let project_python = match argv.first() {
        Some(program) => crate::project_python::identify(root, program)?,
        None => None,
    };
    if project_python.is_some() && (level != Level::E2 || ir.route.effect != Effect::Verify) {
        return Err("project Python is supported only for E2 verification".into());
    }
    let used = crate::request::slots(&argv);
    let mut contracts = BTreeMap::new();
    let mut repo_paths = std::collections::BTreeSet::new();
    for name in &used {
        let s = ir.route.slots.iter().find(|s| &s.name == name).ok_or(format!("slot {{{name}}} is not declared"))?;
        match (&s.source, &s.contract, level) {
            // A value from the prompt: a typed span (the older form), or the first prompt token a
            // learned value type accepts (what the learner emits). The runtime binds it
            // (`adage_core::vtype`, checked against the contract), and the directive carries the
            // bound value; here the contract is what the value must match again before it runs.
            (SlotSource::Prompt { .. }, Contract::Pattern { regex }, _) => {
                contracts.insert(name.clone(), regex.clone());
            }
            (SlotSource::PromptValue { vtype, .. }, Contract::Pattern { regex }, _) => {
                // `adage_tracks::io::validate` holds the type's regex to its contract; a track
                // edited by hand past that is refused here too.
                if vtype.regex != *regex || adage_core::vtype::to_regex(&vtype.runs) != *regex {
                    return Err(format!("slot {{{name}}}: its value type ({}) and its contract disagree", vtype.id));
                }
                contracts.insert(name.clone(), regex.clone());
            }
            // At E2 an action's inputs are bound from the stimulus (the edited path's segments),
            // so where the route binds them from at E0 does not matter; the contract does.
            (_, Contract::Pattern { regex }, Level::E2) => {
                contracts.insert(name.clone(), regex.clone());
            }
            (_, Contract::RepoPath, Level::E2) => {
                contracts.insert(name.clone(), REPO_PATH_CONTRACT.to_string());
                repo_paths.insert(name.clone());
            }
            (source, _, Level::E1) => {
                let from = match source {
                    SlotSource::Prompt { .. } | SlotSource::PromptValue { .. } => {
                        "the prompt, without a pattern contract"
                    }
                    SlotSource::ResultField { .. } | SlotSource::OutputValue { .. } => "a command's output",
                    SlotSource::EditSegment { .. } | SlotSource::EditTests { .. } => "an edited path",
                };
                return Err(format!(
                    "slot {{{name}}} is bound from {from}; E1 prefetches only a value from the prompt that a \
                     pattern contract checks"
                ));
            }
        }
    }
    let from_pack = ir.exec.as_ref().map(|_| pack_terms(tracks, ir));
    let (scripts, origin, digest, pack) = match (&ir.exec, from_pack) {
        (Some(b), Some(Some(t))) => {
            if b.max_level < adage_tracks::index::Level::E1 {
                return Err("the pack binds this route at E0 only".into());
            }
            let mut scripts: Vec<ScriptRef> = Vec::new();
            for (p, h) in &b.scripts {
                if let Some(hex) = p.strip_prefix("cas/") {
                    // L-vendor: argv names `scripts/sha256-<hex>`; the bytes run from the CAS.
                    let token = adage_pack::vendor::script_path(hex);
                    if argv.iter().skip(1).any(|a| a == &token) {
                        scripts.push(ScriptRef { path: format!("cas:{hex}"), sha256: h.clone() });
                    }
                } else if argv.iter().any(|a| a.trim_start_matches("./") == p.trim_start_matches("./")) {
                    scripts.push(ScriptRef { path: p.clone(), sha256: h.clone() });
                }
            }
            let origin = Origin::Pack { signer: None, trusted: false, forming_passed: false };
            (scripts, origin, Some(b.route_exec_digest.clone()), Some(t))
        }
        (Some(_), _) => return Err("the route's pack cannot be read".into()),
        (None, _) => {
            let mut scripts = Vec::new();
            let python_pytest = project_python.is_some()
                && argv.get(1).is_some_and(|a| a == "-m")
                && argv.get(2).is_some_and(|a| a == "pytest");
            for (position, a) in argv.iter().enumerate() {
                // pytest selection paths are mutable staged source inputs, not standalone
                // scripts. Hashing/shadowing them would lapse a grant on every test edit and
                // change pytest's configuration/conftest discovery root.
                if python_pytest || project_python.as_ref().is_some_and(|p| &p.program == a) {
                    continue;
                }
                let python_entrypoint = project_python.is_some()
                    && position == 1
                    && a.ends_with(".py")
                    && adage_core::repo::is_safe_relative(a)
                    && std::fs::symlink_metadata(root.join(a)).is_ok_and(|m| m.is_file());
                if let Some(p) = python_entrypoint.then(|| a.clone()).or_else(|| is_script_path(a, root)) {
                    let h =
                        hash_file(&root.join(p.trim_start_matches("./"))).ok_or(format!("script {p} is unreadable"))?;
                    scripts.push(ScriptRef { path: p, sha256: h });
                }
            }
            (scripts, Origin::Local, None, None)
        }
    };
    // The effect a person approved a scripted step at is theirs to give, for a pack route too.
    let approved_effect = terms.and_then(|t| t.effect);
    let terms = pack.as_ref().or(terms);
    let fields = match terms {
        Some(t) if !t.fields.is_empty() => t.fields.clone(),
        _ => {
            let implicit = implicit_outputs(ir, index, table);
            // A lookup nothing reads: the paths its output names, whatever its shape.
            if implicit.is_empty() && level == Level::E1 && pack.is_none() {
                lookup_outputs(ir).unwrap_or_default()
            } else {
                implicit
            }
        }
    };
    let mode = if terms.is_some_and(|t| t.raw) { OutputMode::Raw } else { OutputMode::Typed };
    if level == Level::E2 && mode == OutputMode::Raw {
        return Err("an E2 action returns its outcome and changed paths, never raw output".into());
    }
    if mode == OutputMode::Typed && fields.is_empty() && level != Level::E2 {
        return Err("no typed output: declare fields at approval (--fields) or a later route that reads them".into());
    }
    let (effect, action) = if level == Level::E2 {
        let effect = match approved_effect {
            Some(e) if !scripts.is_empty() && e < ir.route.effect => e,
            _ => ir.route.effect,
        };
        let kind = match effect {
            Effect::Verify => ActionKind::Verify,
            Effect::Regenerate | Effect::Environment => ActionKind::Regenerate,
            e => {
                return Err(format!("a step with the {} effect is not an E2 action (verify or regenerate)", e.as_str()))
            }
        };
        let (outputs, cache) = terms.map(|t| (t.outputs.clone(), t.cache.clone())).unwrap_or_default();
        let action = ActionSpec { kind, outputs, cache };
        action.validate().map_err(|e| format!("{e} (declare them at approval: --outputs <glob>)"))?;
        (effect, Some(action))
    } else {
        (Effect::Observe, None)
    };
    let mut profile = Profile::for_level(level);
    if let Some(ms) = terms.and_then(|t| t.timeout_ms) {
        profile.limits = profile.limits.with_timeout(level, ms);
    }
    let exit_when = terms.map(|t| t.exit_when.clone()).unwrap_or_default();
    let plan = Plan {
        request: ExecRequest {
            repo_root: root.to_path_buf(),
            project_python,
            argv,
            inputs: BTreeMap::new(),
            contracts,
            effect,
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
            action,
        },
        exit_when,
        repo_paths,
    };
    crate::project_python::validate_command(&plan.request)?;
    Ok(plan)
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

/// The values a bound command line holds for a template's slots (an obligation's command, bound
/// from the edited path), aligned word by word as [`bound_inputs`] aligns a directive's.
pub fn bound_from_command(template: &[String], command: &str) -> Option<BTreeMap<String, String>> {
    bound_inputs(template, &format!("\n  {command}"))
}

/// Execution needs one unambiguous value per prompt slot. Advisory binding may choose the first
/// mention; a CPU action must abstain instead. Use the same tokenizer, mention constraints and
/// value types as the shared runtime, and refuse prompts beyond that runtime's bounded scan.
pub fn unique_prompt_inputs(
    route: &adage_tracks::Route,
    event: &adage_runtime::Event,
    inputs: &BTreeMap<String, String>,
) -> Result<(), String> {
    if route.slots.is_empty() {
        return Ok(());
    }
    let adage_runtime::Event::Prompt { text } = event else {
        return Err("prompt inputs require a prompt event".into());
    };
    use adage_core::tokens::{with_mentions, MAX_BYTES, PROMPT_TOKENS};
    let tokens = with_mentions(text, PROMPT_TOKENS + 1);
    if text.len() > MAX_BYTES || tokens.len() > PROMPT_TOKENS {
        return Err("prompt exceeds the complete input scan limit".into());
    }
    let spans = adage_core::spans::extract(text);
    for slot in &route.slots {
        let Contract::Pattern { regex } = &slot.contract else {
            return Err(format!("slot {} has no prompt pattern contract", slot.name));
        };
        let re = regex::Regex::new(regex).map_err(|_| format!("slot {} has an invalid pattern", slot.name))?;
        let accepted = |v: &str| re.is_match(v) && crate::request::plain_token(v);
        let values: std::collections::BTreeSet<&str> = match &slot.source {
            SlotSource::PromptValue { vtype, mentions } => tokens
                .iter()
                .filter(|(t, m)| {
                    (mentions.is_empty() || mentions.contains(m))
                        && adage_core::vtype::matches(&vtype.runs, &t.text)
                        && accepted(&t.text)
                })
                .map(|(t, _)| t.text.as_str())
                .collect(),
            SlotSource::Prompt { span } => {
                spans.iter().filter(|s| s.kind == *span && accepted(&s.value)).map(|s| s.value.as_str()).collect()
            }
            _ => return Err(format!("slot {} is not a prompt input", slot.name)),
        };
        if values.len() != 1 || inputs.get(&slot.name).is_none_or(|v| !values.contains(v.as_str())) {
            return Err(format!("slot {} has no unique matching prompt value", slot.name));
        }
    }
    Ok(())
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

    /// The learned lookup as the graph learner emits it (the launch report's acme case): a prompt
    /// value of a learned type, not a prompt span.
    fn learned_lookup(slot: adage_tracks::Slot) -> Index {
        use adage_tracks::*;
        let route = Route {
            id: "lookup-work-item".into(),
            summary: String::new(),
            trigger: Trigger::Prompt { span: adage_core::spans::SpanKind::Ticket },
            step: Step::Run { command: "python3 scripts/wi-lookup {item}".into() },
            slots: vec![slot],
            effect: Effect::Observe,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence { support: 9, provenance: "desire-path".into(), ..Default::default() },
            authored: false,
        };
        let manifest = Manifest {
            track: TrackInfo {
                name: "acme-billing/learned".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme-billing".into() },
            gauge: Gauge::default(),
        };
        Index::compile(&[Track { manifest, routes: vec![route] }])
    }

    /// Bug 3 (launch report): `adage review delegate acme-billing/learned lookup-work-item --level
    /// e1` refused every learned lookup with "slot {item} has no E1 form (a prompt value with a
    /// pattern contract)", which described the very slot it refused.
    #[test]
    fn a_learned_prompt_value_lookup_plans_at_e1() {
        use adage_core::vtype::{Class, Run, Runs};
        let runs = Runs {
            branches: vec![vec![
                Run::Lit { text: "W".into() },
                Run::Lit { text: "-".into() },
                Run::Class { class: Class::D, lo: 4, hi: 4 },
            ]],
        };
        let regex = adage_core::vtype::to_regex(&runs);
        let vtype = adage_tracks::ValueType {
            id: "vt-0123456789abcdef".into(),
            runs,
            regex: regex.clone(),
            samples_hash: String::new(),
        };
        let slot = |source| adage_tracks::Slot {
            name: "item".into(),
            source,
            contract: Contract::Pattern { regex: regex.clone() },
        };
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("scripts")).unwrap();
        std::fs::write(d.path().join("scripts/wi-lookup"), "print('x')\n").unwrap();
        let table = CommandTable::load();
        let index = learned_lookup(slot(SlotSource::PromptValue { vtype: vtype.clone(), mentions: vec![] }));
        let ir = &index.routes[0];
        let p = plan(ir, &index, d.path(), d.path(), Level::E1, None, &table).expect("plans at E1");
        assert_eq!(p.request.contracts["item"], regex);
        assert!(p.request.output.fields.contains_key("title") && p.request.output.fields.contains_key("files"));
        let text = "In this repository, work item details are looked up with:\n  python3 scripts/wi-lookup W-2042\nIgnore this.";
        let mut req = p.request.clone();
        req.inputs = bound_inputs(&req.argv, text).unwrap();
        assert_eq!(req.bind().unwrap(), ["python3", "scripts/wi-lookup", "W-2042"]);
        let prompt = |text: &str| adage_runtime::Event::Prompt { text: text.into() };
        for text in ["Pick up W-2042", "W-2042 then W-2042 again"] {
            assert!(unique_prompt_inputs(&ir.route, &prompt(text), &req.inputs).is_ok());
        }
        for text in ["Compare W-2042 and W-9999", "W-9999", "no matching value"] {
            assert!(unique_prompt_inputs(&ir.route, &prompt(text), &req.inputs).is_err());
        }
        // A matching value beyond the shared tokenizer's window must not silently disappear.
        let long = format!("W-2042 {} W-9999", "word ".repeat(adage_core::tokens::PROMPT_TOKENS));
        assert!(unique_prompt_inputs(&ir.route, &prompt(&long), &req.inputs).is_err());
        let mut mention = ir.route.clone();
        if let SlotSource::PromptValue { mentions, .. } = &mut mention.slots[0].source {
            mentions.push(adage_core::tokens::Mention { prefix: "#".into(), suffix: String::new() });
        }
        assert!(unique_prompt_inputs(&mention, &prompt("W-9999 and #W-2042"), &req.inputs).is_ok());
        assert!(unique_prompt_inputs(&mention, &prompt("#W-9999 and #W-2042"), &req.inputs).is_err());
        let mut span = ir.route.clone();
        span.slots[0].source = SlotSource::Prompt { span: adage_core::spans::SpanKind::Ticket };
        assert!(unique_prompt_inputs(&span, &prompt("W-2042 twice W-2042"), &req.inputs).is_ok());
        assert!(unique_prompt_inputs(&span, &prompt("W-2042 and W-9999"), &req.inputs).is_err());
        // A value the type does not accept never binds.
        req.inputs.insert("item".into(), "W-20421;id".into());
        assert!(req.bind().is_err());
        // A type whose regex disagrees with its contract is refused.
        let mut bad = vtype.clone();
        bad.regex = "^(?:W-[0-9]{5})$".into();
        let index = learned_lookup(slot(SlotSource::PromptValue { vtype: bad, mentions: vec![] }));
        assert!(plan(&index.routes[0], &index, d.path(), d.path(), Level::E1, None, &table)
            .unwrap_err()
            .contains("disagree"));
        // A slot from a command's output says where it comes from.
        let index = learned_lookup(slot(SlotSource::OutputValue { field: None, vtype }));
        let e = plan(&index.routes[0], &index, d.path(), d.path(), Level::E1, None, &table).unwrap_err();
        assert!(e.contains("bound from a command's output") && !e.contains("no E1 form"), "{e}");
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
