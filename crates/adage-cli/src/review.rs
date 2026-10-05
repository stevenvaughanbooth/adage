//! `adage review` (Design J1, §10.4, §10.9; Architecture §6.3): the routes that wait for a person,
//! and the decisions a person makes about them.
//!
//! ```text
//! adage review [--json]                                  list the queue (agents may read it)
//! adage review approve <track> <route> [--by <name>]     accept as is (restores a retired route at probation;
//!                                                        puts an environment step, a draft until then, into service)
//! adage review reject  <track> <route> [--by <name>]     retire it; `retire` is the same
//! adage review demote  <track> <route> [--by <name>]     green → yellow, held across re-learning
//! adage review promote <track> <route> [--by <name>]     yellow → green, within the effect ceiling only
//! adage review pin     <track> <route> [--by <name>]     edit-lock: mark `authored`
//! adage review dismiss <track> <route>                   keep a pinned route; silence its proposed change
//! ```
//!
//! A pinned route whose learned version drifted materially waits with the reason "proposed" and a
//! diff (see [`crate::proposal`]); `approve` applies the proposed version, which stays pinned.
//!
//! `<track>` is a track name (a learned track wins over an import of the same name) or its
//! directory under the tracks directory, e.g. `@imported/acme/work-items`.
//!
//! Decisions change trust, so they refuse to run inside an agent session (review finding F11).
//! They are recorded in the track's `review.yaml` (see [`adage_tracks::review`]); every decision
//! re-validates the track and recompiles the index atomically, as `adage learn` and `adage add` do.

use crate::share::{describe_step, require_person};
use adage_learn::LearnConfig;
use adage_runtime::render;
use adage_store::Paths;
use adage_tracks::proposals::{self, Proposal, Proposals};
use adage_tracks::review::{self, aspect_str, Decision, Review, Reviews};
use adage_tracks::{
    ceiling, index, io, wilson_lower, Aspect, Contract, Index, Route, RouteState, Step, Track, Trigger,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Runtime judgements needed before a derail signal is considered.
const MIN_JUDGED: u32 = 5;
/// A route is flagged when the Wilson lower bound of its derail share reaches this.
const DERAIL_BAR: f64 = 0.5;

const USAGE: &str = "usage: adage review [--json]\n       adage review <approve|reject|retire|demote|promote|pin|dismiss> <track> <route> [--by <name>]";

pub(crate) struct Installed {
    pub(crate) dir: PathBuf,
    /// The track's directory relative to the tracks directory: unambiguous, and what the queue prints.
    pub(crate) reference: String,
    pub(crate) track: Track,
    pub(crate) imported: bool,
    pub(crate) reviews: Reviews,
    /// Proposed changes to pinned routes, from `adage learn` or a re-run `adage add`.
    pub(crate) proposals: Proposals,
}

pub fn review(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("list") | Some("--json") => list(paths, args.iter().any(|a| a == "--json")),
        Some(verb @ ("approve" | "reject" | "retire" | "demote" | "promote" | "pin" | "dismiss")) => {
            decide(verb, &args[1..], paths)
        }
        _ => bail!("{USAGE}"),
    }
}

/// Every installed track with its directory and decisions. Unreadable tracks are skipped, as
/// `io::read_all` does; symlinked directories are never followed.
pub(crate) fn discover(root: &Path) -> Result<Vec<Installed>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join("track.toml").exists() {
            let loaded = io::read(&d).and_then(|t| Ok((t, review::load(&d)?)));
            match loaded {
                Ok((track, reviews)) => out.push(Installed {
                    reference: d.strip_prefix(root).unwrap_or(&d).to_string_lossy().replace('\\', "/"),
                    // Proposals are advisory local state: an unreadable file hides them, nothing more.
                    proposals: proposals::load(&d).unwrap_or_else(|e| {
                        crate::ui::warn(format!("adage: ignoring unreadable proposals in {}: {e:#}", d.display()));
                        Proposals::default()
                    }),
                    dir: d,
                    imported: index::is_imported(&track),
                    track,
                    reviews,
                }),
                Err(e) => crate::ui::warn(format!("adage: skipping invalid track {}: {e:#}", d.display())),
            }
            continue;
        }
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    Ok(out)
}

fn resolve<'a>(all: &'a [Installed], spec: &str) -> Result<&'a Installed> {
    if let Some(i) = all.iter().find(|i| i.reference == spec) {
        return Ok(i);
    }
    let named: Vec<&Installed> = all.iter().filter(|i| i.track.manifest.track.name == spec).collect();
    named
        .iter()
        .find(|i| !i.imported)
        .or(named.first())
        .copied()
        .with_context(|| format!("no installed track {spec:?} (see `adage review` or `adage status`)"))
}

/// Followed / not observed judgements are attributed to the recorded collection/version,
/// never just a route id that could also belong to an import or an older version.
fn judgements(paths: &Paths, installed: &[Installed]) -> BTreeMap<(String, String), (u32, u32)> {
    crate::graph::current_judgements(paths, installed)
}

fn live(s: RouteState) -> bool {
    matches!(s, RouteState::Active | RouteState::Probation)
}

fn by_edit(r: &Route) -> bool {
    matches!(r.trigger, Trigger::AfterEdit { .. })
}

pub(crate) fn ceiling_str(r: &Route) -> &'static str {
    ceiling(r.effect, by_edit(r)).map(aspect_str).unwrap_or("none")
}

fn lower(x: impl std::fmt::Debug) -> String {
    format!("{x:?}").to_lowercase()
}

/// An example value satisfying a simple anchored contract such as `^W-\d{4}$` (literals, `\d`,
/// `\w`, simple classes, and `{n}` `{n,m}` `+` `*` `?`), checked against the contract itself.
fn example_for(regex: &str) -> Option<String> {
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
pub(crate) fn preview(t: &Track, r: &Route) -> (String, BTreeMap<String, String>) {
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
        .unwrap_or_else(|| "(this route cannot be rendered safely; Adage would say nothing)".into());
    let m = &t.manifest.track;
    (render::wrap(&m.name, &m.version, &r.id, r.aspect, &body), bound)
}

struct Item<'a> {
    inst: &'a Installed,
    route: &'a Route,
    reasons: Vec<(&'static str, String)>,
    followed: u32,
    derailed: u32,
    proposal: Option<&'a Proposal>,
}

fn queue<'a>(all: &'a [Installed], judged: &BTreeMap<(String, String), (u32, u32)>, index: &Index) -> Vec<Item<'a>> {
    let mut items = Vec::new();
    for inst in all {
        let m = &inst.track.manifest;
        for r in &inst.track.routes {
            let rv = inst.reviews.routes.get(&r.id);
            let (f0, d0) = judged.get(&(inst.reference.clone(), r.id.clone())).copied().unwrap_or_default();
            // Only what happened since the last decision counts toward a new signal.
            let (followed, derailed) = match rv {
                Some(v) if v.runtime_version.as_deref() == Some(&m.track.version) => {
                    (f0.saturating_sub(v.basis.followed), d0.saturating_sub(v.basis.derailed))
                }
                _ => (f0, d0),
            };
            let mut reasons: Vec<(&'static str, String)> = Vec::new();
            match rv {
                None if r.state == RouteState::Draft && !r.authored => reasons.push((
                    "approval",
                    "an environment step: it is delivered only after a person approves it (`approve`)".into(),
                )),
                None if r.authored || !live(r.state) => {}
                None if inst.imported => reasons.push((
                    "imported",
                    "imported from a shared track and at probation: yellow at most until local first-hand evidence confirms it".into(),
                )),
                None => reasons.push(("new", "learned by `adage learn` and never reviewed".into())),
                Some(v) if v.decision == Decision::Rejected => {
                    if r.state != RouteState::Retired {
                        let why = review::material_change(&v.basis, r).unwrap_or_else(|| "its evidence strengthened".into());
                        reasons.push(("resurfaced", format!("rejected on {}; since then {why}, so it is back at probation", v.on)));
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
            let n = followed + derailed;
            if delivered && r.aspect != Aspect::Red && n >= MIN_JUDGED && wilson_lower(derailed, n) >= DERAIL_BAR {
                let advice = if r.aspect == Aspect::Green { "demoting it" } else { "retiring it" };
                let since = if rv.is_some_and(|v| v.runtime_version.as_deref() == Some(&m.track.version)) {
                    " since the last decision"
                } else {
                    ""
                };
                reasons.push((
                    "derailing",
                    format!("the response was not observed within the next 3 observed tool batches in {derailed} of {n} judged follow windows for this version{since}; inspect the traces before {advice}"),
                ));
            }
            if !reasons.is_empty() {
                items.push(Item { inst, route: r, reasons, followed, derailed, proposal });
            }
        }
    }
    items
}

fn list(paths: &Paths, as_json: bool) -> Result<()> {
    let all = discover(&paths.tracks())?;
    let judged = judgements(paths, &all);
    let index = Index::compile(&all.iter().map(|i| i.track.clone()).collect::<Vec<_>>());
    let items = queue(&all, &judged, &index);
    if as_json {
        let rows: Vec<Value> = items
            .iter()
            .map(|it| {
                let (t, r, e) = (&it.inst.track, it.route, &it.route.evidence);
                let (directive, examples) = preview(t, r);
                json!({
                    "track": it.inst.reference,
                    "name": t.manifest.track.name,
                    "version": t.manifest.track.version,
                    "repo": t.manifest.scope.repo,
                    "imported": it.inst.imported,
                    "route": r.id,
                    "reasons": it.reasons.iter().map(|(k, d)| json!({ "kind": k, "detail": d })).collect::<Vec<_>>(),
                    "aspect": aspect_str(r.aspect),
                    "ceiling": ceiling_str(r),
                    "effect": r.effect.as_str(),
                    "state": lower(r.state),
                    "authored": r.authored,
                    "decisions": verbs(r, it.proposal.is_some()),
                    "step": describe_step(&r.step),
                    "evidence": { "support": e.support, "opportunities": e.opportunities, "days": e.days, "rate": e.rate, "lower_bound": e.lower_bound },
                    "runtime": { "followed": it.followed, "derailed": it.derailed },
                    "directive": directive,
                    "example_values": examples,
                    "last_review": it.inst.reviews.routes.get(&r.id).map(|v| json!({ "decision": v.decision.as_str(), "on": v.on, "by": v.by })),
                    "proposal": it.proposal.map(|p| {
                        let diff: Vec<Value> = crate::proposal::diff(r, &p.route)
                            .into_iter()
                            .map(|(field, pinned, proposed)| json!({ "field": field, "pinned": pinned, "proposed": proposed }))
                            .collect();
                        json!({ "on": p.on, "reason": p.reason, "diff": diff, "directive": preview(t, &crate::proposal::applied(p)).0 })
                    }),
                })
            })
            .collect();
        println!("{}", serde_json::to_string_pretty(&json!({ "count": rows.len(), "queue": rows }))?);
        return Ok(());
    }
    if items.is_empty() {
        println!("adage review: nothing awaits a decision.");
        return Ok(());
    }
    println!("adage review: {} route(s) await a decision\n", items.len());
    for it in &items {
        let (t, r, e) = (&it.inst.track, it.route, &it.route.evidence);
        let origin = if it.inst.imported { "  (imported)" } else { "" };
        println!(
            "{} @ {}  {}   [{}]{origin}",
            it.inst.reference, t.manifest.track.version, r.id, t.manifest.scope.repo
        );
        for (kind, detail) in &it.reasons {
            println!("  why        {kind}: {detail}");
        }
        let ceil = if r.aspect == Aspect::Red { String::new() } else { format!(" (ceiling {})", ceiling_str(r)) };
        println!(
            "  aspect     {}{ceil} · effect {} · state {}{}",
            aspect_str(r.aspect),
            r.effect.as_str(),
            lower(r.state),
            if r.authored { " · authored" } else { "" }
        );
        println!(
            "  evidence   {}/{} sessions over {} day(s) · rate {:.2} · Wilson lower bound {:.2}",
            e.support, e.opportunities, e.days, e.rate, e.lower_bound
        );
        println!(
            "  runtime    followed {} · not observed in follow window {} (guided, current version)",
            it.followed, it.derailed
        );
        println!("  step       {}", describe_step(&r.step));
        let (directive, examples) = preview(t, r);
        let ex = if examples.is_empty() {
            String::new()
        } else {
            format!(
                " (example values: {})",
                examples.iter().map(|(k, v)| format!("{k} = {v}")).collect::<Vec<_>>().join(", ")
            )
        };
        println!("  directive{ex}");
        for line in directive.lines() {
            println!("    {line}");
        }
        if let Some(p) = it.proposal {
            println!("  proposed   `adage review approve` applies these changes (the route stays pinned):");
            for (field, pinned, proposed) in crate::proposal::diff(r, &p.route) {
                println!("    - {field:<9} {pinned}");
                println!("    + {field:<9} {proposed}");
            }
        }
        println!(
            "  decide     adage review <{}> {} {}\n",
            verbs(r, it.proposal.is_some()).join("|"),
            it.inst.reference,
            r.id
        );
    }
    Ok(())
}

/// The decisions that can apply to a route as it stands (promotion of an import may still be
/// refused for want of first-hand evidence).
fn verbs(r: &Route, proposed: bool) -> Vec<&'static str> {
    let mut v = vec!["approve", "reject"];
    if proposed {
        v.push("dismiss");
    }
    if live(r.state) && r.aspect == Aspect::Green {
        v.push("demote");
    }
    if live(r.state) && r.aspect == Aspect::Yellow && ceiling(r.effect, by_edit(r)) == Some(Aspect::Green) {
        v.push("promote");
    }
    if !r.authored {
        v.push("pin");
    }
    v
}

/// What `adage ui` shows beside each installed route: the decisions that apply to it, and why it
/// awaits review (empty when it does not). Keyed by (track reference, route id).
pub(crate) struct Standing {
    pub(crate) verbs: Vec<&'static str>,
    pub(crate) reasons: Vec<(&'static str, String)>,
    pub(crate) preview: ReviewPreview,
}

/// Everything a person can approve, captured from the same version of the installed state.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ReviewPreview {
    pub(crate) revision: String,
    pub(crate) current_directive: String,
    pub(crate) proposal: Option<ProposalPreview>,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ProposalPreview {
    pub(crate) on: String,
    pub(crate) reason: String,
    pub(crate) diff: Vec<(&'static str, String, String)>,
    pub(crate) directive: String,
}

/// A revision includes the proposal's absence: a newly arrived proposal must never be accepted
/// by a confirmation that only showed the current reflex. All writers hold `tracks_lock`.
fn revision(inst: &Installed, route: &Route) -> Result<String> {
    // Discovery tolerates broken advisory files for read-only reports. A review must fail closed.
    proposals::load(&inst.dir).context("Cannot inspect the proposed change; no review decision was made")?;
    let reviewed = json!({
        "reference": inst.reference,
        "manifest": inst.track.manifest,
        "route": route,
        "review": inst.reviews.routes.get(&route.id),
        "proposal": inst.proposals.routes.get(&route.id),
        "dismissed": inst.proposals.dismissed.get(&route.id),
    });
    Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(&reviewed)?)))
}

fn review_preview(inst: &Installed, route: &Route) -> Result<ReviewPreview> {
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
            directive: preview(&track, &applied).0,
        }
    });
    Ok(ReviewPreview { revision: revision(inst, route)?, current_directive: preview(&inst.track, route).0, proposal })
}

fn proposed_route(inst: &Installed, p: &Proposal) -> Route {
    let mut route = crate::proposal::applied(p);
    if inst.imported && route.aspect == Aspect::Green {
        route.aspect = Aspect::Yellow;
    }
    route
}

pub(crate) fn standings(paths: &Paths) -> Result<BTreeMap<(String, String), Standing>> {
    let _tracks = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let all = discover(&paths.tracks())?;
    let judged = judgements(paths, &all);
    let index = Index::compile(&all.iter().map(|i| i.track.clone()).collect::<Vec<_>>());
    let mut out: BTreeMap<(String, String), Standing> = BTreeMap::new();
    for inst in &all {
        for r in &inst.track.routes {
            let proposed = crate::proposal::current(inst.proposals.routes.get(&r.id), r).is_some();
            out.insert(
                (inst.reference.clone(), r.id.clone()),
                Standing { verbs: verbs(r, proposed), reasons: vec![], preview: review_preview(inst, r)? },
            );
        }
    }
    for it in queue(&all, &judged, &index) {
        if let Some(s) = out.get_mut(&(it.inst.reference.clone(), it.route.id.clone())) {
            s.reasons = it.reasons;
        }
    }
    Ok(out)
}

/// A learned (non-imported) route in the same repository whose own evidence clears the default
/// green bar: the local first-hand evidence Design O2 requires before an import may be green.
pub(crate) fn first_hand<'a>(all: &'a [Installed], repo: &str, id: &str) -> Option<&'a Route> {
    let bar = LearnConfig::default();
    all.iter().filter(|i| !i.imported && i.track.manifest.scope.repo == repo).flat_map(|i| i.track.routes.iter()).find(
        |r| {
            r.id == id
                && live(r.state)
                && r.evidence.support >= bar.min_support
                && r.evidence.lower_bound >= bar.green_lower_bound
        },
    )
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Bump {
    Patch,
    Minor,
    Major,
}

fn bumped(v: &str, b: Bump) -> String {
    let p: Vec<u64> = v.split('.').map(|x| x.parse().unwrap_or(0)).collect();
    let (ma, mi, pa) =
        (p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0), p.get(2).copied().unwrap_or(0));
    match b {
        Bump::Patch => format!("{ma}.{mi}.{}", pa + 1),
        Bump::Minor => format!("{ma}.{}.0", mi + 1),
        Bump::Major => format!("{}.0.0", ma + 1),
    }
}

fn decide(verb: &str, args: &[String], paths: &Paths) -> Result<()> {
    require_person(&format!("review {verb}"))?;
    // One writer of the tracks and index at a time: an automatic learner may be running.
    let _tracks = adage_store::lock::exclusive(&paths.tracks_lock())?;
    let pos: Vec<&String> = {
        let mut v = Vec::new();
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--by" {
                i += 2;
                continue;
            }
            v.push(&args[i]);
            i += 1;
        }
        v
    };
    let (Some(spec), Some(route_id), None) = (pos.first(), pos.get(1), pos.get(2)) else { bail!("{USAGE}") };
    let by = crate::flag(args, "--by");
    for line in apply(verb, spec, route_id, by, None, paths)? {
        println!("{line}");
    }
    Ok(())
}

/// A decision from `adage ui`: the same guard and the same code path as `adage review <verb>`.
pub(crate) fn decide_quietly(
    verb: &str,
    spec: &str,
    route_id: &str,
    expected_revision: &str,
    paths: &Paths,
) -> Result<Vec<String>> {
    require_person(&format!("review {verb}"))?;
    apply_reviewed(verb, spec, route_id, expected_revision, paths)
}

fn apply_reviewed(
    verb: &str,
    spec: &str,
    route_id: &str,
    expected_revision: &str,
    paths: &Paths,
) -> Result<Vec<String>> {
    let _tracks = adage_store::lock::try_exclusive(&paths.tracks_lock())?
        .context("Learning or another review is updating reflexes. Wait a moment, reload, and review again.")?;
    apply(verb, spec, route_id, None, Some(expected_revision), paths)
}

/// Record one decision and recompile the index; returns the report `adage review` prints. The
/// caller has checked for a person at the terminal and holds the tracks writer lock.
fn apply(
    verb: &str,
    spec: &str,
    route_id: &str,
    by: Option<String>,
    expected_revision: Option<&str>,
    paths: &Paths,
) -> Result<Vec<String>> {
    if !matches!(verb, "approve" | "reject" | "retire" | "demote" | "promote" | "pin" | "dismiss") {
        bail!("{USAGE}");
    }
    if let Some(b) = &by {
        review::check_by(b)?;
    }
    let route_id = &route_id.to_string();
    let mut out: Vec<String> = Vec::new();
    let all = discover(&paths.tracks())?;
    let inst = resolve(&all, spec)?;
    let mut track = inst.track.clone();
    let mut reviews = inst.reviews.clone();
    let mut props = inst.proposals.clone();
    let repo = track.manifest.scope.repo.clone();
    let r = track
        .routes
        .iter_mut()
        .find(|r| r.id == **route_id)
        .with_context(|| format!("{} has no route {route_id:?}", inst.reference))?;
    let before = r.clone();
    if let Some(expected) = expected_revision {
        if revision(inst, r)? != expected {
            bail!("This reflex or its proposed change changed after you opened the review. Nothing was written. Reload the reflex and inspect the new proposal before confirming.");
        }
    }
    let proposal = crate::proposal::current(props.routes.get(route_id.as_str()), r).cloned();
    if verb == "dismiss" {
        let p =
            proposal.with_context(|| format!("{route_id} has no proposed change to dismiss (see `adage review`)"))?;
        props.routes.remove(route_id.as_str());
        props.dismissed.insert(route_id.to_string(), review::basis(&p.route, 0, 0));
        proposals::save(&inst.dir, &props)?;
        out.push(format!("adage review: dismissed the proposed change to {} {route_id}", inst.reference));
        out.push(
            "  kept       the pinned route as it is; learning proposes again only if its version changes materially"
                .into(),
        );
        out.push(format!("  recorded   {}/{}", inst.reference, proposals::FILE));
        return Ok(out);
    }
    let (decision, bump) = match verb {
        "approve" if proposal.is_some() => {
            // Apply the proposed version; it stays pinned, so later drift is proposed again.
            let p = proposal.as_ref().unwrap();
            *r = proposed_route(inst, p);
            props.routes.remove(route_id.as_str());
            props.dismissed.remove(route_id.as_str());
            let bump = if review::shape(r) != review::shape(&before) { Bump::Major } else { Bump::Patch };
            (Decision::Pinned, bump)
        }
        "approve" if r.state == RouteState::Draft => {
            // An environment step waiting for a person (O11): approval puts it into service,
            // yellow at most; an import stays at probation.
            r.state = if inst.imported { RouteState::Probation } else { RouteState::Active };
            if r.aspect == Aspect::Green {
                r.aspect = Aspect::Yellow;
            }
            (Decision::Approved, Bump::Minor)
        }
        "approve" => {
            if !live(r.state) {
                // Approving a retired or demoted route puts it back into service at probation.
                r.state = RouteState::Probation;
                if r.aspect == Aspect::Green {
                    r.aspect = Aspect::Yellow;
                }
            }
            (Decision::Approved, Bump::Minor)
        }
        "reject" | "retire" => {
            r.state = RouteState::Retired;
            (Decision::Rejected, Bump::Major)
        }
        "demote" => {
            if r.aspect != Aspect::Green || !live(r.state) {
                bail!(
                    "{route_id} is {} ({}); only a green route in service can be demoted. To take it out of service: adage review reject {} {route_id}",
                    aspect_str(r.aspect),
                    lower(r.state),
                    inst.reference
                );
            }
            r.aspect = Aspect::Yellow;
            (Decision::Demoted, Bump::Major)
        }
        "promote" => {
            if r.aspect != Aspect::Yellow || !live(r.state) {
                bail!(
                    "{route_id} is {} ({}); only a yellow route in service can be promoted",
                    aspect_str(r.aspect),
                    lower(r.state)
                );
            }
            if ceiling(r.effect, by_edit(r)) != Some(Aspect::Green) {
                bail!(
                    "refusing: {route_id} is a {} step{}, whose ceiling is {}; no one can raise a route above its effect ceiling",
                    r.effect.as_str(),
                    if r.effect == adage_core::Effect::Regenerate { " not triggered by an edit" } else { "" },
                    ceiling_str(r)
                );
            }
            if inst.imported && first_hand(&all, &repo, route_id).is_none() {
                let bar = LearnConfig::default();
                bail!(
                    "refusing: {route_id} was imported, and an import cannot be green without local first-hand evidence (Design O2): \
                     a route learned here with at least {} sessions and a lower bound of at least {:.2}",
                    bar.min_support,
                    bar.green_lower_bound
                );
            }
            r.aspect = Aspect::Green;
            (Decision::Promoted, Bump::Minor)
        }
        "pin" => {
            r.authored = true;
            (Decision::Pinned, Bump::Patch)
        }
        _ => bail!("{USAGE}"),
    };
    let after = r.clone();
    // An import keeps its author's version: it names the release that was shared.
    if after != before && !inst.imported {
        track.manifest.track.version = bumped(&track.manifest.track.version, bump);
    }
    let (followed, derailed) = if track.manifest.track.version == inst.track.manifest.track.version {
        judgements(paths, &all).get(&(inst.reference.clone(), route_id.to_string())).copied().unwrap_or_default()
    } else {
        // Changing a learned reflex starts a new version with no outcomes yet.
        (0, 0)
    };
    let basis = review::basis(&after, followed, derailed);
    reviews.routes.insert(
        route_id.to_string(),
        Review {
            decision,
            on: review::today(),
            by,
            runtime_version: Some(track.manifest.track.version.clone()),
            basis,
        },
    );
    io::validate(&track).context("the reviewed track fails validation; nothing was written")?;
    io::write(&inst.dir, &track)?;
    review::save(&inst.dir, &reviews)?;
    if props != inst.proposals {
        proposals::save(&inst.dir, &props)?;
    }

    // Recompile the index exactly as `adage learn` and `adage add` do.
    let tracks = io::read_all(&paths.tracks())?;
    let index = Index::compile(&tracks);
    index.write_atomic(&paths.index())?;

    let change = |a: String, b: String| if a == b { a } else { format!("{a} → {b}") };
    if proposal.is_some() && verb == "approve" {
        out.push(format!(
            "adage review: applied the proposed change to {} {route_id}; it stays pinned",
            inst.reference
        ));
    } else {
        out.push(format!("adage review: {} {} {}", decision.as_str(), inst.reference, route_id));
    }
    if verb == "approve" && proposal.is_some() {
        out.push(format!("  step       {}", change(describe_step(&before.step), describe_step(&after.step))));
    }
    out.push(format!("  aspect     {}", change(aspect_str(before.aspect).into(), aspect_str(after.aspect).into())));
    out.push(format!("  state      {}", change(lower(before.state), lower(after.state))));
    if after.authored != before.authored {
        out.push("  authored   yes: `adage learn` will not overwrite it".into());
    }
    out.push(format!("  track      {} @ {}", track.manifest.track.name, track.manifest.track.version));
    out.push(format!("  recorded   {}/{}", inst.reference, review::FILE));
    out.push(format!("index generation {}, {} routes", index.generation, index.routes.len()));
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proposed_fixture() -> (tempfile::TempDir, Paths, Track, Proposals) {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path());
        let mut track = crate::ui::demo::track(0);
        track.routes[0].authored = true;
        let mut proposed = track.routes[0].clone();
        let Step::Run { command } = &mut proposed.step else { panic!("run fixture") };
        command.push_str(" --json");
        let proposals =
            proposals::propose(&Proposals::default(), &[(track.routes[0].clone(), Some(proposed))], "2026-10-05");
        let dest = paths.tracks().join(&track.manifest.track.name);
        io::write(&dest, &track).unwrap();
        proposals::save(&dest, &proposals).unwrap();
        Index::compile(&[track.clone()]).write_atomic(&paths.index()).unwrap();
        (dir, paths, track, proposals)
    }

    #[test]
    fn approval_preview_exposes_the_actual_proposed_directive_and_diff() {
        let (_dir, paths, track, _) = proposed_fixture();
        let s = standings(&paths).unwrap();
        let preview = &s[&(track.manifest.track.name.clone(), track.routes[0].id.clone())].preview;
        let proposed = preview.proposal.as_ref().expect("proposed change must be inspectable");
        assert!(!preview.current_directive.contains("--json"));
        assert!(proposed.directive.contains("--json"));
        assert!(proposed.directive.contains("incidents@1.0.0"), "preview uses the version that approval installs");
        assert!(proposed.diff.iter().any(|(field, before, after)| {
            *field == "step" && !before.contains("--json") && after.contains("--json")
        }));
    }

    #[test]
    fn unchanged_review_applies_exactly_the_previewed_proposal() {
        let (_dir, paths, track, _) = proposed_fixture();
        let name = &track.manifest.track.name;
        let id = &track.routes[0].id;
        let s = standings(&paths).unwrap();
        let previewed = &s[&(name.clone(), id.clone())].preview;
        let _lock = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        apply("approve", name, id, None, Some(&previewed.revision), &paths).unwrap();
        let installed = io::read(&paths.tracks().join(name)).unwrap();
        let actual = installed.routes.iter().find(|r| r.id == *id).unwrap();
        assert_eq!(preview(&installed, actual).0, previewed.proposal.as_ref().unwrap().directive);
        assert!(actual.authored, "approval preserves the edit lock");
        assert!(proposals::load(&paths.tracks().join(name)).unwrap().routes.is_empty());
        assert_eq!(review::load(&paths.tracks().join(name)).unwrap().routes[id].decision, Decision::Pinned);
    }

    #[test]
    fn stale_confirmation_never_applies_a_changed_or_disappeared_proposal() {
        for change in ["proposal", "removed", "current", "version", "decision"] {
            let (_dir, paths, mut track, mut props) = proposed_fixture();
            let name = track.manifest.track.name.clone();
            let id = track.routes[0].id.clone();
            let s = standings(&paths).unwrap();
            let revision = &s[&(name.clone(), id.clone())].preview.revision;
            let dest = paths.tracks().join(&name);
            match change {
                "proposal" => props.routes.get_mut(&id).unwrap().route.summary = "A newer proposal".into(),
                "removed" => props.routes.clear(),
                "current" => track.routes[0].summary = "A newer current reflex".into(),
                "version" => track.manifest.track.version = "0.4.0".into(),
                "decision" => {
                    let mut reviews = Reviews::default();
                    reviews.routes.insert(
                        id.clone(),
                        Review {
                            decision: Decision::Pinned,
                            on: "2026-10-05".into(),
                            by: Some("Another reviewer".into()),
                            runtime_version: None,
                            basis: review::basis(&track.routes[0], 0, 0),
                        },
                    );
                    review::save(&dest, &reviews).unwrap();
                }
                _ => unreachable!(),
            }
            io::write(&dest, &track).unwrap();
            proposals::save(&dest, &props).unwrap();
            let track_before = io::read(&dest).unwrap();
            let reviews_before = review::load(&dest).unwrap();
            let index_before = std::fs::read(paths.index()).unwrap();
            let _lock = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
            let error = apply("approve", &name, &id, None, Some(revision), &paths).unwrap_err();
            assert!(error.to_string().contains("changed after you opened the review"), "{change}: {error}");
            assert_eq!(io::read(&dest).unwrap(), track_before, "{change}: no track write");
            assert_eq!(proposals::load(&dest).unwrap(), props, "{change}: no proposal write");
            assert_eq!(review::load(&dest).unwrap(), reviews_before, "{change}: no review write");
            assert_eq!(std::fs::read(paths.index()).unwrap(), index_before, "{change}: no index write");
        }
    }

    #[test]
    fn confirmation_without_a_proposal_cannot_accept_a_newly_arrived_one() {
        let (_dir, paths, track, props) = proposed_fixture();
        let name = &track.manifest.track.name;
        let id = &track.routes[0].id;
        let dest = paths.tracks().join(name);
        proposals::save(&dest, &Proposals::default()).unwrap();
        let s = standings(&paths).unwrap();
        let preview = &s[&(name.clone(), id.clone())].preview;
        assert!(preview.proposal.is_none());
        proposals::save(&dest, &props).unwrap();
        let track_before = io::read(&dest).unwrap();
        let _lock = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        assert!(apply("approve", name, id, None, Some(&preview.revision), &paths).is_err());
        assert_eq!(io::read(&dest).unwrap(), track_before);
        assert_eq!(proposals::load(&dest).unwrap(), props);
    }

    #[test]
    fn malformed_proposal_refuses_an_approval_instead_of_hiding_it() {
        let (_dir, paths, track, _) = proposed_fixture();
        let name = &track.manifest.track.name;
        let id = &track.routes[0].id;
        let s = standings(&paths).unwrap();
        let revision = &s[&(name.clone(), id.clone())].preview.revision;
        let dest = paths.tracks().join(name);
        std::fs::write(dest.join(proposals::FILE), "invalid: [").unwrap();
        let track_before = io::read(&dest).unwrap();
        assert!(standings(&paths).is_err());
        let _lock = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        assert!(apply("approve", name, id, None, Some(revision), &paths).is_err());
        assert_eq!(io::read(&dest).unwrap(), track_before);
        assert!(review::load(&dest).unwrap().routes.is_empty());
    }

    #[test]
    fn interactive_review_does_not_race_or_block_behind_an_active_learner() {
        let (_dir, paths, track, _) = proposed_fixture();
        let name = &track.manifest.track.name;
        let id = &track.routes[0].id;
        let s = standings(&paths).unwrap();
        let revision = &s[&(name.clone(), id.clone())].preview.revision;
        let lock = adage_store::lock::exclusive(&paths.tracks_lock()).unwrap();
        let error = apply_reviewed("approve", name, id, revision, &paths).unwrap_err();
        assert!(error.to_string().contains("updating reflexes"));
        assert!(review::load(&paths.tracks().join(name)).unwrap().routes.is_empty());
        drop(lock);
        assert!(apply_reviewed("approve", name, id, revision, &paths).is_ok());
    }

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
}
