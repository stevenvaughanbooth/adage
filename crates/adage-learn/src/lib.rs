//! `adage learn`: import sessions, mine routes, assemble versioned tracks, compile the index.

pub mod bridge;
pub mod cache;
pub mod codex;
pub mod diff;
pub mod graph;
pub mod import;
pub mod instincts;
pub mod intent;
pub mod mine;
pub mod opencode;
pub mod stale;

use adage_core::CommandTable;
use adage_intent::model::StaticModel;
use adage_tracks::proposals::Proposals;
use adage_tracks::{io, Gauge, Index, Manifest, Route, Scope, Track, TrackInfo, Trigger};
use anyhow::Result;
use import::Session;
pub use mine::LearnConfig;
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, Default, Serialize)]
pub struct Report {
    pub sessions_read: usize,
    pub sessions_used: usize,
    pub sessions_steered: usize,
    pub repos: BTreeMap<String, RepoReport>,
    pub index_generation: String,
    pub thresholds: String,
    /// The embedding model intent families were learned with; `None` if none is installed (intent
    /// families are then skipped, and existing intent tracks are left as they are).
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
    /// Session facts the graph engine computed (`None` with the miners alone).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub graph: Option<graph::FactsReport>,
}

#[derive(Debug, Default, Serialize)]
pub struct TrackReport {
    pub name: String,
    pub version: String,
    pub change: String,
    pub routes: Vec<String>,
}

pub struct Inputs {
    pub transcripts: Vec<PathBuf>,
    /// Session id → task succeeded, where known.
    pub outcomes: BTreeMap<String, bool>,
    pub repo_override: Option<String>,
    /// The models directory (`$ADAGE_HOME/models`). No usable model there: no intent families.
    pub models: Option<PathBuf>,
    /// The parse cache directory (`$ADAGE_HOME/cache/sessions`, [`cache`]); `None` parses every
    /// transcript. The results are the same either way.
    pub cache: Option<PathBuf>,
    /// Learn only this repository (automatic learning): sessions of other repositories are not
    /// read into the run, not counted in `sessions_read`, and their tracks are left as they are.
    pub only_repo: Option<String>,
}

pub fn slug(repo: &str) -> String {
    repo.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
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
        (r.id.clone(), serde_json::to_string(&(&trigger, &r.step, &r.slots, &r.aspect)).unwrap_or_default())
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
        (format!("{}.0.0", ma + 1), "route changed or removed".into())
    } else {
        (format!("{ma}.{}.0", mi + 1), "route added".into())
    }
}

/// One track as a learn run would write it, with the proposals it would record for pinned routes.
#[derive(Debug, Clone)]
pub struct Planned {
    pub dir: PathBuf,
    pub track: Track,
    pub proposals: Proposals,
    /// Decisions to save beside the track (`review.yaml`), when learning made one itself (an
    /// instinct retired automatically).
    pub reviews: Option<adage_tracks::review::Reviews>,
}

impl Planned {
    /// Write the track, its proposals and any decisions learning recorded.
    pub fn write(&self) -> Result<()> {
        io::write(&self.dir, &self.track)?;
        adage_tracks::proposals::save(&self.dir, &self.proposals)?;
        if let Some(r) = &self.reviews {
            adage_tracks::review::save(&self.dir, r)?;
        }
        Ok(())
    }
}

/// Read one session file: a Claude Code transcript, an OpenCode `opencode export` file, or a Codex
/// rollout (recognised by content).
pub fn read_session(path: &Path, table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    if let Some(lines) = codex::read_rollout(path) {
        return codex::parse_rollout(&lines, table, repo_override);
    }
    match opencode::read_export(path) {
        Some(v) => opencode::parse_export(&v, table, repo_override),
        None => import::parse_transcript(path, table, repo_override),
    }
}

/// `adage learn`: import sessions, mine routes, write tracks, compile the index.
pub fn learn(inputs: &Inputs, tracks_root: &Path, index_path: &Path, cfg: &LearnConfig) -> Result<Report> {
    let table = CommandTable::load();
    let mut sessions: Vec<Session> = Vec::new();
    let (mut read, mut steered) = (0, 0);
    let mut cache = inputs.cache.as_deref().map(cache::Cache::open);
    // Repository keys by root, found once per run.
    let mut keys: BTreeMap<PathBuf, String> = BTreeMap::new();
    let mut repo_of = |root: &Path| -> String {
        keys.entry(root.to_path_buf()).or_insert_with(|| adage_core::repo::repo_key(root)).clone()
    };
    let (over, only) = (inputs.repo_override.as_deref(), inputs.only_repo.as_deref());
    for p in &inputs.transcripts {
        let got = match cache.as_mut() {
            Some(c) => c.read(p, &table, over, only, &mut repo_of),
            None => match read_session(p, &table, over) {
                None => cache::Read::Nothing,
                Some(s) if only.is_some_and(|r| r != s.repo) => cache::Read::Elsewhere,
                Some(s) => cache::Read::Session(Box::new(s)),
            },
        };
        match got {
            cache::Read::Elsewhere => {}
            // A file with no session belongs to no repository: counted only in a full run.
            cache::Read::Nothing => read += usize::from(only.is_none()),
            cache::Read::Session(s) => {
                let mut s = *s;
                read += 1;
                steered += usize::from(s.steered);
                s.outcome = inputs.outcomes.get(&s.id).copied();
                if !s.steps.is_empty() {
                    sessions.push(s);
                }
            }
        }
    }
    let (hits, misses) = cache.as_ref().map(|c| (c.hits, c.misses)).unwrap_or((0, 0));
    if let Some(c) = cache {
        c.save();
    }
    if let (Some(repo), true, Some(ic)) = (only, sessions.is_empty(), cfg.instincts.as_ref()) {
        // No session to learn from yet: bind the repository's instincts from its files (day 0),
        // when its root is known and they are not bound already.
        let bound = io::read(&tracks_root.join(slug(repo)).join(instincts::TRACK)).is_ok();
        if let (Some(root), false) = (ic.roots.get(repo), bound) {
            let mut report = instincts::learn_only(repo, root, tracks_root, index_path, ic)?;
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
            index_generation: Index::load(index_path).generation,
            thresholds: thresholds(cfg),
            cache_hits: hits,
            cache_misses: misses,
            ..Default::default()
        };
        return Ok(report);
    }
    // Session facts are cached beside the parse cache (`$ADAGE_HOME/cache/facts`).
    let mut cfg = cfg.clone();
    if cfg.facts_cache.is_none() {
        cfg.facts_cache = inputs.cache.as_deref().and_then(Path::parent).map(|p| p.join("facts"));
    }
    let cfg = &cfg;
    let (mut report, planned) = plan(sessions, inputs.models.as_deref(), tracks_root, cfg)?;
    report.sessions_read = read;
    report.sessions_steered = steered;
    (report.cache_hits, report.cache_misses) = (hits, misses);
    for p in &planned {
        p.write()?;
    }
    let all = io::read_all(tracks_root)?;
    let (index, conflicts) = Index::compile_with_conflicts(&all);
    index.write_atomic(index_path)?;
    report.index_generation = index.generation.clone();
    report.conflicts = conflicts;
    Ok(report)
}

fn thresholds(cfg: &LearnConfig) -> String {
    format!(
        "min_support={} green_lower_bound={} yellow_rate={}",
        cfg.min_support, cfg.green_lower_bound, cfg.yellow_rate
    )
}

/// The dry run of `adage learn`: everything it would mine, reconcile, and version from these
/// sessions against the tracks installed under `tracks_root`, with nothing written. `adage survey`
/// shows these as candidates. `sessions_read`, `sessions_steered` and `index_generation` are left
/// to the caller; sessions without steps are ignored.
pub fn plan(
    sessions: Vec<Session>,
    models: Option<&Path>,
    tracks_root: &Path,
    cfg: &LearnConfig,
) -> Result<(Report, Vec<Planned>)> {
    let mut report = Report { thresholds: thresholds(cfg), ..Default::default() };
    let mut sessions: Vec<Session> = sessions.into_iter().filter(|s| !s.steps.is_empty()).collect();
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    report.sessions_used = sessions.len();
    // Fail open: without a usable model, intent families are skipped and everything else is
    // learned exactly as before.
    let model = models.and_then(|d| StaticModel::load(&adage_intent::model::dir_in(d)).ok());
    report.intent_model = model.as_ref().map(|m| m.id.clone());

    let mut by_repo: BTreeMap<String, Vec<&Session>> = BTreeMap::new();
    for s in &sessions {
        by_repo.entry(s.repo.clone()).or_default().push(s);
    }

    let today = adage_tracks::review::today();
    let judged =
        cfg.instincts.as_ref().and_then(|c| c.spools.as_deref()).map(instincts::judgements).unwrap_or_default();
    let mut planned = Vec::new();
    for (repo, ss) in &by_repo {
        let first_planned = planned.len();
        let mut rr = RepoReport { sessions: ss.len(), ..Default::default() };
        // Hysteresis: routes that already exist in this repository's tracks keep a lower bar.
        let mut cfg = cfg.clone();
        for t in io::read_all(&tracks_root.join(slug(repo))).unwrap_or_default() {
            cfg.existing.extend(t.routes.iter().map(|r| r.id.clone()));
        }
        let cfg = &cfg;
        // Each miner sees only sessions not steered on its own kind: those that received a
        // directive which could have influenced it (`import::steers`; a directive of unknown kind
        // steers every miner).
        let unsteered = |kind: &str| -> Vec<&Session> { ss.iter().copied().filter(|s| !s.steered_by(kind)).collect() };
        let ws = unsteered("work-items");
        let mut work = if cfg.graph() { graph::lookup_routes(&ws, cfg) } else { mine::prompt_routes(&ws, cfg) };
        if cfg.graph() {
            let facts = graph::facts(ss, model.as_ref(), cfg);
            rr.graph = Some(facts);
        }
        if let Some(r) = work.first() {
            if let adage_tracks::Step::Run { .. } = &r.step {
                let typed = r.slots.iter().any(|s| matches!(s.source, adage_tracks::SlotSource::PromptValue { .. }));
                let key = if typed { graph::lookup_key(&ws, r) } else { lookup_key(&ws, r) };
                if let Some(k) = key {
                    work.extend(mine::read_batch_routes(&ws, &k, cfg));
                }
            }
        }
        let fs = unsteered("finish-line");
        let fs: Vec<&Session> = fs.into_iter().filter(|s| !s.directed.contains("finish-line-stop")).collect();
        let ds = unsteered("dead-ends");
        let fl = mine::obligation_routes(&fs, cfg);
        let dl = mine::dead_end_routes(&ds, cfg);
        // (kind, description, mined routes, the sessions that miner saw)
        let mut groups: Vec<(&str, &str, Vec<Route>, Vec<&Session>)> = vec![
            ("work-items", "Context gathering for prompts that name a work item", work, ws),
            ("finish-line", "Steps this repository requires after certain edits", fl, fs),
            ("dead-ends", "Commands that fail in this repository, and what works instead", dl, ds),
        ];
        if let Some(m) = &model {
            let is = unsteered("intents");
            let mined = intent::intent_routes(&is, m, cfg);
            rr.intent_families = mined.families;
            groups.push(("intents", "Commands that requests like these lead to in this repository", mined.routes, is));
        }
        let mut emb = stale::Embeddings::new(model.as_ref());
        // The harnesses these sessions ran in (Claude Code transcripts, OpenCode exports).
        let harness: Vec<String> = ss.iter().map(|s| s.harness.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        let models: Vec<String> =
            ss.iter().filter_map(|s| s.model.clone()).collect::<BTreeSet<_>>().into_iter().collect();
        for (suffix, description, routes, seen) in groups {
            let name = format!("{}/{suffix}", slug(repo));
            let dir = tracks_root.join(slug(repo)).join(suffix);
            let prev = io::read(&dir).ok();
            if routes.is_empty() && prev.is_none() {
                continue;
            }
            // Human-edited routes are preserved; the miner never overwrites them. Where the miner's
            // own version differs materially, it is proposed in `adage review` instead.
            let mut routes = routes;
            // The sessions each route could not learn from, because they were steered on its kind.
            for r in routes.iter_mut() {
                r.evidence.exposed = stale::exposure(r, suffix, ss, &mut emb).exposed;
            }
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
            let reviews = match adage_tracks::review::load(&dir) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("adage: not updating {name}: {e:#}");
                    continue;
                }
            };
            // Proposals are recomputed from scratch every run; an unreadable file only loses dates
            // and dismissals, never a decision.
            let prev_proposals = adage_tracks::proposals::load(&dir).unwrap_or_else(|e| {
                eprintln!("adage: ignoring unreadable proposals for {name}: {e:#}");
                Proposals::default()
            });
            let proposals =
                adage_tracks::proposals::propose_with(&prev_proposals, &pinned, &today, cfg.material_lower_bound);
            let routes =
                adage_tracks::review::reconcile_with(prev.as_ref(), &reviews, routes, cfg.material_lower_bound);
            // A route in service that was not found again only because it steered the sessions
            // that would confirm it is kept, stale, on its last evidence (E1; `stale`).
            let mut routes = adage_tracks::review::keep_stale(prev.as_ref(), &reviews, routes, &today, |p| {
                stale::verdict(p, suffix, ss, &seen, cfg, &mut emb)
            });
            routes.sort_by(|a, b| a.id.cmp(&b.id));
            // Mined routes are built from untrusted session content. One that fails validation
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
            let (version, change) = bump(prev.as_ref(), &routes);
            let track = Track {
                manifest: Manifest {
                    track: TrackInfo {
                        name: name.clone(),
                        version: version.clone(),
                        description: description.into(),
                        provenance: cfg.provenance.clone(),
                    },
                    scope: Scope { repo: repo.clone() },
                    gauge: Gauge { harness: harness.clone(), models: models.clone() },
                },
                routes,
            };
            io::validate(&track)?;
            let mut lines: Vec<String> =
                track.routes.iter().map(|r| format!("{} [{:?}]", r.id, r.aspect).to_lowercase()).collect();
            lines.extend(
                proposals.routes.keys().map(|id| format!("{id}: pinned; a change is proposed in `adage review`")),
            );
            rr.tracks.push(TrackReport { name, version, change, routes: lines });
            planned.push(Planned { dir, track, proposals, reviews: None });
        }
        // Instincts, after the miners and review: bound from the repository's own files, with
        // every route from another source taking precedence (docs/instincts.md).
        if let Some(ic) = &cfg.instincts {
            let mut others: Vec<Route> =
                planned[first_planned..].iter().flat_map(|p: &Planned| p.track.routes.iter().cloned()).collect();
            let planned_names: BTreeSet<&str> =
                planned[first_planned..].iter().map(|p| p.track.manifest.track.name.as_str()).collect();
            // Installed tracks this run did not re-plan (imports, and learned kinds not mined now).
            others.extend(instincts::other_routes_except(tracks_root, repo, &planned_names));
            let root = ic.roots.get(repo).cloned().or_else(|| instincts::root_of(ss.iter().map(|s| s.root.as_path())));
            let (ir, plan) = instincts::plan_repo(repo, root.as_deref(), tracks_root, &others, ic, &judged, &today)?;
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

/// The canonical key of the mined lookup command — the same key the runtime computes for the
/// agent's call, so the linked-files route triggers on exactly this command.
fn lookup_key(_ss: &[&Session], r: &Route) -> Option<String> {
    let adage_tracks::Step::Run { command } = &r.step else { return None };
    let sample = command.replace("{item}", "X-1");
    Some(adage_runtime::command_key(&sample, &CommandTable::load()))
}

pub fn is_prompt_route(r: &Route) -> bool {
    matches!(r.trigger, Trigger::Prompt { .. })
}
