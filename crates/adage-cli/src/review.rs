//! `adage review` (Design J1, §10.4, §10.9; Architecture §6.3): the reflexes that wait for a
//! person, and the decisions a person makes about them.
//!
//! ```text
//! adage review [--json]                                     list the queue (agents may read it)
//! adage review approve    <track> <route> [--by <name>]     accept as is (restores an extinguished route at
//!                                                           forming; puts a nascent environment step into service)
//! adage review extinguish <track> <route> [--by <name>]     take it out of service, and keep it out
//! adage review weaken     <track> <route> [--by <name>]     fire → prime, held across conditioning
//! adage review strengthen <track> <route> [--by <name>]     prime → fire, within the effect ceiling only
//! adage review pin        <track> <route> [--by <name>]     edit-lock: mark `authored`
//! adage review dismiss    <track> <route>                   keep a pinned route; silence its proposed change
//! adage review delegate   <track> <route> --level e1|e2 …   let Adage run it (`crate::exec`)
//! adage review revoke     <track> <route>                   withdraw that grant
//! ```
//!
//! A pinned route whose learned version drifted materially waits with the reason "proposed" and a
//! diff (see [`crate::proposal`]); `approve` applies the proposed version, which stays pinned.
//!
//! Instinct routes (Adage's built-in defaults, bound from a repository's files; track
//! `<repo>/instincts`) are listed with their origin until a person decides. They stay prime at
//! most and can never be strengthened. Approving an environment-effect instinct (an install step,
//! nascent until then) approves every environment step of its pack for that repository, recorded
//! in the track's `review.yaml` (`packs`); extinguishing one keeps it out of later binds.
//!
//! `<track>` is a track name (a learned track wins over an import of the same name) or its
//! directory under the tracks directory, e.g. `@imported/acme/work-items`.
//!
//! Decisions change trust, so they refuse to run inside an agent session (review finding F11).
//! They are recorded in the track's `review.yaml` (see [`adage_tracks::review`]); every decision
//! re-validates the track and recompiles the index atomically, as `adage condition` and `adage add` do.

use crate::share::{describe_step, require_person};
use adage_learn::LearnConfig;
use adage_runtime::{render, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::proposals::{self, Proposal, Proposals};
use adage_tracks::review::{self, strength_str, Decision, Review, Reviews};
use adage_tracks::{
    ceiling, index, io, wilson_lower, Contract, Index, Route, RouteState, Step, Strength, Track, Trigger,
};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Runtime judgements needed before an ignore signal is considered.
const MIN_JUDGED: u32 = 5;
/// A route is flagged when the Wilson lower bound of its ignore share reaches this.
const IGNORE_BAR: f64 = 0.5;

const USAGE: &str = "usage: adage review [--json]\n       adage review <approve|extinguish|weaken|strengthen|pin|dismiss> <track> <route> [--by <name>]\n       adage review delegate <track> <route> --level e1|e2 [...]\n       adage review revoke <track> <route>";

pub(crate) struct Installed {
    pub(crate) dir: PathBuf,
    /// The track's directory relative to the tracks directory: unambiguous, and what the queue prints.
    pub(crate) reference: String,
    pub(crate) track: Track,
    pub(crate) imported: bool,
    pub(crate) reviews: Reviews,
    /// Proposed changes to pinned routes, from `adage condition` or a re-run `adage add`.
    pub(crate) proposals: Proposals,
}

pub fn review(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("list") | Some("--json") => list(paths, args.iter().any(|a| a == "--json")),
        Some(verb @ ("approve" | "extinguish" | "weaken" | "strengthen" | "pin" | "dismiss")) => {
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
            // Hidden directories hold no track in service (`<repo>/.shadow/`, Track G G3).
            if e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.') {
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

/// Followed / ignored judgements per (repository, route), from treated sessions' spools only:
/// in the control arm nothing was delivered, so nothing could be followed or ignored.
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
                    Some("followed" | "misfired") => e.0 += 1,
                    Some("ignored") => e.1 += 1,
                    _ => {}
                }
            }
        }
    }
    out
}

fn live(s: RouteState) -> bool {
    matches!(s, RouteState::Firm | RouteState::Forming | RouteState::Dormant)
}

fn by_edit(r: &Route) -> bool {
    matches!(r.trigger, Trigger::AfterEdit { .. })
}

/// A route's strength ceiling: its effect's, and prime at most for a procedure card (advice to
/// read, never a command to run).
fn route_ceiling(r: &Route) -> Option<Strength> {
    if matches!(r.step, Step::Card { .. }) {
        return Some(Strength::Prime);
    }
    ceiling(r.effect, by_edit(r))
}

pub(crate) fn ceiling_str(r: &Route) -> &'static str {
    route_ceiling(r).map(strength_str).unwrap_or("none")
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
    let strength = if origin.is_some() { adage_tracks::delivered_strength(r) } else { r.strength };
    let kind = if origin.is_some() { adage_tracks::shape(r) } else { "" };
    (render::wrap_origin(&m.name, &m.version, &r.id, kind, strength, &body, origin.as_ref()), bound)
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
    ignored: u32,
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
            let (followed, ignored) = match rv {
                Some(v) => (f0.saturating_sub(v.basis.followed), d0.saturating_sub(v.basis.ignored)),
                None => (f0, d0),
            };
            let mut reasons: Vec<(&'static str, String)> = Vec::new();
            let instinct = origin_label(r);
            match rv {
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
                let since = if rv.is_some() { " since the last decision" } else { "" };
                reasons.push((
                    "ignoring",
                    format!("agents ignored this reflex in {ignored} of {n} treated traces{since}; {advice}"),
                ));
            }
            if !reasons.is_empty() {
                items.push(Item { inst, route: r, reasons, followed, ignored, proposal });
            }
        }
    }
    items
}

fn list(paths: &Paths, as_json: bool) -> Result<()> {
    let all = discover(&paths.tracks())?;
    let judged = judgements(paths);
    let index = Index::compile_root(&paths.tracks(), &all.iter().map(|i| i.track.clone()).collect::<Vec<_>>()).0;
    let items = queue(&all, &judged, &index);
    let inert = crate::packs::inert_routes(paths);
    let attested = attestations(paths);
    let revoked = adage_exec::approvals::Store::load(&adage_exec::approvals::path(&paths.root))
        .map(|s| s.revoked)
        .unwrap_or_default();
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
                    "strength": strength_str(r.strength),
                    "ceiling": ceiling_str(r),
                    "effect": r.effect.as_str(),
                    "state": lower(r.state),
                    "authored": r.authored,
                    "decisions": verbs(r, it.proposal.is_some()),
                    "step": describe_step(&r.step),
                    "evidence": { "support": e.support, "opportunities": e.opportunities, "days": e.days, "rate": e.rate, "lower_bound": e.lower_bound },
                    "runtime": { "followed": it.followed, "ignored": it.ignored },
                    "why": adage_tracks::why::load(&it.inst.dir).remove(&r.id),
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
        let attestations: Vec<Value> = attested
            .iter()
            .map(|(pack, lines)| json!({ "pack": pack, "attestations": lines, "note": "self-reported by signer; informational only" }))
            .collect();
        let revoked: Vec<Value> = revoked
            .iter()
            .map(|r| json!({ "kind": "exec-revoked", "route": r.route, "repo": r.repo, "level": r.level.as_str(), "on": r.on, "cause": r.cause }))
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({ "count": rows.len(), "queue": rows, "inert": inert, "attestations": attestations, "exec_revoked": revoked,
                         "exec_last_withheld": adage_exec::last_withheld(&paths.root) })
            )?
        );
        return Ok(());
    }
    print_inert(&inert);
    print_revoked(&revoked);
    print_attested(&attested);
    // Why the last prefetch or action was not delivered (every withheld stage records a reason).
    if let Some(w) = adage_exec::last_withheld(&paths.root) {
        println!(
            "adage review: last reflex run by Adage {}
",
            w.line()
        );
    }
    if items.is_empty() {
        println!("adage review: nothing awaits a decision.");
        return Ok(());
    }
    println!("adage review: {} reflex(es) await a decision\n", items.len());
    for it in &items {
        let (t, r) = (&it.inst.track, it.route);
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
        let ceil =
            if r.strength == Strength::Inhibit { String::new() } else { format!(" (ceiling {})", ceiling_str(r)) };
        println!(
            "  strength   {}{ceil} · effect {} · state {}{}",
            strength_str(r.strength),
            r.effect.as_str(),
            lower(r.state),
            if r.authored { " · authored" } else { "" }
        );
        let whys = adage_tracks::why::load(&it.inst.dir);
        println!("  evidence   {}", crate::evidence::describe(r, whys.get(&r.id)));
        if let Some(o) = crate::evidence::obligation(r) {
            println!("  enforced   {o}");
        }
        println!("  runtime    followed {} · ignored {} (treated sessions)", it.followed, it.ignored);
        // The graph engine's reasons for a reflex (Track G §6.3): for people, never the agent.
        if let Some(w) = adage_tracks::why::load(&it.inst.dir).get(&r.id) {
            for (i, line) in w.lines().iter().enumerate() {
                println!("  {} {line}", if i == 0 { "reflex    " } else { "          " });
            }
        }
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
            println!("  proposed   `adage review approve` applies these changes (the reflex stays pinned):");
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

/// Each installed pack's attestations (Track C §1.8), one line each, marked self-reported by its
/// signer.
fn attestations(paths: &Paths) -> Vec<(String, Vec<String>)> {
    let mut packs: Vec<String> = crate::packs::bindings(paths).into_iter().map(|(_, b)| b.pack).collect();
    packs.sort();
    packs.dedup();
    packs
        .into_iter()
        .map(|p| {
            let lines = crate::packs::attestation_lines(paths, &p);
            (p, lines)
        })
        .filter(|(_, l)| !l.is_empty())
        .collect()
}

fn print_attested(attested: &[(String, Vec<String>)]) {
    for (pack, lines) in attested {
        println!("adage review: {pack} carries {} attestation(s), self-reported by its signer:\n", lines.len());
        for l in lines {
            println!("  attested   {l}");
        }
        println!("  note       attestations are informational only: they never set a reflex's strength or an execution level\n");
    }
}

/// Execution grants Adage withdrew by itself (Track C §3.6): losing authority is automatic,
/// regaining it never is.
fn print_revoked(revoked: &[adage_exec::approvals::Revocation]) {
    if revoked.is_empty() {
        return;
    }
    println!("adage review: {} execution grant(s) were revoked automatically:\n", revoked.len());
    for r in revoked {
        let (track, route) = r.route.rsplit_once('/').unwrap_or((&r.route, ""));
        println!("{}  {}   [{}]", r.route, r.level.as_str().to_uppercase(), r.repo);
        println!("  why        exec-revoked: {} (on {})", r.cause, r.on);
        println!(
            "  decide     adage review delegate {track} {route} --level {} (after fixing what it changes)\n",
            r.level.as_str()
        );
    }
}

/// Routes of installed packs that are inert here (Track C §2.3): never delivered, and no decision
/// changes that; restoring what they need (a script, a tool) and running `adage add` again does.
fn print_inert(inert: &[crate::packs::InertRoute]) {
    if inert.is_empty() {
        return;
    }
    println!("adage review: {} pack route(s) are inert here (not delivered):\n", inert.len());
    for i in inert {
        println!(
            "{} @ {}  {}   [{}]  (pack, sha256 {})",
            i.pack,
            i.version,
            i.route,
            i.repo,
            &i.digest[..i.digest.len().min(16)]
        );
        println!("  why        inert: {}\n", i.reason);
    }
}

/// The decisions that can apply to a route as it stands (strengthening of an import may still be
/// refused for want of first-hand evidence).
fn verbs(r: &Route, proposed: bool) -> Vec<&'static str> {
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
pub(crate) struct Standing {
    pub(crate) verbs: Vec<&'static str>,
    pub(crate) reasons: Vec<(&'static str, String)>,
}

pub(crate) fn standings(paths: &Paths) -> Result<BTreeMap<(String, String), Standing>> {
    let all = discover(&paths.tracks())?;
    let judged = judgements(paths);
    let index = Index::compile_root(&paths.tracks(), &all.iter().map(|i| i.track.clone()).collect::<Vec<_>>()).0;
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
/// fire bar: the local first-hand evidence Design O2 requires before an import may be fire.
pub(crate) fn first_hand<'a>(all: &'a [Installed], repo: &str, id: &str) -> Option<&'a Route> {
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
    if !matches!(verb, "approve" | "extinguish" | "weaken" | "strengthen" | "pin" | "dismiss") {
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
            if inst.imported && r.strength == Strength::Fire {
                r.strength = Strength::Prime;
            }
            props.routes.remove(route_id.as_str());
            props.dismissed.remove(route_id.as_str());
            let bump = if review::shape(r) != review::shape(&before) { Bump::Major } else { Bump::Patch };
            (Decision::Pinned, bump)
        }
        "approve" if r.state == RouteState::Nascent => {
            // An environment step waiting for a person (O11): approval puts it into service,
            // prime at most; an import or an instinct stays at forming.
            r.state = if inst.imported || r.is_instinct() { RouteState::Forming } else { RouteState::Firm };
            if r.strength == Strength::Fire {
                r.strength = Strength::Prime;
            }
            (Decision::Approved, Bump::Minor)
        }
        "approve" => {
            if !live(r.state) {
                // Approving an extinguished or weakened route puts it back into service at forming.
                r.state = RouteState::Forming;
                if r.strength == Strength::Fire {
                    r.strength = Strength::Prime;
                }
            }
            (Decision::Approved, Bump::Minor)
        }
        "extinguish" => {
            r.state = RouteState::Extinguished;
            (Decision::Extinguished, Bump::Major)
        }
        "weaken" => {
            if r.strength != Strength::Fire || !live(r.state) {
                bail!(
                    "{route_id} is {} ({}); only a fire reflex in service can be weakened. To take it out of service: adage review extinguish {} {route_id}",
                    strength_str(r.strength),
                    lower(r.state),
                    inst.reference
                );
            }
            r.strength = Strength::Prime;
            (Decision::Weakened, Bump::Major)
        }
        "strengthen" => {
            if r.strength != Strength::Prime || !live(r.state) {
                bail!(
                    "{route_id} is {} ({}); only a prime reflex in service can be strengthened",
                    strength_str(r.strength),
                    lower(r.state)
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
        "pin" => {
            r.authored = true;
            (Decision::Pinned, Bump::Patch)
        }
        _ => bail!("{USAGE}"),
    };
    let (followed, ignored) = judgements(paths).get(&(repo.clone(), route_id.to_string())).copied().unwrap_or_default();
    let basis = review::basis(r, followed, ignored);
    let after = r.clone();
    // Pack-level approval: approving one environment-effect instinct approves the pack's
    // environment steps for this repository, now and when learning binds them again.
    let mut pack_approved: Vec<String> = Vec::new();
    if verb == "approve" && before.state == RouteState::Nascent && after.is_instinct() {
        if let adage_tracks::Origin::Instinct { pack, .. } = after.origin() {
            if !pack.is_empty() {
                reviews.packs.insert(pack.clone(), review::PackApproval { on: review::today(), by: by.clone() });
                for other in track.routes.iter_mut().filter(|x| x.state == RouteState::Nascent && x.is_instinct()) {
                    other.state = RouteState::Forming;
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

    // Recompile the index exactly as `adage condition` and `adage add` do.
    let tracks = io::read_all(&paths.tracks())?;
    let index = Index::compile_root(&paths.tracks(), &tracks).0;
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
    out.push(format!(
        "  strength   {}",
        change(strength_str(before.strength).into(), strength_str(after.strength).into())
    ));
    out.push(format!("  state      {}", change(lower(before.state), lower(after.state))));
    if after.authored != before.authored {
        out.push("  authored   yes: `adage condition` will not overwrite it".into());
    }
    if let Some((pack, others)) = pack_approved.split_first() {
        let also =
            if others.is_empty() { String::new() } else { format!("; also in service now: {}", others.join(", ")) };
        out.push(format!("  approved   {pack}'s environment steps for {repo}{also}"));
    }
    out.push(format!("  track      {} @ {}", track.manifest.track.name, track.manifest.track.version));
    out.push(format!("  recorded   {}/{}", inst.reference, review::FILE));
    out.push(format!("index generation {}, {} reflexes", index.generation, index.routes.len()));
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
