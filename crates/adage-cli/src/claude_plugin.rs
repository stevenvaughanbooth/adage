//! The Claude Code plugin from this binary (onboarding plan O4, the plugin route first): `adage
//! init` stages it as a one-plugin marketplace under `$ADAGE_HOME/plugin/` and installs it with
//! Claude Code's own command line, so a person who installed `adage` in a terminal gets the
//! plugin with no published release and no `/plugin` typing.
//!
//! ```text
//! $ADAGE_HOME/plugin/                          the marketplace `adage-local`
//!   .claude-plugin/marketplace.json            one plugin, `adage`, from `./adage`
//!   adage/                                     the plugin, `adage@adage-local`
//!     .claude-plugin/plugin.json               \
//!     hooks/hooks.json, hooks/**/*.ts(x)        | embedded in this binary at build time
//!     scripts/session-start, scripts/quiet      | (build.rs: plugin/claude-code without dist/,
//!     types/index.d.ts, README.md              /   the mod's tests and their fixtures)
//!     dist/<target>/adage, adage-exec, adage-train   this binary and its companions, copied
//!     dist/SHA256SUMS                          what the shim verifies before it links a binary
//! ```
//!
//! then `claude plugin marketplace add <$ADAGE_HOME/plugin>` and `claude plugin install
//! adage@adage-local`, verified with `claude plugin list --json`. Claude Code reads a directory
//! marketplace's plugin in place, so staging again (`adage init` again, `adage init --restage` from
//! `adage update`) takes effect at the next session start. Staging is idempotent: a file whose
//! bytes are already there is not rewritten, binary contents are verified even when their version
//! or size is unchanged, and a file the new set does not have is removed. `.staged.json` records
//! the source of each copied binary.
//!
//! Every `claude` call goes through [`ClaudeCli`], so tests stub them. When `claude` is not on
//! `PATH`, or it refuses, the typed spellings of the published marketplace stay the route
//! ([`SLASH_ADD`], [`SLASH_INSTALL`]).

use adage_store::Paths;
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::{Duration, Instant};

/// The local marketplace's name, and the plugin's id in it.
pub(crate) const MARKETPLACE: &str = "adage-local";
pub(crate) const PLUGIN_ID: &str = "adage@adage-local";
/// What a person types in Claude Code to install the published plugin (the release's
/// `marketplace` branch).
pub(crate) const SLASH_ADD: &str = "/plugin marketplace add stevenvaughanbooth/adage";
pub(crate) const SLASH_INSTALL: &str = "/plugin install adage@adage";
/// The binaries the plugin bundles: `adage` (always), then its companions.
pub(crate) const BINARIES: [&str; 3] = ["adage", "adage-exec", "adage-train"];
pub(crate) const COMPANIONS: [&str; 2] = ["adage-exec", "adage-train"];
/// How long one `claude plugin …` call may take before it is stopped.
const CLAUDE_TIMEOUT: Duration = Duration::from_secs(120);

/// One of the plugin's files, embedded at build time.
pub(crate) struct Embedded {
    /// Relative to the plugin's directory, `/`-separated.
    pub(crate) path: &'static str,
    pub(crate) bytes: &'static [u8],
    pub(crate) executable: bool,
}

include!(concat!(env!("OUT_DIR"), "/plugin_files.rs"));

/// The plugin's files this binary carries (empty when it was built without `plugin/claude-code`
/// beside it).
pub(crate) fn embedded() -> &'static [Embedded] {
    FILES
}

/// The target triple this binary was built for: the shim's `dist/<target>/` directory.
pub(crate) fn target() -> &'static str {
    env!("ADAGE_BUILD_TARGET")
}

/// `name` beside `exe` (this binary, symlinks resolved), else on `PATH`; resolved.
pub(crate) fn companion(exe: &Path, name: &str) -> Option<PathBuf> {
    let file = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    let beside = exe.parent().map(|d| d.join(&file)).filter(|p| crate::detect::executable(p));
    beside.or_else(|| crate::detect::on_path(&file)).map(|p| p.canonicalize().unwrap_or(p))
}

/// The plugin directory inside the marketplace.
pub(crate) fn plugin_dir(paths: &Paths) -> PathBuf {
    paths.plugin().join("adage")
}

/// What one staging did.
#[derive(Debug, Clone)]
pub(crate) struct Staged {
    /// The marketplace (`$ADAGE_HOME/plugin`) and the plugin in it.
    pub(crate) root: PathBuf,
    pub(crate) plugin: PathBuf,
    /// Each binary staged, and where it was copied from.
    pub(crate) binaries: Vec<(&'static str, PathBuf)>,
    /// Companions found neither beside this binary nor on `PATH`.
    pub(crate) missing: Vec<&'static str>,
    /// Files written or removed: 0 when everything was already in place.
    pub(crate) changed: usize,
    /// The plugin's version (`plugin.json`).
    pub(crate) version: String,
}

/// The plugin's version and description, from the embedded `plugin.json`.
fn manifest() -> (String, String) {
    let doc: Value = embedded()
        .iter()
        .find(|f| f.path == ".claude-plugin/plugin.json")
        .and_then(|f| serde_json::from_slice(f.bytes).ok())
        .unwrap_or(Value::Null);
    let s = |k: &str| doc[k].as_str().unwrap_or_default().to_string();
    let version = Some(s("version")).filter(|v| !v.is_empty()).unwrap_or_else(|| env!("CARGO_PKG_VERSION").into());
    (version, s("description"))
}

/// The marketplace's catalogue: one plugin, from the directory beside it.
fn marketplace_json(version: &str, description: &str) -> String {
    let doc = json!({
        "name": MARKETPLACE,
        "owner": { "name": "Adage" },
        "metadata": {
            "description": "Adage's Claude Code plugin, staged on this machine by `adage init` from the adage binary it ran; `adage update` stages it again.",
            "version": version,
        },
        "plugins": [{
            "name": "adage",
            "source": "./adage",
            "description": description,
            "version": version,
            "repository": "https://github.com/stevenvaughanbooth/adage",
            "homepage": "https://github.com/stevenvaughanbooth/adage",
        }],
    });
    serde_json::to_string_pretty(&doc).unwrap_or_default() + "\n"
}

/// The source identities recorded by the last staging. Contents are checked independently: a
/// same-sized destination or a shared version string does not prove that a copy is current.
#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
struct Stamp {
    binaries: std::collections::BTreeMap<String, Source>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
struct Source {
    from: PathBuf,
    size: u64,
    modified_ns: u128,
    sha256: String,
}

fn source_of(p: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some((m.len(), t))
}

/// Write `bytes` to `to` unless it already holds them; returns whether it wrote.
fn put(to: &Path, bytes: &[u8], executable: bool) -> Result<bool> {
    let same = std::fs::read(to).is_ok_and(|b| b == bytes);
    if same {
        if executable && !crate::detect::executable(to) {
            crate::net::make_executable(to)?;
            return Ok(true);
        }
        return Ok(false);
    }
    let dir = to.parent().context("no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let tmp = dir.join(format!(".adage-stage-{}.tmp", std::process::id()));
    std::fs::write(&tmp, bytes).with_context(|| format!("cannot write in {}", dir.display()))?;
    if executable {
        crate::net::make_executable(&tmp)?;
    }
    std::fs::rename(&tmp, to).with_context(|| format!("cannot replace {}", to.display()))?;
    Ok(true)
}

/// Copy the binary `from` to `to` (beside a temporary name, then renamed) unless its contents
/// already match; returns the source identity and whether it wrote.
fn put_binary(from: &Path, to: &Path) -> Result<(Source, bool)> {
    let (size, modified_ns) = source_of(from).with_context(|| format!("cannot read {}", from.display()))?;
    let sha256 = crate::net::sha256_file(from).with_context(|| format!("cannot read {}", from.display()))?;
    let src = Source { from: from.to_path_buf(), size, modified_ns, sha256 };
    if crate::net::sha256_file(to).is_ok_and(|hash| hash == src.sha256) {
        let changed = !crate::detect::executable(to);
        if changed {
            crate::net::make_executable(to)?;
        }
        return Ok((src, changed));
    }
    let dir = to.parent().context("no parent directory")?;
    std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    let tmp = dir.join(format!(".adage-stage-{}.tmp", std::process::id()));
    std::fs::copy(from, &tmp).with_context(|| format!("cannot copy {} into {}", from.display(), dir.display()))?;
    crate::net::make_executable(&tmp)?;
    std::fs::rename(&tmp, to).with_context(|| format!("cannot replace {}", to.display()))?;
    Ok((src, true))
}

/// Every file under `dir`, relative to it (`/`-separated); symlinks are listed, never followed.
fn files_under(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(_) => {
                    out.push(p.strip_prefix(dir).unwrap_or(&p).to_string_lossy().replace('\\', "/"));
                }
                Err(_) => {}
            }
        }
    }
    out
}

/// Stage the plugin from this binary (`exe`, resolved) and its companions into
/// `$ADAGE_HOME/plugin`. Writes only there.
pub(crate) fn stage(paths: &Paths, exe: &Path) -> Result<Staged> {
    if embedded().is_empty() {
        bail!("this adage was built without the Claude Code plugin's files (plugin/claude-code was not beside it)");
    }
    let root = paths.plugin();
    let plugin = plugin_dir(paths);
    let (version, description) = manifest();
    let mut changed = 0usize;
    let mut want: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for f in embedded() {
        changed += usize::from(put(&plugin.join(f.path), f.bytes, f.executable)?);
        want.insert(f.path.to_string());
    }
    // The binaries, under the target the shim maps this machine to.
    let stamp_file = root.join(".staged.json");
    let mut stamp = Stamp::default();
    let mut binaries = Vec::new();
    let mut missing = Vec::new();
    let mut sums = Vec::new();
    for name in BINARIES {
        let from = if name == "adage" { Some(exe.to_path_buf()) } else { companion(exe, name) };
        let Some(from) = from else {
            missing.push(name);
            continue;
        };
        let rel = format!("{}/{name}{}", target(), std::env::consts::EXE_SUFFIX);
        let (src, wrote) = put_binary(&from, &plugin.join("dist").join(&rel))?;
        changed += usize::from(wrote);
        sums.push(format!("{}  {rel}", src.sha256));
        want.insert(format!("dist/{rel}"));
        stamp.binaries.insert(name.to_string(), src);
        binaries.push((name, from));
    }
    changed += usize::from(put(&plugin.join("dist/SHA256SUMS"), (sums.join("\n") + "\n").as_bytes(), false)?);
    want.insert("dist/SHA256SUMS".into());
    // What an earlier staging left that this one does not have (a module since removed, another
    // target's binaries).
    for rel in files_under(&plugin) {
        if !want.contains(&rel) {
            std::fs::remove_file(plugin.join(&rel)).with_context(|| format!("cannot remove {rel}"))?;
            changed += 1;
        }
    }
    changed += usize::from(put(
        &root.join(".claude-plugin/marketplace.json"),
        marketplace_json(&version, &description).as_bytes(),
        false,
    )?);
    let _ = adage_store::config::write_atomic(&stamp_file, &serde_json::to_vec_pretty(&stamp)?);
    Ok(Staged { root, plugin, binaries, missing, changed, version })
}

/// Whether the locally managed plugin still contains this install's modules and binaries.
/// This deliberately checks content rather than versions: two builds can both report 0.1.0.
/// No files are written, and separately installed marketplace plugins are not inspected.
pub(crate) fn staged_matches(paths: &Paths, exe: &Path) -> bool {
    let plugin = plugin_dir(paths);
    let (version, description) = manifest();
    if embedded().is_empty()
        || !std::fs::read(paths.plugin().join(".claude-plugin/marketplace.json"))
            .is_ok_and(|bytes| bytes == marketplace_json(&version, &description).as_bytes())
    {
        return false;
    }
    let mut want = std::collections::BTreeSet::new();
    for f in embedded() {
        let file = plugin.join(f.path);
        if !std::fs::read(&file).is_ok_and(|bytes| bytes == f.bytes)
            || (f.executable && !crate::detect::executable(&file))
        {
            return false;
        }
        want.insert(f.path.to_string());
    }
    let mut sums = Vec::new();
    for name in BINARIES {
        let from = if name == "adage" { Some(exe.to_path_buf()) } else { companion(exe, name) };
        let Some(from) = from else { continue };
        let Ok(hash) = crate::net::sha256_file(&from) else { return false };
        let rel = format!("{}/{name}{}", target(), std::env::consts::EXE_SUFFIX);
        let file = plugin.join("dist").join(&rel);
        if !crate::net::sha256_file(&file).is_ok_and(|other| other == hash) || !crate::detect::executable(&file) {
            return false;
        }
        sums.push(format!("{hash}  {rel}"));
        want.insert(format!("dist/{rel}"));
    }
    want.insert("dist/SHA256SUMS".into());
    std::fs::read(plugin.join("dist/SHA256SUMS")).is_ok_and(|bytes| bytes == (sums.join("\n") + "\n").as_bytes())
        && files_under(&plugin).into_iter().collect::<std::collections::BTreeSet<_>>() == want
}

// ---------------------------------------------------------------------------------------------
// Claude Code's command line

/// One finished `claude` run.
#[derive(Debug, Clone, Default)]
pub(crate) struct Ran {
    pub(crate) code: Option<i32>,
    pub(crate) stdout: String,
    pub(crate) stderr: String,
}

/// `claude plugin …`, behind a trait so tests stub it.
pub(crate) trait ClaudeCli {
    /// Run `claude <args>` in `cwd`. `Err` when it could not be started or did not finish in time.
    fn run(&self, args: &[&str], cwd: &Path) -> std::io::Result<Ran>;
}

/// The `claude` on `PATH`.
pub(crate) struct System {
    program: PathBuf,
}

impl System {
    pub(crate) fn on_path() -> Option<System> {
        let names: &[&str] = if cfg!(windows) { &["claude.exe", "claude.cmd"] } else { &["claude"] };
        names.iter().find_map(|n| crate::detect::on_path(n)).map(|program| System { program })
    }
}

impl ClaudeCli for System {
    fn run(&self, args: &[&str], cwd: &Path) -> std::io::Result<Ran> {
        let mut child = std::process::Command::new(&self.program)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;
        let read = |r: Option<Box<dyn std::io::Read + Send>>| {
            std::thread::spawn(move || {
                let mut s = String::new();
                if let Some(mut r) = r {
                    let _ = r.read_to_string(&mut s);
                }
                s
            })
        };
        let out = read(child.stdout.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
        let err = read(child.stderr.take().map(|s| Box::new(s) as Box<dyn std::io::Read + Send>));
        let start = Instant::now();
        let status = loop {
            if let Some(s) = child.try_wait()? {
                break s;
            }
            if start.elapsed() > CLAUDE_TIMEOUT {
                let _ = child.kill();
                let _ = child.wait();
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    format!("claude {} did not finish in {} s", args.join(" "), CLAUDE_TIMEOUT.as_secs()),
                ));
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        Ok(Ran { code: status.code(), stdout: out.join().unwrap_or_default(), stderr: err.join().unwrap_or_default() })
    }
}

/// One installed plugin, as `claude plugin list --json` reports it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Installed {
    pub(crate) id: String,
    pub(crate) enabled: bool,
    pub(crate) scope: String,
    pub(crate) version: String,
    /// The directory a directory-marketplace plugin is read from, in place.
    pub(crate) read_from: Option<PathBuf>,
}

impl Installed {
    /// Is this Adage (from any marketplace)?
    pub(crate) fn is_adage(&self) -> bool {
        self.id.split('@').next() == Some("adage")
    }
    /// The marketplace it came from (`adage-local`, `adage`).
    pub(crate) fn marketplace(&self) -> &str {
        self.id.split_once('@').map(|(_, m)| m).unwrap_or("")
    }
}

/// A `claude` call that did not do what was asked: which step, and its own words.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Refused {
    pub(crate) step: String,
    pub(crate) why: String,
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "`claude {}` {}", self.step, self.why)
    }
}

/// The last line of `stdout` that is a JSON object (`--json` prints one result line, last).
fn result_line(stdout: &str) -> Option<Value> {
    stdout.lines().rev().find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(Value::is_object))
}

/// Run one `claude` step that answers a `--json` result line, and require `outcome: ok`.
fn step(cli: &dyn ClaudeCli, args: &[&str], cwd: &Path) -> Result<Value, Refused> {
    let refused = |why: String| Refused {
        step: args.iter().take_while(|a| **a != "--json").copied().collect::<Vec<_>>().join(" "),
        why,
    };
    let ran = cli.run(args, cwd).map_err(|e| refused(format!("could not run: {e}")))?;
    let line = result_line(&ran.stdout);
    let message = line.as_ref().and_then(|v| v["message"].as_str()).map(str::to_string);
    let ok = ran.code == Some(0) && line.as_ref().is_some_and(|v| v["outcome"] == "ok");
    if ok {
        return Ok(line.unwrap_or(Value::Null));
    }
    let said = message
        .or_else(|| ran.stderr.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
        .or_else(|| ran.stdout.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
        .unwrap_or_default();
    let code = ran.code.map(|c| format!("exit {c}")).unwrap_or_else(|| "stopped".into());
    Err(refused(if said.is_empty() { format!("failed ({code})") } else { format!("failed ({code}): {said}") }))
}

/// The plugins Claude Code has installed (`claude plugin list --json`).
pub(crate) fn list(cli: &dyn ClaudeCli, cwd: &Path) -> Result<Vec<Installed>, Refused> {
    let refused = |why: String| Refused { step: "plugin list".into(), why };
    let ran = cli.run(&["plugin", "list", "--json"], cwd).map_err(|e| refused(format!("could not run: {e}")))?;
    if ran.code != Some(0) {
        let said = ran.stderr.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").to_string();
        return Err(refused(format!("failed ({}): {said}", ran.code.map(|c| c.to_string()).unwrap_or_default())));
    }
    let start = ran.stdout.find('[').ok_or_else(|| refused("printed no list".into()))?;
    let v: Value = serde_json::from_str(ran.stdout[start..].trim())
        .map_err(|e| refused(format!("printed a list that is not JSON: {e}")))?;
    let s = |e: &Value, k: &str| e[k].as_str().unwrap_or_default().to_string();
    Ok(v.as_array()
        .into_iter()
        .flatten()
        .map(|e| Installed {
            id: s(e, "id"),
            enabled: e["enabled"].as_bool().unwrap_or(false),
            scope: s(e, "scope"),
            version: s(e, "version"),
            read_from: e["readFromFolder"].as_str().map(PathBuf::from),
        })
        .collect())
}

/// What connecting did, step by step, as people read it.
#[derive(Debug, Clone)]
pub(crate) struct Connected {
    pub(crate) staged: Staged,
    /// The installed plugin, as `claude plugin list` reports it after the install.
    pub(crate) installed: Installed,
    /// The marketplace was already added / the plugin already installed (a re-run).
    pub(crate) already: bool,
}

/// Same directory, compared as given and then resolved.
fn same_dir(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().is_some_and(|x| b.canonicalize().ok().is_some_and(|y| x == y))
}

/// The plugin route: stage the plugin from this binary, add the local marketplace, install the
/// plugin at `scope` (`user`, or `local` for this repository alone), and verify that Claude Code
/// lists it enabled, read from the staged directory. `cwd` is where `claude` runs (the
/// repository, for `local`).
pub(crate) fn connect(cli: &dyn ClaudeCli, paths: &Paths, exe: &Path, scope: &str, cwd: &Path) -> Result<Connected> {
    let staged = stage(paths, exe)?;
    let before = list(cli, cwd).ok().unwrap_or_default();
    let root = staged.root.to_string_lossy().into_owned();
    step(cli, &["plugin", "marketplace", "add", &root, "--scope", "user", "--json"], cwd)
        .map_err(anyhow::Error::msg)?;
    let was = before.iter().any(|p| p.id == PLUGIN_ID);
    step(cli, &["plugin", "install", PLUGIN_ID, "--scope", scope, "--json"], cwd).map_err(anyhow::Error::msg)?;
    let installed = verify(cli, &staged, cwd)?;
    Ok(Connected { staged, installed, already: was })
}

/// Claude Code lists the local plugin enabled and reads it from `staged.plugin`.
pub(crate) fn verify(cli: &dyn ClaudeCli, staged: &Staged, cwd: &Path) -> Result<Installed> {
    let now = list(cli, cwd).map_err(anyhow::Error::msg)?;
    let Some(p) = now.into_iter().find(|p| p.id == PLUGIN_ID) else {
        bail!("`claude plugin list` does not list {PLUGIN_ID} after installing it");
    };
    if !p.enabled {
        bail!("`claude plugin list` lists {PLUGIN_ID} but not enabled");
    }
    let Some(from) = p.read_from.as_deref() else {
        bail!("Claude Code did not report where it reads {PLUGIN_ID}; the refreshed local plugin could not be verified (update Claude Code, then run `adage init`)");
    };
    if !same_dir(from, &staged.plugin) {
        bail!("Claude Code reads {PLUGIN_ID} from {}, not from {}", from.display(), staged.plugin.display());
    }
    Ok(p)
}

/// For `adage update` (`adage init --restage`): when the installed plugin is the local one, stage
/// it again from this binary and refresh the marketplace, so the next session runs these binaries
/// and modules. `Ok(None)` when Claude Code has no local Adage plugin (nothing to do).
pub(crate) fn refresh(cli: &dyn ClaudeCli, paths: &Paths, exe: &Path, cwd: &Path) -> Result<Option<Staged>> {
    let installed = list(cli, cwd).map_err(anyhow::Error::msg)?;
    if !installed.iter().any(|p| p.id == PLUGIN_ID) {
        return Ok(None);
    }
    let staged = stage(paths, exe)?;
    step(cli, &["plugin", "marketplace", "update", MARKETPLACE, "--json"], cwd).map_err(anyhow::Error::msg)?;
    verify(cli, &staged, cwd)?;
    Ok(Some(staged))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::cell::RefCell;

    /// How a stub answers one call (its arguments joined by spaces); `None` is "not on PATH".
    type Answer = Box<dyn Fn(&str) -> Option<Ran>>;

    /// A `claude` that answers each call from a script and records it.
    pub(crate) struct Stub {
        pub(crate) calls: RefCell<Vec<String>>,
        pub(crate) answer: Answer,
    }

    impl ClaudeCli for Stub {
        fn run(&self, args: &[&str], _cwd: &Path) -> std::io::Result<Ran> {
            let line = args.join(" ");
            self.calls.borrow_mut().push(line.clone());
            (self.answer)(&line).ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no claude"))
        }
    }

    fn ok(stdout: &str) -> Option<Ran> {
        Some(Ran { code: Some(0), stdout: stdout.into(), stderr: String::new() })
    }

    /// A Claude Code that keeps its installed plugins in memory, as the real one does on disk.
    pub(crate) fn fake_claude(read_from: PathBuf) -> Stub {
        let installed = std::rc::Rc::new(RefCell::new(false));
        let i = installed.clone();
        Stub {
            calls: RefCell::new(vec![]),
            answer: Box::new(move |line| {
                if line.starts_with("plugin marketplace add") || line.starts_with("plugin marketplace update") {
                    return ok(
                        r#"{"command":"marketplace-add","outcome":"ok","marketplace":"adage-local","message":"Successfully added marketplace: adage-local"}"#,
                    );
                }
                if line.starts_with("plugin install") {
                    *i.borrow_mut() = true;
                    return ok(
                        r#"{"command":"install","outcome":"ok","plugin":"adage@adage-local","scope":"user","message":"Successfully installed plugin: adage@adage-local (scope: user)"}"#,
                    );
                }
                if line == "plugin list --json" {
                    let list = if *i.borrow() {
                        json!([{ "id": PLUGIN_ID, "version": "0.1.0", "scope": "user", "enabled": true, "readFromFolder": read_from }])
                    } else {
                        json!([])
                    };
                    return ok(&list.to_string());
                }
                None
            }),
        }
    }

    fn exe_with_companions(dir: &Path, companions: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        let exe = dir.join("adage");
        std::fs::write(&exe, b"#!/bin/sh\necho adage 0.1.0\n").unwrap();
        crate::net::make_executable(&exe).unwrap();
        for c in companions {
            let p = dir.join(c);
            std::fs::write(&p, format!("#!/bin/sh\necho {c}\n")).unwrap();
            crate::net::make_executable(&p).unwrap();
        }
        exe
    }

    #[test]
    fn the_binary_carries_the_plugins_files_and_not_its_tests() {
        let paths: Vec<&str> = embedded().iter().map(|f| f.path).collect();
        for want in [".claude-plugin/plugin.json", "hooks/hooks.json", "hooks/register.ts", "scripts/session-start"] {
            assert!(paths.contains(&want), "{want} is not embedded: {paths:?}");
        }
        assert!(
            !paths.iter().any(|p| p.ends_with(".test.ts") || p.contains("fixtures/") || p.starts_with("dist/")),
            "{paths:?}"
        );
        let shim = embedded().iter().find(|f| f.path == "scripts/session-start").unwrap();
        assert!(shim.executable);
        // Every file the repository's plugin has (outside dist, tests and fixtures) is embedded.
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plugin/claude-code");
        let mut on_disk: Vec<String> = files_under(&repo)
            .into_iter()
            .filter(|p| {
                !p.starts_with("dist/")
                    && !p.contains("fixtures/")
                    && !p.ends_with(".test.ts")
                    && !p.ends_with(".test.tsx")
                    && !p.ends_with("VALIDATE.txt")
            })
            .collect();
        on_disk.sort();
        let mut ours: Vec<String> = paths.iter().map(|p| p.to_string()).collect();
        ours.sort();
        assert_eq!(ours, on_disk, "build.rs and the plugin directory disagree: rebuild adage");
    }

    #[test]
    #[cfg(unix)]
    fn staging_writes_the_marketplace_and_is_idempotent() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let exe = exe_with_companions(&d.path().join("bin"), &["adage-exec"]);
        let s = stage(&paths, &exe).unwrap();
        assert_eq!(s.root, paths.plugin());
        assert_eq!(s.missing, vec!["adage-train"], "not beside it and not on PATH");
        let market: Value =
            serde_json::from_slice(&std::fs::read(s.root.join(".claude-plugin/marketplace.json")).unwrap()).unwrap();
        assert_eq!(market["name"], MARKETPLACE);
        assert_eq!(market["plugins"][0]["name"], "adage");
        assert_eq!(market["plugins"][0]["source"], "./adage");
        assert_eq!(market["plugins"][0]["version"], s.version);
        assert!(s.plugin.join("hooks/hooks.json").is_file() && s.plugin.join(".claude-plugin/plugin.json").is_file());
        assert!(crate::detect::executable(&s.plugin.join("scripts/session-start")));
        let bin = s.plugin.join("dist").join(target()).join("adage");
        assert_eq!(std::fs::read(&bin).unwrap(), std::fs::read(&exe).unwrap());
        let sums = std::fs::read_to_string(s.plugin.join("dist/SHA256SUMS")).unwrap();
        assert_eq!(
            sums,
            format!(
                "{}  {t}/adage\n{}  {t}/adage-exec\n",
                crate::net::sha256_file(&exe).unwrap(),
                crate::net::sha256_file(&d.path().join("bin/adage-exec")).unwrap(),
                t = target()
            )
        );
        // Again: nothing to write.
        let again = stage(&paths, &exe).unwrap();
        assert_eq!(again.changed, 0, "a re-run writes nothing");
        // A file an earlier version left is removed; a changed binary is copied again.
        std::fs::write(s.plugin.join("hooks/gone.ts"), "x").unwrap();
        std::fs::write(&exe, b"#!/bin/sh\necho adage 0.2.0\n").unwrap();
        let third = stage(&paths, &exe).unwrap();
        assert!(!s.plugin.join("hooks/gone.ts").exists());
        assert!(third.changed >= 2, "{third:?}");
        assert_eq!(std::fs::read(&bin).unwrap(), std::fs::read(&exe).unwrap());
        assert!(std::fs::read_to_string(s.plugin.join("dist/SHA256SUMS"))
            .unwrap()
            .starts_with(&crate::net::sha256_file(&exe).unwrap()));
    }

    #[test]
    #[cfg(unix)]
    fn staging_repairs_same_size_binary_damage_and_checks_every_part() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let exe = exe_with_companions(&d.path().join("bin"), &COMPANIONS);
        let staged = stage(&paths, &exe).unwrap();
        assert!(staged_matches(&paths, &exe));
        // The source and staging stamp have not changed. Size alone must not accept a stale copy.
        let bin = staged.plugin.join("dist").join(target()).join("adage");
        let mut damaged = std::fs::read(&bin).unwrap();
        damaged[0] ^= 1;
        std::fs::write(&bin, damaged).unwrap();
        assert!(!staged_matches(&paths, &exe));
        assert!(stage(&paths, &exe).unwrap().changed > 0);
        assert_eq!(std::fs::read(&bin).unwrap(), std::fs::read(&exe).unwrap());
        assert!(staged_matches(&paths, &exe));

        // The main binary is already current: modules, companions, checksums and obsolete files
        // must still cause update to restage. Version strings do not participate in this decision.
        for rel in ["hooks/register.ts", "dist/SHA256SUMS", "hooks/removed.ts"] {
            std::fs::write(staged.plugin.join(rel), b"stale").unwrap();
            assert!(!staged_matches(&paths, &exe), "{rel}");
            assert!(stage(&paths, &exe).unwrap().changed > 0);
            assert!(staged_matches(&paths, &exe), "{rel}");
        }
        for name in COMPANIONS {
            let from = d.path().join("bin").join(name);
            std::fs::write(&from, format!("#!/bin/sh\necho new {name}\n")).unwrap();
            assert!(!staged_matches(&paths, &exe), "{name}");
            assert!(stage(&paths, &exe).unwrap().changed > 0);
            assert!(staged_matches(&paths, &exe), "{name}");
        }
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(!staged_matches(&paths, &exe));
        assert!(stage(&paths, &exe).unwrap().changed > 0);
        assert!(staged_matches(&paths, &exe));
        assert_eq!(stage(&paths, &exe).unwrap().changed, 0);
    }

    #[test]
    #[cfg(unix)]
    fn connecting_adds_installs_and_verifies() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let exe = exe_with_companions(&d.path().join("bin"), &["adage-exec", "adage-train"]);
        let claude = fake_claude(paths.plugin().join("adage"));
        let c = connect(&claude, &paths, &exe, "user", d.path()).unwrap();
        assert!(c.installed.enabled && !c.already);
        let calls = claude.calls.borrow().clone();
        let root = paths.plugin().display().to_string();
        assert_eq!(
            calls,
            vec![
                "plugin list --json".to_string(),
                format!("plugin marketplace add {root} --scope user --json"),
                "plugin install adage@adage-local --scope user --json".into(),
                "plugin list --json".into(),
            ]
        );
        // A refresh (adage update) stages again and updates the marketplace.
        claude.calls.borrow_mut().clear();
        let r = refresh(&claude, &paths, &exe, d.path()).unwrap();
        assert!(r.is_some());
        assert!(claude.calls.borrow().contains(&"plugin marketplace update adage-local --json".to_string()));
    }

    #[test]
    #[cfg(unix)]
    fn a_refusal_names_the_step_and_claudes_words() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let exe = exe_with_companions(&d.path().join("bin"), &[]);
        let refusing = Stub {
            calls: RefCell::new(vec![]),
            answer: Box::new(|line| {
                if line.starts_with("plugin marketplace add") {
                    return Some(Ran {
                        code: Some(1),
                        stdout: r#"{"command":"marketplace-add","outcome":"error","message":"Marketplace sources are restricted by policy"}"#.into(),
                        stderr: String::new(),
                    });
                }
                ok("[]")
            }),
        };
        let e = connect(&refusing, &paths, &exe, "user", d.path()).unwrap_err().to_string();
        assert!(
            e.contains("plugin marketplace add") && e.contains("restricted by policy") && e.contains("exit 1"),
            "{e}"
        );
        // Not on PATH at all.
        let absent = Stub { calls: RefCell::new(vec![]), answer: Box::new(|_| None) };
        let e = connect(&absent, &paths, &exe, "user", d.path()).unwrap_err().to_string();
        assert!(e.contains("could not run"), "{e}");
        // Installed, but read from somewhere else: not verified.
        let elsewhere = fake_claude(d.path().join("other"));
        let e = connect(&elsewhere, &paths, &exe, "user", d.path()).unwrap_err().to_string();
        assert!(e.contains("reads adage@adage-local from"), "{e}");
        // A version and enabled flag alone do not prove which plugin files will run.
        let unknown_source = Stub {
            calls: RefCell::new(vec![]),
            answer: Box::new(|_| ok(r#"[{"id":"adage@adage-local","version":"0.1.0","scope":"user","enabled":true}]"#)),
        };
        let staged = stage(&paths, &exe).unwrap();
        let e = verify(&unknown_source, &staged, d.path()).unwrap_err().to_string();
        assert!(e.contains("did not report where it reads") && e.contains("could not be verified"), "{e}");
    }

    #[test]
    fn the_list_is_read_from_claudes_json() {
        let stub = Stub {
            calls: RefCell::new(vec![]),
            answer: Box::new(|_| {
                ok(r#"[{"id":"adage@adage","version":"0.1.0","scope":"user","enabled":false,"installPath":"/x"}]"#)
            }),
        };
        let l = list(&stub, Path::new("/")).unwrap();
        assert_eq!(l.len(), 1);
        assert!(l[0].is_adage() && !l[0].enabled && l[0].marketplace() == "adage" && l[0].read_from.is_none());
    }
}
