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
//! Instinct routes (Adage's built-in defaults, bound from a repository's files; track
//! `<repo>/instincts`) are listed with their origin until a person decides. They stay yellow at
//! most and can never be promoted. Approving an environment-effect instinct (an install step,
//! a draft until then) approves every environment step of its pack for that repository, recorded
//! in the track's `review.yaml` (`packs`); rejecting one keeps it out of later binds.
//!
//! `<track>` is a track name (a learned track wins over an import of the same name) or its
//! directory under the tracks directory, e.g. `@imported/acme/work-items`.
//!
//! Decisions change trust, so they refuse to run inside an agent session (review finding F11).
//! They are recorded in the track's `review.yaml` (see [`adage_tracks::review`]); every decision
//! re-validates the track and recompiles the index atomically, as `adage learn` and `adage add` do.

use crate::share::{describe_step, require_person};
use adage_learn::LearnConfig;
use adage_runtime::{render, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::proposals::{self, Proposal, Proposals};
use adage_tracks::review::{self, aspect_str, Decision, Review, Reviews};
use adage_tracks::{
    ceiling, index, io, wilson_lower, Aspect, Contract, Index, Route, RouteState, Step, Track, Trigger,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
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

/// Followed / derailed judgements per (repository, route), from treated sessions' spools only:
/// in the holdout nothing was delivered, so nothing could be followed or left.
fn judgements(paths: &Paths) -> BTreeMap<(String, String), (u32, u32)> {
    let mut out: BTreeMap<(String, String), (u32, u32)> = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(paths.sessions()) else { return out };
    let mut spools: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "spool")).collect();
    spools.sort();
    for p in spools {
        let st: SessionState = state::read(&p.with_extension("state")).unwrap_or_default();
        let Some(repo) = st.repo else { continue };
        let recs: Vec<Value> = spool::read_all(&p).unwrap_or_default();
        for r in recs.iter().filter(|r| r["arm"] == "treated") {
            for j in r["decision"]["judgements"].as_array().into_iter().flatten() {
                let Some(route) = j["route"].as_str() else { continue };
                let e = out.entry((repo.clone(), route.to_string())).or_default();
                match j["kind"].as_str() {
                    // Followed, whether or not the command then failed.
                    Some("followed" | "followed_failed") => e.0 += 1,
                    Some("derailed") => e.1 += 1,
                    _ => {}
                }
            }
        }
    }
    out
}

fn live(s: RouteState) -> bool {
    matches!(s, RouteState::Active | RouteState::Probation | RouteState::Stale)
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
    let origin = r.is_instinct().then(|| r.origin());
    let aspect = if origin.is_some() { adage_tracks::delivered_aspect(r) } else { r.aspect };
    let kind = if origin.is_some() { adage_tracks::route_kind(r) } else { "" };
    (render::wrap_origin(&m.name, &m.version, &r.id, kind, aspect, &body, origin.as_ref()), bound)
}

/// Where an instinct route came from, for people: `instinct U1 (from Makefile)`.
pub(crate) fn origin_label(r: &Route) -> Option<String> {
    match r.origin() {
        adage_tracks::Origin::Instinct { instinct, declared, .. } if r.is_instinct() => {
            let id = if instinct.is_empty() { String::new() } else { format!(" {instinct}") };
            let from = declared.map(|d| format!(" (from {d})")).unwrap_or_default();
            Some(format!("instinct{id}{from}"))
        }
        _ => None,
    }
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
            let (f0, d0) = judged.get(&(m.scope.repo.clone(), r.id.clone())).copied().unwrap_or_default();
            // Only what happened since the last decision counts toward a new signal.
            let (followed, derailed) = match rv {
                Some(v) => (f0.saturating_sub(v.basis.followed), d0.saturating_sub(v.basis.derailed)),
                None => (f0, d0),
            };
            let mut reasons: Vec<(&'static str, String)> = Vec::new();
            let instinct = origin_label(r);
            match rv {
                None if r.state == RouteState::Draft && !r.authored && instinct.is_some() => reasons.push((
                    "approval",
                    "an environment step Adage would suggest by default: delivered only after a person approves it; approving it approves this pack's environment steps for the repository".into(),
                )),
                None if r.state == RouteState::Draft && !r.authored => reasons.push((
                    "approval",
                    "an environment step: it is delivered only after a person approves it (`approve`)".into(),
                )),
                None if instinct.is_some() && live(r.state) => reasons.push((
                    "instinct",
                    format!(
                        "{}: Adage's built-in default, bound from this repository's files with no session history; yellow at most, and any learned route overrides it",
                        instinct.clone().unwrap_or_default()
                    ),
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
                let advice = if r.aspect == Aspect::Green { "consider demoting it" } else { "consider retiring it" };
                let since = if rv.is_some() { " since the last decision" } else { "" };
                reasons.push((
                    "derailing",
                    format!("agents left this route in {derailed} of {n} treated sessions{since}; {advice}"),
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
    let judged = judgements(paths);
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
                    "origin": r.evidence.origin.clone().map(|o| serde_json::to_value(o).unwrap_or_default()).unwrap_or_else(|| json!({ "kind": if it.inst.imported { "imported" } else { "learned" } })),
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
        let origin = match origin_label(r) {
            Some(l) => format!("  ({l})"),
            None if it.inst.imported => "  (imported)".into(),
            None => String::new(),
        };
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
        println!("  runtime    followed {} · derailed {} (treated sessions)", it.followed, it.derailed);
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
    // An instinct is yellow at most, whatever anyone decides.
    if live(r.state)
        && r.aspect == Aspect::Yellow
        && ceiling(r.effect, by_edit(r)) == Some(Aspect::Green)
        && !r.is_instinct()
    {
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
}

pub(crate) fn standings(paths: &Paths) -> Result<BTreeMap<(String, String), Standing>> {
    let all = discover(&paths.tracks())?;
    let judged = judgements(paths);
    let index = Index::compile(&all.iter().map(|i| i.track.clone()).collect::<Vec<_>>());
    let mut out: BTreeMap<(String, String), Standing> = BTreeMap::new();
    for inst in &all {
        for r in &inst.track.routes {
            let proposed = crate::proposal::current(inst.proposals.routes.get(&r.id), r).is_some();
            out.insert((inst.reference.clone(), r.id.clone()), Standing { verbs: verbs(r, proposed), reasons: vec![] });
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
    for line in apply(verb, spec, route_id, by, paths)? {
        println!("{line}");
    }
    Ok(())
}

/// A decision from `adage ui`: the same guard and the same code path as `adage review <verb>`.
pub(crate) fn decide_quietly(verb: &str, spec: &str, route_id: &str, paths: &Paths) -> Result<Vec<String>> {
    require_person(&format!("review {verb}"))?;
    apply(verb, spec, route_id, None, paths)
}

/// Record one decision and recompile the index; returns the report `adage review` prints. The
/// caller has checked for a person at the terminal.
fn apply(verb: &str, spec: &str, route_id: &str, by: Option<String>, paths: &Paths) -> Result<Vec<String>> {
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
            *r = crate::proposal::applied(p);
            if inst.imported && r.aspect == Aspect::Green {
                r.aspect = Aspect::Yellow;
            }
            props.routes.remove(route_id.as_str());
            props.dismissed.remove(route_id.as_str());
            let bump = if review::shape(r) != review::shape(&before) { Bump::Major } else { Bump::Patch };
            (Decision::Pinned, bump)
        }
        "approve" if r.state == RouteState::Draft => {
            // An environment step waiting for a person (O11): approval puts it into service,
            // yellow at most; an import or an instinct stays at probation.
            r.state = if inst.imported || r.is_instinct() { RouteState::Probation } else { RouteState::Active };
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
            if r.is_instinct() {
                bail!("refusing: {route_id} is an instinct (Adage's built-in default), which is yellow at most; a route learned from this repository's sessions can be green");
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
    let (followed, derailed) =
        judgements(paths).get(&(repo.clone(), route_id.to_string())).copied().unwrap_or_default();
    let basis = review::basis(r, followed, derailed);
    let after = r.clone();
    // Pack-level approval: approving one environment-effect instinct approves the pack's
    // environment steps for this repository, now and when learning binds them again.
    let mut pack_approved: Vec<String> = Vec::new();
    if verb == "approve" && before.state == RouteState::Draft && after.is_instinct() {
        if let adage_tracks::Origin::Instinct { pack, .. } = after.origin() {
            if !pack.is_empty() {
                reviews.packs.insert(pack.clone(), review::PackApproval { on: review::today(), by: by.clone() });
                for other in track.routes.iter_mut().filter(|x| x.state == RouteState::Draft && x.is_instinct()) {
                    other.state = RouteState::Probation;
                    pack_approved.push(other.id.clone());
                }
                pack_approved.insert(0, format!("pack {pack}"));
            }
        }
    }
    reviews.routes.insert(route_id.to_string(), Review { decision, on: review::today(), by, basis });
    // An import keeps its author's version: it names the release that was shared.
    if after != before && !inst.imported {
        track.manifest.track.version = bumped(&track.manifest.track.version, bump);
    }
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
    if let Some((pack, others)) = pack_approved.split_first() {
        let also =
            if others.is_empty() { String::new() } else { format!("; also in service now: {}", others.join(", ")) };
        out.push(format!("  approved   {pack}'s environment steps for {repo}{also}"));
    }
    out.push(format!("  track      {} @ {}", track.manifest.track.name, track.manifest.track.version));
    out.push(format!("  recorded   {}/{}", inst.reference, review::FILE));
    out.push(format!("index generation {}, {} routes", index.generation, index.routes.len()));
    Ok(out)
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
}
