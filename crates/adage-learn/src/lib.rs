//! `adage condition`: import sessions, learn reflexes (the graph engine, [`engine`]), assemble one
//! versioned track per repository (`<repo>/learned`), bind instincts, compile the index.

pub mod bridge;
pub mod cards;
pub mod cases;
mod checkpoint;
pub mod claims;
pub mod corpus;
pub mod diff;
pub mod dormant;
pub mod effects;
pub mod engine;
pub mod evidence;
pub mod graph;
pub mod import;
pub mod instincts;
pub mod ledger;
pub mod outcome;
pub mod store;
pub mod testmap;
pub mod value;

pub use adage_ingest::{cache, codex, opencode, scope};

pub use adage_trace::EvidenceIdentity;
use adage_tracks::proposals::Proposals;
use adage_tracks::{io, Gauge, Manifest, Route, Scope, Track, TrackInfo, Trigger};
use anyhow::Result;
pub use evidence::LearnConfig;
use import::Session;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub sessions_read: usize,
    pub sessions_used: usize,
    /// Identical copies removed before any evidence is counted. Conflicting ids are an error.
    pub sessions_duplicates: usize,
    /// Sessions that received a directive: what it named is not their evidence.
    pub sessions_steered: usize,
    pub repos: BTreeMap<String, RepoReport>,
    pub index_generation: String,
    pub thresholds: String,
    /// The embedding model prompt-intent prototypes were learned with; `None` if none is
    /// installed (there are then none).
    pub intent_model: Option<String>,
    /// Transcripts read from the parse cache, and parsed afresh (with [`Inputs::cache`]).
    #[serde(skip)]
    pub cache_hits: usize,
    #[serde(skip)]
    pub cache_misses: usize,
    /// Interlocking conflicts the compiled index resolved (`adage_tracks::interlock`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub conflicts: Vec<adage_tracks::interlock::Conflict>,
}

#[derive(Debug, Default, Serialize)]
pub struct RepoReport {
    pub sessions: usize,
    pub intent_families: usize,
    pub tracks: Vec<TrackReport>,
    /// What binding instincts did (`None` when learning did not bind them).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instincts: Option<instincts::InstinctReport>,
    /// What learning mined, compiled and judged.
    pub engine: engine::EngineReport,
}

#[derive(Debug, Default, Serialize)]
pub struct TrackReport {
    pub name: String,
    pub version: String,
    pub change: String,
    pub routes: Vec<String>,
}

#[derive(Default)]
pub struct Inputs {
    pub transcripts: Vec<PathBuf>,
    /// Session id → task succeeded, where known.
    pub outcomes: BTreeMap<String, bool>,
    /// Session id → declared evidence identity, applied after parsing/cache lookup.
    pub identities: BTreeMap<String, adage_trace::EvidenceIdentity>,
    pub repo_override: Option<String>,
    /// The models directory (`$ADAGE_HOME/models`). No usable model there: no prompt-intent
    /// prototypes.
    pub models: Option<PathBuf>,
    /// The parse cache directory (`$ADAGE_HOME/cache/sessions`, [`cache`]); `None` parses every
    /// transcript. The results are the same either way.
    pub cache: Option<PathBuf>,
    /// Learn only this repository (automatic learning): sessions of other repositories are not
    /// read into the run, not counted in `sessions_read`, and their tracks are left as they are.
    pub only_repo: Option<String>,
}

/// Semantic-version bump from the difference between the previous and new routes (Design §10.9).
pub(crate) fn bump(prev: Option<&Track>, routes: &[Route]) -> (String, String) {
    let Some(p) = prev else { return ("0.1.0".into(), "created".into()) };
    let v: Vec<u64> = p.manifest.track.version.split('.').filter_map(|x| x.parse().ok()).collect();
    let (ma, mi, pa) =
        (v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(1), v.get(2).copied().unwrap_or(0));
    // An intent family's centroid and threshold drift as sessions accumulate; that is evidence,
    // not a change of route.
    let shape = |r: &Route| {
        let trigger = match &r.trigger {
            Trigger::Intent { family, model, .. } => {
                Trigger::Intent { family: family.clone(), model: model.clone(), centroid: vec![], threshold: 0.0 }
            }
            t => t.clone(),
        };
        (r.id.clone(), serde_json::to_string(&(&trigger, &r.step, &r.slots, &r.strength)).unwrap_or_default())
    };
    let old: BTreeMap<_, _> = p.routes.iter().map(shape).collect();
    let new: BTreeMap<_, _> = routes.iter().map(shape).collect();
    if old == new {
        let same_evidence = p.routes == routes;
        return if same_evidence {
            (p.manifest.track.version.clone(), "unchanged".into())
        } else {
            (format!("{ma}.{mi}.{}", pa + 1), "evidence updated".into())
        };
    }
    let removed_or_changed = old.iter().any(|(k, v)| new.get(k).is_some_and(|n| n != v) || !new.contains_key(k));
    if removed_or_changed {
        (format!("{}.0.0", ma + 1), "reflex changed or removed".into())
    } else {
        (format!("{ma}.{}.0", mi + 1), "reflex added".into())
    }
}

/// The track learning writes in each repository's tracks directory.
pub const TRACK: &str = "learned";

/// Track directories earlier learners wrote, one per route kind. A learn run folds their routes,
/// review decisions and proposals into [`TRACK`] (ids are unchanged) and removes them.
pub const LEGACY_TRACKS: [&str; 5] = ["work-items", "finish-line", "dead-ends", "intents", "graph"];

/// One track as a learn run would write it, with the proposals it would record for pinned routes.
#[derive(Debug, Clone)]
pub struct Planned {
    /// The track's name (`<repo slug>/learned`): where the store keeps it.
    pub name: String,
    pub track: Track,
    pub proposals: Proposals,
    /// Decisions to save beside the track (`review.yaml`), when learning made one itself (an
    /// instinct extinguished automatically).
    pub reviews: Option<adage_tracks::review::Reviews>,
    /// Why each learned route exists (`.why.json`, for people; never rendered to the agent).
    pub why: Option<BTreeMap<String, adage_tracks::why::Why>>,
    /// Tracks this one replaces ([`LEGACY_TRACKS`]), by name, removed once it is written.
    pub replaces: Vec<String>,
}

impl Planned {
    /// Write the track, its proposals and any decisions learning recorded, as directories under
    /// `tracks_root` ([`store::Files`]).
    pub fn write_under(&self, tracks_root: &Path) -> Result<()> {
        let dir = tracks_root.join(&self.name);
        io::write(&dir, &self.track)?;
        adage_tracks::proposals::save(&dir, &self.proposals)?;
        if let Some(r) = &self.reviews {
            adage_tracks::review::save(&dir, r)?;
        }
        if let Some(w) = &self.why {
            adage_tracks::why::save(&dir, w)?;
        }
        for d in &self.replaces {
            std::fs::remove_dir_all(tracks_root.join(d))?;
        }
        Ok(())
    }
}

pub use adage_ingest::read_session;
pub use adage_tracks::layout::{follow_weights_path, read_follow_weights, slug};

/// `adage condition`: ingest the traces, then learn from their sessions ([`learn_sessions`]).
pub fn learn(inputs: &Inputs, tracks_root: &Path, index_path: &Path, cfg: &LearnConfig) -> Result<Report> {
    let opts = adage_ingest::batch::Options {
        repo_override: inputs.repo_override.clone(),
        only_repo: inputs.only_repo.clone(),
        cache: inputs.cache.clone(),
        ..Default::default()
    };
    let mut batch = adage_ingest::batch::read(&inputs.transcripts, &opts);
    for s in &mut batch.sessions {
        s.outcome = inputs.outcomes.get(&s.id).copied();
        if let Some(identity) = inputs.identities.get(&s.id) {
            s.identity = identity.clone();
        }
    }
    learn_sessions(batch, inputs.only_repo.as_deref(), inputs.models.as_deref(), tracks_root, index_path, cfg)
}

/// The learning engine's entry: learn from canonical sessions (an ingested batch, whatever its
/// traces came from), write the tracks, and compile the index. With `only`, learn that repository
/// alone; with no session for it yet, bind its instincts from its files (day 0).
pub fn learn_sessions(
    batch: adage_ingest::batch::Batch,
    only: Option<&str>,
    models: Option<&Path>,
    tracks_root: &Path,
    index_path: &Path,
    cfg: &LearnConfig,
) -> Result<Report> {
    learn_sessions_into(batch, only, models, &store::Files::new(tracks_root, index_path), cfg)
}

/// [`learn_sessions`], against `store` ([`store::Store`]): what was learned before is read from
/// it, and everything the run decides is written only through it.
pub fn learn_sessions_into(
    batch: adage_ingest::batch::Batch,
    only: Option<&str>,
    models: Option<&Path>,
    store: &dyn store::Store,
    cfg: &LearnConfig,
) -> Result<Report> {
    let adage_ingest::batch::Batch { sessions, read, steered, cache_hits: hits, cache_misses: misses } = batch;
    let sessions = sessions.into_iter().filter(|s| only.is_none_or(|repo| s.repo == repo)).collect();
    let (sessions, duplicates) = evidence::unique_sessions(sessions)?;
    if let (Some(repo), true, Some(ic)) = (only, sessions.is_empty(), cfg.instincts.as_ref()) {
        // No session to learn from yet: bind the repository's instincts from its files (day 0),
        // when its root is known and they are not bound against it as its files are now.
        let bound = store.track(&instincts::track_name(repo)).is_some();
        let recorded = store.binding(repo);
        let fresh = |root: &Path| bound && !adage_pack::binding::is_stale(recorded.as_ref(), root, &ic.selection());
        if let Some(root) = ic.roots.get(repo).filter(|r| !fresh(r)) {
            let mut report = instincts::learn_only_into(repo, root, store, ic)?;
            report.sessions_read = read;
            report.sessions_steered = steered;
            report.thresholds = thresholds(cfg);
            (report.cache_hits, report.cache_misses) = (hits, misses);
            return Ok(report);
        }
    }
    if only.is_some() && sessions.is_empty() {
        // Nothing to learn for this repository: its tracks and the index stay exactly as they are.
        let report = Report {
            sessions_read: read,
            sessions_steered: steered,
            index_generation: store.index_generation(),
            thresholds: thresholds(cfg),
            cache_hits: hits,
            cache_misses: misses,
            ..Default::default()
        };
        return Ok(report);
    }
    if cfg.instincts.as_ref().is_some_and(instincts::Config::binds) {
        store.install_base_packs();
    }
    let weights = follow_weights(&sessions);
    let (mut report, planned) = plan_repository(sessions, None, models, store, cfg, true)?;
    report.sessions_read = read;
    report.sessions_duplicates = duplicates;
    report.sessions_steered = steered;
    (report.cache_hits, report.cache_misses) = (hits, misses);
    for p in &planned {
        store.write_track(p)?;
    }
    for (repo, rr) in &report.repos {
        if let (Some(root), Some(ic)) = (rr.instincts.as_ref().and_then(|i| i.root.as_deref()), cfg.instincts.as_ref())
        {
            store.write_binding(repo, &instincts::binding_now(root, ic));
        }
    }
    for (repo, w) in &weights {
        store.write_follow_weights(repo, w);
    }
    let index = store.compile_index()?;
    report.index_generation = index.generation;
    report.conflicts = index.conflicts;
    Ok(report)
}

/// The learned follow judge's weights per repository, from its sessions' shell commands: in how
/// many sessions each token appears (`adage_core::judge::Weights::fit`).
fn follow_weights(sessions: &[Session]) -> BTreeMap<String, adage_core::judge::Weights> {
    let mut by: BTreeMap<&str, Vec<(&str, &str)>> = BTreeMap::new();
    for s in sessions {
        for st in &s.steps {
            if let Some(c) = st.call.input.get("command").and_then(|c| c.as_str()) {
                by.entry(s.repo.as_str()).or_default().push((s.id.as_str(), c));
            }
        }
    }
    by.into_iter().map(|(r, pairs)| (r.to_string(), adage_core::judge::Weights::fit(pairs))).collect()
}

fn thresholds(cfg: &LearnConfig) -> String {
    format!("min_support={} fire_lower_bound={} prime_rate={}", cfg.min_support, cfg.fire_lower_bound, cfg.prime_rate)
}

/// The dry run of `adage condition`: everything it would mine, reconcile, and version from these
/// sessions against the tracks `store` holds, with nothing written. `adage sense`
/// shows these as candidates. `sessions_read`, `sessions_steered` and `index_generation` are left
/// to the caller; sessions without steps are ignored.
pub fn plan(
    sessions: Vec<Session>,
    models: Option<&Path>,
    store: &dyn store::Store,
    cfg: &LearnConfig,
) -> Result<(Report, Vec<Planned>)> {
    plan_repository(sessions, None, models, store, cfg, false)
}

/// A committed corpus may explicitly remove every session of a repository. Include that empty
/// repository in reconciliation so unsupported learned routes are not left live by a no-op.
pub(crate) fn plan_repository(
    sessions: Vec<Session>,
    include_repo: Option<&str>,
    models: Option<&Path>,
    store: &dyn store::Store,
    cfg: &LearnConfig,
    cache: bool,
) -> Result<(Report, Vec<Planned>)> {
    let mut report = Report { thresholds: thresholds(cfg), ..Default::default() };
    let (sessions, duplicates) = evidence::unique_sessions(sessions)?;
    report.sessions_duplicates = duplicates;
    let sessions: Vec<Session> = sessions.into_iter().filter(|s| !s.steps.is_empty()).collect();
    report.sessions_used = sessions.len();
    // The installed model, else the one bundled in the binary (`model::resolve`, as the hook
    // chooses). Fail open: without one, intent families are skipped and everything else is
    // learned exactly as before.
    let model = models.and_then(|d| adage_intent::model::resolve_static(d, true));
    report.intent_model = model.as_ref().map(|m| m.id.clone());

    let mut by_repo: BTreeMap<String, Vec<&Session>> = BTreeMap::new();
    for s in &sessions {
        by_repo.entry(s.repo.clone()).or_default().push(s);
    }
    if let Some(repo) = include_repo {
        by_repo.entry(repo.into()).or_default();
    }

    let today = adage_tracks::review::today();
    let judged =
        cfg.instincts.as_ref().and_then(|c| c.spools.as_deref()).map(instincts::judgements).unwrap_or_default();
    let mut planned = Vec::new();
    for (repo, ss) in &by_repo {
        let first_planned = planned.len();
        let mut rr = RepoReport { sessions: ss.len(), ..Default::default() };
        let name = format!("{}/{TRACK}", slug(repo));
        // What was learned before: this track, or the per-kind tracks of an earlier learner (their
        // routes, decisions and proposals are folded in; the ids are the same).
        let replaces: Vec<String> = if store.holds(&name) {
            Vec::new()
        } else {
            LEGACY_TRACKS.iter().map(|t| format!("{}/{t}", slug(repo))).filter(|n| store.holds(n)).collect()
        };
        let prev = match replaces.is_empty() {
            true => store.track(&name),
            false => fold_tracks(store, &replaces, &name),
        };
        // Hysteresis: routes that already exist keep a lower bar.
        let mut cfg = cfg.clone();
        cfg.existing.extend(prev.iter().flat_map(|t| t.routes.iter().map(|r| r.id.clone())));
        let cfg = &cfg;
        let priors: Vec<Route> = store.track(&instincts::track_name(repo)).map(|t| t.routes).unwrap_or_default();
        let run = if cache && cfg.computation_cache {
            engine::run_cached(ss, cfg, &priors, model.as_ref(), store)
        } else {
            engine::run(ss, cfg, &priors, model.as_ref())
        };
        rr.intent_families = run.report.intent_families;
        rr.engine = run.report.clone();
        let mut routes: Vec<Route> = run.routes.iter().map(|j| j.route.clone()).collect();
        // What delivering each route changes (uplift, `value::assess`): one agents already take
        // unaided waits, nascent, for a person's approval.
        value::assess(&mut routes, ss, repo, &judged, &cfg.priors, cfg.uplift_bar());
        // Without the embedding model, prompt intents cannot be learned or judged: the ones
        // already learned are kept as they are.
        if model.is_none() && !ss.is_empty() {
            for r in prev.iter().flat_map(|t| t.routes.iter()) {
                if matches!(r.trigger, Trigger::Intent { .. }) && !routes.iter().any(|x| x.id == r.id) {
                    routes.push(r.clone());
                }
            }
        }
        let mut why: BTreeMap<String, adage_tracks::why::Why> =
            run.routes.iter().map(|j| (j.route.id.clone(), j.why.clone())).collect();
        if !(routes.is_empty() && prev.is_none()) {
            let mut emb = dormant::Embeddings::new(model.as_ref());
            // The traces each route could not learn from: a directive they received named it.
            for r in routes.iter_mut() {
                r.evidence.exposed = dormant::exposure(r, ss, &mut emb).exposed;
            }
            // Human-edited routes are preserved; learning never overwrites them. Where its own
            // version differs materially, it is proposed in `adage review` instead.
            let mut pinned: Vec<(Route, Option<Route>)> = Vec::new();
            if let Some(p) = &prev {
                for r in p.routes.iter().filter(|r| r.authored) {
                    let mined = routes.iter().find(|x| x.id == r.id).cloned();
                    routes.retain(|x| x.id != r.id);
                    routes.push(r.clone());
                    pinned.push((r.clone(), mined));
                }
            }
            // A person's decisions in `adage review` hold across re-learning (review.yaml). If they
            // cannot be read, leave the track as it is rather than silently undo them.
            let from: Vec<String> = if replaces.is_empty() { vec![name.clone()] } else { replaces.clone() };
            let reviews = match fold_reviews(store, &from) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("adage: not updating {name}: {e:#}");
                    report.repos.insert(repo.clone(), rr);
                    continue;
                }
            };
            // Proposals are recomputed from scratch every run; an unreadable file only loses dates
            // and dismissals, never a decision.
            let prev_proposals = fold_proposals(store, &from, &name);
            let proposals =
                adage_tracks::proposals::propose_with(&prev_proposals, &pinned, &today, cfg.material_lower_bound);
            let routes =
                adage_tracks::review::reconcile_with(prev.as_ref(), &reviews, routes, cfg.material_lower_bound);
            // A route in service that was not found again only because it steered the traces that
            // would confirm it is kept, dormant, on its last evidence (E1; `dormant`): judged on the
            // scorer's counts for its own pattern (its `why`), else on the traces.
            let prev_why: BTreeMap<String, adage_tracks::why::Why> = from.iter().flat_map(|n| store.why(n)).collect();
            let mut emb = dormant::Embeddings::new(model.as_ref());
            let mut routes = adage_tracks::review::keep_dormant(prev.as_ref(), &reviews, routes, &today, |p| {
                engine::verdict(prev_why.get(&p.id), &run.counts, cfg)
                    .unwrap_or_else(|| dormant::verdict(p, ss, cfg, &mut emb))
            });
            routes.sort_by(|a, b| a.id.cmp(&b.id));
            // Learned routes are built from untrusted session content. One that fails validation
            // (a quoted argument, an unrenderable signature) is dropped on its own; it must not
            // stop the rest of the repository from being learned.
            routes.retain(|r| {
                let probe = Track {
                    manifest: Manifest {
                        track: TrackInfo {
                            name: name.clone(),
                            version: "0.0.0".into(),
                            description: String::new(),
                            provenance: String::new(),
                        },
                        scope: Scope { repo: repo.clone() },
                        gauge: Gauge::default(),
                    },
                    routes: vec![r.clone()],
                };
                match io::validate(&probe) {
                    Ok(()) => true,
                    Err(e) => {
                        eprintln!("adage: not learning a route: {e:#}");
                        false
                    }
                }
            });
            if !(routes.is_empty() && prev.is_none()) {
                let (version, change) = bump(prev.as_ref(), &routes);
                // What explains each route that is still here (one kept dormant: its last one).
                for r in &routes {
                    if !why.contains_key(&r.id) {
                        if let Some(p) = prev_why.get(&r.id) {
                            why.insert(r.id.clone(), p.clone());
                        }
                    }
                }
                why.retain(|id, _| routes.iter().any(|r| &r.id == id));
                let track = Track {
                    manifest: Manifest {
                        track: TrackInfo {
                            name: name.clone(),
                            version: version.clone(),
                            description: "Reflexes learned from this repository's agent sessions".into(),
                            provenance: cfg.provenance.clone(),
                        },
                        scope: Scope { repo: repo.clone() },
                        gauge: Gauge { harness: harness_of(ss), models: evidence::models(ss) },
                    },
                    routes,
                };
                io::validate(&track)?;
                let mut lines: Vec<String> = track.routes.iter().map(|r| route_line(&name, r)).collect();
                lines.extend(
                    proposals.routes.keys().map(|id| format!("{id}: pinned; a change is proposed in `adage review`")),
                );
                rr.tracks.push(TrackReport { name: name.clone(), version, change, routes: lines });
                planned.push(Planned { name: name.clone(), track, proposals, reviews: None, why: Some(why), replaces });
            }
        }
        // Instincts, after learning and review: bound from the repository's own files, with
        // every route from another source taking precedence (docs/instincts.md).
        if let Some(ic) = &cfg.instincts {
            let mut others: Vec<Route> =
                planned[first_planned..].iter().flat_map(|p: &Planned| p.track.routes.iter().cloned()).collect();
            let planned_names: BTreeSet<&str> =
                planned[first_planned..].iter().map(|p| p.track.manifest.track.name.as_str()).collect();
            // Installed tracks this run did not re-plan (imports).
            others.extend(instincts::other_routes_except(store, repo, &planned_names));
            let root = ic.roots.get(repo).cloned().or_else(|| instincts::root_of(ss.iter().map(|s| s.root.as_path())));
            let seen =
                instincts::Seen { judged: &judged, sessions: ss, priors: &cfg.priors, uplift_min: cfg.uplift_bar() };
            let (ir, plan) = instincts::plan_repo(repo, root.as_deref(), store, &others, ic, &seen, &today)?;
            if let Some((tr, p)) = plan {
                rr.tracks.push(tr);
                planned.push(p);
            }
            rr.instincts = Some(ir);
        }
        report.repos.insert(repo.clone(), rr);
    }
    Ok((report, planned))
}

/// One route in `adage condition`'s summary: its id and strength, and, for a reflex that is not
/// delivered yet, what it waits for and how to decide it (docs/lexicon.md: a nascent is *nascent*).
/// A reflex that formed on outcome lift alone says so.
pub fn route_line(track: &str, r: &Route) -> String {
    let strength = match r.strength {
        adage_tracks::Strength::Fire => "fire",
        adage_tracks::Strength::Prime => "prime",
        adage_tracks::Strength::Inhibit => "inhibit",
    };
    let mut s = format!("{} [{}]", r.id, if r.evidence.execution.is_some() { "execution only" } else { strength });
    if let Some(e) = &r.evidence.execution {
        s.push_str(&format!(" ({}/{} supported continuations across {} sessions); silent without a local E2 grant; review with `adage review delegate {track} {} --level e2`", e.successful, e.opportunities, e.sessions, r.id));
    }
    if let Some(l) = &r.evidence.outcome_lift {
        s.push_str(&format!(
            " (outcome lift: {}/{} passed with it, {}/{} without; lower bound {:+.2})",
            l.with_passed, l.with, l.without_passed, l.without, l.lower_bound
        ));
    }
    if r.evidence.effect_evidence.is_some() {
        s.push_str(" (observe, from the traces)");
    }
    if let Some(u) = r.evidence.uplift.as_ref().filter(|u| u.gated) {
        s.push_str(&format!(" ({})", u.line()));
    }
    if r.state == adage_tracks::RouteState::Nascent {
        s.push_str(&format!(
            " NASCENT: not delivered until a person approves it; review with `adage review`, then `adage review approve {track} {}`",
            r.id
        ));
    }
    s
}

/// The harnesses these traces ran in (Claude Code transcripts, OpenCode exports, Codex rollouts).
fn harness_of(ss: &[&Session]) -> Vec<String> {
    ss.iter().map(|s| s.harness.clone()).collect::<BTreeSet<_>>().into_iter().collect()
}

/// The routes of earlier per-kind tracks, as one track to reconcile against (`None` when none
/// reads).
fn fold_tracks(store: &dyn store::Store, names: &[String], name: &str) -> Option<Track> {
    let tracks: Vec<Track> = names.iter().filter_map(|n| store.track(n)).collect();
    let first = tracks.first()?.clone();
    let mut routes: Vec<Route> = tracks.into_iter().flat_map(|t| t.routes).collect();
    routes.sort_by(|a, b| a.id.cmp(&b.id));
    routes.dedup_by(|a, b| a.id == b.id);
    let mut t = Track { routes, ..first };
    t.manifest.track.name = name.to_string();
    Some(t)
}

/// Review decisions from these tracks, as one.
fn fold_reviews(store: &dyn store::Store, names: &[String]) -> Result<adage_tracks::review::Reviews> {
    let mut out = adage_tracks::review::Reviews::default();
    for n in names {
        let r = store.reviews(n)?;
        out.routes.extend(r.routes);
        out.packs.extend(r.packs);
    }
    Ok(out)
}

/// Proposals (dates and dismissals) from these tracks, as one.
fn fold_proposals(store: &dyn store::Store, names: &[String], name: &str) -> Proposals {
    let mut out = Proposals::default();
    for n in names {
        let p = store.proposals(n).unwrap_or_else(|e| {
            eprintln!("adage: ignoring unreadable proposals for {name}: {e:#}");
            Proposals::default()
        });
        out.routes.extend(p.routes);
        out.dismissed.extend(p.dismissed);
    }
    out
}

pub fn is_prompt_route(r: &Route) -> bool {
    matches!(r.trigger, Trigger::Prompt { .. })
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_tracks::{Evidence, RouteState, Step, Strength};

    #[test]
    fn repository_slugs_preserve_valid_names_and_repair_invalid_track_paths() {
        assert_eq!(slug("github.com/acme/billing"), "github-com-acme-billing");
        assert_eq!(slug("acme_repo-v2"), "acme_repo-v2");
        let originals = [
            "MixedCase".to_string(),
            "MIXEDCASE".to_string(),
            ".private".to_string(),
            "日本語".to_string(),
            String::new(),
            "x".repeat(119),
            "x".repeat(120),
        ];
        let repaired: BTreeSet<_> = originals.iter().map(|r| slug(r)).collect();
        assert_eq!(repaired.len(), originals.len(), "distinct invalid identities retain distinct hash suffixes");
        for repo in originals {
            let name = slug(&repo);
            assert!(name.starts_with("repo-"), "{repo:?}: {name}");
            assert!(io::safe_name(&format!("{name}/{}", instincts::TRACK)), "{name}");
            assert!(io::safe_name(&format!("{name}/{TRACK}")), "{name}");
            assert_eq!(name, slug(&repo), "deterministic directory identity");
        }
        assert_eq!(slug(&"x".repeat(118)), "x".repeat(118), "largest already-valid slug stays unchanged");
    }

    #[test]
    fn the_summary_marks_nascent_reflexes_and_says_how_to_review_them() {
        let mut r = Route {
            id: "lookup-work-item".into(),
            summary: String::new(),
            trigger: Trigger::Prompt { span: adage_core::spans::SpanKind::Ticket },
            step: Step::Run { command: "python3 tools/case {item}".into() },
            slots: vec![],
            effect: adage_core::Effect::Environment,
            strength: Strength::Prime,
            state: RouteState::Nascent,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        };
        let line = route_line("clinic/work-items", &r);
        assert!(line.starts_with("lookup-work-item [prime] NASCENT: not delivered"), "{line}");
        assert!(line.contains("`adage review approve clinic/work-items lookup-work-item`"), "{line}");
        r.state = RouteState::Firm;
        r.strength = Strength::Fire;
        assert_eq!(route_line("clinic/work-items", &r), "lookup-work-item [fire]");
        r.evidence.execution = Some(adage_tracks::ExecutionEvidence { opportunities: 5, successful: 5, sessions: 5 });
        let line = route_line("clinic/work-items", &r);
        assert!(line.contains("[execution only]") && line.contains("5/5 supported continuations"), "{line}");
        assert!(line.contains("adage review delegate clinic/work-items lookup-work-item --level e2"), "{line}");
    }
}
