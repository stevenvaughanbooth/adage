//! Health for the live screens: `adage doctor`'s checks (including its configuration and live
//! learning checks), the compact lamps `adage watch` shows, the one-key fix for each problem that
//! has one, and whether a background learner is running.

use super::theme::Lamp;
use crate::autolearn::Queue;
use crate::detect;
use crate::doctor::{self, Check, Status};
use adage_core::repo;
use adage_store::{config, lock, Paths};
use adage_tracks::Index;
use std::path::Path;

/// Fail-open errors within this many days count as recent (as `adage doctor` counts them).
const RECENT_DAYS: u64 = 7;

/// What a key press can do about a check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Register Adage's hooks with this harness (project or user scope, after a confirmation
    /// showing the exact file and diff).
    InstallHooks(String),
    FetchModel,
    LearnNow,
    OpenFailOpen,
    EnableAutoLearn,
    OpenSettings,
    SetHistorySource(String),
    ImportOpenCode,
}

impl Fix {
    pub fn label(&self) -> String {
        match self {
            Fix::InstallHooks(h) => format!("install {h} hooks"),
            Fix::FetchModel => "fetch the embedding model".into(),
            Fix::LearnNow => "learn this repository now".into(),
            Fix::OpenFailOpen => "open the fail-open log".into(),
            Fix::EnableAutoLearn => "turn auto-learn on".into(),
            Fix::OpenSettings => "open the settings".into(),
            Fix::SetHistorySource(harness) => format!("Configure {} history", detect::harness_name(harness)),
            Fix::ImportOpenCode => "import OpenCode history".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Light {
    pub label: String,
    pub lamp: Lamp,
}

#[derive(Debug, Clone, Default)]
pub struct Health {
    pub checks: Vec<Check>,
    /// One per check: its fix, if it needs and has one.
    pub fixes: Vec<Option<Fix>>,
    pub lights: Vec<Light>,
    pub generation: Option<String>,
    pub routes_total: usize,
    /// Routes for this repository, when it has a key.
    pub routes_repo: Option<usize>,
    pub repo_key: Option<String>,
    /// The effective `learn.auto`, and where it came from (`default`, `file`, `env …`).
    pub auto_learn: Option<(bool, String)>,
}

impl Health {
    pub fn count(&self, s: Status) -> usize {
        self.checks.iter().filter(|c| c.status == s).count()
    }
}

fn lamp(s: Status) -> Lamp {
    match s {
        Status::Pass => Lamp::Ok,
        Status::Warn => Lamp::Warn,
        Status::Fail => Lamp::Fail,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Fail-open errors in the last [`RECENT_DAYS`] days.
fn fail_open_recent(paths: &Paths) -> usize {
    let Ok(text) = std::fs::read_to_string(paths.root.join(doctor::FAIL_OPEN_LOG)) else { return 0 };
    let now = now_ms();
    text.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["ts"].as_u64().is_some_and(|t| now.saturating_sub(t) <= RECENT_DAYS * 86_400_000))
        .count()
}

/// The repository key hooks would use for `root`, given the index; else the key `adage learn`
/// would give it.
pub fn repo_key(paths: &Paths, root: &Path, index: &Index) -> String {
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    repo::lookup_key(root, Some(&paths.repo_keys()), &keys)
        .unwrap_or_else(|| repo::repo_key_cached(root, &paths.repo_keys()))
}

/// Repositories a background learner is working on now: `repo` only, or every one in the queue.
/// A learner holds `learn-queue/<slug>.lock` while it runs or waits (live learning).
pub fn learning_now(paths: &Paths, repo: Option<&str>) -> Vec<String> {
    match repo {
        Some(r) => {
            let held = lock::is_held(&Queue::new(paths, r).lock()).unwrap_or(false);
            if held {
                vec![r.to_string()]
            } else {
                vec![]
            }
        }
        None => std::fs::read_dir(paths.learn_queue())
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "lock"))
            .filter(|p| lock::is_held(p).unwrap_or(false))
            .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().to_string()))
            .collect(),
    }
}

/// The fix for a doctor check, when one key can fix it.
fn fix_for(c: &Check) -> Option<Fix> {
    // Auto-learn passes whether it is on or off; off has a one-key fix all the same.
    if c.check == "auto-learn" && c.detail.starts_with("off") {
        return Some(Fix::EnableAutoLearn);
    }
    if c.status == Status::Pass {
        return None;
    }
    if let Some(h) = c.check.strip_prefix("hooks: ") {
        return Some(Fix::InstallHooks(h.to_string()));
    }
    if c.check.starts_with("history") {
        if c.check == "history: opencode" && c.detail.contains("no learning history configured") {
            return Some(Fix::ImportOpenCode);
        }
        return c.check.rsplit_once(": ").map(|(_, harness)| Fix::SetHistorySource(harness.to_string()));
    }
    match c.check.as_str() {
        "embedding model" => Some(Fix::FetchModel),
        "index" | "tracks" | "auto-learn runs" | "auto-learn queue" => Some(Fix::LearnNow),
        "fail-open errors" => Some(Fix::OpenFailOpen),
        "config" => Some(Fix::OpenSettings),
        _ => None,
    }
}

/// Every check, with lamps, for the repository at `root`.
pub fn gather(paths: &Paths, root: Option<&Path>) -> Health {
    let checks = doctor::checks(paths, root);
    let fixes: Vec<Option<Fix>> = checks.iter().map(fix_for).collect();
    let loaded = config::load(paths);
    let auto_learn = loaded.get("learn.auto").map(|(_, src)| (loaded.config.learn_auto, src.to_string()));
    let index = Index::load(&paths.index());
    let generation = std::fs::metadata(paths.index()).is_ok().then(|| index.generation.clone());
    let key = root.map(|r| repo_key(paths, r, &index));
    let routes_repo = key.as_ref().map(|k| index.routes.iter().filter(|r| &r.repo == k).count());
    let fail_open = fail_open_recent(paths);
    let by = |name: &str| checks.iter().find(|c| c.check == name);

    let mut lights = Vec::new();
    lights.push(Light {
        label: format!("adage {}", env!("CARGO_PKG_VERSION")),
        lamp: by("binary").map_or(Lamp::Off, |c| lamp(c.status)),
    });
    // Hooks that would run for this repository: user scope, or registered in it.
    let regs = detect::registrations(root);
    let working: Vec<&str> =
        regs.iter().filter(|r| r.binary.as_deref().is_some_and(detect::executable)).map(|r| r.harness).collect();
    let broken = regs.iter().any(|r| !r.binary.as_deref().is_some_and(detect::executable));
    let installed = detect::harnesses().iter().any(|d| d.found());
    let mut names: Vec<&str> = working.iter().map(|h| super::feed::harness_short(h)).collect();
    names.sort();
    names.dedup();
    lights.push(if broken {
        Light { label: "hooks broken".into(), lamp: Lamp::Fail }
    } else if !names.is_empty() {
        let received = working.iter().all(|h| {
            detect::last_hook_receipt(paths, h, root)
                .is_some_and(|ts| now_ms().saturating_sub(ts) <= RECENT_DAYS * 86_400_000)
        });
        Light {
            label: format!("{} {}", if received { "hooks received" } else { "hooks registered" }, names.join("+")),
            lamp: if received { Lamp::Ok } else { Lamp::Warn },
        }
    } else if installed {
        Light { label: "no hooks".into(), lamp: Lamp::Warn }
    } else {
        Light { label: "no harness".into(), lamp: Lamp::Off }
    });
    let n = routes_repo.unwrap_or(index.routes.len());
    lights.push(Light {
        label: format!("{n} reflex{}", if n == 1 { "" } else { "es" }),
        lamp: by("index").map_or(Lamp::Off, |c| {
            if n == 0 && c.status == Status::Pass {
                Lamp::Warn
            } else {
                lamp(c.status)
            }
        }),
    });
    // Any live-learning check short of pass (a failed run, a stuck learner, a backlog) dims it.
    let learning_trouble = checks.iter().any(|c| c.check.starts_with("auto-learn") && c.status != Status::Pass);
    lights.push(Light {
        label: match &auto_learn {
            Some((true, _)) => "auto-learn on".into(),
            Some((false, _)) => "auto-learn off".into(),
            None => "auto-learn ?".into(),
        },
        lamp: match (&auto_learn, learning_trouble) {
            (_, true) => Lamp::Warn,
            (Some((true, _)), false) => Lamp::Ok,
            _ => Lamp::Off,
        },
    });
    let model = by("embedding model");
    let model_installed = model.is_some_and(|c| c.status == Status::Pass && c.detail.contains("installed;"));
    lights.push(Light {
        label: if model_installed { "model".into() } else { "no model".into() },
        lamp: match model.map(|c| c.status) {
            Some(Status::Pass) if model_installed => Lamp::Ok,
            Some(Status::Pass) | None => Lamp::Off,
            Some(s) => lamp(s),
        },
    });
    lights.push(Light {
        label: format!("fail-open {fail_open}"),
        lamp: if fail_open == 0 { Lamp::Ok } else { Lamp::Warn },
    });
    Health {
        checks,
        fixes,
        lights,
        generation,
        routes_total: index.routes.len(),
        routes_repo,
        repo_key: key,
        auto_learn,
    }
}
