//! `adage doctor [--json]` (plan §6.3): is this machine's Adage set up and healthy?
//!
//! Each check is pass, warn, or fail, with a fix for anything short of pass. The command exits
//! non-zero only when a check fails. Read-only, apart from one probe file created and removed to
//! prove the data directory is writable; agents may run it.
//!
//! Checks: the binary; the data directory; the compiled index (present, intact, current); the
//! installed tracks; the embedding model and its checksums; each harness's hook registrations
//! (and that they run an adage binary that exists); the Claude Code plugin's staged binary; kill
//! switches and experiment settings; the size of the session spools; fail-open errors, if any
//! are recorded in `$ADAGE_HOME/fail-open.log` (one JSON object per line, newest last); the
//! configuration file; and live learning (`crate::autolearn`): whether it is on, each repository's
//! last automatic learn and its outcome, a learner that has held its lock too long, and requests
//! still waiting.

use crate::detect;
use adage_core::repo;
use adage_store::Paths;
use adage_tracks::{index, io, proposals, review, Index, Trigger};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

/// Session spools above this total size get a warning.
const SPOOL_WARN_BYTES: u64 = 256 << 20;
/// Fail-open errors within this many days count as recent.
const RECENT_DAYS: u64 = 7;
pub use crate::failopen::FAIL_OPEN_LOG;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Serialize)]
pub struct Check {
    pub check: String,
    pub status: Status,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

fn pass(check: &str, detail: impl Into<String>) -> Check {
    Check { check: check.into(), status: Status::Pass, detail: detail.into(), fix: None }
}
fn warn(check: &str, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check { check: check.into(), status: Status::Warn, detail: detail.into(), fix: Some(fix.into()) }
}
fn fail(check: &str, detail: impl Into<String>, fix: impl Into<String>) -> Check {
    Check { check: check.into(), status: Status::Fail, detail: detail.into(), fix: Some(fix.into()) }
}

fn sha256_file(p: &Path) -> Option<String> {
    use std::io::Read;
    let mut f = std::fs::File::open(p).ok()?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = f.read(&mut buf).ok()?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Some(io::hex(&h.finalize()))
}

fn binary() -> (Check, Option<PathBuf>) {
    let exe = std::env::current_exe().ok().map(|p| p.canonicalize().unwrap_or(p));
    let at = exe.as_deref().map(|p| format!(" at {}", detect::tilde(p))).unwrap_or_default();
    (pass("binary", format!("adage {}{at}", env!("CARGO_PKG_VERSION"))), exe)
}

fn data_dir(paths: &Paths) -> Check {
    let root = &paths.root;
    let name = "data directory";
    if !root.exists() {
        let ancestor = root.ancestors().skip(1).find(|a| a.is_dir());
        return match ancestor {
            Some(a) if probe(a) => warn(
                name,
                format!("{} does not exist yet", detect::tilde(root)),
                "`adage learn` creates it; nothing to do",
            ),
            _ => fail(
                name,
                format!("{} does not exist and cannot be created", detect::tilde(root)),
                "set ADAGE_HOME to a writable directory",
            ),
        };
    }
    if !root.is_dir() {
        return fail(
            name,
            format!("{} is not a directory", detect::tilde(root)),
            "remove it, or set ADAGE_HOME elsewhere",
        );
    }
    if probe(root) {
        pass(name, format!("{} is writable", detect::tilde(root)))
    } else {
        fail(
            name,
            format!("{} is not writable", detect::tilde(root)),
            format!("fix the permissions of {}, or set ADAGE_HOME", root.display()),
        )
    }
}

/// Can a file be created in `dir`? The probe file is removed at once.
fn probe(dir: &Path) -> bool {
    let p = dir.join(format!(".adage-doctor-{}", std::process::id()));
    let ok = std::fs::OpenOptions::new().write(true).create_new(true).open(&p).is_ok();
    let _ = std::fs::remove_file(&p);
    ok
}

struct TrackScan {
    valid: Vec<adage_tracks::Track>,
    imported: usize,
    invalid: Vec<String>,
}

/// Every track directory under the tracks directory, read and validated (symlinks not followed).
fn scan_tracks(root: &Path) -> TrackScan {
    let mut s = TrackScan { valid: vec![], imported: 0, invalid: vec![] };
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        if d.join("track.toml").exists() {
            let rel = d.strip_prefix(root).unwrap_or(&d).display().to_string();
            let checked = io::read(&d).and_then(|t| {
                review::load(&d)?;
                proposals::load(&d)?;
                Ok(t)
            });
            match checked {
                Ok(t) => {
                    s.imported += usize::from(index::is_imported(&t));
                    s.valid.push(t);
                }
                Err(e) => s.invalid.push(format!("{rel}: {e:#}")),
            }
            continue;
        }
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                stack.push(e.path());
            }
        }
    }
    s
}

fn tracks_check(scan: &TrackScan) -> Check {
    let routes: usize = scan.valid.iter().map(|t| t.routes.len()).sum();
    if !scan.invalid.is_empty() {
        let shown: Vec<&str> = scan.invalid.iter().take(3).map(String::as_str).collect();
        return fail(
            "tracks",
            format!("{} invalid track(s), which hooks ignore: {}", scan.invalid.len(), shown.join("; ")),
            "fix or remove the named track directories, then run `adage learn` to recompile",
        );
    }
    if scan.valid.is_empty() {
        return warn(
            "tracks",
            "no tracks installed",
            "`adage survey` shows what would be learned; `adage learn` builds tracks",
        );
    }
    pass("tracks", format!("{} track(s) ({} imported), {routes} route(s), all valid", scan.valid.len(), scan.imported))
}

fn index_check(paths: &Paths, scan: &TrackScan) -> (Check, Index) {
    let p = paths.index();
    let bytes = match std::fs::read(&p) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (warn("index", "no index yet: hooks have nothing to direct", "run `adage learn`"), Index::default())
        }
        Err(e) => {
            return (
                fail("index", format!("{} cannot be read: {e}", detect::tilde(&p)), "check its permissions"),
                Index::default(),
            )
        }
    };
    let idx: Index = match serde_json::from_slice(&bytes) {
        Ok(i) => i,
        Err(e) => {
            return (
                fail(
                    "index",
                    format!("{} is not a valid index ({e}); hooks fail open and say nothing", detect::tilde(&p)),
                    "run `adage learn` to recompile it",
                ),
                Index::default(),
            )
        }
    };
    let body = serde_json::to_vec(&idx.routes).unwrap_or_default();
    let gen = io::hex(&Sha256::digest(&body))[..16].to_string();
    if gen != idx.generation {
        return (
            fail(
                "index",
                format!(
                    "generation {} does not match its routes ({gen}): it was built by a different Adage version, or edited",
                    idx.generation
                ),
                "run `adage learn` to recompile it",
            ),
            idx,
        );
    }
    let repos: std::collections::BTreeSet<&str> = idx.routes.iter().map(|r| r.repo.as_str()).collect();
    let current = Index::compile(&scan.valid);
    let detail = format!(
        "generation {}, {} route(s) in {} repositor{}",
        idx.generation,
        idx.routes.len(),
        repos.len(),
        if repos.len() == 1 { "y" } else { "ies" }
    );
    if current.generation != idx.generation {
        return (
            warn(
                "index",
                format!("{detail}; stale: the installed tracks compile to {}", current.generation),
                "run `adage learn` (or any `adage review` decision) to recompile",
            ),
            idx,
        );
    }
    (pass("index", detail), idx)
}

fn model_check(paths: &Paths, idx: &Index) -> Check {
    let dir = adage_intent::model::dir_in(&paths.models());
    let wants = idx.routes.iter().any(|r| matches!(r.route.trigger, Trigger::Intent { .. }));
    match adage_intent::model::status(&paths.models()) {
        Ok(m) => pass("embedding model", format!("{} installed; checksums verified", m.id)),
        Err(_) if !dir.join("manifest.json").exists() && wants => warn(
            "embedding model",
            "not installed, so the index's intent routes cannot fire",
            "run `adage models fetch`",
        ),
        Err(_) if !dir.join("manifest.json").exists() => {
            pass("embedding model", "not installed (optional: `adage models fetch` enables intent families)")
        }
        Err(e) => fail(
            "embedding model",
            format!("installed but unusable: {e:#}"),
            "run `adage models fetch` to reinstall it",
        ),
    }
}

fn hooks_checks(repo_root: Option<&Path>, exe: Option<&Path>) -> Vec<Check> {
    let found = detect::harnesses();
    let regs = detect::registrations(repo_root);
    let mut out = Vec::new();
    for d in &found {
        let name = format!("hooks: {}", d.harness);
        let mine: Vec<&detect::Registration> =
            regs.iter().filter(|r| r.harness == d.harness && r.kind != "plugin-data").collect();
        let plugin = regs.iter().any(|r| r.harness == d.harness && r.kind == "plugin-data");
        if mine.is_empty() {
            out.push(match (d.found(), plugin) {
                (_, true) => pass(&name, "registered by the Adage plugin"),
                (true, false) => warn(
                    &name,
                    format!("{} is installed, but Adage's hooks are not registered with it", d.harness),
                    format!("`adage install {} --write` (this repository) or add `--scope user`", d.harness),
                ),
                (false, false) => pass(&name, "harness not installed; nothing to check"),
            });
            continue;
        }
        for r in mine {
            let at = detect::tilde(&r.location);
            match &r.binary {
                None => out.push(fail(
                    &name,
                    format!("{at} names no adage binary"),
                    format!("`adage install {} --write` rewrites it", d.harness),
                )),
                Some(b) if !detect::executable(b) => out.push(fail(
                    &name,
                    format!(
                        "{at} runs {}, which does not exist or is not executable, so its hooks do nothing",
                        detect::tilde(b)
                    ),
                    format!("re-run `adage install {} --write` with this binary (or --bin <path>)", d.harness),
                )),
                Some(b) => {
                    let same = exe.is_some_and(|e| b.canonicalize().ok().as_deref() == Some(e));
                    let note = if same { " (this binary)".to_string() } else { " (another adage binary)".to_string() };
                    out.push(pass(&name, format!("{at} runs {}{note}", detect::tilde(b))));
                }
            }
        }
    }
    out
}

fn plugin_check(repo_root: Option<&Path>, exe: Option<&Path>) -> Option<Check> {
    let regs = detect::registrations(repo_root);
    let r = regs.iter().find(|r| r.kind == "plugin-data")?;
    let name = "claude-code plugin binary";
    let bin = r.binary.as_ref()?;
    let Some(theirs) = sha256_file(bin) else {
        return Some(fail(
            name,
            format!("{} is a broken link", detect::tilde(bin)),
            "start a new Claude Code session so the plugin restages it, or rebuild with plugin/build.sh",
        ));
    };
    let ours = exe.and_then(sha256_file);
    Some(if ours.as_deref() == Some(theirs.as_str()) {
        pass(name, format!("{} matches this binary", detect::tilde(bin)))
    } else {
        warn(
            name,
            format!("{} is a different build (sha256 {}) from this binary", detect::tilde(bin), &theirs[..12]),
            "rebuild and restage with plugin/build.sh, then start a new Claude Code session",
        )
    })
}

fn kill_switches(repo_root: Option<&Path>) -> Check {
    let mut on = Vec::new();
    if std::env::var("ADAGE_DISABLE").as_deref() == Ok("1") {
        on.push("ADAGE_DISABLE=1: every hook does nothing".to_string());
    }
    if let Some(r) = repo_root.filter(|r| r.join(".adage/disabled").exists()) {
        on.push(format!("{}: Adage is off in this repository", detect::tilde(&r.join(".adage/disabled"))));
    }
    if let Ok(v) = std::env::var("ADAGE_HOLDOUT") {
        match v.parse::<f64>() {
            Ok(x) if x >= 1.0 => on.push(format!("ADAGE_HOLDOUT={v}: shadow mode, nothing is delivered")),
            Ok(x) if (0.0..1.0).contains(&x) => {}
            _ => on.push(format!("ADAGE_HOLDOUT={v} is not a share between 0 and 1; the default 0.1 applies")),
        }
    }
    if std::env::var("ADAGE_ARM").as_deref() == Ok("holdout") {
        on.push("ADAGE_ARM=holdout: this shell's sessions receive nothing".into());
    }
    if on.is_empty() {
        pass("kill switches", "none set")
    } else {
        warn("kill switches", on.join("; "), "unset them (or remove the .adage/disabled file) when you want Adage on")
    }
}

fn spools(paths: &Paths) -> Check {
    let (mut bytes, mut sessions) = (0u64, 0usize);
    for e in std::fs::read_dir(paths.sessions()).into_iter().flatten().flatten() {
        if let Ok(m) = e.metadata() {
            bytes += m.len();
        }
        sessions += usize::from(e.path().extension().is_some_and(|x| x == "spool"));
    }
    let mb = bytes as f64 / (1 << 20) as f64;
    let detail = format!("{sessions} session spool(s), {mb:.1} MB in {}", detect::tilde(&paths.sessions()));
    if bytes > SPOOL_WARN_BYTES {
        warn(
            "session spools",
            detail,
            "delete old *.spool and *.state files you no longer need to measure; hooks only append",
        )
    } else {
        pass("session spools", detail)
    }
}

fn fail_open(paths: &Paths) -> Check {
    let p = paths.root.join(FAIL_OPEN_LOG);
    let Ok(text) = std::fs::read_to_string(&p) else { return pass("fail-open errors", "none recorded") };
    let now =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
    let recent: Vec<serde_json::Value> = text
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["ts"].as_u64().is_some_and(|t| now.saturating_sub(t) <= RECENT_DAYS * 86_400_000))
        .collect();
    match recent.last() {
        None => pass("fail-open errors", format!("none in the last {RECENT_DAYS} days")),
        Some(last) => {
            let what: String = last["error"].as_str().unwrap_or("unknown error").chars().take(160).collect();
            warn(
                "fail-open errors",
                format!("{} in the last {RECENT_DAYS} days (hooks said nothing then); latest: {what}", recent.len()),
                format!("report it with the lines from {}", detect::tilde(&p)),
            )
        }
    }
}

/// A learner holding its repository's lock for longer than this is reported as stuck.
const STUCK_MS: u64 = 15 * 60_000;
/// A request still waiting this long after it could have been learned is backlog.
const BACKLOG_SLACK_MS: u64 = 5 * 60_000;

fn ago(now: u64, ts: u64) -> String {
    let s = now.saturating_sub(ts) / 1000;
    match s {
        0..=59 => format!("{s}s ago"),
        60..=3599 => format!("{}m ago", s / 60),
        3600..=172_799 => format!("{}h ago", s / 3600),
        _ => format!("{}d ago", s / 86_400),
    }
}

/// Live learning: the configuration, then per repository in the learn queue its last automatic
/// learn, a stuck learner, and waiting requests.
fn live_learning(paths: &Paths) -> Vec<Check> {
    use crate::autolearn::{read_json, Request, RunState, Spawned};
    let loaded = adage_store::config::load(paths);
    let cfg = &loaded.config;
    let mut out = Vec::new();
    out.push(if loaded.problems.is_empty() {
        let at = detect::tilde(&paths.config());
        pass("config", if paths.config().exists() { format!("{at} is valid") } else { format!("no {at}: defaults") })
    } else {
        warn(
            "config",
            format!("{}: {}", detect::tilde(&paths.config()), loaded.problems.join("; ")),
            "fix it with `adage config set <key> <value>`, or edit the file; `adage config` shows every key",
        )
    });
    let source = &loaded.values["learn.auto"].1;
    out.push(if cfg.learn_auto {
        pass(
            "auto-learn",
            format!(
                "on ({source}): each repository is learned in the background when a session stops, at most every {}s, {} thresholds",
                cfg.learn_min_interval_secs,
                cfg.learn_thresholds.as_str()
            ),
        )
    } else {
        pass("auto-learn", format!("off ({source}): run `adage learn` yourself, or `adage config set learn.auto true`"))
    });

    let now = crate::learnlog::now_ms();
    let dir = paths.learn_queue();
    let mut slugs: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().to_str().map(str::to_string))
        .filter_map(|n| n.strip_suffix(".json").map(str::to_string))
        .filter(|n| !n.contains('.'))
        .collect();
    slugs.sort();
    if slugs.is_empty() {
        return out;
    }
    // The last automatic learn per repository, from learn.log.
    let mut last: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for l in crate::learnlog::read(paths) {
        if l["trigger"] == "auto" {
            if let Some(r) = l["repo"].as_str() {
                last.insert(r.to_string(), l);
            }
        }
    }
    let (mut runs, mut failed, mut stuck, mut backlog) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for slug in &slugs {
        let file = |suffix: &str| dir.join(format!("{slug}{suffix}"));
        let Some(req) = read_json::<Request>(&file(".json")) else { continue };
        let state: Option<RunState> = read_json(&file(".state.json"));
        let spawned: Spawned = read_json(&file(".spawn.json")).unwrap_or_default();
        let held = adage_store::lock::is_held(&file(".lock")).unwrap_or(false);
        let repo = &req.repo;
        match last.get(repo) {
            Some(l) if l["kind"] == "learn_error" => {
                let ts = l["ts"].as_u64().unwrap_or(0);
                let what = l["error"].as_str().unwrap_or("unknown error").to_string();
                failed.push(format!("{repo} ({}, {}: {what})", ago(now, ts), l["class"].as_str().unwrap_or("?")));
            }
            Some(l) => {
                let ts = l["ts"].as_u64().unwrap_or(0);
                let n = l["changes"].as_array().map(|c| c.len()).unwrap_or(0);
                runs.push(format!("{repo} ({}, ok, {n} change(s))", ago(now, ts)));
            }
            // Rotated out of learn.log: the learner's own record of its last run.
            None => match &state {
                Some(s) if s.finished_at.is_some() => {
                    runs.push(format!("{repo} ({}, {})", ago(now, s.finished_at.unwrap_or(0)), s.outcome))
                }
                _ => runs.push(format!("{repo} (not learned automatically yet)")),
            },
        }
        let started = state.as_ref().map(|s| s.started_at).unwrap_or(0);
        let waiting = spawned.after > now;
        if held
            && !waiting
            && state.as_ref().is_some_and(|s| s.outcome == "running")
            && now.saturating_sub(started) > STUCK_MS
        {
            let pid = state.as_ref().map(|s| s.pid).unwrap_or(0);
            stuck.push(format!("{repo} (learner pid {pid}, started {})", ago(now, started)));
        }
        let processed = state.as_ref().map(|s| s.processed).unwrap_or(0);
        let due =
            req.requested_at.max(spawned.after).saturating_add(cfg.learn_min_interval_secs * 1000 + BACKLOG_SLACK_MS);
        if cfg.learn_auto && req.requested_at > processed && !held && now > due {
            backlog.push(format!("{repo} (requested {})", ago(now, req.requested_at)));
        }
    }
    let shown = |v: &[String]| {
        let mut s = v.iter().take(5).cloned().collect::<Vec<_>>().join("; ");
        if v.len() > 5 {
            s.push_str(&format!("; and {} more", v.len() - 5));
        }
        s
    };
    out.push(if failed.is_empty() {
        pass("auto-learn runs", shown(&runs))
    } else {
        warn(
            "auto-learn runs",
            format!("last automatic learn failed for {}", shown(&failed)),
            "run `adage learn` in a terminal to see the error; `adage doctor` passes once a learn succeeds",
        )
    });
    if !stuck.is_empty() {
        out.push(warn(
            "auto-learn lock",
            format!("a learner has held its lock for over {} minutes: {}", STUCK_MS / 60_000, shown(&stuck)),
            "stop that process; the lock is released when it exits and the next session stop starts a new learner",
        ));
    } else {
        out.push(pass("auto-learn lock", "no learner stuck"));
    }
    out.push(if backlog.is_empty() {
        pass(
            "auto-learn queue",
            format!("{} repositor{} queued, none waiting", slugs.len(), if slugs.len() == 1 { "y" } else { "ies" }),
        )
    } else {
        warn(
            "auto-learn queue",
            format!("{} request(s) not learned yet: {}", backlog.len(), shown(&backlog)),
            "`adage learn` learns everything now; if requests keep waiting, check `fail-open.log` (class learn)",
        )
    });
    out
}

pub fn doctor(args: &[String], paths: &Paths) -> Result<()> {
    let repo_root = std::env::current_dir().ok().and_then(|d| repo::find_root(&d));
    let (bin, exe) = binary();
    let scan = scan_tracks(&paths.tracks());
    let (index, idx) = index_check(paths, &scan);
    let mut checks = vec![bin, data_dir(paths), index, tracks_check(&scan), model_check(paths, &idx)];
    checks.extend(hooks_checks(repo_root.as_deref(), exe.as_deref()));
    checks.extend(plugin_check(repo_root.as_deref(), exe.as_deref()));
    checks.push(kill_switches(repo_root.as_deref()));
    checks.push(spools(paths));
    checks.push(fail_open(paths));
    checks.extend(live_learning(paths));

    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let (p, w, f) = (count(Status::Pass), count(Status::Warn), count(Status::Fail));
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "checks": checks, "summary": { "pass": p, "warn": w, "fail": f } }))?
        );
    } else {
        println!("adage doctor");
        for c in &checks {
            let tag = match c.status {
                Status::Pass => "pass",
                Status::Warn => "WARN",
                Status::Fail => "FAIL",
            };
            println!("  {tag}  {:<26} {}", c.check, c.detail);
            if let Some(fix) = &c.fix {
                println!("        {:<26} fix: {fix}", "");
            }
        }
        println!("\n{p} passed, {w} warning(s), {f} failure(s)");
    }
    if f > 0 {
        bail!("{f} check(s) failed");
    }
    Ok(())
}
