//! The binder: declared facts + the catalogue → concrete routes and advisories for one repository.
//!
//! Rules, in order, for every candidate a detector proposes:
//!
//! 1. **One directive per trigger.** Detectors run in [`BIND_ORDER`]; a trigger (a failing
//!    command, an edited glob) already claimed by an earlier instinct is not bound again.
//! 2. **Ids the miner would derive.** A dead-end-shaped route directing `S` takes
//!    `dead-end-<stable(key(S))>` (further triggers for the same `S` add the trigger's key); a
//!    finish line takes `finish-line-<stable(glob, command)>`. So a learned route for the same
//!    behaviour has the same id, and the index's "learned wins" de-duplication applies.
//! 3. **Learned routes win.** An instinct is skipped if a learned route has its id, avoids its
//!    command, directs something (the same or different) for the same trigger, or a person
//!    rejected its id in review.
//! 4. **Capped.** Yellow, probation (an environment step: draft, which waits for review, as
//!    Design O11 requires), no slots, provenance `instinct:<pack>@<version>`, no evidence.
//! 5. **Validated.** Every route passes [`adage_tracks::io::validate`] plus the checks here.
//!
//! The same repository files always produce byte-identical results: everything is sorted, and no
//! hash-map order reaches the output.

use crate::catalogue::{catalogue, Catalogue};
use crate::declared::{self, safe_glob, DeclaredFacts};
use crate::detect::{self, Ctx, IdHint, RouteCandidate, BIND_ORDER};
use crate::files::safe_rel;
use crate::safety::{command_key, rank, stable};
use adage_core::{CommandTable, Effect};
use adage_tracks::{Aspect, Evidence, Gauge, Manifest, Route, RouteState, Scope, Step, Track, TrackInfo, Trigger};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// An instinct that cannot be a route yet: the model has no advisory step (`Step::Advise`). The
/// binder detects and validates it now, so the runtime can render it from a fixed template once
/// the step exists (docs/instincts.md, "Integration plan").
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Advisory {
    /// `instinct-<tag>-<stable>`.
    pub id: String,
    /// The catalogue id (U4, U6, N1).
    pub instinct: String,
    /// One of [`TEMPLATES`].
    pub template_id: String,
    pub trigger: Trigger,
    /// Validated slot values; only these are interpolated into the template.
    pub slots: BTreeMap<String, String>,
    /// `instinct:<pack>@<version>`.
    pub provenance: String,
}

/// Advisory templates and their slot contracts: `(template id, required slots, optional slots)`.
pub const TEMPLATES: [(&str, &[&str], &[&str]); 3] = [
    ("generated-file", &["path"], &["command"]),
    ("lockfile", &["path", "manager"], &[]),
    ("package-manager", &["manager", "lockfile"], &[]),
];

/// The fixed text of each template, by template id and whether the optional `command` slot is
/// bound. No text in a track or a repository ever reaches the agent; only validated slot values do.
fn template_text(id: &str, with_command: bool) -> Option<&'static str> {
    Some(match (id, with_command) {
        ("generated-file", true) => {
            "In this repository, {path} is generated: change its inputs and run {command} rather than editing it by hand."
        }
        ("generated-file", false) => {
            "In this repository, {path} is generated: change its inputs and regenerate it rather than editing it by hand."
        }
        ("lockfile", _) => {
            "In this repository, {path} is maintained by {manager}: change the manifest and let {manager} update it rather than editing it by hand."
        }
        ("package-manager", _) => {
            "This repository uses {manager} ({lockfile}), not npm: use {manager} for installs and scripts here."
        }
        _ => return None,
    })
}

impl Advisory {
    /// The text the runtime would deliver, from the fixed template and the validated slots.
    pub fn text(&self) -> Option<String> {
        let (_, required, optional) = TEMPLATES.iter().find(|(id, ..)| *id == self.template_id)?;
        let mut s = template_text(&self.template_id, self.slots.contains_key("command"))?.to_string();
        for k in required.iter().chain(optional.iter()) {
            if let Some(v) = self.slots.get(*k) {
                s = s.replace(&format!("{{{k}}}"), v);
            } else if required.contains(k) {
                return None;
            }
        }
        (!s.contains('{')).then_some(s)
    }
}

/// Everything one bind produced, sorted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BindResult {
    pub routes: Vec<Route>,
    pub advisories: Vec<Advisory>,
    /// `(instinct id, reason)` for each candidate not bound, and each file the binder skipped
    /// (instinct id `declared`).
    pub skipped: Vec<(String, String)>,
    /// Route or advisory id → the instinct that bound it (route ids equal the miner's, so the id
    /// alone does not say).
    pub origins: BTreeMap<String, String>,
}

/// Per-repository configuration (`instincts.enabled`, `instincts.packs` in Adage's config).
#[derive(Debug, Clone, Default)]
pub struct BindOptions {
    /// Instinct ids not to bind, beyond those the catalogue disables.
    pub disabled: BTreeSet<String>,
}

/// Bind every enabled instinct for the repository at `repo_root`.
///
/// `existing` are the repository's routes from every other source (learned, imported, authored);
/// routes whose provenance is an instinct's are ignored. `rejections` are route ids a person
/// rejected in `adage review`.
pub fn bind(repo_root: &Path, existing: &[Route], rejections: &BTreeSet<String>) -> BindResult {
    bind_with(repo_root, existing, rejections, &BindOptions::default())
}

pub fn bind_with(
    repo_root: &Path,
    existing: &[Route],
    rejections: &BTreeSet<String>,
    opts: &BindOptions,
) -> BindResult {
    let facts = declared::extract(repo_root);
    bind_facts(&facts, existing, rejections, opts)
}

/// A trigger's identity for "one directive per trigger": a failure trigger is its command,
/// whatever the signature.
fn trigger_slot(t: &Trigger) -> String {
    match t {
        Trigger::AfterFailure { command, .. } => format!("failure:{command}"),
        Trigger::AfterCommand { command } => format!("command:{command}"),
        Trigger::AfterEdit { paths } => format!("edit:{paths}"),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn describe(c: &RouteCandidate) -> String {
    match (&c.trigger, &c.runner) {
        (Trigger::AfterEdit { paths }, _) => format!("after editing {paths}, {}", c.command),
        (_, Some(r)) => format!("after {r} fails, {}", c.command),
        _ => c.command.clone(),
    }
}

pub fn bind_facts(
    facts: &DeclaredFacts,
    existing: &[Route],
    rejections: &BTreeSet<String>,
    opts: &BindOptions,
) -> BindResult {
    let cat = catalogue();
    let table = CommandTable::load();
    let ctx = Ctx { facts, table: &table, cat };
    let provenance = cat.provenance();
    let learned: Vec<&Route> = existing.iter().filter(|r| !r.evidence.provenance.starts_with("instinct:")).collect();

    let mut out = BindResult::default();
    for (file, why) in &facts.skipped_files {
        out.skipped.push(("declared".into(), format!("{file}: {why}")));
    }
    let mut claimed: BTreeMap<String, String> = BTreeMap::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();

    for id in BIND_ORDER {
        let Some(inst) = cat.get(id) else { continue };
        if !inst.enabled || opts.disabled.contains(id) {
            continue;
        }
        let d = detect::detect(id, &ctx);
        out.skipped.extend(d.skipped);
        for c in d.routes {
            let slot = trigger_slot(&c.trigger);
            if let Some(by) = claimed.get(&slot) {
                if by != &c.instinct {
                    out.skipped.push((c.instinct.clone(), format!("{}: already bound by {by}", describe(&c))));
                }
                continue;
            }
            let rid = match route_id(&c, &ids) {
                Some(x) => x,
                None => {
                    out.skipped.push((c.instinct.clone(), format!("{}: no free id", describe(&c))));
                    continue;
                }
            };
            // Reserved even if this candidate is not bound: a learned route holding the miner's id
            // covers this trigger, not the others that direct the same command.
            ids.insert(rid.clone());
            if rejections.contains(&rid) {
                claimed.insert(slot, c.instinct.clone());
                out.skipped.push((c.instinct.clone(), format!("{rid} ({}): rejected in review", describe(&c))));
                continue;
            }
            if let Some(why) = learned_conflict(&c, &rid, &learned, &table) {
                claimed.insert(slot, c.instinct.clone());
                out.skipped.push((c.instinct.clone(), format!("{rid} ({}): {why}", describe(&c))));
                continue;
            }
            let route = build(&c, &rid, &provenance);
            match check_route(&route, cat, &table) {
                Ok(()) => {
                    claimed.insert(slot, c.instinct.clone());
                    out.origins.insert(rid, c.instinct.clone());
                    out.routes.push(route);
                }
                Err(e) => out.skipped.push((c.instinct.clone(), format!("{rid} ({}): {e}", describe(&c)))),
            }
        }
        for a in d.advisories {
            let aid = format!(
                "instinct-{}-{}",
                a.instinct.to_ascii_lowercase(),
                stable(&a.id_parts.iter().map(String::as_str).collect::<Vec<_>>())
            );
            if out.origins.contains_key(&aid) {
                continue;
            }
            if rejections.contains(&aid) {
                out.skipped.push((a.instinct.clone(), format!("{aid}: rejected in review")));
                continue;
            }
            let adv = Advisory {
                id: aid.clone(),
                instinct: a.instinct.clone(),
                template_id: a.template_id,
                trigger: a.trigger,
                slots: a.slots,
                provenance: provenance.clone(),
            };
            match check_advisory(&adv, cat, &table) {
                Ok(()) => {
                    out.origins.insert(aid, a.instinct);
                    out.advisories.push(adv);
                }
                Err(e) => out.skipped.push((a.instinct, format!("{aid}: {e}"))),
            }
        }
    }
    out.routes.sort_by(|a, b| a.id.cmp(&b.id));
    out.advisories.sort_by(|a, b| a.id.cmp(&b.id));
    out.skipped.sort();
    out.skipped.dedup();
    out
}

fn route_id(c: &RouteCandidate, used: &BTreeSet<String>) -> Option<String> {
    let id = match &c.id {
        IdHint::DeadEnd { wkey } => {
            let first = format!("dead-end-{}", stable(&[wkey]));
            if !used.contains(&first) {
                return Some(first);
            }
            let Trigger::AfterFailure { command, .. } = &c.trigger else { return None };
            format!("dead-end-{}", stable(&[wkey, command]))
        }
        IdHint::FinishLine { glob } => format!("finish-line-{}", stable(&[glob, &c.command])),
        IdHint::Own { tag, parts } => {
            format!("instinct-{tag}-{}", stable(&parts.iter().map(String::as_str).collect::<Vec<_>>()))
        }
    };
    (!used.contains(&id)).then_some(id)
}

/// Why a learned (or imported, or authored) route overrides this candidate, if one does.
fn learned_conflict(c: &RouteCandidate, rid: &str, learned: &[&Route], table: &CommandTable) -> Option<String> {
    let s_key = command_key(&c.command, table);
    for r in learned {
        if r.id == rid {
            return Some(format!("route {} has this id (learned wins)", r.id));
        }
        match (&r.trigger, &r.step) {
            (_, Step::Avoid { avoid, instead, .. }) => {
                let avoided: Vec<Option<String>> = avoid.split(" / ").map(|a| command_key(a, table)).collect();
                if s_key.is_some() && avoided.contains(&s_key) {
                    return Some(format!("route {} avoids {}", r.id, c.command));
                }
                if let Trigger::AfterFailure { command, .. } = &c.trigger {
                    if avoided.contains(&Some(command.clone())) {
                        return Some(if *instead == c.command {
                            format!("route {} already directs {instead} for this failure", r.id)
                        } else {
                            format!("route {} directs {instead} for this failure", r.id)
                        });
                    }
                }
            }
            (Trigger::AfterFailure { command: rc, .. }, Step::Run { command: rs }) => {
                if matches!(&c.trigger, Trigger::AfterFailure { command, .. } if command == rc) {
                    return Some(format!("route {} directs {rs} for this failure", r.id));
                }
            }
            (Trigger::AfterEdit { paths }, Step::Run { command: rs }) => {
                if matches!(&c.trigger, Trigger::AfterEdit { paths: p } if p == paths) {
                    return Some(format!("route {} directs {rs} after editing {paths}", r.id));
                }
            }
            _ => {}
        }
    }
    None
}

fn build(c: &RouteCandidate, id: &str, provenance: &str) -> Route {
    Route {
        id: id.to_string(),
        summary: c.summary.clone(),
        trigger: c.trigger.clone(),
        step: Step::Run { command: c.command.clone() },
        slots: vec![],
        effect: c.effect,
        aspect: Aspect::Yellow,
        // Design O11: an environment step is delivered only on a route a person approved.
        state: if c.effect == Effect::Environment { RouteState::Draft } else { RouteState::Probation },
        exit: vec![c.exit.clone()],
        evidence: Evidence { provenance: provenance.to_string(), ..Default::default() },
        authored: false,
    }
}

/// A command Adage may name: plain argv, as `adage_tracks::io` requires of a directed command.
pub fn plain_command(c: &str) -> bool {
    let inside = |part: &str| !part.starts_with('/') && !part.split('/').any(|seg| seg == "..");
    !c.is_empty()
        && c.len() <= 200
        && c.split(' ').all(|t| !t.is_empty())
        && c.chars().all(|ch| ch.is_ascii_alphanumeric() || " _./:=@,+-".contains(ch))
        && c.split(' ').all(|t| t.split(['=', ',']).all(inside))
}

/// No invisible, control or non-ASCII character anywhere in the serialised value.
fn visible(v: &impl Serialize) -> Result<(), String> {
    let s = serde_json::to_string(v).map_err(|e| e.to_string())?;
    match s.chars().find(|c| !c.is_ascii() || c.is_ascii_control() || adage_tracks::io::invisible(*c)) {
        Some(c) => Err(format!("contains U+{:04X}", c as u32)),
        None => Ok(()),
    }
}

/// Track validation, then the instinct rules that validation does not know about.
pub fn check_route(r: &Route, cat: &Catalogue, table: &CommandTable) -> Result<(), String> {
    let t = Track {
        manifest: Manifest {
            track: TrackInfo {
                name: format!("instincts/{}", cat.pack.name),
                version: cat.pack.version.clone(),
                description: "instinct validation".into(),
                provenance: cat.provenance(),
            },
            scope: Scope { repo: "instincts".into() },
            gauge: Gauge::default(),
        },
        routes: vec![r.clone()],
    };
    adage_tracks::io::validate(&t).map_err(|e| format!("fails track validation: {e:#}"))?;
    if r.aspect != Aspect::Yellow {
        return Err("an instinct is yellow".into());
    }
    let Step::Run { command } = &r.step else { return Err("an instinct directs a run step".into()) };
    if !plain_command(command) || !r.slots.is_empty() {
        return Err(format!("{command:?} is not plain argv"));
    }
    let actual = adage_tracks::io::classify(command, table);
    if rank(actual) > rank(r.effect) {
        return Err(format!("{command:?} classifies as {}", actual.as_str()));
    }
    match (r.effect, r.state) {
        (Effect::Observe | Effect::Verify | Effect::Regenerate, RouteState::Probation) => {}
        (Effect::Environment, RouteState::Draft) => {}
        (e, s) => return Err(format!("effect {} in state {s:?} may not be bound", e.as_str())),
    }
    match &r.trigger {
        Trigger::AfterEdit { paths } if !safe_glob(paths) => {
            return Err(format!("glob {paths:?} is not repository-relative"))
        }
        Trigger::AfterFailure { command, signature }
            if !command.starts_with("exec ") || !adage_tracks::io::safe_text(signature) =>
        {
            return Err("failure trigger is not a plain command key".into())
        }
        Trigger::AfterEdit { .. } | Trigger::AfterFailure { .. } => {}
        _ => return Err("an instinct is reactive: after a failure or an edit".into()),
    }
    if !r.evidence.provenance.starts_with("instinct:") {
        return Err("provenance must name the instinct pack".into());
    }
    visible(r)
}

pub fn check_advisory(a: &Advisory, cat: &Catalogue, table: &CommandTable) -> Result<(), String> {
    let Some((_, required, optional)) = TEMPLATES.iter().find(|(id, ..)| *id == a.template_id) else {
        return Err(format!("unknown template {:?}", a.template_id));
    };
    let allowed: BTreeSet<&str> = required.iter().chain(optional.iter()).copied().collect();
    for k in *required {
        if !a.slots.contains_key(*k) {
            return Err(format!("slot {k} is missing"));
        }
    }
    let managers: BTreeSet<&str> = cat.lockfiles.values().map(String::as_str).collect();
    for (k, v) in &a.slots {
        if !allowed.contains(k.as_str()) {
            return Err(format!("slot {k} is not in template {}", a.template_id));
        }
        let ok = match k.as_str() {
            "path" => safe_glob(v),
            "lockfile" => safe_rel(v) && cat.lockfiles.contains_key(v),
            "manager" => managers.contains(v.as_str()),
            "command" => plain_command(v) && rank(adage_tracks::io::classify(v, table)) <= rank(Effect::Regenerate),
            _ => false,
        };
        if !ok {
            return Err(format!("slot {k} value {v:?} fails its contract"));
        }
    }
    match &a.trigger {
        Trigger::AfterEdit { paths } if safe_glob(paths) => {}
        Trigger::AfterCommand { command } if command.starts_with("exec ") && plain_command(&command[5..]) => {}
        _ => return Err("advisory trigger is not an edit glob or a plain command key".into()),
    }
    if !a.provenance.starts_with("instinct:") {
        return Err("provenance must name the instinct pack".into());
    }
    a.text().ok_or_else(|| "template cannot be rendered".to_string())?;
    visible(a)
}
