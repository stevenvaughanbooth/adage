//! `adage learn`: import sessions, mine routes, assemble versioned tracks, compile the index.

pub mod import;
pub mod intent;
pub mod mine;
pub mod opencode;

use adage_core::CommandTable;
use adage_intent::model::StaticModel;
use adage_tracks::{io, Gauge, Index, Manifest, Route, Scope, Track, TrackInfo, Trigger};
use anyhow::Result;
use import::Session;
pub use mine::LearnConfig;
use serde::Serialize;
use std::collections::BTreeMap;
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
}

#[derive(Debug, Default, Serialize)]
pub struct RepoReport {
    pub sessions: usize,
    pub intent_families: usize,
    pub tracks: Vec<TrackReport>,
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
}

pub fn slug(repo: &str) -> String {
    repo.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' }).collect()
}

/// Semantic-version bump from the difference between the previous and new routes (Design §10.9).
fn bump(prev: Option<&Track>, routes: &[Route]) -> (String, String) {
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

pub fn learn(inputs: &Inputs, tracks_root: &Path, index_path: &Path, cfg: &LearnConfig) -> Result<Report> {
    let table = CommandTable::load();
    let mut report = Report {
        thresholds: format!(
            "min_support={} green_lower_bound={} yellow_rate={}",
            cfg.min_support, cfg.green_lower_bound, cfg.yellow_rate
        ),
        ..Default::default()
    };
    let mut sessions: Vec<Session> = Vec::new();
    for p in &inputs.transcripts {
        report.sessions_read += 1;
        // Claude Code transcripts, or OpenCode `opencode export` files (recognised by content).
        let parsed = match opencode::read_export(p) {
            Some(v) => opencode::parse_export(&v, &table, inputs.repo_override.as_deref()),
            None => import::parse_transcript(p, &table, inputs.repo_override.as_deref()),
        };
        if let Some(mut s) = parsed {
            if s.steered {
                report.sessions_steered += 1;
            }
            s.outcome = inputs.outcomes.get(&s.id).copied();
            if !s.steps.is_empty() {
                sessions.push(s);
            }
        }
    }
    sessions.sort_by(|a, b| a.id.cmp(&b.id));
    report.sessions_used = sessions.len();
    // Fail open: without a usable model, intent families are skipped and everything else is
    // learned exactly as before.
    let model = inputs.models.as_deref().and_then(|d| StaticModel::load(&adage_intent::model::dir_in(d)).ok());
    report.intent_model = model.as_ref().map(|m| m.id.clone());

    let mut by_repo: BTreeMap<String, Vec<&Session>> = BTreeMap::new();
    for s in &sessions {
        by_repo.entry(s.repo.clone()).or_default().push(s);
    }

    for (repo, ss) in &by_repo {
        let mut rr = RepoReport { sessions: ss.len(), ..Default::default() };
        // Hysteresis: routes that already exist in this repository's tracks keep a lower bar.
        let mut cfg = cfg.clone();
        for t in io::read_all(&tracks_root.join(slug(repo))).unwrap_or_default() {
            cfg.existing.extend(t.routes.iter().map(|r| r.id.clone()));
        }
        let cfg = &cfg;
        // Each miner sees only sessions not steered by its own kind of route.
        let unsteered = |kind: &str| -> Vec<&Session> { ss.iter().copied().filter(|s| !s.steered_by(kind)).collect() };
        let ws = unsteered("work-items");
        let mut work = mine::prompt_routes(&ws, cfg);
        if let Some(r) = work.first() {
            if let adage_tracks::Step::Run { .. } = &r.step {
                let key = lookup_key(&ws, r);
                if let Some(k) = key {
                    work.extend(mine::read_batch_routes(&ws, &k, cfg));
                }
            }
        }
        let fs = unsteered("finish-line");
        let fs: Vec<&Session> = fs.into_iter().filter(|s| !s.directed.contains("finish-line-stop")).collect();
        let ds = unsteered("dead-ends");
        let mut groups: Vec<(&str, &str, Vec<Route>)> = vec![
            ("work-items", "Context gathering for prompts that name a work item", work),
            ("finish-line", "Steps this repository requires after certain edits", mine::obligation_routes(&fs, cfg)),
            (
                "dead-ends",
                "Commands that fail in this repository, and what works instead",
                mine::dead_end_routes(&ds, cfg),
            ),
        ];
        if let Some(m) = &model {
            let mined = intent::intent_routes(&unsteered("intents"), m, cfg);
            rr.intent_families = mined.families;
            groups.push(("intents", "Commands that requests like these lead to in this repository", mined.routes));
        }
        for (suffix, description, routes) in groups {
            let name = format!("{}/{suffix}", slug(repo));
            let dir = tracks_root.join(slug(repo)).join(suffix);
            let prev = io::read(&dir).ok();
            if routes.is_empty() && prev.is_none() {
                continue;
            }
            // Human-edited routes are preserved; the miner never overwrites them.
            let mut routes = routes;
            if let Some(p) = &prev {
                for r in p.routes.iter().filter(|r| r.authored) {
                    routes.retain(|x| x.id != r.id);
                    routes.push(r.clone());
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
            let mut routes = adage_tracks::review::reconcile(prev.as_ref(), &reviews, routes);
            routes.sort_by(|a, b| a.id.cmp(&b.id));
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
                    gauge: Gauge {
                        harness: vec!["claude-code".into()],
                        models: ss
                            .iter()
                            .filter_map(|s| s.model.clone())
                            .collect::<std::collections::BTreeSet<_>>()
                            .into_iter()
                            .collect(),
                    },
                },
                routes,
            };
            io::validate(&track)?;
            io::write(&dir, &track)?;
            rr.tracks.push(TrackReport {
                name,
                version,
                change,
                routes: track.routes.iter().map(|r| format!("{} [{:?}]", r.id, r.aspect).to_lowercase()).collect(),
            });
        }
        report.repos.insert(repo.clone(), rr);
    }

    let all = io::read_all(tracks_root)?;
    let index = Index::compile(&all);
    index.write_atomic(index_path)?;
    report.index_generation = index.generation.clone();
    Ok(report)
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
