//! Convert a local track (`track.toml` + `routes/*.yaml`, the `adage_tracks` model that
//! `adage condition` writes) to a pack with no vendored scripts: L-none, or L-ref where a route
//! runs a repository script (Track C §1.1). `adage share` uses this; packs are the only format
//! that travels.
//!
//! | track trigger | track step | pack shape |
//! | --- | --- | --- |
//! | `prompt`, `intent`, `session_start` | `run` | `run` |
//! | `after_command`, `after_failure` | `run` | `after` |
//! | `after_edit` | `run` | `obligation` |
//! | any | `read_batch` | `read` |
//! | any | `avoid` | `avoid` |
//! | any | `advise` | `advise` |
//!
//! Slots become typed inputs: a `pattern` contract becomes a user type named by the pattern's
//! hash (`c-<12 hex>`, so identical contracts deduplicate), a `repo_path` contract becomes
//! `repo-path` (or `repo-path-list` for the slot a `read_batch` reads). A run step whose argv names
//! a repository script is pinned by the SHA-256 of that file under `repo_root` (L-ref). Without a
//! root, or when the file is missing, the route is skipped and reported: a pack never holds an
//! unpinned script reference. Per-machine state (`state`, `dormant_since`, `authored`) is dropped.

use crate::digest::sha256_hex;
use crate::model::*;
use crate::validate::{people_text_ok, program, safe_pack_name, Program};
use crate::Pack;
use adage_core::CommandTable;
use adage_tracks::{io, Contract, Exit, Origin, Route, SlotSource, Step, Track, Trigger};
use anyhow::{bail, Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// A pack converted from a local track, and the routes that could not be carried (id, reason).
#[derive(Debug, Clone)]
pub struct Converted {
    pub pack: Pack,
    pub skipped: Vec<(String, String)>,
}

/// Convert the track directory `dir` (read and validated with `adage_tracks::io::read`), with
/// no repository to pin scripts against.
pub fn from_track_dir(dir: &Path) -> Result<Converted> {
    from_track_dir_with(dir, None)
}

/// [`from_track_dir`], pinning repository scripts against `repo_root` (L-ref).
pub fn from_track_dir_with(dir: &Path, repo_root: Option<&Path>) -> Result<Converted> {
    let t = io::read(dir).with_context(|| format!("{} is not a valid track", dir.display()))?;
    from_track(&t, repo_root)
}

/// Convert an in-memory track.
pub fn from_track(t: &Track, repo_root: Option<&Path>) -> Result<Converted> {
    let table = CommandTable::load();
    let mut cx = Cx { types: BTreeMap::new(), repo_files: BTreeMap::new(), tools: BTreeSet::new(), table };
    let mut routes = Vec::new();
    let mut skipped = Vec::new();
    for r in &t.routes {
        // A skipped route leaves nothing behind (no type, tool or repository file of its own).
        let saved = (cx.types.clone(), cx.tools.clone(), cx.repo_files.clone());
        match cx.route(r, repo_root) {
            Ok(v2) => routes.push(v2),
            Err(why) => {
                (cx.types, cx.tools, cx.repo_files) = saved;
                skipped.push((r.id.clone(), why));
            }
        }
    }
    if routes.is_empty() {
        bail!(
            "no route of {} can be carried into a pack: {}",
            t.manifest.track.name,
            skipped.iter().map(|(id, why)| format!("{id}: {why}")).collect::<Vec<_>>().join("; ")
        );
    }
    let m = &t.manifest;
    let name = if m.track.name.contains('/') { m.track.name.clone() } else { format!("local/{}", m.track.name) };
    if !safe_pack_name(&name) {
        bail!("track name {:?} is not a pack name", m.track.name);
    }
    let kept: Vec<&Route> = t.routes.iter().filter(|r| routes.iter().any(|x: &RouteV2| x.id == r.id)).collect();
    let origin = if kept.iter().all(|r| r.is_instinct()) {
        PackOrigin::Instinct
    } else if m.track.provenance.starts_with("synthetic") || kept.iter().all(|r| r.origin() == Origin::Synthetic) {
        PackOrigin::Trained
    } else if m.track.provenance == "authored" || kept.iter().all(|r| r.origin() == Origin::Authored) {
        PackOrigin::Authored
    } else {
        PackOrigin::Reflex
    };

    let mut templates: BTreeSet<String> = routes.iter().map(|r| r.shape.template()).collect();
    templates.extend(
        routes
            .iter()
            .flat_map(|r| r.steps.iter().filter_map(|s| s.advise.as_ref().map(|a| format!("advise:{}@1", a.template)))),
    );
    let models: BTreeSet<String> = routes
        .iter()
        .filter_map(|r| match &r.trigger {
            TriggerV2::Intent { model, .. } => Some(model.clone()),
            _ => None,
        })
        .collect();
    let harness: Vec<String> = m
        .gauge
        .harness
        .iter()
        .filter(|h| {
            !h.is_empty()
                && h.len() <= 32
                && h.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
        })
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();

    let mut evidence = PackEvidence::new();
    let mut total = OriginCounts::default();
    for r in &kept {
        let e = &r.evidence;
        let get = |k: &str| e.by_origin.get(k).copied().unwrap_or(0) as u64;
        let c = OriginCounts {
            organic: e.support as u64,
            synthetic: get("synthetic"),
            declared: get("declared"),
            instinct: get("instinct"),
            imported: get("imported"),
        };
        total.organic += c.organic;
        total.synthetic += c.synthetic;
        total.declared += c.declared;
        total.instinct += c.instinct;
        total.imported += c.imported;
        evidence.insert(r.id.clone(), c);
    }

    let manifest = PackManifest {
        format: FORMAT.to_string(),
        pack: PackInfo {
            name,
            version: m.track.version.clone(),
            origin,
            description: if people_text_ok(&m.track.description, 1000) {
                m.track.description.clone()
            } else {
                String::new()
            },
        },
        applicability: Applicability::Repo { repo_keys: vec![m.scope.repo.clone()] },
        requires: Requires {
            os: Vec::new(),
            tools: cx.tools.iter().map(|n| ToolReq { name: n.clone(), version: None }).collect(),
            harness,
            models: models.into_iter().collect(),
            templates: templates.into_iter().collect(),
            command_table: None,
            repo_files: cx.repo_files.into_values().collect(),
        },
        exec: ExecCeiling::default(),
        provenance: Provenance { tool: format!("adage {}", env!("CARGO_PKG_VERSION")), origins: total, skill: None },
    };
    let mut pack = Pack::new(manifest, routes, cx.types, BTreeMap::new())
        .with_context(|| format!("converting {}", m.track.name))?;
    pack.evidence = Some(evidence);
    pack.check()?;
    Ok(Converted { pack, skipped })
}

struct Cx {
    types: BTreeMap<String, TypeDef>,
    repo_files: BTreeMap<String, RepoFile>,
    tools: BTreeSet<String>,
    table: CommandTable,
}

fn argv(command: &str) -> Vec<String> {
    command.split(' ').map(str::to_string).collect()
}

impl Cx {
    fn route(&mut self, r: &Route, root: Option<&Path>) -> std::result::Result<RouteV2, String> {
        #[allow(unreachable_patterns)]
        let trigger = match &r.trigger {
            Trigger::Prompt { span } => TriggerV2::Prompt { span: *span },
            Trigger::AfterCommand { command } => TriggerV2::AfterCommand { command: command.clone() },
            Trigger::AfterFailure { command, signature } => {
                TriggerV2::AfterFailure { command: command.clone(), signature: signature.clone() }
            }
            Trigger::AfterEdit { paths } => TriggerV2::AfterEdit { paths: paths.clone() },
            Trigger::SessionStart => TriggerV2::SessionStart,
            Trigger::Intent { family, model, centroid, threshold } => TriggerV2::Intent {
                family: family.clone(),
                model: model.clone(),
                centroid: centroid.clone(),
                threshold: *threshold,
            },
            other => return Err(format!("trigger {other:?} has no v2 form")),
        };
        let read_slot = match &r.step {
            Step::ReadBatch { slot, .. } => Some(slot.as_str()),
            _ => None,
        };
        let mut inputs = BTreeMap::new();
        for s in &r.slots {
            #[allow(unreachable_patterns)]
            let mut ty = match &s.contract {
                Contract::Pattern { regex } => {
                    let id = format!("c-{}", &sha256_hex(regex.as_bytes())[..12]);
                    self.types.entry(id.clone()).or_insert_with(|| TypeDef::pattern(&id, regex));
                    id
                }
                Contract::RepoPath if read_slot == Some(s.name.as_str()) => "repo-path-list".to_string(),
                Contract::RepoPath => "repo-path".to_string(),
                other => return Err(format!("slot {} has contract {other:?}, which has no v2 type", s.name)),
            };
            #[allow(unreachable_patterns)]
            let from = match &s.source {
                SlotSource::Prompt { span } => InputFrom {
                    prompt: Some(PromptSource { span: *span, vtype: None, mentions: vec![] }),
                    ..InputFrom::default()
                },
                // The samples' hash stays home: over a small id space it would give them away.
                SlotSource::PromptValue { vtype, mentions } => InputFrom {
                    prompt: Some(PromptSource {
                        span: adage_core::spans::SpanKind::Ticket,
                        vtype: Some(adage_tracks::ValueType { samples_hash: String::new(), ..vtype.clone() }),
                        mentions: mentions.clone(),
                    }),
                    ..InputFrom::default()
                },
                SlotSource::ResultField { field } => {
                    InputFrom { result: Some(ResultSource { field: field.clone() }), ..InputFrom::default() }
                }
                SlotSource::EditTests { k, map, .. } => {
                    ty = "repo-path-list".to_string();
                    InputFrom { edit_tests: Some(EditTestsSource { k: *k, map: map.clone() }), ..InputFrom::default() }
                }
                other => return Err(format!("slot {} has source {other:?}, which has no v2 form", s.name)),
            };
            inputs.insert(s.name.clone(), Input { ty, from, values: None, max: None });
        }
        let empty = || StepV2 {
            id: String::new(),
            run: None,
            read: None,
            avoid: None,
            advise: None,
            effect: r.effect,
            script: None,
            outputs: BTreeMap::new(),
        };
        let mut extra: Vec<StepV2> = Vec::new();
        #[allow(unreachable_patterns)]
        let (shape, step) = match &r.step {
            Step::Run { command } => {
                let argv = argv(command);
                let script = self.pin(&argv, root)?;
                let shape = match r.trigger {
                    Trigger::AfterCommand { .. } | Trigger::AfterFailure { .. } => Shape::After,
                    Trigger::AfterEdit { .. } => Shape::Obligation,
                    _ => Shape::Run,
                };
                (shape, StepV2 { id: "run".into(), run: Some(RunSpec { argv }), script, ..empty() })
            }
            Step::Sequence { commands } => {
                // One run step per command, in order (`run-1`, `run-2`, …).
                let mut steps = Vec::new();
                for (i, c) in commands.iter().enumerate() {
                    let argv = argv(c);
                    let script = self.pin(&argv, root)?;
                    steps.push(StepV2 { id: format!("run-{}", i + 1), run: Some(RunSpec { argv }), script, ..empty() });
                }
                let first = steps.remove(0);
                extra = steps;
                (Shape::Sequence, first)
            }
            Step::ReadBatch { slot, max } => (
                Shape::Read,
                StepV2 { id: "read".into(), read: Some(ReadSpec { from: slot.clone(), max: *max as u32 }), ..empty() },
            ),
            Step::Avoid { avoid, instead, signature } => (
                Shape::Avoid,
                StepV2 {
                    id: "avoid".into(),
                    avoid: Some(AvoidSpec {
                        runners: avoid.split(" / ").map(argv).collect(),
                        instead: argv(instead),
                        signature: signature.clone(),
                    }),
                    ..empty()
                },
            ),
            Step::Advise { template, slots } => (
                Shape::Advise,
                StepV2 {
                    id: "advise".into(),
                    advise: Some(AdviseSpec { template: template.clone(), slots: slots.clone() }),
                    ..empty()
                },
            ),
            other => return Err(format!("step {other:?} has no v2 form")),
        };
        #[allow(unreachable_patterns)]
        let exit = r
            .exit
            .iter()
            .filter_map(|e| match e {
                Exit::IntentChanges => Some(ExitV2::IntentChanges),
                Exit::ExploratoryChange => Some(ExitV2::ExploratoryChange),
                _ => None,
            })
            .collect();
        let e = &r.evidence;
        #[allow(unreachable_patterns)]
        let origin = e.origin.as_ref().map(|o| match o {
            Origin::Learned => RouteOrigin::Learned,
            Origin::Instinct { pack, version, instinct, ecosystem, declared } => RouteOrigin::Instinct {
                pack: pack.clone(),
                version: version.clone(),
                instinct: instinct.clone(),
                ecosystem: ecosystem.clone(),
                declared: declared.clone(),
            },
            Origin::Imported => RouteOrigin::Imported,
            Origin::Authored => RouteOrigin::Authored,
            Origin::Synthetic => RouteOrigin::Synthetic,
            _ => RouteOrigin::Imported,
        });
        let evidence = RouteEvidence {
            opportunities: e.opportunities,
            support: e.support,
            days: e.days,
            successes: e.successes,
            deviations_failed: e.deviations_failed,
            rate: e.rate,
            lower_bound: e.lower_bound,
            median_position: e.median_position,
            turns_saved: e.turns_saved,
            provenance: e.provenance.clone(),
            models: e.models.clone(),
            exposed: e.exposed,
            origin,
            by_origin: e.by_origin.clone(),
        };
        Ok(RouteV2 {
            id: r.id.clone(),
            summary: r.summary.clone(),
            shape,
            trigger,
            entry: true,
            inputs,
            steps: std::iter::once(step).chain(extra).collect(),
            links: Vec::new(),
            strength: r.strength,
            exit,
            exec: None,
            evidence: Some(evidence),
        })
    }

    /// The tool an argv needs, and the L-ref pin of the script it runs, if any.
    fn pin(&mut self, argv: &[String], root: Option<&Path>) -> std::result::Result<Option<ScriptRef>, String> {
        match program(argv, &self.table).map_err(|e| e.to_string())? {
            Program::Tool(t) => {
                self.tools.insert(t);
                Ok(None)
            }
            Program::Script { path, interpreter } => {
                let Some(root) = root else {
                    return Err(format!(
                        "runs the repository script {path}; pass the repository root (`adage share --repo-root <dir>`) to pin it (L-ref)"
                    ));
                };
                let sha = hash_repo_file(root, &path).map_err(|e| format!("cannot pin {path}: {e:#}"))?;
                if let Some(i) = interpreter {
                    self.tools.insert(i);
                }
                let f = RepoFile { path: path.clone(), sha256: sha.clone(), deps: Vec::new() };
                self.repo_files.insert(path.clone(), f);
                Ok(Some(ScriptRef { path, closure: ClosureLevel::Ref, sha256: sha, deps: Vec::new() }))
            }
        }
    }
}

/// SHA-256 of `root/<rel>`: a regular file (no symlink on the way), inside the root, at most
/// 1 MiB (Track C §2.3).
pub fn hash_repo_file(root: &Path, rel: &str) -> Result<String> {
    let mut p = root.to_path_buf();
    for seg in rel.split('/') {
        p.push(seg);
        let m = std::fs::symlink_metadata(&p).with_context(|| format!("{rel} does not exist"))?;
        if m.file_type().is_symlink() {
            bail!("{rel} passes through a symlink");
        }
    }
    let m = std::fs::metadata(&p)?;
    if !m.is_file() {
        bail!("{rel} is not a regular file");
    }
    if m.len() > 1 << 20 {
        bail!("{rel} is larger than 1 MiB");
    }
    let canon_root = root.canonicalize()?;
    if !p.canonicalize()?.starts_with(&canon_root) {
        bail!("{rel} is outside the repository");
    }
    Ok(sha256_hex(&std::fs::read(&p)?))
}
