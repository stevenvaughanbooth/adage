//! The review queue and the decisions a person makes about reflexes (Design J1, §10.4, §10.9;
//! Architecture §6.3): what `adage review`, `adage ui` and `adage status` show and do.
//!
//! [`queue`] lists every installed reflex that waits for a person, with typed reasons; [`decide`]
//! records one decision: it checks the verb against the route (the effect ceiling, an instinct's
//! and a card's prime cap, an import's need for first-hand evidence, Design O2), applies a
//! proposal, approves a pack's environment steps together, bumps the track's version, records
//! the review, re-validates and writes the track, and recompiles the index atomically, under
//! the tracks lock. It needs a [`Person`]: decisions change trust, so they never run inside an
//! agent session (review finding F11).
//!
//! Nothing here prints. Callers render [`QueueItem`]s and [`Outcome`]s as they need: the terminal,
//! the TUI, JSON.

use crate::error::hint;
use crate::person::Person;
use adage_learn::LearnConfig;
use adage_runtime::render;
use adage_store::Paths;
use adage_tracks::proposals::{self, Proposal, Proposals};
use adage_tracks::review::{self, strength_str, Decision, Review, Reviews};
use adage_tracks::{
    ceiling, index, io, wilson_lower, Contract, Index, Route, RouteState, Step, Strength, Track, Trigger,
};
use anyhow::{bail, Context, Result};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Runtime judgements needed before an ignore signal is considered.
const MIN_JUDGED: u32 = 5;
/// A route is flagged when the Wilson lower bound of its ignore share reaches this.
const IGNORE_BAR: f64 = 0.5;

/// An installed track, with its directory and the decisions and proposals recorded beside it.
pub struct Installed {
    pub dir: PathBuf,
    /// The track's directory relative to the tracks directory: unambiguous, and what the queue prints.
    pub reference: String,
    pub track: Track,
    pub imported: bool,
    pub reviews: Reviews,
    /// Proposed changes to pinned routes, from `adage condition` or a re-run `adage add`.
    pub proposals: Proposals,
}

/// Every installed track with its directory and decisions, and a warning for each one skipped.
/// Unreadable tracks are skipped, as `io::read_all` does; symlinked directories are never followed.
pub fn discover(root: &Path) -> Result<(Vec<Installed>, Vec<String>)> {
    let (mut out, mut warnings) = (Vec::new(), Vec::new());
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join("track.toml").exists() {
            let loaded = io::read(&d).and_then(|t| Ok((t, review::load(&d)?)));
            match loaded {
                Ok((track, reviews)) => out.push(Installed {
                    reference: d.strip_prefix(root).unwrap_or(&d).to_string_lossy().replace('\\', "/"),
                    // Proposals are advisory local state: an unreadable file hides them, nothing more.
                    proposals: proposals::load(&d).unwrap_or_else(|e| {
                        warnings.push(format!("adage: ignoring unreadable proposals in {}: {e:#}", d.display()));
                        Proposals::default()
                    }),
                    dir: d,
                    imported: index::is_imported(&track),
                    track,
                    reviews,
                }),
                Err(e) => warnings.push(format!("adage: skipping invalid track {}: {e:#}", d.display())),
            }
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            // Hidden directories hold no track in service (`<repo>/.shadow/`, Track G G3).
            if e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.') {
                stack.push(e.path());
            }
        }
    }
    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    Ok((out, warnings))
}

/// The installed track a person names: its directory reference, else its name (a learned track
/// wins over an import of the same name).
pub fn resolve<'a>(all: &'a [Installed], spec: &str) -> Result<&'a Installed> {
    if let Some(i) = all.iter().find(|i| i.reference == spec) {
        return Ok(i);
    }
    let named: Vec<&Installed> = all.iter().filter(|i| i.track.manifest.track.name == spec).collect();
    named
        .iter()
        .find(|i| !i.imported)
        .or(named.first())
        .copied()
        .ok_or_else(|| hint(format!("no installed track {spec:?}"), "`adage status` lists installed tracks"))
}

/// Followed / ignored judgements per (installed reference, route), for the current recorded version:
/// in the control arm nothing was delivered, so nothing could be followed or ignored.
pub fn judgements(paths: &Paths) -> BTreeMap<(String, String), (u32, u32)> {
    let Ok((installed, _)) = discover(&paths.tracks()) else { return BTreeMap::new() };
    crate::graph::current_judgements(paths, &installed)
}

/// Whether a route is in service.
pub fn live(s: RouteState) -> bool {
    matches!(s, RouteState::Firm | RouteState::Forming | RouteState::Dormant)
}

/// Reasons that list a reflex in `adage review` without making it wait for a decision: an
/// instinct in effect (Adage's built-in default; extinguish it to turn it off), and a routine
/// whose evidence was approved and which runs once delegated.
pub const LISTED_ONLY: [&str; 2] = ["instinct", "delegable"];

/// Does a reflex with these reasons wait for a decision (what `adage review`, `adage status`, the
/// card and `condition`'s `next` count)?
pub fn waits(reasons: &[(&'static str, String)]) -> bool {
    reasons.iter().any(|(k, _)| !LISTED_ONLY.contains(k))
}

/// Whether a review reason holds its reflex back until a person decides (a nascent step, a quiet
/// reflex, an execution grant), rather than only being worth a look at a reflex already in use
/// (an instinct, a new or changed reflex, an imported one, one agents ignore).
pub fn needs_person(kind: &str) -> bool {
    matches!(kind, "quiet" | "approval" | "execution")
}

/// Count reflexes by what their reasons ask of a person: `(needs you, to look over)`.
pub fn waiting_counts<'a>(reasons: impl Iterator<Item = &'a [(&'static str, String)]>) -> (usize, usize) {
    let (mut needs, mut look) = (0, 0);
    // Only what waits for a decision: an instinct in effect is listed, never waiting (`waits`).
    for r in reasons.filter(|r| waits(r)) {
        if r.iter().any(|(k, _)| needs_person(k)) {
            needs += 1;
        } else {
            look += 1;
        }
    }
    (needs, look)
}

/// Kept quiet by its uplift: agents already take its step unaided (`adage_learn::value::assess`).
pub fn quiet(r: &Route) -> bool {
    r.evidence.uplift.as_ref().is_some_and(|u| u.gated)
}

fn by_edit(r: &Route) -> bool {
    matches!(r.trigger, Trigger::AfterEdit { .. })
}

/// A route's strength ceiling: its effect's, and prime at most for a procedure card (advice to
/// read, never a command to run).
pub fn route_ceiling(r: &Route) -> Option<Strength> {
    if matches!(r.step, Step::Card { .. }) {
        return Some(Strength::Prime);
    }
    ceiling(r.effect, by_edit(r))
}

pub fn ceiling_str(r: &Route) -> &'static str {
    route_ceiling(r).map(strength_str).unwrap_or("none")
}

/// An example value satisfying a simple anchored contract such as `^W-\d{4}$` (literals, `\d`,
/// `\w`, simple classes, and `{n}` `{n,m}` `+` `*` `?`), checked against the contract itself.
pub fn example_for(regex: &str) -> Option<String> {
    let body: Vec<char> = regex.strip_prefix('^')?.strip_suffix('$')?.chars().collect();
    let (mut out, mut digit, mut i) = (String::new(), 0u32, 0usize);
    while i < body.len() {
        let atom: Option<char> = match body[i] {
            '\\' => {
                i += 1;
                match *body.get(i)? {
                    'd' => None,
                    'w' => Some('a'),
                    c if !c.is_ascii_alphanumeric() => Some(c),
                    _ => return None,
                }
            }
            '[' => {
                let end = i + body[i..].iter().position(|c| *c == ']')?;
                let first = *body.get(i + 1)?;
                if first == '^' || first == '\\' {
                    return None;
                }
                i = end;
                Some(first)
            }
            '(' | ')' | '|' | '.' | '{' | '+' | '*' | '?' => return None,
            c => Some(c),
        };
        i += 1;
        let n = match body.get(i) {
            Some('{') => {
                let end = i + body[i..].iter().position(|c| *c == '}')?;
                let inner: String = body[i + 1..end].iter().collect();
                i = end + 1;
                inner.split(',').next()?.trim().parse::<usize>().ok()?.max(1)
            }
            Some('+' | '*' | '?') => {
                i += 1;
                1
            }
            _ => 1,
        };
        for _ in 0..n.min(16) {
            out.push(atom.unwrap_or_else(|| {
                digit += 1;
                char::from_digit(digit % 10, 10).unwrap_or('1')
            }));
        }
    }
    regex::Regex::new(regex).ok()?.is_match(&out).then_some(out)
}

/// The exact text the route would deliver, with example slot values (and which were used).
pub fn preview(t: &Track, r: &Route) -> (String, BTreeMap<String, String>) {
    let mut bound = BTreeMap::new();
    for s in &r.slots {
        let v = match (&s.contract, &r.step) {
            (Contract::RepoPath, Step::ReadBatch { slot, .. }) if *slot == s.name => {
                Some("src/example_a.py, src/example_b.py".to_string())
            }
            (Contract::RepoPath, _) => Some("src/example.py".to_string()),
            (Contract::Pattern { regex }, _) => example_for(regex),
        };
        if let Some(v) = v {
            bound.insert(s.name.clone(), v);
        }
    }
    let pairs: Vec<(String, String)> = bound.iter().map(|(k, v)| (k.clone(), v.clone())).collect();
    let body = render::route_text(r, &pairs)
        .unwrap_or_else(|| "(this reflex cannot be rendered safely; Adage would say nothing)".into());
    let m = &t.manifest.track;
    let origin = r.is_instinct().then(|| r.origin());
    let strength = if origin.is_some() { adage_tracks::delivered_strength(r) } else { r.strength };
    let kind = if origin.is_some() { adage_tracks::shape(r) } else { "" };
    (render::wrap_origin(&m.name, &m.version, &r.id, kind, strength, &body, origin.as_ref()), bound)
}

/// Where an instinct route came from, for people: `instinct U1 (from Makefile)`.
pub fn origin_label(r: &Route) -> Option<String> {
    match r.origin() {
        adage_tracks::Origin::Instinct { instinct, declared, .. } if r.is_instinct() => {
            let id = if instinct.is_empty() { String::new() } else { format!(" {instinct}") };
            let from = declared.map(|d| format!(" (from {d})")).unwrap_or_default();
            Some(format!("instinct{id}{from}"))
        }
        _ => None,
    }
}

/// One reflex the review queue lists, and why.
pub struct QueueItem<'a> {
    pub inst: &'a Installed,
    pub route: &'a Route,
    pub reasons: Vec<(&'static str, String)>,
    pub followed: u32,
    pub ignored: u32,
    pub proposal: Option<&'a Proposal>,
}

/// The review queue: every installed reflex that waits for a decision or is listed for one,
/// with its reasons. `here` is the checkout the person is in (execution grants are per checkout).
pub fn queue<'a>(
    all: &'a [Installed],
    judged: &BTreeMap<(String, String), (u32, u32)>,
    index: &Index,
    paths: &Paths,
    here: Option<&Path>,
) -> Vec<QueueItem<'a>> {
    let store = adage_exec::approvals::Store::load(&adage_exec::approvals::path(&paths.root)).ok();
    let root = here.map(Path::to_path_buf);
    let repo = root.as_deref().map(adage_core::repo::repo_key);
    let table = adage_core::CommandTable::load();
    let granted = |inst: &Installed, r: &Route| {
        let Some((store, root)) = store.as_ref().zip(root.as_deref()) else { return false };
        if repo.as_deref() != Some(inst.track.manifest.scope.repo.as_str()) {
            return false;
        }
        let Some(ir) = index.routes.iter().find(|ir| {
            ir.repo == inst.track.manifest.scope.repo
                && ir.track == inst.track.manifest.track.name
                && ir.route.id == r.id
        }) else {
            return false;
        };
        let label = format!("{}/{}", ir.track, r.id);
        store
            .approvals
            .iter()
            .filter(|a| {
                a.route == label
                    && a.key.repo == ir.repo
                    && a.key.level == adage_exec::Level::E2
                    && a.key.mode.is_execute()
            })
            .any(|a| {
                adage_exec::route::plan(
                    ir,
                    index,
                    root,
                    &paths.tracks(),
                    adage_exec::Level::E2,
                    a.terms.as_ref(),
                    table,
                )
                .is_ok_and(|p| {
                    store.find(&adage_exec::approvals::ApprovalKey::for_request(&p.request, &ir.repo)).is_some()
                })
            })
    };
    let mut items = Vec::new();
    for inst in all {
        let m = &inst.track.manifest;
        for r in &inst.track.routes {
            let rv = inst.reviews.routes.get(&r.id);
            let (f0, d0) = judged.get(&(inst.reference.clone(), r.id.clone())).copied().unwrap_or_default();
            // Only what happened since the last decision counts toward a new signal.
            let (followed, ignored) = match rv {
                Some(v) if v.basis.runtime_version.as_deref() == Some(&m.track.version) => {
                    (f0.saturating_sub(v.basis.followed), d0.saturating_sub(v.basis.ignored))
                }
                _ => (f0, d0),
            };
            let mut reasons: Vec<(&'static str, String)> = Vec::new();
            let instinct = origin_label(r);
            match rv {
                // Quiet on its uplift (`adage_learn::value::assess`): agents already take its step
                // unaided, so it waits for a person rather than being delivered.
                None if r.state == RouteState::Nascent && !r.authored && quiet(r) => reasons.push((
                    "quiet",
                    format!(
                        "{}; `approve` delivers it anyway",
                        r.evidence.uplift.as_ref().map(|u| u.line()).unwrap_or_default()
                    ),
                )),
                None if r.state == RouteState::Nascent && !r.authored && instinct.is_some() => reasons.push((
                    "approval",
                    "an environment step Adage would suggest by default: delivered only after a person approves it; approving it approves this pack's environment steps for the repository".into(),
                )),
                None if r.state == RouteState::Nascent && !r.authored => reasons.push((
                    "approval",
                    "an environment step: it is delivered only after a person approves it (`approve`)".into(),
                )),
                None if instinct.is_some() && live(r.state) => reasons.push((
                    "instinct",
                    format!(
                        "{}: Adage's built-in default, bound from this repository's files with no trace history; prime at most, and any conditioned reflex overrides it",
                        instinct.clone().unwrap_or_default()
                    ),
                )),
                None if r.authored || !live(r.state) => {}
                None if inst.imported => reasons.push((
                    "imported",
                    "imported from a pack and forming: prime at most until local first-hand evidence confirms it".into(),
                )),
                None => reasons.push(("new", "from `adage condition`, never reviewed".into())),
                Some(v) if v.decision == Decision::Extinguished => {
                    if r.state != RouteState::Extinguished {
                        let why = review::material_change(&v.basis, r).unwrap_or_else(|| "its evidence strengthened".into());
                        reasons.push(("resurfaced", format!("extinguished on {}; since then {why}, so it is back, forming", v.on)));
                    }
                }
                Some(v) if v.decision == Decision::Pinned => {}
                Some(v) => {
                    if let Some(why) = review::material_change(&v.basis, r) {
                        reasons.push(("changed", format!("{} on {}; since then {why}", v.decision.as_str(), v.on)));
                    }
                }
            }
            let proposal = crate::proposal::current(inst.proposals.routes.get(&r.id), r);
            if let Some(p) = proposal {
                reasons.push((
                    "proposed",
                    format!(
                        "pinned, so learning left it as it is; since {} its learned version differs: {}",
                        p.on, p.reason
                    ),
                ));
            }
            let delivered = index.routes.iter().any(|ir| {
                ir.repo == m.scope.repo && ir.track == m.track.name && ir.version == m.track.version && ir.route == *r
            });
            let n = followed + ignored;
            if delivered && r.strength != Strength::Inhibit && n >= MIN_JUDGED && wilson_lower(ignored, n) >= IGNORE_BAR
            {
                let advice =
                    if r.strength == Strength::Fire { "consider weakening it" } else { "consider extinguishing it" };
                let since = if rv.is_some_and(|v| v.basis.runtime_version.as_deref() == Some(&m.track.version)) {
                    " since the last decision"
                } else {
                    ""
                };
                reasons.push((
                    "ignoring",
                    format!("the directed action was not observed within the next three action batches in {ignored} of {n} treated traces{since}; {advice}"),
                ));
            }
            // A routine with execution evidence waits for a decision on its evidence once; after
            // `approve` it no longer waits, and is listed as ready to delegate until it is.
            if r.evidence.execution.is_some() && live(r.state) && !granted(inst, r) {
                match rv {
                    // Decided, and nothing new since: it waits for nothing.
                    Some(v) if reasons.is_empty() => reasons.push((
                        "delegable",
                        format!("{} on {}; it runs only once delegated (an E2 grant)", v.decision.as_str(), v.on),
                    )),
                    _ => reasons.push(("execution", "a routine Adage could run itself (execution only): without a local E2 grant it stays silent. `approve` accepts the evidence only; `adage review delegate <track> <reflex> --level e2`, from this repository, reviews running it.".into())),
                }
            }
            if !reasons.is_empty() {
                items.push(QueueItem { inst, route: r, reasons, followed, ignored, proposal });
            }
        }
    }
    items
}

/// The decisions that can apply to a route as it stands (strengthening of an import may still be
/// refused for want of first-hand evidence).
pub fn verbs(r: &Route, proposed: bool) -> Vec<&'static str> {
    let mut v = vec!["approve", "extinguish"];
    if proposed {
        v.push("dismiss");
    }
    if live(r.state) && r.strength == Strength::Fire {
        v.push("weaken");
    }
    // An instinct is prime at most, whatever anyone decides.
    if live(r.state) && r.strength == Strength::Prime && route_ceiling(r) == Some(Strength::Fire) && !r.is_instinct() {
        v.push("strengthen");
    }
    if !r.authored {
        v.push("pin");
    }
    v
}

/// What `adage ui` shows beside each installed route: the decisions that apply to it, and why it
/// awaits review (empty when it does not). Keyed by (track reference, route id).
pub struct Standing {
    pub verbs: Vec<&'static str>,
    pub reasons: Vec<(&'static str, String)>,
    /// Absent when advisory proposal state cannot be read safely; no decision may be confirmed.
    pub preview: Option<ReviewPreview>,
}

/// The state a person inspected, bound to the exact installed route and proposal.
#[derive(Debug, Clone, PartialEq)]
pub struct ReviewPreview {
    pub revision: String,
    pub current_directive: String,
    pub proposal: Option<ProposalPreview>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProposalPreview {
    pub on: String,
    pub reason: String,
    pub diff: Vec<(&'static str, String, String)>,
    pub directive: String,
}

fn revision(inst: &Installed, route: &Route) -> Result<String> {
    // Discovery is tolerant for read-only views. Decisions must not hide a malformed proposal.
    let proposals =
        proposals::load(&inst.dir).context("Cannot inspect the proposed change; no review decision was made")?;
    if proposals != inst.proposals {
        bail!("The proposed change changed while loading; reload the reflex before reviewing it");
    }
    let value = json!({ "reference": inst.reference, "manifest": inst.track.manifest, "routes": inst.track.routes,
        "route": route, "reviews": inst.reviews, "proposals": proposals });
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&value)?)))
}

/// The effective proposed route, including the imported-reflex strength ceiling.
pub fn proposed_route(inst: &Installed, p: &Proposal) -> Route {
    let mut route = crate::proposal::applied(p);
    if inst.imported && route.strength == Strength::Fire {
        route.strength = Strength::Prime;
    }
    route
}

fn directive(track: &Track, route: &Route) -> String {
    if route.evidence.execution.is_some() {
        "Execution only: no directive is delivered to the agent. Approval of this evidence does not grant execution."
            .into()
    } else {
        preview(track, route).0
    }
}

pub fn review_preview(inst: &Installed, route: &Route) -> Result<ReviewPreview> {
    let proposal = crate::proposal::current(inst.proposals.routes.get(&route.id), route).map(|p| {
        let applied = proposed_route(inst, p);
        let mut track = inst.track.clone();
        if applied != *route && !inst.imported {
            let bump = if review::shape(&applied) != review::shape(route) { Bump::Major } else { Bump::Patch };
            track.manifest.track.version = bumped(&track.manifest.track.version, bump);
        }
        ProposalPreview {
            on: p.on.clone(),
            reason: p.reason.clone(),
            diff: crate::proposal::diff(route, &applied),
            directive: directive(&track, &applied),
        }
    });
    Ok(ReviewPreview { revision: revision(inst, route)?, current_directive: directive(&inst.track, route), proposal })
}

pub fn standings(paths: &Paths, here: Option<&Path>) -> Result<BTreeMap<(String, String), Standing>> {
    let (all, _) = discover(&paths.tracks())?;
    let judged = judgements(paths);
    let index = Index::compile_root(&paths.tracks(), &all.iter().map(|i| i.track.clone()).collect::<Vec<_>>()).0;
    let mut out: BTreeMap<(String, String), Standing> = BTreeMap::new();
    for inst in &all {
        for r in &inst.track.routes {
            let proposed = crate::proposal::current(inst.proposals.routes.get(&r.id), r).is_some();
            out.insert(
                (inst.reference.clone(), r.id.clone()),
                Standing { verbs: verbs(r, proposed), reasons: vec![], preview: review_preview(inst, r).ok() },
            );
        }
    }
    for it in queue(&all, &judged, &index, paths, here) {
        if let Some(s) = out.get_mut(&(it.inst.reference.clone(), it.route.id.clone())) {
            s.reasons = it.reasons;
        }
    }
    Ok(out)
}

/// A learned (non-imported) route in the same repository whose own evidence clears the default
/// fire bar: the local first-hand evidence Design O2 requires before an import may be fire.
pub fn first_hand<'a>(all: &'a [Installed], repo: &str, id: &str) -> Option<&'a Route> {
    let bar = LearnConfig::default();
    all.iter().filter(|i| !i.imported && i.track.manifest.scope.repo == repo).flat_map(|i| i.track.routes.iter()).find(
        |r| {
            r.id == id
                && live(r.state)
                && r.evidence.support >= bar.min_support
                && r.evidence.lower_bound >= bar.fire_lower_bound
        },
    )
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
/// How much a decision changes a track's version.
pub enum Bump {
    Patch,
    Minor,
    Major,
}

/// `v` bumped by `b` (semver).
pub fn bumped(v: &str, b: Bump) -> String {
    let p: Vec<u64> = v.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let (ma, mi, pa) =
        (p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0), p.get(2).copied().unwrap_or(0));
    match b {
        Bump::Patch => format!("{ma}.{mi}.{}", pa + 1),
        Bump::Minor => format!("{ma}.{}.0", mi + 1),
        Bump::Major => format!("{}.0.0", ma + 1),
    }
}

/// A decision a person makes about one reflex.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verb {
    /// Accept as is: puts a nascent environment step into service, restores an extinguished route
    /// at forming, or applies a pinned route's proposed change.
    Approve,
    /// Take it out of service, and keep it out.
    Extinguish,
    /// Fire to prime, held across conditioning.
    Weaken,
    /// Prime to fire, within the effect ceiling only.
    Strengthen,
    /// Edit-lock: mark it `authored`, so conditioning never overwrites it.
    Pin,
    /// Keep a pinned route; silence its proposed change.
    Dismiss,
}

impl Verb {
    pub const ALL: [Verb; 6] =
        [Verb::Approve, Verb::Extinguish, Verb::Weaken, Verb::Strengthen, Verb::Pin, Verb::Dismiss];

    pub fn as_str(self) -> &'static str {
        match self {
            Verb::Approve => "approve",
            Verb::Extinguish => "extinguish",
            Verb::Weaken => "weaken",
            Verb::Strengthen => "strengthen",
            Verb::Pin => "pin",
            Verb::Dismiss => "dismiss",
        }
    }

    pub fn parse(s: &str) -> Option<Verb> {
        Verb::ALL.into_iter().find(|v| v.as_str() == s)
    }
}

/// A lifecycle state in the lexicon's words (one definition: [`crate::words::lifecycle`]).
pub use crate::words::lifecycle;

/// What a decision did.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub verb: Verb,
    /// The track's reference (its directory under the tracks directory).
    pub reference: String,
    pub route: String,
    pub repo: String,
    /// The route before and after the decision.
    pub before: Route,
    pub after: Route,
    /// `approve` applied the route's proposed change (it stays pinned).
    pub proposal_applied: bool,
    /// The decision changed nothing that was not already so.
    pub already: Option<Already>,
    /// Approving an environment instinct approved its pack: `pack <name>`, then the other routes
    /// it put into service.
    pub pack_approved: Vec<String>,
    /// `name@version` of the track as written.
    pub track: String,
    /// Where the decision was recorded, relative to the tracks directory.
    pub recorded: String,
    /// The recompiled index (`None` for `dismiss`, which changes no track).
    pub index: Option<IndexSummary>,
    /// Tracks skipped while finding this one.
    pub warnings: Vec<String>,
}

/// A decision that found nothing to change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Already {
    Extinguished,
    Approved,
}

#[derive(Debug, Clone)]
pub struct IndexSummary {
    pub generation: String,
    pub reflexes: usize,
}

/// Record one decision about `route_id` in the track `spec` (a reference or a name), and recompile
/// the index. One writer of the tracks and index at a time: this takes the tracks lock, since an
/// automatic learner may be running.
pub fn decide(
    person: &Person,
    verb: Verb,
    spec: &str,
    route_id: &str,
    by: Option<String>,
    paths: &Paths,
) -> Result<Outcome> {
    let _tracks = adage_store::lock::exclusive(&paths.tracks_lock())?;
    apply(verb, spec, route_id, by, person.via(), None, paths)
}

/// [`decide`], waiting at most `patience` for the tracks lock: for a front end that must not
/// freeze while a learn run holds it (`adage ui`). A decision that cannot get the lock in time is
/// refused, with nothing written, and can be made again in a moment.
pub fn decide_within(
    person: &Person,
    verb: Verb,
    spec: &str,
    route_id: &str,
    by: Option<String>,
    paths: &Paths,
    patience: std::time::Duration,
) -> Result<Outcome> {
    let Some(_tracks) = adage_store::lock::exclusive_within(&paths.tracks_lock(), patience)? else {
        anyhow::bail!("a learn run is writing the reflexes now; nothing was recorded, so try again in a moment");
    };
    apply(verb, spec, route_id, by, person.via(), None, paths)
}

/// Confirm exactly what a front end showed, rechecking the revision under the writer lock.
pub fn decide_reviewed_within(
    person: &Person,
    verb: Verb,
    spec: &str,
    route_id: &str,
    expected_revision: &str,
    paths: &Paths,
    patience: std::time::Duration,
) -> Result<Outcome> {
    let Some(_tracks) = adage_store::lock::exclusive_within(&paths.tracks_lock(), patience)? else {
        bail!("Conditioning is updating reflexes now; nothing was recorded. Reload and review again in a moment");
    };
    apply(verb, spec, route_id, None, person.via(), Some(expected_revision), paths)
}

fn apply(
    verb: Verb,
    spec: &str,
    route_id: &str,
    by: Option<String>,
    via: crate::person::Via,
    expected_revision: Option<&str>,
    paths: &Paths,
) -> Result<Outcome> {
    if let Some(b) = &by {
        review::check_by(b)?;
    }
    let route_id = &route_id.to_string();
    let (all, warnings) = discover(&paths.tracks())?;
    let inst = resolve(&all, spec)?;
    let mut track = inst.track.clone();
    let mut reviews = inst.reviews.clone();
    let mut props = inst.proposals.clone();
    let repo = track.manifest.scope.repo.clone();
    let r = track.routes.iter_mut().find(|r| r.id == **route_id).ok_or_else(|| {
        let ids: Vec<&str> = inst.track.routes.iter().map(|r| r.id.as_str()).collect();
        hint(format!("{} has no reflex {route_id:?}", inst.reference), format!("its reflexes: {}", ids.join(", ")))
    })?;
    let before = r.clone();
    let actual_revision = revision(inst, r)?;
    if expected_revision.is_some_and(|expected| expected != actual_revision) {
        bail!("This reflex or its proposed change changed after you opened the review. Nothing was written. Reload and inspect the new proposal before confirming.");
    }
    let proposal = crate::proposal::current(props.routes.get(route_id.as_str()), r).cloned();
    if verb == Verb::Dismiss {
        let p = proposal
            .with_context(|| format!("{route_id} has no proposed change to dismiss (`adage review` lists them)"))?;
        props.routes.remove(route_id.as_str());
        props.dismissed.insert(route_id.to_string(), review::basis(&p.route, 0, 0));
        proposals::save(&inst.dir, &props)?;
        return Ok(Outcome {
            verb,
            reference: inst.reference.clone(),
            route: route_id.to_string(),
            repo,
            after: before.clone(),
            before,
            proposal_applied: false,
            already: None,
            pack_approved: vec![],
            track: format!("{}@{}", track.manifest.track.name, track.manifest.track.version),
            recorded: format!("{}/{}", inst.reference, proposals::FILE),
            index: None,
            warnings,
        });
    }
    let (decision, bump) = match verb {
        Verb::Approve if proposal.is_some() => {
            // Apply the proposed version; it stays pinned, so later drift is proposed again.
            let p = proposal.as_ref().unwrap();
            *r = proposed_route(inst, p);
            props.routes.remove(route_id.as_str());
            props.dismissed.remove(route_id.as_str());
            let bump = if review::shape(r) != review::shape(&before) { Bump::Major } else { Bump::Patch };
            (Decision::Pinned, bump)
        }
        Verb::Approve if r.state == RouteState::Nascent => {
            // An environment step waiting for a person (O11): approval puts it into service,
            // prime at most; an import or an instinct stays at forming.
            r.state = if inst.imported || r.is_instinct() { RouteState::Forming } else { RouteState::Firm };
            if r.strength == Strength::Fire {
                r.strength = Strength::Prime;
            }
            (Decision::Approved, Bump::Minor)
        }
        Verb::Approve => {
            if !live(r.state) {
                // Approving an extinguished or weakened route puts it back into service at forming.
                r.state = RouteState::Forming;
                if r.strength == Strength::Fire {
                    r.strength = Strength::Prime;
                }
            }
            (Decision::Approved, Bump::Minor)
        }
        Verb::Extinguish => {
            r.state = RouteState::Extinguished;
            (Decision::Extinguished, Bump::Major)
        }
        Verb::Weaken => {
            if r.strength != Strength::Fire || !live(r.state) {
                return Err(hint(
                    format!(
                        "{route_id} is {} ({}); only a fire reflex in service can be weakened",
                        strength_str(r.strength),
                        lifecycle(r.state)
                    ),
                    format!("to take it out of service: adage review extinguish {} {route_id}", inst.reference),
                ));
            }
            r.strength = Strength::Prime;
            (Decision::Weakened, Bump::Major)
        }
        Verb::Strengthen => {
            if r.strength != Strength::Prime || !live(r.state) {
                bail!(
                    "{route_id} is {} ({}); only a prime reflex in service can be strengthened",
                    strength_str(r.strength),
                    lifecycle(r.state)
                );
            }
            if r.is_instinct() {
                bail!("refusing: {route_id} is an instinct (Adage's built-in default), which is prime at most; a reflex conditioned on this repository's traces can be fire");
            }
            if matches!(r.step, Step::Card { .. }) {
                bail!("refusing: {route_id} is a procedure card, which is prime at most: it is advice to read, not a command to run");
            }
            if route_ceiling(r) != Some(Strength::Fire) {
                bail!(
                    "refusing: {route_id} is a {} step{}, whose ceiling is {}; no one can raise a reflex above its effect ceiling",
                    r.effect.as_str(),
                    if r.effect == adage_core::Effect::Regenerate { " not triggered by an edit" } else { "" },
                    ceiling_str(r)
                );
            }
            if inst.imported && first_hand(&all, &repo, route_id).is_none() {
                let bar = LearnConfig::default();
                bail!(
                    "refusing: {route_id} was imported, and an import cannot be fire without local first-hand evidence (Design O2): \
                     a reflex conditioned here with at least {} traces and a lower bound of at least {:.2}",
                    bar.min_support,
                    bar.fire_lower_bound
                );
            }
            r.strength = Strength::Fire;
            (Decision::Strengthened, Bump::Minor)
        }
        Verb::Pin => {
            r.authored = true;
            (Decision::Pinned, Bump::Patch)
        }
        Verb::Dismiss => unreachable!("handled above"),
    };
    let (followed, ignored) =
        judgements(paths).get(&(inst.reference.clone(), route_id.to_string())).copied().unwrap_or_default();
    let mut basis = review::basis(r, followed, ignored);
    let after = r.clone();
    // Pack-level approval: approving one environment-effect instinct approves the pack's
    // environment steps for this repository, now and when learning binds them again.
    let mut pack_approved: Vec<String> = Vec::new();
    if verb == Verb::Approve && before.state == RouteState::Nascent && after.is_instinct() && !quiet(&before) {
        if let adage_tracks::Origin::Instinct { pack, .. } = after.origin() {
            if !pack.is_empty() {
                reviews.packs.insert(pack.clone(), review::PackApproval { on: review::today(), by: by.clone() });
                let same_pack = |x: &adage_tracks::Route| matches!(x.origin(), adage_tracks::Origin::Instinct { pack: p, .. } if p == pack);
                for other in track
                    .routes
                    .iter_mut()
                    .filter(|x| x.state == RouteState::Nascent && x.is_instinct() && !quiet(x) && same_pack(x))
                {
                    other.state = RouteState::Forming;
                    pack_approved.push(other.id.clone());
                }
                pack_approved.insert(0, format!("pack {pack}"));
            }
        }
    }
    // An import keeps its author's version: it names the release that was shared.
    if after != before && !inst.imported {
        track.manifest.track.version = bumped(&track.manifest.track.version, bump);
    }
    // A version bump starts a new measurement cohort; old counts cannot mask new outcomes.
    basis.runtime_version = Some(track.manifest.track.version.clone());
    if track.manifest.track.version != inst.track.manifest.track.version {
        basis.followed = 0;
        basis.ignored = 0;
    }
    reviews.routes.insert(route_id.to_string(), Review { decision, on: review::today(), by, basis, via: Some(via) });
    io::validate(&track).context("the reviewed track fails validation; nothing was written")?;
    io::write(&inst.dir, &track)?;
    review::save(&inst.dir, &reviews)?;
    if props != inst.proposals {
        proposals::save(&inst.dir, &props)?;
    }

    // Recompile the index exactly as `adage condition` and `adage add` do.
    let tracks = io::read_all(&paths.tracks())?;
    let index = Index::compile_root(&paths.tracks(), &tracks).0;
    index.write_atomic(&paths.index())?;

    let was = inst.reviews.routes.get(route_id.as_str()).map(|v| v.decision);
    let already = if verb == Verb::Extinguish && before.state == RouteState::Extinguished {
        Some(Already::Extinguished)
    } else if verb == Verb::Approve && proposal.is_none() && was == Some(Decision::Approved) && after == before {
        Some(Already::Approved)
    } else {
        None
    };
    Ok(Outcome {
        verb,
        reference: inst.reference.clone(),
        route: route_id.to_string(),
        repo,
        before,
        after,
        proposal_applied: verb == Verb::Approve && proposal.is_some(),
        already,
        pack_approved,
        track: format!("{}@{}", track.manifest.track.name, track.manifest.track.version),
        recorded: format!("{}/{}", inst.reference, review::FILE),
        index: Some(IndexSummary { generation: index.generation.clone(), reflexes: index.routes.len() }),
        warnings,
    })
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn examples_satisfy_their_contracts() {
        assert_eq!(example_for(r"^W-\d{4}$").unwrap(), "W-1234");
        assert_eq!(example_for(r"^PROJ-\d{2,5}$").unwrap(), "PROJ-12");
        assert_eq!(example_for(r"^[A-Z]+-\d+$").unwrap(), "A-1");
        assert!(example_for(r"^(a|b)$").is_none());
        assert!(example_for("W-1").is_none(), "unanchored contracts get no example");
    }

    #[test]
    fn versions_bump() {
        assert_eq!(bumped("1.2.3", Bump::Patch), "1.2.4");
        assert_eq!(bumped("1.2.3", Bump::Minor), "1.3.0");
        assert_eq!(bumped("1.2.3", Bump::Major), "2.0.0");
    }

    use adage_core::Effect;
    use adage_tracks::{Evidence, Exit, Gauge, Manifest, Scope, TrackInfo};

    fn route(id: &str, strength: Strength, state: RouteState) -> Route {
        Route {
            id: id.into(),
            summary: format!("route {id}"),
            trigger: Trigger::SessionStart,
            step: Step::Run { command: "make test".into() },
            slots: vec![],
            effect: Effect::Verify,
            strength,
            state,
            exit: vec![Exit::IntentChanges],
            evidence: Evidence {
                support: 10,
                opportunities: 12,
                days: 2,
                rate: 10.0 / 12.0,
                lower_bound: wilson_lower(10, 12),
                provenance: "desire-path".into(),
                ..Default::default()
            },
            authored: false,
        }
    }

    /// An `ADAGE_HOME` holding one learned track `acme/learned` with these routes.
    fn home(routes: Vec<Route>) -> (tempfile::TempDir, Paths) {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let track = Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme/learned".into(),
                    version: "0.1.0".into(),
                    description: "test".into(),
                    provenance: "learned".into(),
                },
                scope: Scope { repo: "acme".into() },
                gauge: Gauge::default(),
            },
            routes,
        };
        io::write(&paths.tracks().join("acme/learned"), &track).unwrap();
        (d, paths)
    }

    fn decide_as_person(paths: &Paths, verb: Verb, route: &str) -> Result<Outcome> {
        decide(&Person::for_tests(), verb, "acme/learned", route, None, paths)
    }

    #[test]
    fn a_decision_versions_the_track_records_it_and_recompiles_the_index() {
        let (_d, paths) = home(vec![route("t", Strength::Fire, RouteState::Firm)]);
        let o = decide_as_person(&paths, Verb::Weaken, "t").unwrap();
        assert_eq!((o.before.strength, o.after.strength), (Strength::Fire, Strength::Prime));
        assert_eq!(o.track, "acme/learned@1.0.0", "weakening is a major change");
        assert_eq!(o.recorded, "acme/learned/review.yaml");
        assert_eq!(
            review::load(&paths.tracks().join("acme/learned")).unwrap().routes["t"].decision,
            Decision::Weakened
        );
        let index = Index::load(&paths.index());
        assert_eq!(o.index.as_ref().map(|i| i.generation.as_str()), Some(index.generation.as_str()));
        assert_eq!(index.routes[0].route.strength, Strength::Prime);
    }

    #[test]
    fn refusals_change_nothing() {
        let (_d, paths) = home(vec![route("t", Strength::Prime, RouteState::Firm)]);
        let e = decide_as_person(&paths, Verb::Weaken, "t").unwrap_err();
        let h = e.downcast_ref::<crate::error::Hint>().expect("a refusal says what to do instead");
        assert!(h.what.contains("only a fire reflex in service can be weakened"), "{}", h.what);
        assert!(h.help.contains("adage review extinguish acme/learned t"), "{}", h.help);
        assert!(decide_as_person(&paths, Verb::Dismiss, "t").unwrap_err().to_string().contains("no proposed change"));
        assert!(decide_as_person(&paths, Verb::Approve, "missing").is_err());
        let t = io::read(&paths.tracks().join("acme/learned")).unwrap();
        assert_eq!((t.manifest.track.version.as_str(), t.routes[0].strength), ("0.1.0", Strength::Prime));
        assert!(!paths.index().exists(), "nothing was compiled");
    }

    #[test]
    fn approval_puts_a_nascent_step_into_service_at_prime_at_most() {
        let (_d, paths) = home(vec![route("t", Strength::Fire, RouteState::Nascent)]);
        let o = decide_as_person(&paths, Verb::Approve, "t").unwrap();
        assert_eq!((o.after.state, o.after.strength), (RouteState::Firm, Strength::Prime));
        assert_eq!(o.already, None);
        assert_eq!(decide_as_person(&paths, Verb::Approve, "t").unwrap().already, Some(Already::Approved));
    }

    #[test]
    fn extinguishing_twice_says_so() {
        let (_d, paths) = home(vec![route("t", Strength::Prime, RouteState::Firm)]);
        assert_eq!(decide_as_person(&paths, Verb::Extinguish, "t").unwrap().already, None);
        let again = decide_as_person(&paths, Verb::Extinguish, "t").unwrap();
        assert_eq!(again.already, Some(Already::Extinguished));
        assert_eq!(again.track, "acme/learned@1.0.0", "nothing changed, so no second bump");
    }

    /// One writer of the tracks at a time, whichever front end decides (`adage ui` once wrote
    /// without the lock while a learner could be writing).
    #[test]
    fn a_decision_waits_for_the_tracks_lock() {
        let (_d, paths) = home(vec![route("t", Strength::Fire, RouteState::Firm)]);
        let held = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let p = paths.clone();
        let decider = std::thread::spawn(move || {
            let r = decide_as_person(&p, Verb::Weaken, "t").map(|o| o.after.strength);
            tx.send(()).unwrap();
            r
        });
        assert!(rx.recv_timeout(std::time::Duration::from_millis(300)).is_err(), "decided while the lock was held");
        drop(held);
        assert_eq!(decider.join().unwrap().unwrap(), Strength::Prime);
    }

    /// A front end that must not freeze is refused, not blocked, while a learner holds the lock.
    #[test]
    fn a_bounded_decision_is_refused_while_the_lock_is_held() {
        let (_d, paths) = home(vec![route("t", Strength::Fire, RouteState::Firm)]);
        let held = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        let person = Person::for_tests();
        let wait = std::time::Duration::from_millis(50);
        let busy = decide_within(&person, Verb::Weaken, "acme/learned", "t", None, &paths, wait).unwrap_err();
        assert!(format!("{busy:#}").contains("try again"), "{busy:#}");
        drop(held);
        let o = decide_within(&person, Verb::Weaken, "acme/learned", "t", None, &paths, wait).unwrap();
        assert_eq!(o.after.strength, Strength::Prime);
    }

    #[test]
    fn verbs_round_trip() {
        for v in Verb::ALL {
            assert_eq!(Verb::parse(v.as_str()), Some(v));
        }
        assert_eq!(Verb::parse("delegate"), None);
    }

    fn proposal_home() -> (tempfile::TempDir, Paths) {
        let mut pinned = route("t", Strength::Prime, RouteState::Firm);
        pinned.authored = true;
        let (dir, paths) = home(vec![pinned.clone()]);
        let mut changed = pinned.clone();
        changed.step = Step::Run { command: "make check".into() };
        changed.summary = "Run all checks".into();
        let props = proposals::propose(&Proposals::default(), &[(pinned, Some(changed))], "2026-10-05");
        proposals::save(&paths.tracks().join("acme/learned"), &props).unwrap();
        (dir, paths)
    }
    fn inspected(paths: &Paths) -> ReviewPreview {
        standings(paths, None).unwrap().remove(&("acme/learned".into(), "t".into())).unwrap().preview.unwrap()
    }
    fn confirm(paths: &Paths, preview: &ReviewPreview) -> Result<Outcome> {
        decide_reviewed_within(
            &Person::for_tests(),
            Verb::Approve,
            "acme/learned",
            "t",
            &preview.revision,
            paths,
            std::time::Duration::from_millis(20),
        )
    }
    #[test]
    fn the_proposed_preview_is_exactly_what_approval_installs() {
        let (_dir, paths) = proposal_home();
        let shown = inspected(&paths);
        assert!(shown.current_directive.contains("make test"));
        let proposed = shown.proposal.as_ref().unwrap();
        assert!(proposed.diff.iter().any(|(key, before, after)| *key == "step"
            && before.contains("make test")
            && after.contains("make check")));
        assert!(proposed.diff.iter().any(|(key, _, _)| *key == "summary"));
        assert!(proposed.directive.contains("make check"));
        let result = confirm(&paths, &shown).unwrap();
        let track = io::read(&paths.tracks().join("acme/learned")).unwrap();
        assert_eq!(preview(&track, &track.routes[0]).0, proposed.directive);
        assert!(result.proposal_applied);
        assert!(track.routes[0].authored);
    }
    #[test]
    fn changed_removed_or_new_proposals_cannot_be_approved_from_an_old_preview() {
        for change in ["changed", "removed", "new", "route", "version", "malformed"] {
            let (_dir, paths) = proposal_home();
            let dir = paths.tracks().join("acme/learned");
            let original_props = proposals::load(&dir).unwrap();
            if change == "new" {
                proposals::save(&dir, &Proposals::default()).unwrap();
            }
            let shown = inspected(&paths);
            match change {
                "changed" => {
                    let mut p = original_props.clone();
                    p.routes.get_mut("t").unwrap().route.step = Step::Run { command: "make lint".into() };
                    proposals::save(&dir, &p).unwrap();
                }
                "removed" => proposals::save(&dir, &Proposals::default()).unwrap(),
                "new" => proposals::save(&dir, &original_props).unwrap(),
                "route" | "version" => {
                    let mut t = io::read(&dir).unwrap();
                    if change == "route" {
                        t.routes[0].summary = "Changed".into();
                    } else {
                        t.manifest.track.version = "0.2.0".into();
                    }
                    io::write(&dir, &t).unwrap();
                }
                "malformed" => std::fs::write(dir.join(proposals::FILE), "routes: [invalid").unwrap(),
                _ => unreachable!(),
            }
            let before = io::read(&dir).unwrap();
            assert!(confirm(&paths, &shown).is_err(), "accepted {change} state");
            assert_eq!(io::read(&dir).unwrap(), before);
            assert!(!paths.index().exists());
            assert!(!dir.join(review::FILE).exists());
        }
    }
    #[test]
    fn malformed_proposals_disable_confirmation_and_lock_contention_writes_nothing() {
        let (_dir, paths) = proposal_home();
        let shown = inspected(&paths);
        let held = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        assert!(confirm(&paths, &shown).is_err());
        drop(held);
        let dir = paths.tracks().join("acme/learned");
        std::fs::write(dir.join(proposals::FILE), "routes: [invalid").unwrap();
        assert!(standings(&paths, None).unwrap()[&("acme/learned".into(), "t".into())].preview.is_none());
        assert!(!paths.index().exists());
    }
}
