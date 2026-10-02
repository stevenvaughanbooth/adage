//! Health for the live screens: `adage doctor`'s checks (including its configuration and live
//! learning checks), the compact lamps `adage watch` shows, the one-key fix for each problem that
//! has one, and whether a background learner is running.

use super::theme::Lamp;
use crate::autolearn::Queue;
use crate::detect;
use crate::doctor::{self, Check, Status};
use adage_core::repo;
use adage_intent::model;
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
}

impl Fix {
    pub fn label(&self) -> String {
        match self {
            Fix::InstallHooks(h) => format!("install {h} hooks"),
            Fix::FetchModel => "fetch the intent model".into(),
            Fix::LearnNow => "condition this repository now".into(),
            Fix::OpenFailOpen => "open the fail-open log".into(),
            Fix::EnableAutoLearn => "turn auto-condition on".into(),
            Fix::OpenSettings => "open the settings".into(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Light {
    pub label: String,
    pub lamp: Lamp,
    /// The command that fixes it, when it needs one.
    pub hint: Option<String>,
}

impl Light {
    pub fn new(label: impl Into<String>, lamp: Lamp) -> Light {
        Light { label: label.into(), lamp, hint: None }
    }

    fn hint(mut self, cmd: &str) -> Light {
        self.hint = Some(cmd.to_string());
        self
    }
}

#[derive(Debug, Clone, Default)]
pub struct Health {
    pub checks: Vec<Check>,
    /// One per check: its fix, if it needs and has one.
    pub fixes: Vec<Option<Fix>>,
    pub lights: Vec<Light>,
    /// Harnesses whose hooks run for this repository (`claude`, `codex`, …).
    pub harnesses: Vec<String>,
    pub generation: Option<String>,
    pub repo_key: Option<String>,
    /// The repository's savings estimate (`crate::savings`), once it has a ledger.
    pub savings: Option<crate::savings::Summary>,
    /// The notice shown at session start today, if one was (`ui.notices`).
    pub notice: Option<String>,
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

/// The repository key hooks would use for `root`, given the index; else the key `adage condition`
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
        // Registered twice (the plugin and a settings file): installing again cannot fix that;
        // the check's own advice says which entries to remove.
        if h.ends_with(" (twice)") {
            return None;
        }
        return Some(Fix::InstallHooks(h.to_string()));
    }
    match c.check.as_str() {
        // The fix fetches the default model: only when that is what the check asks for (not the
        // reinstall of another installed model, nor a binary whose bundled model is damaged).
        "embedding model" => {
            let fix = c.fix.as_deref().unwrap_or("");
            let other = fix.contains("models fetch ") && !fix.contains(&format!("models fetch {}", model::BASE_8M.key));
            (fix.contains("adage models fetch") && !other && !fix.starts_with("reinstall adage"))
                .then_some(Fix::FetchModel)
        }
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
    if let Some(c) = by("binary").filter(|c| c.status != Status::Pass) {
        lights.push(Light::new(format!("adage {}", env!("CARGO_PKG_VERSION")), lamp(c.status)).hint("adage doctor"));
    }
    // Hooks that would run for this repository: user scope, or registered in it.
    let regs = detect::registrations(root);
    let working: Vec<&str> =
        regs.iter().filter(|r| r.binary.as_deref().is_some_and(detect::executable)).map(|r| r.harness).collect();
    let broken = regs.iter().any(|r| !r.binary.as_deref().is_some_and(detect::executable));
    let installed = detect::harnesses().iter().any(|d| d.found());
    let mut names: Vec<String> = working.iter().map(|h| super::feed::harness_name(h).to_string()).collect();
    names.sort();
    names.dedup();
    if broken {
        lights.push(Light::new("hooks broken", Lamp::Fail).hint("adage doctor"));
    }
    for n in &names {
        lights.push(Light::new(format!("hooks {n}"), Lamp::Ok));
    }
    if names.is_empty() && !broken {
        lights.push(if installed {
            Light::new("no hooks", Lamp::Warn).hint("adage init")
        } else {
            Light::new("no harness found", Lamp::Off)
        });
    }
    let n = routes_repo.unwrap_or(index.routes.len());
    let index_check = by("index");
    lights.push(match index_check.map(|c| c.status) {
        // A fresh install has none yet: nothing is wrong, the first session conditions.
        Some(Status::Pass) if n == 0 => Light::new("no reflexes yet", Lamp::Off).hint("adage condition"),
        Some(Status::Pass) => Light::new(format!("{n} reflex{}", if n == 1 { "" } else { "es" }), Lamp::Ok),
        Some(s) => Light::new("index", lamp(s)).hint("adage condition"),
        None => Light::new("no index", Lamp::Off).hint("adage condition"),
    });
    // Any live-conditioning check short of pass (a failed run, a stuck learner, a backlog) dims it.
    let learning_trouble = checks.iter().any(|c| c.check.starts_with("auto-learn") && c.status != Status::Pass);
    // Recorded, never read: this repository is not one `learn.repos` names.
    let left_out = key.as_deref().is_some_and(|k| !loaded.config.learns(k));
    lights.push(match (&auto_learn, learning_trouble) {
        (_, true) => Light::new("auto-condition", Lamp::Warn).hint("adage doctor"),
        (Some((true, _)), false) if left_out => Light::new("not conditioned here", Lamp::Off).hint("adage init"),
        (Some((true, _)), false) => Light::new("auto-condition", Lamp::Ok),
        _ => Light::new("auto-condition off", Lamp::Off),
    });
    // What hooks embed with: an installed model, the one bundled in the binary, or the hashed
    // fallback (`status_card::embeddings`, as the status card and doctor say it).
    let model = by("embedding model");
    let (embedder, unchosen_fallback) = crate::status_card::embeddings(paths);
    let hashed = embedder == "hashed fallback";
    let label = if hashed {
        "no model"
    } else if embedder.ends_with("(bundled)") {
        "bundled model"
    } else {
        "model"
    };
    lights.push(match model.map(|c| c.status) {
        Some(Status::Pass) if !hashed => Light::new(label, Lamp::Ok),
        Some(Status::Pass) | None => Light::new(label, Lamp::Off),
        // An installed model that cannot be used, while another one serves.
        Some(_) if !hashed && !unchosen_fallback => Light::new(label, Lamp::Warn).hint("adage doctor"),
        Some(s) => Light::new(label, lamp(s)).hint("adage models fetch"),
    });
    if fail_open > 0 {
        lights.push(
            Light::new(format!("{fail_open} fail-open error{}", if fail_open == 1 { "" } else { "s" }), Lamp::Warn)
                .hint("adage doctor"),
        );
    }
    let savings = key.as_deref().and_then(|k| crate::savings::read(paths, k));
    let notice = key.as_deref().and_then(|k| crate::status_card::notice_today(paths, k, now_ms()));
    Health { checks, fixes, lights, harnesses: names, generation, repo_key: key, savings, notice }
}
