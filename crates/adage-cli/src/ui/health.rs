//! Health for the live screens: `adage doctor`'s checks plus a few the UI adds (configuration,
//! auto-learning, a stale learning lock), the compact lamps `adage watch` shows, and the one-key
//! fix for each problem that has one.

use super::config;
use super::learnlog;
use super::theme::Lamp;
use crate::detect;
use crate::doctor::{self, Check, Status};
use adage_core::repo;
use adage_store::Paths;
use adage_tracks::Index;
use std::path::{Path, PathBuf};

/// The lock a running `adage learn` holds (live-learning contract). One older than
/// [`STALE_LOCK_SECS`] is presumed left behind by a learner that died.
pub const LEARN_LOCK: &str = "learn.lock";
pub const STALE_LOCK_SECS: u64 = 30 * 60;
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
    ClearLock(PathBuf),
    OpenFailOpen,
    EnableAutoLearn,
}

impl Fix {
    pub fn label(&self) -> String {
        match self {
            Fix::InstallHooks(h) => format!("install {h} hooks"),
            Fix::FetchModel => "fetch the embedding model".into(),
            Fix::LearnNow => "run learn now".into(),
            Fix::ClearLock(_) => "clear the stale lock".into(),
            Fix::OpenFailOpen => "open the fail-open log".into(),
            Fix::EnableAutoLearn => "turn auto-learn on".into(),
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
    pub auto_learn: Option<bool>,
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

/// The newest learn.log event, if it was an error.
fn last_learn_error(paths: &Paths) -> Option<learnlog::Tick> {
    let mut tail = super::tail::LineTail::new(paths.learn_log());
    let lines = tail.poll();
    let last = lines.iter().rev().find_map(|l| learnlog::parse(l).into_iter().next())?;
    (last.tone == learnlog::Tone::Error).then_some(last)
}

fn extra_checks(paths: &Paths) -> Vec<(Check, Option<Fix>)> {
    let mut out = Vec::new();
    let store = config::TomlFile::in_home(&paths.root);
    let cfg = config::ConfigStore::get(&store, "learn.auto");
    let (auto, cfg_check) = match cfg {
        Ok(v) => {
            let exists = config::ConfigStore::path(&store).exists();
            let detail = if exists {
                format!("{} is valid", detect::tilde(config::ConfigStore::path(&store)))
            } else {
                "no config.toml: Adage's defaults apply".to_string()
            };
            (v.and_then(|v| v.as_bool()), Check { check: "config".into(), status: Status::Pass, detail, fix: None })
        }
        Err(e) => (
            None,
            Check {
                check: "config".into(),
                status: Status::Fail,
                detail: format!("{e:#}"),
                fix: Some(format!("fix {} by hand", detect::tilde(config::ConfigStore::path(&store)))),
            },
        ),
    };
    out.push((cfg_check, None));
    let err = last_learn_error(paths);
    let learn = match (auto, &err) {
        (_, Some(e)) => (
            Check {
                check: "auto-learn".into(),
                status: Status::Warn,
                detail: format!("the last learning run failed: {}", e.text),
                fix: Some("run `adage learn` to see the error in full".into()),
            },
            Some(Fix::LearnNow),
        ),
        (Some(true), None) => (
            Check {
                check: "auto-learn".into(),
                status: Status::Pass,
                detail: "on: Adage learns after agent sessions end".into(),
                fix: None,
            },
            None,
        ),
        (Some(false), None) => (
            Check {
                check: "auto-learn".into(),
                status: Status::Pass,
                detail: "off: routes change only when you run `adage learn`".into(),
                fix: None,
            },
            None,
        ),
        (None, None) => (
            Check {
                check: "auto-learn".into(),
                status: Status::Pass,
                detail: "not set in config.toml (Adage's default applies)".into(),
                fix: None,
            },
            Some(Fix::EnableAutoLearn),
        ),
    };
    out.push(learn);
    let lock = paths.root.join(LEARN_LOCK);
    if let Ok(m) = std::fs::metadata(&lock) {
        let age = m.modified().ok().and_then(|t| t.elapsed().ok()).map(|d| d.as_secs()).unwrap_or(0);
        if age > STALE_LOCK_SECS {
            out.push((
                Check {
                    check: "learn lock".into(),
                    status: Status::Warn,
                    detail: format!(
                        "{} is {} minutes old: a learner probably died holding it, and learning waits on it",
                        detect::tilde(&lock),
                        age / 60
                    ),
                    fix: Some("remove it (press Enter)".into()),
                },
                Some(Fix::ClearLock(lock)),
            ));
        } else {
            out.push((
                Check {
                    check: "learn lock".into(),
                    status: Status::Pass,
                    detail: format!("a learning run is in progress ({age}s)"),
                    fix: None,
                },
                None,
            ));
        }
    }
    out
}

/// The fix for a doctor check, when it is short of pass and one key can fix it.
fn fix_for(c: &Check) -> Option<Fix> {
    if c.status == Status::Pass {
        return None;
    }
    if let Some(h) = c.check.strip_prefix("hooks: ") {
        return Some(Fix::InstallHooks(h.to_string()));
    }
    match c.check.as_str() {
        "embedding model" => Some(Fix::FetchModel),
        "index" | "tracks" => Some(Fix::LearnNow),
        "fail-open errors" => Some(Fix::OpenFailOpen),
        _ => None,
    }
}

/// Every check, with lamps, for the repository at `root`.
pub fn gather(paths: &Paths, root: Option<&Path>) -> Health {
    let mut checks = doctor::checks(paths, root);
    let mut fixes: Vec<Option<Fix>> = checks.iter().map(fix_for).collect();
    let auto_learn = config::auto_learn(&paths.root);
    for (c, f) in extra_checks(paths) {
        checks.push(c);
        fixes.push(f);
    }
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
        Light { label: format!("hooks {}", names.join("+")), lamp: Lamp::Ok }
    } else if installed {
        Light { label: "no hooks".into(), lamp: Lamp::Warn }
    } else {
        Light { label: "no harness".into(), lamp: Lamp::Off }
    });
    let n = routes_repo.unwrap_or(index.routes.len());
    lights.push(Light {
        label: format!("{n} route{}", if n == 1 { "" } else { "s" }),
        lamp: by("index").map_or(Lamp::Off, |c| {
            if n == 0 && c.status == Status::Pass {
                Lamp::Warn
            } else {
                lamp(c.status)
            }
        }),
    });
    let learn_check = by("auto-learn");
    lights.push(Light {
        label: match auto_learn {
            Some(true) => "auto-learn on".into(),
            Some(false) => "auto-learn off".into(),
            None => "auto-learn default".into(),
        },
        lamp: match (auto_learn, learn_check.map(|c| c.status)) {
            (_, Some(Status::Warn | Status::Fail)) => Lamp::Warn,
            (Some(true), _) => Lamp::Ok,
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
