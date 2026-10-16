//! Discover declared project checks without certifying script effects or execution outcomes.
//! A declaration authored after a request is useful for future investigation, but cannot prove
//! the earlier request used that configuration. All joins retain their order and native evidence.

use super::procedure_group;
use adage_core::{shell, CanonicalAction};
use adage_trace::{By, Session, StepObs};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

const MAX_DOCUMENT: usize = 256 * 1024;
const MAX_CALLS: usize = 4096;
const MAX_DECLARATIONS: usize = 128;
const MAX_OBSERVATIONS: usize = 24;
const MAX_CANDIDATES: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationOrigin {
    Read,
    Authored,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclarationTiming {
    BeforeInvocation,
    AfterInvocation,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeReference {
    pub step: usize,
    pub turn: u32,
    pub call_id: String,
    pub input_sha256: String,
    pub output_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValidatorObservation {
    pub session: String,
    pub group: Option<String>,
    pub parent: Option<String>,
    pub declaration: NativeReference,
    pub invocation: NativeReference,
    pub declaration_origin: DeclarationOrigin,
    pub declaration_timing: DeclarationTiming,
    pub document_sha256: String,
    pub declared_role: String,
    /// Which declarative label or conventional script filename supplied the role claim.
    pub declared_role_basis: String,
    /// A native request is not proof that the script completed, even with failed=false.
    pub invocation_shape: String,
    pub outcome: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeclaredValidator {
    pub id: String,
    pub name: String,
    pub command_key: String,
    pub declaration_path: String,
    pub declaration_field: String,
    pub script: String,
    pub working_directory: String,
    /// Kept separately: an absolute interpreter and a PATH lookup are not equivalent binaries.
    pub declared_argv: Vec<String>,
    pub observed_argv: Vec<String>,
    pub sessions: usize,
    pub evidence_groups: usize,
    pub observations: Vec<ValidatorObservation>,
    pub observations_omitted: usize,
    pub status: String,
    pub requirements: Vec<String>,
}

struct NativeCall<'a> {
    step: usize,
    steps: &'a [StepObs],
}

impl NativeCall<'_> {
    fn first(&self) -> &StepObs {
        &self.steps[0]
    }

    fn reference(&self) -> NativeReference {
        let st = self.first();
        NativeReference {
            step: self.step,
            turn: st.turn,
            call_id: st.call.tool_use_id.clone().unwrap_or_default(),
            input_sha256: hash(serde_json::to_vec(&st.call.input).unwrap_or_default()),
            output_sha256: hash(&st.call.output),
        }
    }
}

fn hash(bytes: impl AsRef<[u8]>) -> String {
    format!("{:x}", Sha256::digest(bytes.as_ref()))
}

fn calls(s: &Session) -> Vec<NativeCall<'_>> {
    if s.influence.is_empty() && (s.steered || !s.directed.is_empty()) {
        return Vec::new();
    }
    let mut ids: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, st) in s.steps.iter().enumerate() {
        if let Some(id) = st.call.tool_use_id.as_deref().filter(|id| !id.is_empty() && id.len() <= 256) {
            ids.entry(id).or_default().push(i);
        }
    }
    let mut out = Vec::new();
    for indices in ids.values() {
        let start = indices[0];
        let first = &s.steps[start];
        if indices.len() != indices[indices.len() - 1] - start + 1
            || indices.iter().any(|&i| !first.same_native_call(&s.steps[i]))
            || first.call.by != By::Agent
            || indices.iter().any(|&i| s.steps[i].call.refused())
            || s.influence
                .iter()
                .any(|d| d.at_step as usize <= start && (d.all || !d.commands.is_empty() || !d.avoided.is_empty()))
        {
            continue;
        }
        out.push(NativeCall { step: start, steps: &s.steps[start..=indices[indices.len() - 1]] });
    }
    out.sort_by_key(|c| c.step);
    out.truncate(MAX_CALLS);
    out
}

/// Only fixed repository paths. No traversal, interpolation, symlink or checkout claims.
fn relative(path: &str) -> Option<String> {
    if path.is_empty() || path.len() > 1024 || path.contains(['$', '`', '\\', '\0', '\n', '\r']) {
        return None;
    }
    let mut parts = Vec::new();
    for c in Path::new(path).components() {
        match c {
            Component::CurDir => {}
            Component::Normal(s) => parts.push(s.to_str()?),
            _ => return None,
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn under_root(s: &Session, path: &str) -> Option<String> {
    let path = Path::new(path);
    if !s.root.is_absolute() || !path.is_absolute() {
        return None;
    }
    let rel = path.strip_prefix(&s.root).ok()?.to_str()?;
    if rel.is_empty() {
        Some(".".into())
    } else {
        relative(rel)
    }
}

fn joined(cwd: &str, path: &str) -> Option<String> {
    relative(&format!("{cwd}/{path}"))
}

fn text(v: &Value, key: &str) -> Option<String> {
    v.get(key)?.as_str().map(str::to_string)
}

fn role(s: &str) -> bool {
    s.len() <= 256
        && s.split(|c: char| !c.is_ascii_alphabetic()).any(|w| {
            matches!(
                w.to_ascii_lowercase().as_str(),
                "test" | "tests" | "check" | "checks" | "verify" | "validation" | "validate" | "lint" | "audit"
            )
        })
}

fn display_label(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).take(120).collect()
}

#[derive(Clone)]
struct Script {
    family: String,
    path: String,
    args: Vec<String>,
}

fn script(argv: &[String], cwd: &str) -> Option<Script> {
    if argv.is_empty() || argv.len() > 32 || argv.iter().any(|v| v.len() > 1024) {
        return None;
    }
    let program = Path::new(&argv[0]).file_name()?.to_str()?;
    let interpreter = matches!(program, "node" | "python" | "python3" | "bash" | "sh");
    let (family, file, rest) = if interpreter {
        (program.to_string(), argv.get(1)?.as_str(), &argv[2..])
    } else {
        if !argv[0].contains('/') || Path::new(&argv[0]).is_absolute() {
            return None;
        }
        ("direct-script".into(), argv[0].as_str(), &argv[1..])
    };
    if file.starts_with('-') {
        return None;
    }
    Some(Script { family, path: joined(cwd, file)?, args: rest.to_vec() })
}

struct Declaration {
    call: usize,
    source: String,
    field: String,
    role: String,
    role_basis: String,
    origin: DeclarationOrigin,
    document_hash: String,
    argv: Vec<String>,
    cwd: String,
    script: Script,
}

fn one_command(command: &str) -> Option<Vec<String>> {
    let stages = shell::plain_pipeline(command.trim())?;
    (stages.len() == 1).then(|| stages[0].argv.clone())
}

/// Output of exactly one literal cat, or a natively acknowledged full Write. No grep, sed,
/// concatenated output, source-code extraction, transformed JSON or reconstruction of edits.
fn document(s: &Session, c: &NativeCall<'_>) -> Option<(String, String, DeclarationOrigin)> {
    let st = c.first();
    if c.steps.iter().any(|s| s.call.failed) || c.steps.len() != 1 {
        return None;
    }
    let (path, body, origin) = match &st.norm.action {
        CanonicalAction::Write { path } if st.call.tool == "Write" => {
            if st.call.input.get("file_path")?.as_str()? != path {
                return None;
            }
            let acknowledgment = format!("File created successfully at: {path}");
            if st.call.output != acknowledgment
                && st.call.output
                    != format!("{acknowledgment} (file state is current in your context — no need to Read it back)")
            {
                return None;
            }
            (path.as_str(), st.call.input.get("content")?.as_str()?, DeclarationOrigin::Authored)
        }
        CanonicalAction::Read { path, range: None } if st.call.tool == "Bash" => {
            let argv = one_command(st.call.input.get("command")?.as_str()?)?;
            if argv.len() != 2 || argv[0] != "cat" || argv[1].starts_with('-') {
                return None;
            }
            // Resolve against the recorded root, not a live checkout or a guessed shell cwd.
            if shell::resolve_path(s.root.to_str()?, &argv[1]) != *path {
                return None;
            }
            (path.as_str(), st.call.output.as_str(), DeclarationOrigin::Read)
        }
        _ => return None,
    };
    if body.is_empty() || body.len() > MAX_DOCUMENT || body.contains('\0') {
        return None;
    }
    Some((under_root(s, path)?, body.into(), origin))
}

fn declarations(s: &Session, calls: &[NativeCall<'_>]) -> Vec<Declaration> {
    let mut out = Vec::new();
    for (at, c) in calls.iter().enumerate() {
        let Some((source, body, origin)) = document(s, c) else { continue };
        let package = Path::new(&source).file_name().is_some_and(|n| n == "package.json");
        let workflow = source.starts_with(".github/workflows/")
            && source.rsplit('/').count() == 3
            && (source.ends_with(".yml") || source.ends_with(".yaml"));
        if !package && !workflow {
            continue;
        }
        // Strict JSON syntax for package manifests, plus the YAML decoder's duplicate-key
        // rejection. Neither parser evaluates scripts, expressions, imports or configuration.
        if package && serde_json::from_str::<serde_json::Value>(&body).is_err() {
            continue;
        }
        let Ok(value) = serde_yaml::from_str::<Value>(&body) else { continue };
        let mut add = |field: String, labels: Vec<(&str, String)>, command: String, cwd: String| {
            if out.len() >= MAX_DECLARATIONS {
                return;
            }
            let Some(argv) = one_command(&command) else { return };
            let Some(script) = script(&argv, &cwd) else { return };
            // Labels and conventional filenames are discovery clues, never proof of effects
            // or success. Retain the exact source of the claim instead of inferring it from
            // arbitrary directory names, command descriptions or output substrings.
            let filename = Path::new(&script.path).file_name().and_then(|p| p.to_str()).unwrap_or("");
            let Some((basis, name)) =
                labels.into_iter().chain([("script_filename", filename.into())]).find(|(_, name)| role(name))
            else {
                return;
            };
            out.push(Declaration {
                call: at,
                source: source.clone(),
                field,
                role: name,
                role_basis: basis.into(),
                origin,
                document_hash: hash(&body),
                argv,
                cwd,
                script,
            });
        };
        if package {
            let cwd = Path::new(&source).parent().and_then(Path::to_str).filter(|p| !p.is_empty()).unwrap_or(".");
            if let Some(scripts) = value.get("scripts").and_then(Value::as_mapping) {
                for (name, command) in scripts {
                    if let (Some(name), Some(command)) = (name.as_str(), command.as_str()) {
                        add(
                            format!("/scripts/{}", pointer(name)),
                            vec![("package_script_name", name.into())],
                            command.into(),
                            cwd.into(),
                        );
                    }
                }
            }
        } else if let Some(jobs) = value.get("jobs").and_then(Value::as_mapping) {
            for (job_id, job) in jobs {
                let Some(job_id) = job_id.as_str() else { continue };
                let Some(steps) = job.get("steps").and_then(Value::as_sequence) else { continue };
                for (i, step) in steps.iter().enumerate().take(256) {
                    let Some(command) = text(step, "run") else { continue };
                    let mut labels = Vec::new();
                    if let Some(name) = text(step, "name") {
                        labels.push(("ci_step_name", name));
                    }
                    if let Some(name) = text(job, "name") {
                        labels.push(("ci_job_name", name));
                    }
                    labels.push(("ci_job_id", job_id.into()));
                    let cwd = step
                        .get("working-directory")
                        .or_else(|| {
                            job.get("defaults").and_then(|v| v.get("run")).and_then(|v| v.get("working-directory"))
                        })
                        .or_else(|| {
                            value.get("defaults").and_then(|v| v.get("run")).and_then(|v| v.get("working-directory"))
                        });
                    let cwd = match cwd {
                        None => ".".into(),
                        Some(Value::String(s)) if s == "." => ".".into(),
                        Some(Value::String(s)) => match relative(s) {
                            Some(s) => s,
                            None => continue,
                        },
                        _ => continue,
                    };
                    add(format!("/jobs/{}/steps/{i}/run", pointer(job_id)), labels, command, cwd);
                }
            }
        }
    }
    out
}

fn pointer(s: &str) -> String {
    s.replace('~', "~0").replace('/', "~1")
}

pub(super) fn learn(sessions: &[&Session]) -> Vec<DeclaredValidator> {
    let mut found: BTreeMap<String, DeclaredValidator> = BTreeMap::new();
    let mut support: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>, usize)> = BTreeMap::new();
    let mut unique: BTreeMap<(&str, &str), Option<&Session>> = BTreeMap::new();
    for s in sessions {
        unique
            .entry((&s.repo, &s.id))
            .and_modify(|prior| {
                if *prior != Some(*s) {
                    *prior = None;
                }
            })
            .or_insert(Some(s));
    }
    for s in unique.into_values().flatten() {
        let calls = calls(s);
        let declarations = declarations(s, &calls);
        for c in &calls {
            let st = c.first();
            if st.call.tool != "Bash" {
                continue;
            }
            let Some(command) = st.call.input.get("command").and_then(serde_json::Value::as_str) else { continue };
            let Some(stages) = shell::plain_pipeline(command) else { continue };
            let Some(stage) = stages.first() else { continue };
            let Some((argv, cwd, key)) = c.steps.iter().find_map(|st| match &st.norm.action {
                CanonicalAction::Exec { argv, cwd } if *argv == stage.argv => Some((argv, cwd, &st.key)),
                _ => None,
            }) else {
                continue;
            };
            let Some(cwd) = under_root(s, cwd) else { continue };
            let Some(actual) = script(argv, &cwd) else { continue };
            for declaration in &declarations {
                let dc = &calls[declaration.call];
                if dc.first().turn == st.turn
                    || (dc.step < c.step) != (dc.first().turn < st.turn)
                    || dc.first().prompt != st.prompt
                    || dc.first().call.agent != st.call.agent
                    || declaration.cwd != cwd
                    || declaration.script.family != actual.family
                    || declaration.script.path != actual.path
                    || declaration.script.args != actual.args
                {
                    continue;
                }
                let identity = serde_json::to_vec(&(
                    &s.repo,
                    &declaration.source,
                    &declaration.field,
                    &declaration.argv,
                    argv,
                    &cwd,
                ))
                .unwrap_or_default();
                let id = format!("declared-validator-{}", &hash(identity)[..16]);
                if !found.contains_key(&id) && found.len() >= MAX_CANDIDATES {
                    continue;
                }
                let proposal = found.entry(id.clone()).or_insert_with(|| DeclaredValidator {
                    id: id.clone(), name: format!("Investigate declared check: {}", display_label(&declaration.role)), command_key: key.clone(),
                    declaration_path: declaration.source.clone(), declaration_field: declaration.field.clone(),
                    script: actual.path.clone(), working_directory: cwd.clone(), declared_argv: declaration.argv.clone(),
                    observed_argv: argv.clone(), sessions: 0, evidence_groups: 0, observations: Vec::new(),
                    observations_omitted: 0, status: "needs_evidence_review".into(), requirements: vec![
                        "Confirm that the declared role is a validation obligation for the changed surface; labels are claims, not proof.".into(),
                        "Recheck the current declaration, script, dependencies, interpreter, environment and source revision; historical requests establish no current configuration or script purity.".into(),
                        "Bind exact targets and owned complete output before inferring a result. A later declaration cannot establish an earlier request's intent or configuration.".into(),
                        "Establish a useful trigger and independent evidence before promotion; this candidate is not an active reflex and grants no execution authority.".into(),
                    ],
                });
                let group = procedure_group(s);
                let (seen_sessions, seen_groups, count) = support.entry(id).or_default();
                seen_sessions.insert(s.id.clone());
                if let Some(g) = &group {
                    seen_groups.insert(g.clone());
                }
                *count += 1;
                if proposal.observations.len() >= MAX_OBSERVATIONS {
                    continue;
                }
                proposal.observations.push(ValidatorObservation {
                    session: s.id.clone(),
                    group,
                    parent: s.delegation.as_ref().map(|d| d.parent_id.clone()),
                    declaration: dc.reference(),
                    invocation: c.reference(),
                    declaration_origin: declaration.origin,
                    declaration_timing: if dc.step < c.step {
                        DeclarationTiming::BeforeInvocation
                    } else {
                        DeclarationTiming::AfterInvocation
                    },
                    document_sha256: declaration.document_hash.clone(),
                    declared_role: declaration.role.clone(),
                    declared_role_basis: declaration.role_basis.clone(),
                    invocation_shape: if stages.len() == 1 {
                        "single_command_request"
                    } else {
                        "pipeline_segment_request"
                    }
                    .into(),
                    outcome: "unknown".into(),
                });
            }
        }
    }
    for p in found.values_mut() {
        p.observations.sort_by(|a, b| {
            (&a.session, a.invocation.step, a.declaration.step).cmp(&(
                &b.session,
                b.invocation.step,
                b.declaration.step,
            ))
        });
        if let Some((sessions, groups, count)) = support.get(&p.id) {
            p.sessions = sessions.len();
            p.evidence_groups = groups.len();
            p.observations_omitted = count.saturating_sub(p.observations.len());
        }
    }
    found.into_values().collect()
}

#[cfg(test)]
#[path = "procedures_validators_tests.rs"]
mod tests;
