//! `adage doctor [--json]` (plan §6.3): is this machine's Adage set up and healthy?
//! `adage doctor --watch` is `adage watch`: the same checks, live, with hook activity.
//!
//! Each check is pass, warn, or fail, with a fix for anything short of pass. The command exits
//! non-zero only when a check fails. Read-only, apart from one probe file created and removed to
//! prove the data directory is writable, and the hook latency measurement it keeps for a few
//! seconds in `cache/` once spools exist ([`latency`]); agents may run it.
//!
//! `--json` gives each check with a fix a structured `action` beside the prose: `{"kind": "run",
//! "command": "adage …"}` when the fix is one `adage` command a surface may offer to run, else
//! `{"kind": "manual"}` (a fix the person makes themselves).
//!
//! Checks: the binary; the data directory; the compiled index (present, intact, current); the
//! installed tracks; the embedding model and its checksums; each harness's hook registrations
//! (and that they run an adage binary that exists); the Claude Code plugin's staged binary; the
//! versions of every Adage binary installed here (`versions`: two that disagree are a warning,
//! never silent; onboarding plan O8); kill
//! switches and experiment settings; the size of the session spools; the hooks' p95 latency on this
//! machine, from the spools' `duration_us` ([`latency`]); fail-open errors, if any
//! are recorded in `$ADAGE_HOME/fail-open.log` (one JSON object per line, newest last); the
//! configuration file; installed packs and their inert routes; and live learning (`crate::autolearn`): whether it is on, each repository's
//! last automatic learn and its outcome, a learner that has held its lock too long, and requests
//! still waiting.

use crate::detect;
use crate::helpers::ago_short as ago;
use adage_core::repo;
use adage_store::Paths;
use adage_tracks::{index, io, proposals, review, Index, Trigger};
use anyhow::{bail, Result};
use serde::Serialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub mod latency;

/// Session spools above this total size get a warning.
const SPOOL_WARN_BYTES: u64 = 256 << 20;
/// Fail-open errors within this many days count as recent.
const RECENT_DAYS: u64 = 7;
pub use crate::failopen::FAIL_OPEN_LOG;

/// A check's lamp. A newer Adage being out is information, never a problem: it stays `Pass`.
/// Two installed Adage versions that disagree are a problem (`Warn`, `versions`), and so is an
/// index this binary cannot read because a newer one wrote it (`Fail`, `index`); `adage update` is
/// the fix for both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Status {
    Pass,
    Warn,
    Fail,
}

#[derive(Debug, Clone)]
pub struct Check {
    /// A stable identifier (the UI and tools match on it); `label` is what people read.
    pub check: String,
    /// The check's name in the lexicon's words (`auto-condition`, not `auto-learn`).
    pub label: String,
    pub status: Status,
    pub detail: String,
    pub fix: Option<Fix>,
}

/// How a check is fixed, typed where the check is made: one `adage` command a surface may offer
/// to run, or something the person does themselves. Everything that says the fix derives from it:
/// doctor's `fix` line ([`Fix::prose`]), its `next`, `--json`'s `action`, the status card's
/// command and the TUI's one-key fixes. Nothing reads a command back out of the prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Fix {
    /// Running `command` (an `adage` command, whole and runnable) fixes it. `how` is the sentence
    /// doctor says it in, `{}` standing for the command in backticks; empty, the fix is the
    /// command alone.
    Run { command: String, how: String },
    /// A fix the person makes themselves (a terminal, a file, a setting, a choice), in prose.
    Manual(String),
}

impl Fix {
    /// The fix is `command` and nothing else.
    pub fn run(command: impl Into<String>) -> Fix {
        let command = command.into();
        debug_assert!(command.starts_with("adage "), "{command:?} is an adage command");
        Fix::Run { command, how: String::new() }
    }

    /// The fix is `command`, said as `how` (`"run {} to recompile it"`).
    pub fn run_in(how: impl Into<String>, command: impl Into<String>) -> Fix {
        let (how, command) = (how.into(), command.into());
        debug_assert!(how.matches("{}").count() == 1, "{how:?} names the command once");
        debug_assert!(command.starts_with("adage "), "{command:?} is an adage command");
        Fix::Run { command, how }
    }

    pub fn manual(text: impl Into<String>) -> Fix {
        Fix::Manual(text.into())
    }

    /// The command to run, for a fix that is one.
    pub fn command(&self) -> Option<&str> {
        match self {
            Fix::Run { command, .. } => Some(command),
            Fix::Manual(_) => None,
        }
    }

    /// The fix as doctor says it.
    pub fn prose(&self) -> String {
        match self {
            Fix::Run { command, how } if how.is_empty() => command.clone(),
            Fix::Run { command, how } => how.replacen("{}", &format!("`{command}`"), 1),
            Fix::Manual(text) => text.clone(),
        }
    }

    /// What a surface acting on it reads (`adage doctor --json`'s `action`).
    pub fn action(&self) -> Action {
        match self {
            Fix::Run { command, .. } => Action::Run { command: command.clone() },
            Fix::Manual(_) => Action::Manual,
        }
    }

    /// What doctor's `next` names: the command, or the person's own fix as said.
    pub fn next(&self) -> String {
        self.command().map_or_else(|| self.prose(), str::to_string)
    }

    /// The words around the command in the lexicon's (`crate::words::prose`); the command is
    /// never touched.
    fn in_words(self) -> Fix {
        match self {
            Fix::Run { command, how } => Fix::Run { command, how: crate::words::prose(&how) },
            Fix::Manual(text) => Fix::Manual(crate::words::prose(&text)),
        }
    }
}

impl std::fmt::Display for Fix {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.prose())
    }
}

/// What a check's fix asks for, for a surface that can act on it (the Claude Code mod's pane): one
/// `adage` command to run, or something the person does themselves. [`Fix`] as JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Action {
    Run { command: String },
    Manual,
}

impl Check {
    /// The command that fixes it, if one does: what the status card names and a pane may run.
    pub fn command(&self) -> Option<&str> {
        self.fix.as_ref().and_then(Fix::command)
    }
}

/// `{check, label, status, detail, fix?, action?}`.
impl Serialize for Check {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut m = s.serialize_map(None)?;
        m.serialize_entry("check", &self.check)?;
        m.serialize_entry("label", &self.label)?;
        m.serialize_entry("status", &self.status)?;
        m.serialize_entry("detail", &self.detail)?;
        if let Some(fix) = &self.fix {
            m.serialize_entry("fix", &fix.prose())?;
            m.serialize_entry("action", &fix.action())?;
        }
        m.end()
    }
}

/// A check's name as people read it.
pub fn label(check: &str) -> String {
    if let Some(rest) = check.strip_prefix("auto-learn") {
        return format!("auto-condition{rest}");
    }
    match check {
        "session spools" => "trace spools".into(),
        "tracks" => "reflex collections".into(),
        other => other.into(),
    }
}

fn pass(check: &str, detail: impl Into<String>) -> Check {
    Check { check: check.into(), label: label(check), status: Status::Pass, detail: detail.into(), fix: None }
}
fn warn(check: &str, detail: impl Into<String>, fix: Fix) -> Check {
    Check { check: check.into(), label: label(check), status: Status::Warn, detail: detail.into(), fix: Some(fix) }
}
fn fail(check: &str, detail: impl Into<String>, fix: Fix) -> Check {
    Check { check: check.into(), label: label(check), status: Status::Fail, detail: detail.into(), fix: Some(fix) }
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

/// This binary's version and path. A newer release (from the release check's cache: no request)
/// is information, never a problem (onboarding plan O6): the check stays `Pass`, says
/// ` · 0.2.0 is out` and carries `adage update` as its action, which doctor's `next` names when
/// nothing else needs fixing.
fn binary(paths: &Paths) -> (Check, Option<PathBuf>) {
    let exe = std::env::current_exe().ok().map(|p| p.canonicalize().unwrap_or(p));
    let at = exe.as_deref().map(|p| format!(" at {}", detect::tilde(p))).unwrap_or_default();
    let mut c = pass("binary", format!("adage {}{at}", env!("CARGO_PKG_VERSION")));
    if let Some(r) = crate::update::newer(paths) {
        c.detail.push_str(&format!(" · {} is out", r.version));
        c.fix = Some(Fix::run("adage update"));
    }
    (c, exe)
}

fn data_dir(paths: &Paths) -> Check {
    let root = &paths.root;
    let name = "data directory";
    if !root.exists() {
        let ancestor = root.ancestors().skip(1).find(|a| a.is_dir());
        return match ancestor {
            // A fresh install: the first session (or `adage condition`) creates it.
            Some(a) if probe(a) => {
                pass(name, format!("{} does not exist yet; your first agent session creates it", detect::tilde(root)))
            }
            _ => fail(
                name,
                format!("{} does not exist and cannot be created", detect::tilde(root)),
                Fix::manual("set ADAGE_HOME to a writable directory"),
            ),
        };
    }
    if !root.is_dir() {
        return fail(
            name,
            format!("{} is not a directory", detect::tilde(root)),
            Fix::manual("remove it, or set ADAGE_HOME elsewhere"),
        );
    }
    if probe(root) {
        pass(name, format!("{} is writable", detect::tilde(root)))
    } else {
        fail(
            name,
            format!("{} is not writable", detect::tilde(root)),
            Fix::manual(format!("fix the permissions of {}, or set ADAGE_HOME", root.display())),
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
            format!("{} invalid reflex collection(s), which hooks ignore: {}", scan.invalid.len(), shown.join("; ")),
            // Recompiling alone does not mend a collection: the person fixes it first.
            Fix::manual(
                "fix or remove the named reflex collection directories, then run `adage condition` to recompile",
            ),
        );
    }
    if scan.valid.is_empty() {
        // Nothing learned yet is the state of every fresh install, not a problem: the first agent
        // session in a repository binds its instincts and learns from its traces.
        return pass(
            "tracks",
            "none yet: your first agent session in a repository starts learning (`adage sense` previews what would form)",
        );
    }
    pass(
        "tracks",
        format!(
            "{} reflex collection(s) ({} imported), {routes} reflex(es), all valid",
            scan.valid.len(),
            scan.imported
        ),
    )
}

fn index_check(paths: &Paths, scan: &TrackScan) -> (Check, Index) {
    let p = paths.index();
    let bytes = match std::fs::read(&p) {
        Ok(b) => b,
        // Nothing installed yet (a fresh install): the first session compiles it.
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && scan.valid.is_empty() => {
            return (pass("index", "none yet: your first agent session in a repository compiles it"), Index::default())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return (
                warn(
                    "index",
                    "no index yet, although tracks are installed: hooks have nothing to direct",
                    Fix::run_in("run {}", "adage condition"),
                ),
                Index::default(),
            )
        }
        Err(e) => {
            return (
                fail(
                    "index",
                    format!("{} cannot be read: {e}", detect::tilde(&p)),
                    Fix::manual("check its permissions"),
                ),
                Index::default(),
            )
        }
    };
    let idx: Index = match serde_json::from_slice(&bytes) {
        Ok(i) => i,
        // A newer Adage wrote it (another binary on this machine): not damage, and recompiling
        // it here would undo that binary's work (docs/live-learning.md).
        Err(_) if index::newer_format(&bytes).is_some() => {
            let format = index::newer_format(&bytes).unwrap_or_default();
            return (
                fail(
                    "index",
                    format!(
                        "{} is newer than this binary (format {format}; this adage reads up to {}): hooks run by this binary say nothing",
                        detect::tilde(&p),
                        index::INDEX_FORMAT
                    ),
                    // This binary is the older one: updating it is the fix (the index is never
                    // rebuilt by an older binary).
                    Fix::run_in("run {} (every adage here must be at least as new as the one that wrote it)", "adage update"),
                ),
                Index::default(),
            );
        }
        Err(e) => {
            return (
                fail(
                    "index",
                    format!("{} is not a valid index ({e}); hooks fail open and say nothing", detect::tilde(&p)),
                    Fix::run_in("run {} to recompile it", "adage condition"),
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
                    "generation {} does not match its reflexes ({gen}): it was built by a different Adage version, or edited",
                    idx.generation
                ),
                Fix::run_in("run {} to recompile it", "adage condition"),
            ),
            idx,
        );
    }
    let repos: std::collections::BTreeSet<&str> = idx.routes.iter().map(|r| r.repo.as_str()).collect();
    let (current, conflicts) = Index::compile_root(&paths.tracks(), &scan.valid);
    let mut detail = format!(
        "generation {}, {} reflex(es) in {} repositor{}",
        idx.generation,
        idx.routes.len(),
        repos.len(),
        if repos.len() == 1 { "y" } else { "ies" }
    );
    // Design §10.6 rule 4: tracks that conflict at one decision point are reported.
    if !conflicts.is_empty() {
        detail.push_str(&format!("; {} interlocking conflict(s) resolved", conflicts.len()));
    }
    if current.generation != idx.generation {
        return (
            warn(
                "index",
                format!("{detail}; stale: the installed tracks compile to {}", current.generation),
                Fix::run_in("run {} (or any `adage review` decision) to recompile", "adage condition"),
            ),
            idx,
        );
    }
    (pass("index", detail), idx)
}

/// Instincts per repository: routes bound from its declared files that the index delivers, those
/// waiting for approval, and those a route from another source overrides.
fn instincts_check(paths: &Paths, scan: &TrackScan, idx: &Index) -> Check {
    let cfg = adage_store::config::load(paths).config;
    let mut per: std::collections::BTreeMap<&str, (usize, usize, usize, usize)> = Default::default();
    for t in scan.valid.iter().filter(|t| index::is_instinct_track(t)) {
        let repo = t.manifest.scope.repo.as_str();
        let e = per.entry(repo).or_default();
        for r in &t.routes {
            let served = idx.routes.iter().find(|x| x.repo == repo && x.route.id == r.id);
            match (r.state, served) {
                (adage_tracks::RouteState::Nascent, _) => e.2 += 1,
                (s, Some(x)) if s.is_live() && x.track == t.manifest.track.name => {
                    if matches!(r.step, adage_tracks::Step::Advise { .. }) {
                        e.1 += 1
                    } else {
                        e.0 += 1
                    }
                }
                (s, _) if s.is_live() => e.3 += 1,
                _ => {}
            }
        }
    }
    let state = if cfg.instincts_enabled { "on" } else { "off" };
    if per.is_empty() {
        return pass(
            "instincts",
            format!(
                "{state}; none bound yet (`adage condition --instincts-only` binds them for a repository on day 0)"
            ),
        );
    }
    let parts: Vec<String> = per
        .iter()
        .map(|(repo, (bound, advice, nascent, over))| {
            format!("{repo}: {bound} bound, {advice} advice, {nascent} awaiting approval, {over} overridden")
        })
        .collect();
    pass("instincts", format!("{state}; {}", parts.join("; ")))
}

/// Installed packs (Track C §2.3): how many, bound to how many repositories, and every route
/// whose closure is not satisfied here, with the reason and the pack's origin and digest.
fn packs_check(paths: &Paths) -> Check {
    let bindings = crate::packs::bindings(paths);
    if bindings.is_empty() {
        return pass("packs", "none installed");
    }
    let packs: std::collections::BTreeSet<&str> = bindings.iter().map(|(_, b)| b.pack.as_str()).collect();
    let live: usize = bindings.iter().map(|(_, b)| b.routes.iter().filter(|r| r.status.is_live()).count()).sum();
    let inert = crate::packs::inert_routes(paths);
    let detail = format!(
        "{} pack(s) bound to {} repositor{}: {live} reflex(es) live, {} inert",
        packs.len(),
        bindings.len(),
        if bindings.len() == 1 { "y" } else { "ies" },
        inert.len()
    );
    if inert.is_empty() {
        return pass("packs", detail);
    }
    let shown: Vec<String> = inert
        .iter()
        .take(5)
        .map(|i| {
            format!(
                "{}@{} (sha256 {}) {} in {}: {}",
                i.pack,
                i.version,
                &i.digest[..i.digest.len().min(12)],
                i.route,
                i.repo,
                i.reason
            )
        })
        .collect();
    let more = if inert.len() > 5 { format!("; and {} more", inert.len() - 5) } else { String::new() };
    warn(
        "packs",
        format!("{detail}; {}{more}", shown.join("; ")),
        Fix::manual(
            "inert reflexes are never delivered; `adage review` lists them. Restore what they need (a script, a tool), then run `adage add` again",
        ),
    )
}

/// Where packs come from (`adage marketplace`): the marketplace built into this binary must be
/// readable, and how its packs stand here is reported. Installing them is learning's job (or
/// `adage marketplace update`'s), so one not installed yet, or installed at other bytes, is not a
/// warning: the next sync puts it right, and binding reads the verified copy in the binary.
fn marketplace_check(paths: &Paths) -> Check {
    use adage_pack::marketplace::Installed;
    let m = match adage_pack::builtin::marketplace() {
        Ok(m) => m,
        Err(e) => {
            return fail(
                "marketplace",
                format!("the marketplace built into Adage cannot be read: {e}"),
                Fix::manual("reinstall Adage: this build's base instincts are damaged"),
            )
        }
    };
    let status = m.status(&paths.tracks());
    let current = status.iter().filter(|(_, i)| *i == Installed::Current).count();
    let which = format!("{} (built into Adage {})", m.name(), env!("CARGO_PKG_VERSION"));
    let packs = crate::words::count(status.len(), "pack");
    pass(
        "marketplace",
        if current == status.len() {
            format!("{which}: {packs}, every one installed as pinned")
        } else {
            format!("{which}: {packs}, {current} installed as pinned; the next condition (or `adage marketplace update`) installs the rest")
        },
    )
}

fn model_check(paths: &Paths, idx: &Index) -> Check {
    use adage_intent::model::{self, Source};
    let wants = idx.routes.iter().any(|r| matches!(r.route.trigger, Trigger::Intent { .. }));
    let r = model::resolve(&paths.models(), false);
    // A download that failed (never retried by itself), said once, with its fix
    // (docs/zero-config.md §5).
    let failed = crate::models::fetch_failed(paths)
        .map(|f| {
            format!(
                "; the last download failed: {}",
                crate::models::explain(
                    f["class"].as_str().unwrap_or("error"),
                    f["host"].as_str().unwrap_or("the model host"),
                    f["detail"].as_str().unwrap_or("")
                )
            )
        })
        .unwrap_or_default();
    // An installed model that cannot be used is a failure even when another one serves.
    for spec in model::SPECS {
        let dir = paths.models().join(spec.key);
        if dir.join("manifest.json").exists() {
            if let Err(e) = model::status_of(&dir) {
                return fail(
                    "embedding model",
                    format!("{} is installed but unusable: {e:#}", spec.key),
                    Fix::run_in("run {} to reinstall it", format!("adage models fetch {}", spec.key)),
                );
            }
        }
    }
    match &r.source {
        Source::Installed { .. } => {
            pass("embedding model", format!("{} installed; checksums verified", r.embedder.id()))
        }
        Source::Bundled => match model::bundled_verified() {
            Ok(()) => pass(
                "embedding model",
                format!("{} bundled in this binary (int8, SHA-256 verified){failed}", r.embedder.id()),
            ),
            // The binary itself is damaged: reinstalling it is the fix; a download only works around
            // it, so it is not offered as the one command to run (nor as the TUI's one-key fetch).
            Err(e) => {
                fail("embedding model", format!("{e:#}"), Fix::manual("reinstall adage, or `adage models fetch`"))
            }
        },
        Source::Hashed { why } if r.selected == "hashed" => {
            pass("embedding model", format!("hashed fallback, as selected ({why}); intent reflexes cannot fire"))
        }
        Source::Hashed { why } => {
            let detail = format!("no embedding model, so the hashed fallback is in use: {why}{failed}");
            if wants {
                warn(
                    "embedding model",
                    format!("{detail}; intent reflexes cannot fire"),
                    Fix::run_in("run {}", "adage models fetch"),
                )
            } else {
                warn(
                    "embedding model",
                    detail,
                    Fix::run_in("run {} (or `adage models use hashed` to choose it)", "adage models fetch"),
                )
            }
        }
    }
}

fn hooks_checks(repo_root: Option<&Path>, exe: Option<&Path>) -> Vec<Check> {
    let found = detect::harnesses();
    let regs = detect::registrations(repo_root);
    // A harness the user set Adage up with (a registration of any kind) is one they opted into.
    // Once they have opted into one, another harness merely being installed is not a problem to
    // warn about (a Claude Code plugin user with Codex installed): only a harness they opted into
    // can be broken. With none set up at all, an installed harness is worth the warning.
    let opted_in = |h: &str| regs.iter().any(|r| r.harness == h);
    let any_opted_in = !regs.is_empty();
    let mut out = Vec::new();
    for d in &found {
        let name = format!("hooks: {}", d.harness);
        let mine: Vec<&detect::Registration> =
            regs.iter().filter(|r| r.harness == d.harness && r.kind != "plugin-data").collect();
        let plugin = regs.iter().any(|r| r.harness == d.harness && r.kind == "plugin-data");
        if mine.is_empty() {
            out.push(match (d.found(), plugin) {
                (_, true) => pass(&name, "registered by the Adage plugin"),
                (true, false) if any_opted_in && !opted_in(d.harness) => pass(
                    &name,
                    format!(
                        "{} is installed; Adage is not set up with it (optional: `adage init` connects it)",
                        d.harness
                    ),
                ),
                (true, false) => warn(
                    &name,
                    format!("{} is installed, but Adage's hooks are not registered with it", d.harness),
                    Fix::run_in("{} connects it", "adage init --yes --hooks user"),
                ),
                (false, false) => pass(&name, "harness not installed; nothing to check"),
            });
            continue;
        }
        // Registered in more than one place that applies here (the Claude Code plugin and a
        // settings file, or a user-scope and a project-scope file): every registration runs, so
        // every hook runs twice and each notice the person sees appears twice.
        if let Some(dup) = crate::install::duplicates(&regs, repo_root).into_iter().find(|x| x.harness == d.harness) {
            let files: Vec<&String> = dup.places.iter().filter(|p| *p != "the Adage plugin").collect();
            let at = files.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(" and in ");
            let detail = if dup.plugin {
                format!(
                    "registered by the Adage plugin and in {at}: every hook runs twice, and each notice appears twice"
                )
            } else {
                format!("registered in {at}: every hook runs twice, and each notice appears twice")
            };
            // Which copy to keep is the person's choice, and removing one edits their settings:
            // never one command to run for them.
            let fix = if dup.plugin { format!("{}; or uninstall the plugin", dup.fix) } else { dup.fix };
            out.push(warn(&format!("{name} (twice)"), detail, Fix::manual(fix)));
        }
        // Out of date: an event this version uses that no registration has. Registrations add up
        // (the harness runs every settings file's hooks), so only what all of them lack is missing.
        let have: std::collections::BTreeSet<&str> =
            mine.iter().filter(|r| r.kind == "settings").flat_map(|r| r.events.iter().map(String::as_str)).collect();
        let missing: Vec<String> =
            detect::wanted_events(d.harness).into_iter().filter(|e| !have.contains(e.as_str())).collect();
        if !have.is_empty() && !missing.is_empty() {
            let user =
                mine.iter().any(|r| r.kind == "settings" && repo_root.is_none_or(|root| !r.location.starts_with(root)));
            let them = if missing.len() == 1 { "it" } else { "them" };
            out.push(warn(
                &format!("{name} (out of date)"),
                format!("the registered hooks lack {}, which this version of Adage uses", missing.join(", ")),
                Fix::run_in(
                    format!("{{}} adds {them}"),
                    format!("adage install {}{} --write", d.harness, if user { " --scope user" } else { "" }),
                ),
            ));
        }
        for r in mine {
            let at = detect::tilde(&r.location);
            match &r.binary {
                None => out.push(fail(
                    &name,
                    format!("{at} names no adage binary"),
                    Fix::run_in("{} rewrites it", format!("adage install {} --write", d.harness)),
                )),
                Some(b) if !detect::executable(b) => out.push(fail(
                    &name,
                    format!(
                        "{at} runs {}, which does not exist or is not executable, so its hooks do nothing",
                        detect::tilde(b)
                    ),
                    // Run by this binary, the install names this binary: the command is the fix.
                    Fix::run_in(
                        "re-run {} with this binary (or --bin <path>)",
                        format!("adage install {} --write", d.harness),
                    ),
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

/// How many directory entries the cloud scan reads, for the work tree and again for the git object
/// store: enough to see the pattern, cheap enough for `adage ui`, which re-runs the checks every few
/// seconds.
const CLOUD_SCAN: usize = 2_000;

/// This repository's files a cloud drive has offloaded (`adage_core::cloud`). Adage skips them
/// rather than wait for a download, so it says so once, with the fix; no line when there are none.
fn cloud_check(repo_root: Option<&Path>) -> Option<Check> {
    cloud_check_with(repo_root?, &adage_core::cloud::is_offloaded)
}

fn cloud_check_with(root: &Path, offloaded: &dyn Fn(&Path) -> bool) -> Option<Check> {
    let sorted = |dir: &Path| {
        let mut v: Vec<std::fs::DirEntry> = std::fs::read_dir(dir).map(|d| d.flatten().collect()).unwrap_or_default();
        v.sort_by_key(|e| e.file_name());
        v
    };
    // The work tree to the depth Adage reads it (manifests, CI workflows), skipping hidden
    // directories other than `.github` and build output, as the instincts' walk does.
    let (mut files, mut seen) = (Vec::new(), 0usize);
    let mut stack = vec![(root.to_path_buf(), 0usize)];
    while let Some((dir, depth)) = stack.pop() {
        for e in sorted(&dir) {
            if seen == CLOUD_SCAN {
                break;
            }
            seen += 1;
            let (p, name) = (e.path(), e.file_name().to_string_lossy().into_owned());
            match e.file_type() {
                Ok(t) if t.is_dir() => {
                    let skip = (name.starts_with('.') && name != ".github")
                        || matches!(name.as_str(), "target" | "node_modules");
                    if depth < 2 && !skip {
                        stack.push((p, depth + 1));
                    }
                }
                Ok(t) if t.is_file() && offloaded(&p) => {
                    files.push(p.strip_prefix(root).unwrap_or(&p).to_string_lossy().into_owned());
                }
                _ => {}
            }
        }
    }
    // The object store: loose objects and packs, in the common git directory (a linked worktree's
    // objects are its main checkout's).
    let (mut objects, mut read) = (0usize, 0usize);
    // Normalised: a worktree's `commondir` is relative (`../..`).
    let common = adage_core::gitobj::dirs(root).map(|(_, common)| repo::lexical_normalise(&common));
    if let Some(common) = &common {
        'store: for d in sorted(&common.join("objects")) {
            for o in sorted(&d.path()) {
                if read == CLOUD_SCAN {
                    break 'store;
                }
                read += 1;
                objects += usize::from(offloaded(&o.path()));
            }
        }
    }
    if files.is_empty() && objects == 0 {
        return None;
    }
    let plural = |n: usize, one: &str| if n == 1 { format!("1 {one}") } else { format!("{n} {one}s") };
    let mut what = Vec::new();
    if objects > 0 {
        what.push(plural(objects, "git object"));
    }
    if !files.is_empty() {
        what.push(plural(files.len(), "file"));
    }
    let example = files.first().map(|f| format!(" (such as {f})")).unwrap_or_default();
    let at_least = if seen == CLOUD_SCAN || read == CLOUD_SCAN { "at least " } else { "" };
    // The folder to move or keep downloaded: the main checkout, which a linked worktree's objects
    // live in (and, in Claude Code's `.claude/worktrees/`, the worktree itself).
    let repository =
        common.as_deref().filter(|c| c.file_name().is_some_and(|n| n == ".git")).and_then(Path::parent).unwrap_or(root);
    Some(warn(
        "cloud files",
        format!(
            "a cloud drive (iCloud Drive or another sync app) keeps {at_least}{} of the repository at {} only in the cloud{example}; Adage skips those instead of waiting for a download, so it sees less of this repository",
            what.join(" and "),
            detect::tilde(repository)
        ),
        Fix::manual(
            "move the repository out of the synced folder (for example to ~/code), or in Finder Control-click its folder and choose Keep Downloaded",
        ),
    ))
}

/// The plugin installed from `adage init` must follow this terminal installation byte for byte.
/// Published plugins may intentionally be independent builds; never apply this rule to them.
fn managed_plugin_check(paths: &Paths, repo_root: Option<&Path>, exe: Option<&Path>) -> Option<Check> {
    let exe = exe.filter(|p| !adage_store::release::plugin_binary(p))?;
    let managed =
        detect::claude_plugins(repo_root).into_iter().find(|p| p.enabled && p.id == crate::claude_plugin::PLUGIN_ID)?;
    let name = "claude-code plugin binary";
    if !paths.plugin().join(".claude-plugin/marketplace.json").is_file() {
        return Some(warn(
            name,
            "the locally installed Claude Code plugin is missing its staged marketplace",
            Fix::run("adage init"),
        ));
    }
    if !crate::claude_plugin::staged_matches(paths, exe) {
        return Some(warn(
            name,
            "the local Claude Code plugin differs from this installation; the same version number can contain different builds",
            Fix::run_in("{} refreshes the local plugin's binaries and modules; then start a new Claude Code session", "adage update"),
        ));
    }
    let data = detect::claude_dir()?.join("plugins/data").join(managed.data_dir_name()).join("bin");
    let staged = crate::claude_plugin::plugin_dir(paths).join("dist").join(crate::claude_plugin::target());
    // SessionStart links these two into plugin data. Training remains in the plugin's dist tree.
    for name in ["adage", "adage-exec"] {
        let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let active = data.join(&file);
        if active.symlink_metadata().is_err() {
            continue;
        }
        let matches = sha256_file(&active).zip(sha256_file(&staged.join(file))).is_some_and(|(a, b)| a == b);
        if !matches {
            return Some(warn(
                "claude-code plugin binary",
                format!("the staged local plugin is current, but Claude Code's {name} still uses a different build"),
                Fix::manual("start a new Claude Code session to load the refreshed local plugin"),
            ));
        }
    }
    Some(pass(name, "the local Claude Code plugin's binaries and modules match this installation"))
}

/// Check the managed local plugin first, then independently installed plugin binaries. Different
/// builds of one version are expected for a published plugin (a release and `cargo install` of
/// the same tag); only a developer checkout loaded with `--plugin-dir` is told to rebuild.
fn plugin_check(paths: &Paths, repo_root: Option<&Path>, exe: Option<&Path>) -> Option<Check> {
    if let Some(check) = managed_plugin_check(paths, repo_root, exe) {
        return Some(check);
    }
    let regs = detect::registrations(repo_root);
    let r = regs.iter().find(|r| r.kind == "plugin-data")?;
    let name = "claude-code plugin binary";
    let bin = r.binary.as_ref()?;
    let at = detect::tilde(bin);
    let Some(theirs) = sha256_file(bin) else {
        return Some(fail(
            name,
            format!("{at} is a broken link"),
            Fix::manual("start a new Claude Code session (the plugin relinks its binary)"),
        ));
    };
    let ours = exe.and_then(sha256_file);
    if ours.as_deref() == Some(theirs.as_str()) {
        return Some(pass(name, format!("{at} matches this binary")));
    }
    let version =
        crate::update::probe::askable(bin, repo_root).then(|| crate::update::probe::version_of(bin)).flatten();
    let me = adage_store::release::VERSION;
    Some(match version {
        Some(v) if v != me => warn(
            name,
            format!("{at} is adage {v}; this binary is adage {me}"),
            skew_fix(paths, &[Found { at: bin.clone(), version: v, plugin: true }]),
        ),
        Some(v) if plugin_dir_checkout() => warn(
            name,
            format!("{at} is another build of adage {v} (sha256 {}) than this binary", &theirs[..12]),
            Fix::manual("rebuild and restage with plugin/build.sh, then start a new Claude Code session"),
        ),
        Some(v) => pass(name, format!("{at} is adage {v}, as this binary (another build of it)")),
        None => warn(
            name,
            format!(
                "{at} is a different build (sha256 {}) from this binary, and did not say its version",
                &theirs[..12]
            ),
            Fix::manual("start a new Claude Code session (the plugin relinks its binary)"),
        ),
    })
}

/// No Claude Code marketplace lists Adage as installed (`plugins/installed_plugins.json`): the
/// plugin's data comes from a checkout loaded with `--plugin-dir`.
fn plugin_dir_checkout() -> bool {
    let Some(file) = detect::claude_dir().map(|d| d.join("plugins/installed_plugins.json")) else { return true };
    let Ok(bytes) = std::fs::read(file) else { return true };
    let doc: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();
    !doc["plugins"].as_object().is_some_and(|m| m.keys().any(|k| k.split('@').next() == Some("adage")))
}

/// An installed Adage binary and the version it answered.
struct Found {
    at: PathBuf,
    version: String,
    /// The Claude Code plugin's staged binary.
    plugin: bool,
}

/// The fix when installed versions disagree: `adage update` when this binary is older than one of
/// them (it updates this one, and with it the plugin `adage init` staged); when this binary is the
/// newest, what updates each older one.
fn skew_fix(paths: &Paths, others: &[Found]) -> Fix {
    use adage_store::release::{is_newer, VERSION};
    if others.iter().any(|f| is_newer(&f.version, VERSION)) {
        return Fix::run("adage update");
    }
    let Some(old) = others.iter().find(|f| is_newer(VERSION, &f.version)) else {
        return Fix::run("adage update");
    };
    if old.plugin {
        // The plugin `adage init` staged from this install follows it; the published one is
        // updated by Claude Code.
        return if paths.plugin().join(".claude-plugin/marketplace.json").is_file() {
            Fix::run_in("{} stages the plugin again from this binary", "adage update")
        } else {
            Fix::manual("in Claude Code: /plugin, choose adage, then Update (then start a new session)")
        };
    }
    Fix::manual(format!(
        "update the adage at {} the way it was installed (`{} update`), or remove it",
        detect::tilde(&old.at),
        detect::command_for(&old.at)
    ))
}

/// `versions`: every Adage binary installed here (this one, `PATH`'s `adage`, the Claude Code
/// plugin's, and each one a registration runs) answers the same version. Only binaries that look
/// like Adage's, outside this repository, are asked (`update::probe::askable`).
fn versions_check(paths: &Paths, repo_root: Option<&Path>, exe: Option<&Path>) -> Check {
    use crate::update::probe;
    let me = adage_store::release::VERSION;
    let mut seen: Vec<PathBuf> = exe.map(|e| e.to_path_buf()).into_iter().collect();
    let mut found: Vec<Found> = Vec::new();
    let mut candidates: Vec<(PathBuf, bool)> = Vec::new();
    candidates.extend(detect::on_path(&format!("adage{}", std::env::consts::EXE_SUFFIX)).map(|p| (p, false)));
    for r in detect::registrations(repo_root) {
        if let Some(b) = r.binary {
            candidates.push((b, r.kind == "plugin-data"));
        }
    }
    for (bin, plugin) in candidates {
        let resolved = bin.canonicalize().unwrap_or_else(|_| bin.clone());
        if seen.contains(&resolved) || !probe::askable(&bin, repo_root) {
            continue;
        }
        seen.push(resolved);
        if let Some(version) = probe::version_of(&bin) {
            found.push(Found { at: bin, version, plugin });
        }
    }
    let differ: Vec<&Found> = found.iter().filter(|f| f.version != me).collect();
    let describe = |f: &Found| {
        let whose = if f.plugin { " (the Claude Code plugin's)" } else { "" };
        format!("adage {} at {}{whose}", f.version, detect::tilde(&f.at))
    };
    if differ.is_empty() {
        let also = if found.is_empty() {
            "no other adage is installed here".to_string()
        } else {
            format!(
                "as is every other adage here: {}",
                found.iter().map(|f| detect::tilde(&f.at)).collect::<Vec<_>>().join(", ")
            )
        };
        return pass("versions", format!("this binary is adage {me}; {also}"));
    }
    warn(
        "versions",
        format!(
            "this binary is adage {me}, but {}: hooks may run one version while you read another",
            differ.iter().map(|f| describe(f)).collect::<Vec<_>>().join("; ")
        ),
        skew_fix(paths, &found),
    )
}

fn kill_switches(repo_root: Option<&Path>) -> Check {
    let mut on = Vec::new();
    if std::env::var("ADAGE_DISABLE").as_deref() == Ok("1") {
        on.push("ADAGE_DISABLE=1: every hook does nothing".to_string());
    }
    let off_here = repo_root.is_some_and(|r| crate::switch::marker(r).exists());
    if let Some(r) = repo_root.filter(|_| off_here) {
        on.push(format!("{}: Adage is off in this repository", detect::tilde(&crate::switch::marker(r))));
    }
    if let Ok(v) = std::env::var("ADAGE_CONTROL") {
        match v.parse::<f64>() {
            Ok(x) if x >= 1.0 => on.push(format!("ADAGE_CONTROL={v}: shadow mode, nothing is delivered")),
            Ok(x) if (0.0..1.0).contains(&x) => {}
            _ => on.push(format!("ADAGE_CONTROL={v} is not a share between 0 and 1; the default 0.1 applies")),
        }
    }
    if std::env::var("ADAGE_ARM").as_deref() == Ok("control") {
        on.push("ADAGE_ARM=control: this shell's sessions receive nothing".into());
    }
    if on.is_empty() {
        pass("kill switches", "none set")
    } else {
        // `adage on` turns a repository's switch back (a person's decision); the variables are the
        // shell's own.
        let fix = match (off_here, on.len()) {
            (true, 1) => Fix::run("adage on"),
            (true, _) => Fix::manual("unset the variables, and run `adage on` here, when you want Adage on"),
            (false, _) => Fix::manual("unset them when you want Adage on"),
        };
        warn("kill switches", on.join("; "), fix)
    }
}

fn spools(paths: &Paths) -> Check {
    let (mut bytes, mut sessions, mut newest) = (0u64, 0usize, None);
    for e in std::fs::read_dir(paths.sessions()).into_iter().flatten().flatten() {
        let Ok(m) = e.metadata() else { continue };
        bytes += m.len();
        if e.path().extension().is_some_and(|x| x == "spool") {
            sessions += 1;
            newest = newest.max(m.modified().ok());
        }
    }
    let mb = bytes as f64 / (1 << 20) as f64;
    // Evidence that hooks fire (docs/design/everyday-ux.md, rule 1): a session last reached Adage
    // then, in any repository.
    let heard = match newest.and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()) {
        Some(t) => format!("; the newest {}", ago(crate::learnlog::now_ms(), t.as_millis() as u64)),
        None => "; no session has reached Adage yet".into(),
    };
    let detail = format!("{sessions} trace spool(s), {mb:.1} MB in {}{heard}", detect::tilde(&paths.sessions()));
    if bytes > SPOOL_WARN_BYTES {
        warn(
            "session spools",
            detail,
            Fix::manual("delete old *.spool and *.state files you no longer need to measure; hooks only append"),
        )
    } else {
        pass("session spools", detail)
    }
}

fn fail_open(paths: &Paths) -> Check {
    let p = paths.root.join(FAIL_OPEN_LOG);
    let Ok(text) = std::fs::read_to_string(&p) else { return pass("fail-open errors", "none recorded") };
    let now = adage_core::time::now_ms();
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
                Fix::manual(format!("report it with the lines from {}", detect::tilde(&p))),
            )
        }
    }
}

/// A request still waiting this long after it could have been learned is backlog.
const BACKLOG_SLACK_MS: u64 = 5 * 60_000;

/// The subject for the repositories `learn.repos` names: `the repository you chose (a; learn.repos)
/// is`, `the 3 repositories you chose (a, b, c; learn.repos) are`.
fn chosen(keys: &[String]) -> String {
    match keys {
        [one] => format!("the repository you chose ({one}; learn.repos) is"),
        _ if keys.len() <= 3 => {
            format!("the {} repositories you chose ({}; learn.repos) are", keys.len(), keys.join(", "))
        }
        _ => format!("the {} repositories you chose ({}, …; learn.repos) are", keys.len(), keys[..2].join(", ")),
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
            // `<key> <value>` are the person's to fill in: not a command to run as it stands.
            Fix::manual(
                "fix it with `adage config set <key> <value>`, or edit the file; `adage config` shows every key",
            ),
        )
    });
    let source = &loaded.values["learn.auto"].1;
    out.push(if cfg.learn_auto {
        let which = match &cfg.learn_repos {
            None => "each repository is".to_string(),
            Some(keys) if keys.is_empty() => "no repository is (learn.repos is empty; `adage init` picks some), so none is".to_string(),
            Some(keys) => chosen(keys),
        };
        pass(
            "auto-learn",
            format!(
                "on ({source}): {which} conditioned in the background when a session stops, at most every {}s, {} thresholds",
                cfg.learn_min_interval_secs,
                cfg.learn_thresholds.as_str()
            ),
        )
    } else {
        pass("auto-learn", format!("off ({source}): run `adage condition` yourself, or `adage config set learn.auto true`"))
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
    let log = crate::learnlog::read(paths);
    let mut last: std::collections::BTreeMap<String, serde_json::Value> = Default::default();
    for l in &log {
        if l["trigger"] == "auto" {
            if let Some(r) = l["repo"].as_str() {
                last.insert(r.to_string(), l.clone());
            }
        }
    }
    let (mut runs, mut failed, mut stuck, mut backlog) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for slug in &slugs {
        let file = |suffix: &str| dir.join(format!("{slug}{suffix}"));
        let Some(req) = read_json::<Request>(&file(".json")) else { continue };
        // A repository taken out of `learn.repos`: its requests are never served, by choice.
        if !cfg.learns(&req.repo) {
            continue;
        }
        // A request for a directory that is not (or no longer) a repository — a session opened in
        // `~` or `/` by an older Adage, a checkout since deleted — can never be conditioned: it is
        // not a problem to fix.
        if !Path::new(&req.root).join(".git").exists() {
            continue;
        }
        let state: Option<RunState> = read_json(&file(".state.json"));
        let spawned: Spawned = read_json(&file(".spawn.json")).unwrap_or_default();
        let held = adage_store::lock::is_held(&file(".lock")).unwrap_or(false);
        let repo = &req.repo;
        match last.get(repo) {
            // `learn_error`: learn.log lines written before the lexicon's rename.
            Some(l) if l["kind"] == "condition_error" || l["kind"] == "learn_error" => {
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
                _ => runs.push(format!("{repo} (not conditioned automatically yet)")),
            },
        }
        // Stuck relative to how long a run usually takes (`crate::learning`); a learner waiting for
        // its interval to end has not started.
        let learning = crate::learning::of(paths, repo, &log);
        let is_stuck = spawned.after <= now && learning.stuck(now);
        if is_stuck {
            let since = learning.running_since.unwrap_or(now);
            let usual = learning
                .usual_ms
                .map_or("no run has finished yet".to_string(), |u| format!("usually {}", crate::learning::duration(u)));
            stuck.push(format!(
                "{repo}: running for {} ({usual}), learner pid {}",
                crate::learning::duration(now.saturating_sub(since)),
                learning.pid.unwrap_or(0)
            ));
        }
        // A request behind a learner that holds the lock is that learner's: if it is stuck, the
        // lock check says so (one problem, one line), not the queue.
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
            format!("last automatic conditioning failed for {}", shown(&failed)),
            // Run to read its error, not to fix it: the error says what the fix is.
            Fix::manual(
                "run `adage condition` in a terminal to see the error; `adage doctor` passes once conditioning succeeds",
            ),
        )
    });
    if !stuck.is_empty() {
        out.push(warn(
            "auto-learn lock",
            format!("learning is taking far longer than it usually does: {}", shown(&stuck)),
            Fix::manual(
                "stop that learner (kill <pid>); the next session that ends starts a new one. A learner usually stalls reading files only in the cloud (see cloud files)",
            ),
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
            format!("{} request(s) not conditioned yet: {}", backlog.len(), shown(&backlog)),
            Fix::run_in(
                "{} conditions everything now; if requests keep waiting, check `fail-open.log` (class condition)",
                "adage condition",
            ),
        )
    });
    out
}

/// Whether `PreToolUse` rewrites are on: `runtime.rewrite`, and the `exec/rewrite-off` marker an
/// older `adage delegate off` wrote (either one turns them off). It passes either way; its fix is
/// the switch the other way.
pub fn rewrites_check(paths: &Paths) -> Check {
    let on = adage_store::config::load(paths).config.runtime_rewrite;
    let marker = paths.rewrite_off().exists();
    let (detail, fix) = match (on, marker) {
        (true, false) => ("python tests → .venv (on)".to_string(), "adage delegate rewrites off"),
        (false, _) => (
            "off (runtime.rewrite = false): commands run as the agent wrote them".to_string(),
            "adage delegate rewrites on",
        ),
        (true, true) => (
            "off (the exec/rewrite-off marker): commands run as the agent wrote them".to_string(),
            "adage delegate rewrites on",
        ),
    };
    Check { fix: Some(Fix::run(fix)), ..pass("rewrites", detail) }
}

/// Whether each registered harness's hooks have reached Adage in this repository: the receipt the
/// hook writes on delivery (`adage_hook::receipt`). Registration alone does not prove it. Once one
/// harness has delivered here, another that has not is a problem only if the person used it here
/// since its hooks were registered ([`used_here_since`]); a harness they do not use here is not.
fn hook_delivery_checks(paths: &Paths, root: Option<&Path>) -> Vec<Check> {
    let Some(root) = root else { return Vec::new() };
    let registered = detect::registrations(Some(root));
    let harnesses = ["claude-code", "codex", "opencode"];
    let any_delivered = harnesses.iter().any(|h| adage_hook::receipt::read(paths, root, h).is_some());
    let mut checks = Vec::new();
    for harness in harnesses {
        let Some(registration) = registered.iter().find(|r| r.harness == harness) else { continue };
        let name = format!("hook activity: {harness}");
        match adage_hook::receipt::read(paths, root, harness) {
            Some(receipt) => checks.push(pass(
                &name,
                format!(
                    "{} sends its hooks here: last received {} (refreshed about once per minute); first received {} UTC. Registration and receipt do not prove a reflex fired.",
                    harness_name(harness),
                    ago(crate::learnlog::now_ms(), receipt.last_seen()),
                    review::civil_date((receipt.first_seen_ms / 86_400_000) as i64)
                ),
            )),
            None if any_delivered && !used_here_since(harness, root, modified_ms(&registration.location)) => {
                checks.push(pass(&name, format!("{} is set up but not used in this repository yet; Adage starts when you use it here.", harness_name(harness))))
            }
            // Only the person can make a hook fire: no adage command delivers one.
            None => checks.push(warn(
                &name,
                // Plain words first: what is true, then (in the fix) what only the person can do.
                format!("{} is set up but hasn't sent anything from this repository yet.", harness_name(harness)),
                Fix::manual(if harness == "codex" {
                    "Open Codex in this repository, use /hooks to review and accept Adage's hooks, then send a prompt. This acceptance happens inside Codex; Adage cannot do it for you."
                } else {
                    "Start a new agent session in this repository and send a prompt, then refresh Health."
                }),
            )),
        }
    }
    checks
}

/// Whether the person ran `harness` in this repository (any checkout of it) after `since_ms`: a
/// session it recorded here, newer than the registration. Codex rollouts and Claude Code
/// transcripts are read for their recorded working directory (only those written since); OpenCode
/// keeps no per-session history Adage can read without an export, so it is never counted as used.
fn used_here_since(harness: &str, root: &Path, since_ms: u64) -> bool {
    use adage_learn::scope;
    let roots = scope::checkouts(root);
    let since = |p: &PathBuf| modified_ms(p) > since_ms;
    match harness {
        "codex" => detect::codex_dir().is_some_and(|d| {
            let (files, _) = adage_learn::codex::discover(&d.join("sessions"));
            !scope::recorded_inside(files.into_iter().filter(since).collect(), &roots).is_empty()
        }),
        "claude-code" => detect::claude_dir()
            .is_some_and(|d| scope::claude_transcripts(&d.join("projects"), &roots).iter().any(since)),
        _ => false,
    }
}

fn harness_name(harness: &str) -> &str {
    match harness {
        "claude-code" => "Claude Code",
        "codex" => "Codex",
        "opencode" => "OpenCode",
        other => other,
    }
}

/// When a file was last written, in milliseconds since the Unix epoch (0 if unknown).
fn modified_ms(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

/// Every check, in display order, for the repository at `repo_root` (if any). `adage watch` and
/// `adage ui` re-run these every few seconds.
pub fn checks(paths: &Paths, repo_root: Option<&Path>) -> Vec<Check> {
    let (bin, exe) = binary(paths);
    let scan = scan_tracks(&paths.tracks());
    let (index, idx) = index_check(paths, &scan);
    let mut checks = vec![
        bin,
        data_dir(paths),
        index,
        tracks_check(&scan),
        instincts_check(paths, &scan, &idx),
        packs_check(paths),
        marketplace_check(paths),
        model_check(paths, &idx),
    ];
    checks.extend(hooks_checks(repo_root, exe.as_deref()));
    checks.extend(hook_delivery_checks(paths, repo_root));
    checks.extend(plugin_check(paths, repo_root, exe.as_deref()));
    checks.push(versions_check(paths, repo_root, exe.as_deref()));
    checks.extend(cloud_check(repo_root));
    checks.push(kill_switches(repo_root));
    checks.push(rewrites_check(paths));
    checks.push(spools(paths));
    checks.push(latency::check(&latency::measured(paths, crate::learnlog::now_ms())));
    checks.push(fail_open(paths));
    checks.extend(live_learning(paths));
    // What people read is in the lexicon's words; the check ids stay as they are.
    for c in &mut checks {
        c.detail = crate::words::prose(&c.detail);
        c.fix = c.fix.take().map(Fix::in_words);
    }
    checks
}

/// `adage doctor`'s report in the output grammar: a lamp row per check, its fix as a command
/// under it, and `next` for the first thing to fix.
pub fn render<W: std::io::Write>(o: &mut adage_term::Out<W>, checks: &[Check]) -> std::io::Result<()> {
    use adage_term::{Glyph, Line, Role};
    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let (p, w, f) = (count(Status::Pass), count(Status::Warn), count(Status::Fail));
    let summary = format!("{p} passed · {w} to look at · {f} failing");
    o.header("doctor", &[&summary])?;
    o.blank()?;
    let name_w = checks.iter().map(|c| c.label.chars().count()).max().unwrap_or(10).clamp(10, 26);
    for c in checks {
        let (role, glyph, word) = match c.status {
            Status::Pass => (Role::Fire, Glyph::Ok, "PASS"),
            Status::Warn => (Role::Prime, Glyph::Lamp, "WARN"),
            Status::Fail => (Role::Inhibit, Glyph::Fail, "FAIL"),
        };
        let mark = if o.theme.color() { Line::new().glyph(role, glyph) } else { Line::new().push(role, word) };
        o.line(
            &Line::new()
                .text("  ")
                .extend(mark)
                .pad_to(8)
                .text(c.label.clone())
                .pad_to(10 + name_w)
                .quiet(c.detail.clone()),
        )?;
        // A passing check has nothing to fix; one with a switch (rewrites) says how to flip it,
        // and the binary a newer release says what brings it.
        if let Some(fix) = &c.fix {
            let lead = match (c.status, c.check.as_str()) {
                (Status::Pass, "binary") => "update ",
                (Status::Pass, _) => "switch ",
                _ => "fix ",
            };
            o.line(&Line::new().pad_to(10 + name_w).faint(lead).push(Role::Command, fix.prose()))?;
        }
    }
    o.blank()?;
    // The first failing check's fix, else the first warning's; else a newer release's update;
    // else what to do next.
    let first =
        checks.iter().find(|c| c.status == Status::Fail).or_else(|| checks.iter().find(|c| c.status == Status::Warn));
    let update = checks.iter().find(|c| c.check == "binary" && c.status == Status::Pass).and_then(|c| c.command());
    match (first.and_then(|c| c.fix.as_ref().map(|fx| (c, fx))), update) {
        (Some((c, fx)), _) => o.next(&fx.next(), &format!("fixes {}", c.label))?,
        (None, Some(cmd)) => o.next(cmd, "a newer Adage is out; everything else checks out")?,
        (None, None) => o.next("adage status", "everything checks out; see the reflexes in effect")?,
    }
    Ok(())
}

pub fn doctor(args: &[String], paths: &Paths) -> Result<()> {
    if args.iter().any(|a| a == "--watch") {
        let rest: Vec<String> = args.iter().filter(|a| *a != "--watch").cloned().collect();
        return crate::ui::watch(&rest, paths);
    }
    let repo_root = std::env::current_dir().ok().and_then(|d| repo::find_root(&d));
    let checks = checks(paths, repo_root.as_deref());

    let count = |s: Status| checks.iter().filter(|c| c.status == s).count();
    let (p, w, f) = (count(Status::Pass), count(Status::Warn), count(Status::Fail));
    if args.iter().any(|a| a == "--json") {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({ "checks": checks, "summary": { "pass": p, "warn": w, "fail": f } }))?
        );
    } else {
        render(&mut adage_term::Out::stdout(), &checks)?;
    }
    if f > 0 {
        bail!("{f} check(s) failed");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cloud_files_name_what_is_only_in_the_cloud_and_say_nothing_otherwise() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        for f in [".github/workflows/ci.yml", "Cargo.toml", ".git/objects/ab/cdef", ".git/objects/12/3456"] {
            std::fs::create_dir_all(root.join(f).parent().unwrap()).unwrap();
            std::fs::write(root.join(f), "x").unwrap();
        }
        // A worktree elsewhere in the repository (`.claude/worktrees`) is not this checkout's files.
        std::fs::create_dir_all(root.join(".claude/worktrees/wt")).unwrap();
        std::fs::write(root.join(".claude/worktrees/wt/Cargo.toml"), "x").unwrap();
        assert!(cloud_check_with(root, &|_| false).is_none(), "no line when everything is on this Mac");

        let c = cloud_check_with(root, &|p: &Path| {
            p.ends_with(".github/workflows/ci.yml") || p.ends_with("ab/cdef") || p.starts_with(root.join(".claude"))
        })
        .unwrap();
        assert_eq!(c.status, Status::Warn);
        let at = format!("keeps 1 git object and 1 file of the repository at {} only", detect::tilde(root));
        assert!(c.detail.contains(&at), "{}", c.detail);
        assert!(c.detail.contains("only in the cloud (such as .github/workflows/ci.yml);"), "{}", c.detail);
        assert!(c.detail.contains("instead of waiting for a download"), "{}", c.detail);
        assert!(matches!(c.fix.as_ref().unwrap(), Fix::Manual(t) if t.contains("Keep Downloaded")), "{c:?}");

        // In a linked worktree, the objects are the main checkout's, and that is the folder to move.
        let wt = root.join(".claude/worktrees/wt");
        std::fs::create_dir_all(root.join(".git/worktrees/wt")).unwrap();
        std::fs::write(root.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", root.join(".git/worktrees/wt").display())).unwrap();
        let c = cloud_check_with(&wt, &|p: &Path| p.ends_with("ab/cdef")).unwrap();
        assert!(
            c.detail.contains(&format!("keeps 1 git object of the repository at {} only", detect::tilde(root))),
            "{}",
            c.detail
        );
    }

    /// T-A2-03's structure: a fix that is one `adage` command is an action a pane may run; any
    /// other fix is the person's to make; a check without a fix has no action. The fix is typed
    /// where it is made, and its prose, `next` and action all come from that one value.
    #[test]
    fn a_fix_that_is_one_adage_command_is_an_action() {
        let with = |fix: Fix| Check { fix: Some(fix), ..pass("x", "") };
        let run = |c: &str| Some(Action::Run { command: c.into() });
        let cases = [
            (Fix::run_in("run {}", "adage condition"), "run `adage condition`", run("adage condition")),
            (
                Fix::run_in("run {} (or any `adage review` decision) to recompile", "adage condition"),
                "run `adage condition` (or any `adage review` decision) to recompile",
                run("adage condition"),
            ),
            (
                Fix::manual("run `adage condition` in a terminal to see the error"),
                "run `adage condition` in a terminal to see the error",
                Some(Action::Manual),
            ),
            (
                Fix::run_in("{} recompiles it", "adage condition"),
                "`adage condition` recompiles it",
                run("adage condition"),
            ),
            (
                Fix::run("adage delegate rewrites off"),
                "adage delegate rewrites off",
                run("adage delegate rewrites off"),
            ),
            (
                Fix::manual("set ADAGE_HOME to a writable directory"),
                "set ADAGE_HOME to a writable directory",
                Some(Action::Manual),
            ),
            (
                Fix::manual("unset them (when you want Adage on)"),
                "unset them (when you want Adage on)",
                Some(Action::Manual),
            ),
        ];
        for (fix, prose, action) in cases {
            let c = with(fix.clone());
            assert_eq!((fix.prose(), c.fix.as_ref().map(Fix::action)), (prose.to_string(), action.clone()), "{fix:?}");
            // `next` is the command to run, else the person's fix as said.
            let next = match &action {
                Some(Action::Run { command }) => command.clone(),
                _ => prose.to_string(),
            };
            assert_eq!(fix.next(), next);
            let v = serde_json::to_value(&c).unwrap();
            assert_eq!((v["fix"].as_str(), &v["action"]), (Some(prose), &serde_json::to_value(action).unwrap()));
        }
        // The lexicon's words reach the prose around a command, never the command.
        let f = Fix::run_in("run {} once learned", "adage condition").in_words();
        assert_eq!(
            (f.prose().as_str(), f.command()),
            ("run `adage condition` once conditioned", Some("adage condition"))
        );
        assert_eq!(pass("x", "").fix.as_ref().map(Fix::action), None);
        let v = serde_json::to_value(with(Fix::run_in("{} recompiles it", "adage condition"))).unwrap();
        assert_eq!(v["action"], json!({ "kind": "run", "command": "adage condition" }));
        let v = serde_json::to_value(pass("x", "d")).unwrap();
        assert_eq!(v, json!({ "check": "x", "label": "x", "status": "pass", "detail": "d" }));
    }

    /// Every fix doctor makes on a broken home is typed: a command to run is a whole `adage`
    /// command that its prose names, in backticks unless it is the whole fix.
    #[test]
    fn every_fix_names_its_command_once() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        std::fs::write(paths.index(), "not an index").unwrap();
        let line = json!({ "ts": adage_core::time::now_ms(), "error": "boom" });
        std::fs::write(paths.root.join(FAIL_OPEN_LOG), format!("{line}\n")).unwrap();
        let checks = checks(&paths, Some(d.path()));
        let fixes: Vec<&Fix> = checks.iter().filter_map(|c| c.fix.as_ref()).collect();
        assert!(fixes.iter().any(|f| f.command() == Some("adage condition")), "{checks:?}");
        assert!(fixes.iter().any(|f| matches!(f, Fix::Manual(_))), "{checks:?}");
        for f in fixes {
            if let Some(cmd) = f.command() {
                assert!(cmd.starts_with("adage ") && !cmd.contains('`'), "{f:?}");
                let p = f.prose();
                assert!(p == cmd || p.matches(&format!("`{cmd}`")).count() == 1, "{f:?}");
            }
        }
    }

    /// A repository turned off is turned back on with `adage on`, a command a pane may offer.
    #[test]
    fn a_repository_turned_off_is_fixed_with_adage_on() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".adage")).unwrap();
        std::fs::write(d.path().join(".adage/disabled"), "").unwrap();
        let c = kill_switches(Some(d.path()));
        if std::env::var_os("ADAGE_DISABLE").is_some()
            || std::env::var_os("ADAGE_CONTROL").is_some()
            || std::env::var_os("ADAGE_ARM").is_some()
        {
            return; // the shell's own switches change the fix; covered without them
        }
        assert_eq!((c.status, c.fix.clone()), (Status::Warn, Some(Fix::run("adage on"))));
        assert_eq!(c.fix.as_ref().map(Fix::action), Some(Action::Run { command: "adage on".into() }));
    }

    #[test]
    fn rewrites_say_their_state_and_the_switch_the_other_way() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let c = rewrites_check(&paths);
        assert_eq!((c.status, c.detail.as_str()), (Status::Pass, "python tests → .venv (on)"));
        assert_eq!(c.fix, Some(Fix::run("adage delegate rewrites off")));
        // The older marker turns them off whatever the setting says. Written where the hook reads
        // it; doctor reads it through `Paths`, the one definition.
        let marker = adage_hook::rewrite_off(&paths);
        assert_eq!(marker, paths.rewrite_off());
        std::fs::create_dir_all(marker.parent().unwrap()).unwrap();
        std::fs::write(&marker, "").unwrap();
        let c = rewrites_check(&paths);
        assert!(c.detail.starts_with("off (the exec/rewrite-off marker)"), "{}", c.detail);
        assert_eq!(c.fix, Some(Fix::run("adage delegate rewrites on")));
        std::fs::remove_file(&marker).unwrap();
        std::fs::write(paths.config(), "[runtime]\nrewrite = false\n").unwrap();
        assert!(rewrites_check(&paths).detail.starts_with("off (runtime.rewrite = false)"));
    }

    /// T-D3-08: an index a newer Adage wrote (a step this binary cannot read, a newer `format`) is
    /// reported as newer than this binary, with what to do, not as damage to recompile.
    #[test]
    fn an_index_from_a_newer_binary_is_reported_as_newer() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let scan = TrackScan { valid: vec![], imported: 0, invalid: vec![] };
        let newer = json!({ "format": index::INDEX_FORMAT + 1, "generation": "x",
                            "routes": [{ "repo": "r", "track": "t", "version": "1.0.0", "route": { "step": { "do": "teleport" } } }] });
        std::fs::write(paths.index(), serde_json::to_vec(&newer).unwrap()).unwrap();
        let (c, idx) = index_check(&paths, &scan);
        assert_eq!(c.status, Status::Fail);
        assert!(c.detail.contains("is newer than this binary"), "{}", c.detail);
        // T-O8-03: the fix is the command that updates this (older) binary.
        assert_eq!(c.fix.as_ref().and_then(Fix::command), Some("adage update"), "{c:?}");
        assert!(idx.routes.is_empty());
        // The same bytes at this binary's own format are a damaged index, recompiled as before.
        let mut same = newer.clone();
        same["format"] = json!(index::INDEX_FORMAT);
        std::fs::write(paths.index(), serde_json::to_vec(&same).unwrap()).unwrap();
        let (c, _) = index_check(&paths, &scan);
        assert!(c.detail.contains("is not a valid index"), "{}", c.detail);
    }
}
