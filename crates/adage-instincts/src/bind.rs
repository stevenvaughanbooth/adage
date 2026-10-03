//! The binder: declared facts + the catalogue → concrete routes and advisories for one repository.
//!
//! Rules, in order, for every candidate a detector proposes:
//!
//! 1. **One directive per trigger.** Detectors run in [`BIND_ORDER`]; a trigger (a failing
//!    command, an edited glob) already claimed by an earlier instinct is not bound again.
//! 2. **Ids the learner would derive.** A dead-end-shaped route directing `S` takes
//!    `dead-end-<stable(key(S))>` (further triggers for the same `S` add the trigger's key); a
//!    finish line takes `finish-line-<stable(glob, command)>`. So a learned route for the same
//!    behaviour has the same id, and the index's "learned wins" de-duplication applies.
//! 3. **Learned routes win.** An instinct is skipped if a learned route has its id, avoids its
//!    command, directs something (the same or different) for the same trigger, or a person
//!    rejected its id in review.
//! 4. **Capped.** Prime, forming (an environment step: nascent, which waits for review, as
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
use adage_tracks::{
    Evidence, Gauge, Manifest, Origin, Route, RouteState, Scope, Step, Strength, Track, TrackInfo, Trigger,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// An advisory route ([`Step::Advise`]) seen through its template: which instinct bound it, the
/// fixed template, and the validated slot values (`BindResult::advisories`).
#[derive(Debug, Clone, PartialEq)]
pub struct Advice<'a> {
    pub id: &'a str,
    /// The catalogue id (U4, U6, N1).
    pub instinct: &'a str,
    /// One of `adage_tracks::advice::TEMPLATES`.
    pub template_id: &'a str,
    pub trigger: &'a Trigger,
    pub slots: &'a BTreeMap<String, String>,
}

impl Advice<'_> {
    /// The text the runtime delivers, from the fixed template and the validated slots.
    pub fn text(&self) -> Option<String> {
        adage_tracks::advice::text(self.template_id, self.slots)
    }
}

/// Everything one bind produced, sorted.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct BindResult {
    /// Every bound route, sorted by id: run steps (after a failure or an edit) and advisory
    /// steps (`Step::Advise`, after an edit or a command).
    pub routes: Vec<Route>,
    /// `(instinct id, reason)` for each candidate not bound, and each file the binder skipped
    /// (instinct id `declared`).
    pub skipped: Vec<(String, String)>,
    /// Route id → the instinct that bound it (route ids equal the learner's, so the id alone does
    /// not say).
    pub origins: BTreeMap<String, String>,
}

impl BindResult {
    /// The advisory routes, through their templates.
    pub fn advisories(&self) -> Vec<Advice<'_>> {
        self.routes
            .iter()
            .filter_map(|r| match &r.step {
                Step::Advise { template, slots } => Some(Advice {
                    id: &r.id,
                    instinct: self.origins.get(&r.id).map(String::as_str).unwrap_or(""),
                    template_id: template,
                    trigger: &r.trigger,
                    slots,
                }),
                _ => None,
            })
            .collect()
    }

    /// The routes that direct a command (run steps).
    pub fn runs(&self) -> Vec<&Route> {
        self.routes.iter().filter(|r| matches!(r.step, Step::Run { .. })).collect()
    }
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

/// A trigger's identity for "one directive per trigger": a failure trigger is its command and
/// the signature that says why it failed (`pytest: command not found` and `DJANGO_SETTINGS_MODULE`
/// are different failures of `pytest`, with different answers).
fn trigger_slot(t: &Trigger) -> String {
    match t {
        Trigger::AfterFailure { command, signature } => format!("failure:{command}\u{0}{signature}"),
        Trigger::AfterCommand { command } => format!("command:{command}"),
        Trigger::AfterEdit { paths } => format!("edit:{paths}"),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn describe(c: &RouteCandidate) -> String {
    match (&c.trigger, &c.runner) {
        (Trigger::AfterEdit { paths }, _) => format!("after editing {paths}, {}", c.command),
        (Trigger::AfterFailure { signature, .. }, Some(r)) => {
            format!("after {r} fails with {signature:?}, {}", c.command)
        }
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
    let ctx = Ctx { facts, table, cat };
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
            // Reserved even if this candidate is not bound: a learned route holding the learner's id
            // covers this trigger, not the others that direct the same command.
            ids.insert(rid.clone());
            if rejections.contains(&rid) {
                claimed.insert(slot, c.instinct.clone());
                out.skipped.push((c.instinct.clone(), format!("{rid} ({}): rejected in review", describe(&c))));
                continue;
            }
            if let Some(why) = learned_conflict(&c, &rid, &learned, table) {
                claimed.insert(slot, c.instinct.clone());
                out.skipped.push((c.instinct.clone(), format!("{rid} ({}): {why}", describe(&c))));
                continue;
            }
            let route = build(&c, &rid, cat);
            match check_route(&route, cat, table) {
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
            let route = Route {
                id: aid.clone(),
                summary: format!("Instinct {}: {}", a.instinct, inst.title),
                trigger: a.trigger,
                step: Step::Advise { template: a.template_id, slots: a.slots },
                slots: vec![],
                effect: Effect::Observe,
                strength: Strength::Prime,
                state: RouteState::Forming,
                exit: vec![],
                evidence: evidence(cat, &a.instinct, a.ecosystem, a.declared),
                authored: false,
            };
            match check_advisory(&route, cat, table) {
                Ok(()) => {
                    out.origins.insert(aid, a.instinct);
                    out.routes.push(route);
                }
                Err(e) => out.skipped.push((a.instinct, format!("{aid}: {e}"))),
            }
        }
    }
    out.routes.sort_by(|a, b| a.id.cmp(&b.id));
    out.skipped.sort();
    out.skipped.dedup();
    out
}

fn route_id(c: &RouteCandidate, used: &BTreeSet<String>) -> Option<String> {
    // Further failures of the same command, with other signatures, add the signature.
    let signature = match &c.trigger {
        Trigger::AfterFailure { signature, .. } => Some(signature.as_str()),
        _ => None,
    };
    let (first, then) = match &c.id {
        IdHint::DeadEnd { wkey } => {
            let first = format!("dead-end-{}", stable(&[wkey]));
            let Trigger::AfterFailure { command, .. } = &c.trigger else {
                return (!used.contains(&first)).then_some(first);
            };
            if !used.contains(&first) {
                return Some(first);
            }
            (
                format!("dead-end-{}", stable(&[wkey, command])),
                signature.map(|s| format!("dead-end-{}", stable(&[wkey, command, s]))),
            )
        }
        IdHint::FinishLine { glob } => (format!("finish-line-{}", stable(&[glob, &c.command])), None),
        IdHint::Own { tag, parts } => {
            let mut p: Vec<&str> = parts.iter().map(String::as_str).collect();
            let first = format!("instinct-{tag}-{}", stable(&p));
            p.extend(signature);
            (first, signature.map(|_| format!("instinct-{tag}-{}", stable(&p))))
        }
    };
    [Some(first), then].into_iter().flatten().find(|id| !used.contains(id))
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

/// An instinct's evidence: none of its own, its provenance and origin, and the declared fact that
/// bound it (`by_origin.declared`, roadmap E1).
fn evidence(cat: &Catalogue, instinct: &str, ecosystem: Option<&str>, declared: Option<String>) -> Evidence {
    Evidence {
        provenance: cat.provenance(),
        origin: Some(Origin::Instinct {
            pack: cat.pack.name.clone(),
            version: cat.pack.version.clone(),
            instinct: instinct.to_string(),
            ecosystem: ecosystem.map(str::to_string),
            declared: declared.filter(|d| safe_rel(d)),
        }),
        by_origin: [("declared".to_string(), 1)].into_iter().collect(),
        ..Default::default()
    }
}

fn build(c: &RouteCandidate, id: &str, cat: &Catalogue) -> Route {
    Route {
        id: id.to_string(),
        summary: c.summary.clone(),
        trigger: c.trigger.clone(),
        step: Step::Run { command: c.command.clone() },
        slots: vec![],
        effect: c.effect,
        strength: Strength::Prime,
        // Design O11: an environment step is delivered only on a route a person approved.
        state: if c.effect == Effect::Environment { RouteState::Nascent } else { RouteState::Forming },
        exit: vec![c.exit.clone()],
        evidence: evidence(cat, &c.instinct, c.ecosystem, c.declared.clone()),
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
    if r.strength != Strength::Prime {
        return Err("an instinct is prime".into());
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
        (Effect::Observe | Effect::Verify | Effect::Regenerate, RouteState::Forming) => {}
        (Effect::Environment, RouteState::Nascent) => {}
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
        // Any non-zero exit includes a test failing on its assertions, which no other command
        // fixes: an instinct fires only on the failure it answers.
        Trigger::AfterFailure { signature, .. } if signature.trim().is_empty() => {
            return Err("a failure trigger without a signature would fire on any failure".into())
        }
        Trigger::AfterEdit { .. } | Trigger::AfterFailure { .. } => {}
        _ => return Err("an instinct is reactive: after a failure or an edit".into()),
    }
    if !r.evidence.provenance.starts_with("instinct:") {
        return Err("provenance must name the instinct pack".into());
    }
    visible(r)
}

/// An advisory route: track validation (which checks the template and each slot's contract),
/// then the instinct rules: prime, observe, forming, a repository-relative glob or a plain
/// command key for its trigger, the instinct provenance, and nothing invisible.
pub fn check_advisory(r: &Route, cat: &Catalogue, table: &CommandTable) -> Result<(), String> {
    let Step::Advise { template, slots } = &r.step else { return Err("not an advisory step".into()) };
    adage_tracks::advice::check(template, slots, table)?;
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
    if (r.strength, r.effect, r.state) != (Strength::Prime, Effect::Observe, RouteState::Forming) {
        return Err("advice is prime, observe, in forming".into());
    }
    match &r.trigger {
        Trigger::AfterEdit { paths } if safe_glob(paths) => {}
        Trigger::AfterCommand { command } if command.starts_with("exec ") && plain_command(&command[5..]) => {}
        // After a failure with this signature (never any failure: see `check_route`).
        Trigger::AfterFailure { command, signature }
            if command.starts_with("exec ")
                && plain_command(&command[5..])
                && !signature.trim().is_empty()
                && adage_tracks::io::safe_text(signature) => {}
        _ => return Err("advisory trigger is not an edit glob, a plain command key or a signed failure".into()),
    }
    if !r.evidence.provenance.starts_with("instinct:") {
        return Err("provenance must name the instinct pack".into());
    }
    adage_tracks::advice::text(template, slots).ok_or_else(|| "template cannot be rendered".to_string())?;
    visible(r)
}
