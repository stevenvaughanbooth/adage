//! Instincts in learning (docs/instincts.md): after the learners and review reconciliation, bind
//! the repository's instincts from its declared files and write them as one track,
//! `<repo>/instincts`, provenance `instinct:<pack>@<version>`.
//!
//! - **Learned wins.** The binder sees every other route of the repository (learned, imported,
//!   authored) and skips an instinct any of them covers; the index prefers any other source by
//!   provenance ([`adage_tracks::index`]). When the learner learns a route with an instinct's id,
//!   the instinct route leaves the track (reported as confirmed by sessions).
//! - **Review.** Ids a person rejected in any of the repository's `review.yaml` files are never
//!   bound again; a rejected instinct route stays in the track, extinguished, so the decision shows.
//!   Environment-effect instincts (`pnpm install`) are nascent until a person approves one: that
//!   approves the pack's environment steps for the repository (`Reviews::packs`).
//! - **Auto-extinguish.** The runtime judges every instinct directive ([`adage_runtime::Judgement`]).
//!   From the spools, an instinct route is extinguished ([`should_extinguish`]) when it
//!   misfired at least [`EXTINGUISH_MISFIRED`] times (the agent ran the directed command and it
//!   failed for want of its environment or target), no fewer than its working follows; or when,
//!   over at least [`EXTINGUISH_MIN_JUDGEMENTS`] judgements, a control-arm comparison shows
//!   delivering it changes nothing (its ignore rate's Wilson lower bound reaches
//!   [`EXTINGUISH_IGNORE_LOWER_BOUND`] and it is followed no more often than when withheld); or
//!   when it misfired and its value ([`crate::value::judged`]) is positive with probability under
//!   [`EXTINGUISH_P_POSITIVE`]. Never on ignores alone: an ignore may be a follow the matcher
//!   missed. It is recorded as a person's `extinguish` is (so a re-bind does not bring it back),
//!   and the change appears in `learn.log`. Losing authority is always automatic.
//! - **Capped.** Prime, forming at most (never firm), never an obligation or a Stop request
//!   (the runtime's part).

use crate::{bump, slug, Planned, TrackReport};
use adage_tracks::review::{self, Decision, Review};
use adage_tracks::{io, Gauge, Manifest, Route, RouteState, Scope, Strength, Track, TrackInfo};
use anyhow::Result;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The track instincts are written to, under the repository's directory.
pub const TRACK: &str = "instincts";
/// misfired judgements that extinguish an instinct route.
pub const EXTINGUISH_MISFIRED: u32 = 3;
/// With a control-arm comparison, an ignore rate whose Wilson lower bound reaches this
/// extinguishes an instinct route that delivering changes nothing for.
pub const EXTINGUISH_IGNORE_LOWER_BOUND: f64 = 0.5;
/// A route that misfired and whose value is positive with less than this probability is
/// extinguished...
pub const EXTINGUISH_P_POSITIVE: f64 = 0.1;
/// ...over at least this many judgements.
pub const EXTINGUISH_MIN_JUDGEMENTS: u32 = 5;

/// How learning binds instincts (`instincts.*` in `config.toml`).
#[derive(Debug, Clone)]
pub struct Config {
    /// `instincts.enabled`.
    pub enabled: bool,
    /// `instincts.packs`: packs to bind (only `core` exists).
    pub packs: Vec<String>,
    /// `instincts.disabled`: catalogue ids never to bind.
    pub disabled: BTreeSet<String>,
    /// The spools directory (`$ADAGE_HOME/sessions`), whose judgements extinguish instinct routes.
    pub spools: Option<PathBuf>,
    /// Repository key → its root on disk, overriding what the sessions' working directories say.
    pub roots: BTreeMap<String, PathBuf>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            enabled: true,
            packs: vec!["core".into()],
            disabled: BTreeSet::new(),
            spools: None,
            roots: BTreeMap::new(),
        }
    }
}

impl Config {
    /// Whether the catalogue's pack is enabled.
    pub fn binds(&self) -> bool {
        self.enabled && self.packs.iter().any(|p| p == adage_instincts::pack().0)
    }
}

/// What binding did for one repository (no command, path or prompt: ids, counts and reasons that
/// name only instinct ids and files the repository declares).
#[derive(Debug, Clone, Default, Serialize)]
pub struct InstinctReport {
    /// Run routes in service (after a failure or an edit).
    pub bound: usize,
    /// Advisory routes in service.
    pub advice: usize,
    /// Environment steps waiting for approval in `adage review`.
    pub nascent: usize,
    /// Candidates not bound, and why (`U3: 2 test entry points`, `declared: Makefile: …`).
    pub skipped: Vec<String>,
    /// Routes extinguished this run because agents followed them and they failed.
    pub extinguished: Vec<String>,
    /// Instinct routes a learned route with the same id took over this run.
    pub confirmed: Vec<String>,
    /// Why nothing was bound, if nothing was.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_bound: Option<String>,
    /// The checkout the instincts were bound against (recorded with [`record_binding`] once the
    /// track is written).
    #[serde(skip)]
    pub root: Option<PathBuf>,
}

/// Where the checkout a repository's instincts were bound against is recorded, beside its tracks
/// (a hidden file: never a track).
pub fn binding_path(tracks_root: &Path, repo: &str) -> PathBuf {
    tracks_root.join(slug(repo)).join(".instincts-binding.json")
}

/// What a binding was made from: the checkout and its declared files' fingerprint
/// (`adage_instincts::fingerprint`).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, serde::Deserialize)]
pub struct Binding {
    pub root: String,
    pub declared: String,
    /// The checkout's project environment, verified from its files at bind time
    /// (`adage_instincts::verify_env`): what a `PreToolUse` interpreter rewrite may use.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<adage_instincts::VerifiedEnv>,
}

/// Record that the repository's instincts are bound against `root` as its files are now.
pub fn record_binding(tracks_root: &Path, repo: &str, root: &Path) {
    let b = Binding {
        root: root.to_string_lossy().into(),
        declared: adage_instincts::fingerprint(root),
        env: adage_instincts::verify_env(root),
    };
    let p = binding_path(tracks_root, repo);
    if let (Some(dir), Ok(body)) = (p.parent(), serde_json::to_vec(&b)) {
        let _ = std::fs::create_dir_all(dir);
        let _ = adage_store::config::write_atomic(&p, &body);
    }
}

/// The recorded binding of a repository's instincts, if any.
pub fn binding(tracks_root: &Path, repo: &str) -> Option<Binding> {
    std::fs::read(binding_path(tracks_root, repo)).ok().and_then(|b| serde_json::from_slice(&b).ok())
}

/// Whether the repository's instincts should be bound again against the live checkout `root` (the
/// session's): never bound, bound against another checkout (paths named in old traces, another
/// worktree), or its declared files changed since (a `.venv` made, a Makefile edited).
pub fn stale_binding(tracks_root: &Path, repo: &str, root: &Path) -> bool {
    binding(tracks_root, repo)
        .is_none_or(|b| b.root != root.to_string_lossy() || b.declared != adage_instincts::fingerprint(root))
}

/// Runtime judgements of one route: the treated arm's (the directive was delivered), and the
/// control arm's (withheld: a "followed" there is the agent taking the step unaided).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Judged {
    pub followed: u32,
    pub misfired: u32,
    pub ignored: u32,
    /// Control sessions: the agent took the step without being told.
    pub control_followed: u32,
    /// Control sessions: judged in all (followed, misfired or ignored).
    pub control_total: u32,
}

impl Judged {
    /// Treated judgements.
    pub fn total(&self) -> u32 {
        self.followed + self.misfired + self.ignored
    }
}

/// Judgements per (repository, route) from the spools in `dir`, by arm, as `adage review` and
/// `adage graph` count them.
pub fn judgements(dir: &Path) -> BTreeMap<(String, String), Judged> {
    let mut out: BTreeMap<(String, String), Judged> = BTreeMap::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    let mut spools: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "spool")).collect();
    spools.sort();
    for p in spools {
        let recs: Vec<Value> = adage_store::spool::read_all(&p).unwrap_or_default();
        for r in recs.iter() {
            let control = match r["arm"].as_str() {
                Some("treated") => false,
                Some("control") => true,
                _ => continue,
            };
            let Some(repo) = r["repo"].as_str() else { continue };
            for j in r["decision"]["judgements"].as_array().into_iter().flatten() {
                let Some(route) = j["route"].as_str() else { continue };
                let e = out.entry((repo.to_string(), route.to_string())).or_default();
                match (control, j["kind"].as_str()) {
                    (false, Some("followed")) => e.followed += 1,
                    (false, Some("misfired")) => e.misfired += 1,
                    (false, Some("ignored")) => e.ignored += 1,
                    (true, Some("followed")) => {
                        e.control_followed += 1;
                        e.control_total += 1;
                    }
                    (true, Some("misfired" | "ignored")) => e.control_total += 1,
                    _ => {}
                }
            }
        }
    }
    out
}

/// Why an instinct route with these judgements is extinguished, if it is. Conservative: losing a
/// route that works costs more than keeping one that does not.
///
/// - **Misfires**: at least [`EXTINGUISH_MISFIRED`] (agents followed it and the directed command
///   failed without running), and at least as many as the follows that worked.
/// - **Never on ignores alone**: an "ignored" may be a follow the matcher did not recognise
///   (RepoBench v2 judged 41 of 41 real follows "ignored" that way). An ignore rate whose Wilson
///   lower bound reaches [`EXTINGUISH_IGNORE_LOWER_BOUND`] over at least
///   [`EXTINGUISH_MIN_JUDGEMENTS`] extinguishes only with a control-arm comparison (at least as
///   many control judgements) that shows the matcher sees follows of this route at all (a follow
///   in either arm) and that delivering it made agents take the step no more often than
///   withholding it did.
pub fn should_extinguish(j: &Judged) -> Option<String> {
    if j.misfired >= EXTINGUISH_MISFIRED && j.misfired >= j.followed {
        return Some(format!("misfired {} times", j.misfired));
    }
    let n = j.total();
    if n < EXTINGUISH_MIN_JUDGEMENTS {
        return None;
    }
    let ignored = adage_tracks::wilson_lower(j.ignored, n) >= EXTINGUISH_IGNORE_LOWER_BOUND;
    let compared = j.control_total >= EXTINGUISH_MIN_JUDGEMENTS;
    let seen = j.followed + j.misfired + j.control_followed > 0;
    // The treated follow rate is at most the control's: f_t / n_t <= f_c / n_c.
    let no_better =
        u64::from(j.followed + j.misfired) * u64::from(j.control_total) <= u64::from(j.control_followed) * u64::from(n);
    if ignored && compared && seen && no_better {
        return Some(format!(
            "ignored {} of {n} times; unaided {} of {}",
            j.ignored, j.control_followed, j.control_total
        ));
    }
    // The value model reads ignores as lowering p_follow: with no misfire, many ignores may be a
    // matching gap, not a worthless route, so it decides only when the route has misfired.
    if j.misfired == 0 {
        return None;
    }
    let a = crate::value::judged(j.followed, j.ignored, j.misfired);
    (a.p_positive < EXTINGUISH_P_POSITIVE).then(|| {
        format!(
            "worth {:.1} calls, with probability {:.2} of helping ({} followed, {} ignored, {} misfired)",
            a.mean, a.p_positive, j.followed, j.ignored, j.misfired
        )
    })
}

/// The repository's root on disk: the working directory most of its sessions recorded that still
/// exists (ties: the lexically first).
pub fn root_of<'a>(roots: impl IntoIterator<Item = &'a Path>) -> Option<PathBuf> {
    let mut count: BTreeMap<&Path, usize> = BTreeMap::new();
    for r in roots {
        *count.entry(r).or_default() += 1;
    }
    let mut v: Vec<(&Path, usize)> = count.into_iter().filter(|(p, _)| p.is_dir()).collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));
    v.first().map(|(p, _)| p.to_path_buf())
}

/// Ids a person rejected in any of the repository's tracks.
fn rejected_ids(repo_dir: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Ok(rd) = std::fs::read_dir(repo_dir) else { return out };
    for e in rd.flatten() {
        if let Ok(r) = review::load(&e.path()) {
            out.extend(r.routes.into_iter().filter(|(_, v)| v.decision == Decision::Extinguished).map(|(id, _)| id));
        }
    }
    out
}

fn record(r: &mut Route, j: Option<&Judged>) {
    if let Some(j) = j {
        for (k, v) in [("followed", j.followed), ("misfired", j.misfired), ("ignored", j.ignored)] {
            if v > 0 {
                r.evidence.by_origin.insert(k.to_string(), v);
            }
        }
    }
}

/// Plan the instinct track of one repository: bind, apply review decisions and auto-extinction,
/// and version it like any learned track. `root` is the repository on disk; `learned` its routes
/// from every other source. Nothing is written; the returned [`Planned`] carries the track and,
/// when a route was extinguished, the `review.yaml` to save beside it.
pub fn plan_repo(
    repo: &str,
    root: Option<&Path>,
    tracks_root: &Path,
    learned: &[Route],
    cfg: &Config,
    judged: &BTreeMap<(String, String), Judged>,
    today: &str,
) -> Result<(InstinctReport, Option<(TrackReport, Planned)>)> {
    let mut rep = InstinctReport::default();
    let repo_dir = tracks_root.join(slug(repo));
    let dir = repo_dir.join(TRACK);
    let name = format!("{}/{TRACK}", slug(repo));
    let prev = io::read(&dir).ok();
    let mut reviews = match review::load(&dir) {
        Ok(r) => r,
        Err(e) => {
            rep.not_bound = Some(format!("{name}/review.yaml cannot be read ({e:#})"));
            return Ok((rep, None));
        }
    };
    let cat = adage_instincts::catalogue();
    let mut routes: Vec<Route> = Vec::new();
    let mut extinguished_now: BTreeMap<String, String> = BTreeMap::new();
    if !cfg.binds() {
        rep.not_bound = Some("instincts are turned off (instincts.enabled, instincts.packs)".into());
        if prev.as_ref().is_none_or(|p| p.routes.is_empty()) {
            return Ok((rep, None));
        }
    } else {
        let Some(root) = root.filter(|r| r.is_dir()) else {
            rep.not_bound = Some("the repository's directory is not known here, so its files cannot be read".into());
            return Ok((rep, None));
        };
        rep.root = Some(root.to_path_buf());
        // Auto-extinguish what agents followed and saw fail.
        for p in prev.iter().flat_map(|t| &t.routes).filter(|r| r.state.is_live()) {
            let j = judged.get(&(repo.to_string(), p.id.clone()));
            let Some(why) = j.and_then(should_extinguish) else { continue };
            let j = j.copied().unwrap_or_default();
            let by: String = format!("adage: {why}").chars().take(64).collect();
            reviews.routes.insert(
                p.id.clone(),
                Review {
                    decision: Decision::Extinguished,
                    on: today.to_string(),
                    by: Some(by),
                    basis: review::basis(p, j.followed, j.ignored),
                },
            );
            extinguished_now.insert(p.id.clone(), why);
        }
        let mut rejections = rejected_ids(&repo_dir);
        rejections.extend(
            reviews.routes.iter().filter(|(_, v)| v.decision == Decision::Extinguished).map(|(id, _)| id.clone()),
        );
        let opts = adage_instincts::BindOptions { disabled: cfg.disabled.clone() };
        let others: Vec<Route> = learned.iter().filter(|r| !r.is_instinct()).cloned().collect();
        let result = adage_instincts::bind_with(root, &others, &rejections, &opts);
        rep.skipped = result.skipped.iter().map(|(i, why)| format!("{i}: {why}")).take(64).collect();
        for p in prev.iter().flat_map(|t| &t.routes).filter(|r| r.state.is_live()) {
            let taken = result
                .skipped
                .iter()
                .any(|(_, why)| why.starts_with(&format!("{} (", p.id)) && why.contains("(learned wins)"));
            if taken {
                rep.confirmed.push(p.id.clone());
            }
        }
        routes = result.routes;
        for r in routes.iter_mut() {
            record(r, judged.get(&(repo.to_string(), r.id.clone())));
        }
        // A rejected (or auto-extinguished) route stays in the track, extinguished, so the decision shows.
        for (id, rv) in &reviews.routes {
            if rv.decision != Decision::Extinguished || routes.iter().any(|r| &r.id == id) {
                continue;
            }
            if let Some(p) = prev.as_ref().and_then(|t| t.routes.iter().find(|r| &r.id == id)) {
                let mut r = p.clone();
                r.state = RouteState::Extinguished;
                record(&mut r, judged.get(&(repo.to_string(), id.clone())));
                routes.push(r);
            }
        }
        routes = review::reconcile(prev.as_ref(), &reviews, routes);
        let pack_approved = reviews.packs.contains_key(&cat.pack.name);
        for r in routes.iter_mut() {
            // Capped whatever a decision says: prime, forming at most.
            r.strength = Strength::Prime;
            match r.state {
                RouteState::Firm => r.state = RouteState::Forming,
                RouteState::Nascent if pack_approved => r.state = RouteState::Forming,
                _ => {}
            }
        }
    }
    routes.sort_by(|a, b| a.id.cmp(&b.id));
    routes.retain(|r| {
        let probe = track(&name, repo, "0.0.0", &cat.provenance(), vec![r.clone()]);
        match io::validate(&probe) {
            Ok(()) => true,
            Err(e) => {
                eprintln!("adage: not binding an instinct route: {e:#}");
                false
            }
        }
    });
    for r in routes.iter().filter(|r| r.state.is_live()) {
        if matches!(r.step, adage_tracks::Step::Advise { .. }) {
            rep.advice += 1;
        } else {
            rep.bound += 1;
        }
    }
    rep.nascent = routes.iter().filter(|r| r.state == RouteState::Nascent).count();
    rep.extinguished = extinguished_now.keys().cloned().collect();
    let (version, change) = bump(prev.as_ref(), &routes);
    let t = track(&name, repo, &version, &cat.provenance(), routes);
    io::validate(&t)?;
    let mut lines: Vec<String> = t
        .routes
        .iter()
        .map(|r| {
            let instinct = match r.origin() {
                adage_tracks::Origin::Instinct { instinct, .. } if !instinct.is_empty() => format!(" {instinct}"),
                _ => String::new(),
            };
            format!("{} [{:?}, instinct{instinct}, {:?}]", r.id, r.strength, r.state).to_lowercase()
        })
        .collect();
    lines.extend(extinguished_now.iter().map(|(id, why)| format!("{id}: extinguished automatically ({why})")));
    let tr = TrackReport { name, version, change, routes: lines };
    let planned = Planned {
        dir,
        track: t,
        proposals: Default::default(),
        reviews: (!extinguished_now.is_empty()).then_some(reviews),
        why: None,
        replaces: Vec::new(),
    };
    Ok((rep, Some((tr, planned))))
}

fn track(name: &str, repo: &str, version: &str, provenance: &str, routes: Vec<Route>) -> Track {
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: name.to_string(),
                version: version.to_string(),
                description: "Adage's built-in defaults, bound from this repository's declared files (instincts)"
                    .into(),
                provenance: provenance.to_string(),
            },
            scope: Scope { repo: repo.to_string() },
            gauge: Gauge::default(),
        },
        routes,
    }
}

/// Every route of `repo` installed under `tracks_root` from any source but instincts: its learned
/// tracks and the imports scoped to it.
pub fn other_routes(tracks_root: &Path, repo: &str) -> Vec<Route> {
    other_routes_except(tracks_root, repo, &BTreeSet::new())
}

/// [`other_routes`], leaving out the tracks named in `skip` (those a learn run is replacing).
pub fn other_routes_except(tracks_root: &Path, repo: &str, skip: &BTreeSet<&str>) -> Vec<Route> {
    let mut out = Vec::new();
    for t in io::read_all(&tracks_root.join(slug(repo))).unwrap_or_default() {
        if !adage_tracks::index::is_instinct_track(&t) && !skip.contains(t.manifest.track.name.as_str()) {
            out.extend(t.routes);
        }
    }
    for t in io::read_all(&tracks_root.join(adage_tracks::index::IMPORTED_DIR)).unwrap_or_default() {
        if t.manifest.scope.repo == repo {
            out.extend(t.routes);
        }
    }
    out
}

/// `adage condition --instincts-only`: bind one repository's instincts with no sessions (day 0), write
/// the track, and compile the index. Returns the report for that repository.
pub fn learn_only(
    repo: &str,
    root: &Path,
    tracks_root: &Path,
    index_path: &Path,
    cfg: &Config,
) -> Result<crate::Report> {
    let judged = cfg.spools.as_deref().map(judgements).unwrap_or_default();
    let others = other_routes(tracks_root, repo);
    let (ir, plan) = plan_repo(repo, Some(root), tracks_root, &others, cfg, &judged, &review::today())?;
    let mut report = crate::Report::default();
    let mut rr = crate::RepoReport::default();
    if let Some((tr, p)) = plan {
        p.write()?;
        rr.tracks.push(tr);
    }
    if let Some(r) = &ir.root {
        record_binding(tracks_root, repo, r);
    }
    rr.instincts = Some(ir);
    report.repos.insert(repo.to_string(), rr);
    let all = io::read_all(tracks_root)?;
    let (index, conflicts) = adage_tracks::Index::compile_root(tracks_root, &all);
    index.write_atomic(index_path)?;
    report.index_generation = index.generation;
    report.conflicts = conflicts;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sandbox() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/sandboxes/acme-billing")
    }

    #[test]
    fn binds_writes_and_hands_over_to_a_learned_route() {
        let home = tempfile::tempdir().unwrap();
        let tracks = home.path().join("tracks");
        let cfg = Config::default();
        let none = BTreeMap::new();
        let (rep, plan) = plan_repo("acme", Some(&sandbox()), &tracks, &[], &cfg, &none, "2026-09-27").unwrap();
        let (tr, p) = plan.expect("a track");
        assert_eq!((tr.name.as_str(), tr.version.as_str(), tr.change.as_str()), ("acme/instincts", "0.1.0", "created"));
        // One route per failure signature (docs/instincts.md, Failure signatures).
        assert_eq!((rep.bound, rep.advice, rep.nascent), (17, 1, 0), "{rep:#?}");
        assert!(p.track.manifest.track.provenance.starts_with("instinct:core@"));
        assert!(p.track.routes.iter().all(|r| r.is_instinct() && r.strength == Strength::Prime));
        p.write().unwrap();
        // Unchanged files: unchanged track.
        let (_, again) = plan_repo("acme", Some(&sandbox()), &tracks, &[], &cfg, &none, "2026-09-27").unwrap();
        assert_eq!(again.unwrap().0.change, "unchanged");
        // The learner learns the same dead end: the instinct route leaves, confirmed by sessions.
        let learned = Route {
            evidence: adage_tracks::Evidence { provenance: "desire-path".into(), support: 5, ..Default::default() },
            ..p.track.routes.iter().find(|r| r.id == "dead-end-d41b1e").unwrap().clone()
        };
        let (rep, plan) = plan_repo("acme", Some(&sandbox()), &tracks, &[learned], &cfg, &none, "2026-09-27").unwrap();
        assert_eq!(rep.confirmed, vec!["dead-end-d41b1e".to_string()]);
        assert!(!plan.unwrap().1.track.routes.iter().any(|r| r.id == "dead-end-d41b1e"));
        // Turned off: the track empties; the root unknown: nothing is planned.
        let off = Config { enabled: false, ..Config::default() };
        let (rep, plan) = plan_repo("acme", Some(&sandbox()), &tracks, &[], &off, &none, "2026-09-27").unwrap();
        assert!(rep.not_bound.is_some() && plan.unwrap().1.track.routes.is_empty());
        let (rep, plan) = plan_repo("acme", None, &tracks, &[], &cfg, &none, "2026-09-27").unwrap();
        assert!(rep.not_bound.is_some() && plan.is_none());
    }

    /// Launch report, bug 7: bound against another checkout (the one old traces name), or before
    /// the checkout's declared files changed, the binding is stale.
    #[test]
    fn a_binding_is_stale_for_another_checkout_or_changed_files() {
        let d = tempfile::tempdir().unwrap();
        let tracks = d.path().join("tracks");
        let (live, old) = (d.path().join("live"), d.path().join("old"));
        for r in [&live, &old] {
            std::fs::create_dir_all(r).unwrap();
            std::fs::write(r.join("pyproject.toml"), "[project]\nname = 'click'\n").unwrap();
        }
        assert!(stale_binding(&tracks, "click", &live), "never bound");
        record_binding(&tracks, "click", &old);
        assert!(stale_binding(&tracks, "click", &live), "bound against another checkout");
        record_binding(&tracks, "click", &live);
        assert!(!stale_binding(&tracks, "click", &live));
        std::fs::create_dir_all(live.join(".venv/bin")).unwrap();
        std::fs::write(live.join(".venv/pyvenv.cfg"), "home = /usr/bin\n").unwrap();
        assert!(stale_binding(&tracks, "click", &live), "a .venv made since");
        // The record is a hidden file beside the tracks, never a track.
        assert!(io::read_all(&tracks).unwrap().is_empty());
    }

    #[test]
    fn extinction_rules() {
        let j = |followed, misfired, ignored, control_followed, control_total| Judged {
            followed,
            misfired,
            ignored,
            control_followed,
            control_total,
        };
        assert!(should_extinguish(&j(0, 2, 0, 0, 0)).is_none());
        assert!(should_extinguish(&j(0, 3, 0, 0, 0)).is_some());
        assert!(should_extinguish(&j(10, 3, 0, 0, 0)).is_none(), "most follows worked");
        // The launch report's reproduction, "ignored 10 of 10 times", came from follows the
        // matcher missed: ignores alone never extinguish.
        assert!(should_extinguish(&j(0, 0, 10, 0, 0)).is_none());
        assert!(should_extinguish(&j(0, 0, 40, 0, 0)).is_none());
        // Nor with a control arm where nothing is ever seen followed (a matching gap looks so).
        assert!(should_extinguish(&j(0, 0, 10, 0, 10)).is_none());
        // With a control arm: agents took the step as often unaided, and ignored it when told.
        assert!(should_extinguish(&j(1, 0, 9, 2, 10)).is_some());
        assert!(should_extinguish(&j(1, 0, 9, 2, 4)).is_none(), "too few control judgements");
        // Delivering it made a difference: kept.
        assert!(should_extinguish(&j(4, 0, 6, 1, 10)).is_none());
        assert!(should_extinguish(&j(5, 0, 5, 0, 0)).is_none());
        // The value model decides only for a route that misfired: two hundred ignores alone are
        // no evidence against it (a broken follow judge reads every follow as one).
        assert!(should_extinguish(&j(0, 0, 200, 0, 0)).is_none());
        assert!(should_extinguish(&j(0, 1, 200, 0, 0)).is_some());
    }
}
