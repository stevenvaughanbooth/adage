//! `adage update [--check [--now] [--json]] [--print] [--yes] [--from source|npm|brew|release]
//! [--branch <name>]`, `adage update --rollback [--print]`: replace this install of Adage with the
//! newest release, the same way it was installed, safely, with a way back (onboarding plan O5,
//! O7).
//!
//! **The check** ([`latest`]). Is a newer Adage out? One request to the npm registry, then to
//! GitHub's releases when the package is not published, cached for a day in
//! `$ADAGE_HOME/cache/latest.json`; `--check` prints the answer (`--now` asks again, `--json` as
//! data). The detached learner runs the same check at most daily (`update.check`); no hook ever
//! does.
//!
//! **The transaction.** (1) check: with nothing newer, `adage 0.2.0 is the latest · nothing to
//! do` ends it and nothing runs. (2) the plan, printed: the channel's own command with the version
//! pinned (`npm install --global @adage-sh/cli@0.2.0`, `cargo install … --tag v0.2.0`, the release
//! installer for `v0.2.0` and its companions), never `@latest` or a moving branch; a checkout pulls
//! and rebuilds; Homebrew upgrades its formulas. `--print` stops here. (3) keep: the running
//! `adage`, `adage-exec` and `adage-train` are copied to `$ADAGE_HOME/previous/<version>/`
//! ([`keep`]) before anything is replaced. (4) run: each command with the terminal attached, so its
//! own progress and errors show. (5) verify: the binary the channel installed answers `version`
//! with the version asked for, and (npm, release) its SHA-256 equals the release's published
//! `SHA256SUMS` line when the release publishes one. (6) the new binary finishes
//! (`<new> update --finish <from>`, [`probe::hand_over`]), so what follows is the new version's
//! code: registrations whose binary moved or whose events changed are re-pointed ([`repoint`]),
//! with each diff and a `.adage-bak`, at a terminal only; the Claude Code plugin `adage init`
//! staged is staged again from the new binaries and Claude Code's marketplace refreshed; (7)
//! doctor's summary, the release's first notes, `done`. A failed step stops, leaves `previous/` in
//! place and names `adage update --rollback`, which puts the kept binaries back byte for byte and
//! re-points again.
//!
//! **Channels** ([`channel`]): read from where the binary lives. The Claude Code plugin's binary is
//! updated by Claude Code (`claude plugin marketplace update` and `claude plugin update`, when
//! `claude` is on `PATH`; otherwise the `/plugin` steps are printed and nothing runs).
//!
//! This is the one command that replaces Adage's own binaries, and only when a person runs it: no
//! hook or learner calls it (the learner only checks).

pub(crate) mod channel;
pub(crate) mod keep;
pub(crate) mod latest;
pub(crate) mod probe;
pub(crate) mod repoint;

use crate::claude_plugin::{self, ClaudeCli};
use crate::detect;
use crate::helpers::flag;
use adage_store::release::{Latest, Release, VERSION};
use adage_store::Paths;
use adage_term::{Line, Out, Role};
use anyhow::{Context, Result};
use channel::{Channel, Step};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    let has = |f: &str| args.iter().any(|a| a == f);
    if let Some(from) = flag(args, "--finish") {
        return finish(&from, args, paths);
    }
    if has("--rollback") || has("--back") {
        return rollback(args, paths);
    }
    if has("--check") {
        return check(args, paths);
    }
    update(args, paths)
}

fn now() -> u64 {
    adage_core::time::now_ms()
}

/// The running binary, symlinks resolved.
fn running() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("cannot find the running adage binary")?;
    Ok(exe.canonicalize().unwrap_or(exe))
}

/// The newest release, when the cache says one is newer than this binary (no request is made):
/// what every surface that says "a new Adage is out" reads.
pub(crate) fn newer(paths: &Paths) -> Option<Release> {
    Latest::read(paths).and_then(|l| l.newer_than(VERSION).cloned())
}

/// The status card's `update` field (`adage --json`): `null` unless a newer release is known, then
/// `{current, latest, channel, next, text, published_at, url, notes}`
/// (docs/claude-mod/impl/O5-O7-O8-notes.md). `next` is `/adage update` drawn inside an agent
/// session (`--for-session`), else `adage update`.
pub(crate) fn card_json(paths: &Paths, for_session: bool) -> serde_json::Value {
    let Some(r) = newer(paths) else { return serde_json::Value::Null };
    let channel = running().map(|e| channel::detect(&e, paths).name()).unwrap_or("source");
    serde_json::json!({
        "current": VERSION,
        "latest": r.version,
        "channel": channel,
        "next": if for_session { "/adage update" } else { "adage update" },
        "text": format!("{} is out (you have {VERSION})", r.version),
        "published_at": r.published_at,
        "url": r.url,
        "notes": r.notes,
    })
}

/// `adage update --check [--now] [--json]`.
fn check(args: &[String], paths: &Paths) -> Result<()> {
    let has = |f: &str| args.iter().any(|a| a == f);
    let now = now();
    let l = latest::current(paths, now, has("--now"));
    if has("--json") {
        let r = l.newer_than(VERSION);
        let v = serde_json::json!({
            "current": VERSION,
            "latest": l.latest.as_ref().map(|r| &r.version),
            "newer": r.is_some(),
            "checked_at": l.checked_at,
            "published_at": l.latest.as_ref().and_then(|r| r.published_at.as_ref()),
            "url": l.latest.as_ref().and_then(|r| r.url.as_ref()),
            "notes": l.latest.as_ref().and_then(|r| r.notes.as_ref()),
            "failure": l.failure,
        });
        println!("{}", serde_json::to_string_pretty(&v)?);
        return Ok(());
    }
    let mut o = Out::stdout();
    o.line(&Line::new().text(latest::sentence(&l, now)))?;
    if let Some(f) = l.failure.as_ref().filter(|_| l.newer_than(VERSION).is_none()) {
        o.note(&format!(
            "{}; it is asked again after a day, or now with adage update --check --now",
            latest::explain(f)
        ))?;
    } else if let Some(notes) = l.newer_than(VERSION).and_then(|r| r.notes.as_deref()) {
        for n in notes.lines() {
            o.line(&Line::new().quiet(format!("· {n}")))?;
        }
    }
    if l.newer_than(VERSION).is_some() {
        o.blank()?;
        o.next("adage update", "update this adage, its hooks and the plugin together")?;
    }
    Ok(())
}

/// Lines a person reads for an install: the binaries kept, as names.
fn names(kept: &[(String, PathBuf)]) -> String {
    kept.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>().join(", ")
}

/// Is a person at a terminal, outside an agent session, who may be asked or written for?
fn person_at_terminal() -> Option<bool> {
    if adage_app::person::AGENT_ENV.iter().any(|v| std::env::var_os(v).is_some()) {
        return None;
    }
    if std::io::stdin().is_terminal() {
        return Some(true);
    }
    (std::env::var("ADAGE_ALLOW_NONINTERACTIVE").as_deref() == Ok("1")).then_some(false)
}

/// `adage update`: steps (1) to (5), then the new binary finishes.
fn update(args: &[String], paths: &Paths) -> Result<()> {
    let has = |f: &str| args.iter().any(|a| a == f);
    let print = has("--print");
    let exe = running()?;
    let channel = match flag(args, "--from").as_deref() {
        Some(name @ ("source" | "npm" | "brew" | "release")) => Channel::parse(name).unwrap_or(Channel::Source),
        Some(other) => {
            return Err(crate::hint(
                format!("unknown install channel {other:?}"),
                "use --from source, npm, brew or release",
            ))
        }
        None => channel::detect(&exe, paths),
    };
    let co = if channel == Channel::Checkout { channel::checkout(&exe) } else { None };
    let branch = flag(args, "--branch");
    if let (Some(co), Some(b)) = (&co, &branch) {
        return Err(crate::hint(
            format!(
                "this adage is built from the checkout at {}, so --branch {b} does not apply",
                detect::tilde(&co.root)
            ),
            "switch branches there with git and run `adage update` again, or pass --from source",
        ));
    }
    let now = now();
    let mut o = Out::stdout();

    // (1) Check. A checkout follows its branch; a source build of a named branch is the person's
    // own choice: neither asks for a release.
    let asks = channel != Channel::Checkout && branch.is_none();
    let known = asks.then(|| latest::current(paths, now, false));
    let target = known.as_ref().and_then(|l| l.newer_than(VERSION)).cloned();
    if let Some(l) = &known {
        if target.is_none() && l.failure.is_none() && l.latest.is_some() {
            o.line(&Line::new().text(format!("adage {VERSION} is the latest · nothing to do")))?;
            // The plugin `adage init` staged follows this install: one left behind (staged from
            // an older build) is staged again, so the two never diverge (O8).
            if !print && channel != Channel::Plugin && local_plugin_stale(paths, &exe) {
                let root = std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d));
                restage_plugin(&mut o, paths, &exe, root.as_deref())?;
            }
            return Ok(());
        }
        if target.is_none() && channel.pins() {
            let f = l.failure.clone().unwrap_or_default();
            let mut help = "run `adage update` again when it can be reached".to_string();
            if channel == Channel::Source {
                help.push_str("; `adage update --branch main` builds the main branch instead");
            }
            return Err(crate::refusal(
                format!("cannot check now: {} ({})", f.class, f.host),
                vec![latest::explain(&f)],
                help,
            ));
        }
    }
    let to = target.as_ref().map(|r| r.version.clone());
    let arrow = to.as_deref().map(|t| format!("{VERSION} → {t}")).unwrap_or_else(|| VERSION.to_string());
    o.header("update", &[&arrow, channel.name()])?;
    o.blank()?;
    o.kv("installed", Line::new().text(detect::tilde(&exe)))?;

    if channel == Channel::Plugin {
        return plugin_update(&mut o, paths, print, to.as_deref());
    }

    // (2) The plan.
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let beside = |b: &str| dir.join(channel::exe_name(b)).is_file();
    let inputs = channel::Inputs {
        channel,
        version: to.as_deref(),
        branch: branch.as_deref(),
        checkout: co.as_ref(),
        beside: &beside,
    };
    let steps = channel::plan(&inputs).context("no update plan for this install")?;
    // A source install with no cargo to build with: the release's binaries are offered instead,
    // never a toolchain install.
    if channel == Channel::Source && detect::on_path(&channel::exe_name("cargo")).is_none() {
        let v = to.as_deref().unwrap_or("<version>");
        let what = "this adage was installed from source, and cargo is not on PATH";
        let offer = format!(
            "`adage update --from release` replaces it with the release's binaries ({}), or install Rust from https://rustup.rs",
            channel::installer_line(v)
        );
        if !print {
            return Err(crate::hint(what, offer));
        }
        o.warn(&format!("{what}: {offer}"))?;
    }
    let kept = keep::installed(&exe);
    o.kv(
        "keeps",
        Line::new()
            .text(format!("{}/", detect::tilde(&keep::dir(paths, VERSION))))
            .quiet(format!(" ({})", names(&kept))),
    )?;
    for s in &steps {
        o.kv("runs", Line::new().cmd(s.shown()))?;
    }
    if print {
        o.blank()?;
        o.note("printed only: nothing was kept, run or changed")?;
        o.next("adage update", "runs it")?;
        return Ok(());
    }
    o.blank()?;
    o.flush()?;

    // (3) Keep.
    let (kept_dir, mut prev) = keep::keep(paths, VERSION, channel.name(), "terminal", &exe, now)?;
    let back = format!("adage update --rollback restores {VERSION} from {}", detect::tilde(&kept_dir));

    // (4) Run.
    for s in &steps {
        run_step(s, &exe, paths, &back)?;
    }

    // (5) Verify.
    let new_bin = installed_bin(channel, &exe);
    let want = match channel {
        Channel::Checkout => co.as_ref().and_then(|c| channel::checkout_version(&c.root)),
        _ => to.clone(),
    };
    let got = probe::version_of(&new_bin);
    let mut o = Out::stdout();
    match (&want, &got) {
        (_, None) => {
            return Err(crate::refusal(
                format!("the update ran, but {} does not answer `version`", detect::tilde(&new_bin)),
                vec![format!("expected adage {}", want.as_deref().unwrap_or("of any version"))],
                back,
            ))
        }
        (Some(w), Some(g)) if w != g => {
            return Err(crate::refusal(
                format!("the update ran, but {} is adage {g}, not {w}", detect::tilde(&new_bin)),
                vec![format!("asked for {w}; the binary answers {g}")],
                back,
            ))
        }
        _ => {}
    }
    let got = got.unwrap_or_default();
    let digest = verify_digest(channel, &got, &new_bin);
    match &digest {
        Digest::Mismatch { want, got: g } => {
            return Err(crate::refusal(
                format!("{} does not match the release's SHA256SUMS", detect::tilde(&new_bin)),
                vec![format!("the release publishes sha256 {want}; the installed binary is {g}")],
                back,
            ))
        }
        Digest::Matched => o.kv(
            "version",
            Line::new().text(format!("adage {got} ")).push(Role::Fire, "✓").quiet(" · sha256 matches the release"),
        )?,
        Digest::Unchecked(why) => {
            o.kv("version", Line::new().text(format!("adage {got} ")).push(Role::Fire, "✓").quiet(format!(" · {why}")))?
        }
    }
    prev.to = Some(got.clone());
    prev.bin = Some(new_bin.clone());
    keep::write(&kept_dir, &prev)?;
    o.flush()?;

    // (6, 7) The new binary finishes; one that cannot (a version from before `--finish`) leaves it
    // to this one.
    let extra: Vec<&str> = if has("--yes") || has("-y") { vec!["--yes"] } else { vec![] };
    match probe::hand_over(&new_bin, VERSION, &extra) {
        Ok(s) if s.success() => Ok(()),
        Ok(s) if s.code() != Some(2) => Err(crate::hint(
            format!("adage {got} is installed, but finishing the update failed ({s})"),
            format!("its output above says why; `adage doctor` checks the install; {back}"),
        )),
        _ => finish(VERSION, args, paths),
    }
}

fn same_file(a: &Path, b: &Path) -> bool {
    a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(x, y)| x == y)
}

/// Where the channel put the new `adage`: Homebrew's link on `PATH` (each version has its own
/// `Cellar` directory), cargo's bin directory for a source build, else where the old one was.
fn installed_bin(channel: Channel, exe: &Path) -> PathBuf {
    let name = channel::exe_name("adage");
    match channel {
        Channel::Brew => detect::on_path(&name).unwrap_or_else(|| exe.to_path_buf()),
        Channel::Source => {
            channel::cargo_bin().map(|b| b.join(&name)).filter(|p| p.is_file()).unwrap_or_else(|| exe.to_path_buf())
        }
        _ => exe.to_path_buf(),
    }
}

/// Run one step with the terminal attached; a failure stops the update with the way back.
fn run_step(s: &Step, exe: &Path, paths: &Paths, back: &str) -> Result<()> {
    let shown = s.shown();
    let (program, argv): (String, Vec<String>) = match s {
        Step::Run { program, args } => (program.clone(), args.clone()),
        Step::Installer { version } => {
            // Fetched first (a file the person can read), then run: no pipe into a shell.
            let scratch = paths.cache().join(format!("installer-{}", std::process::id()));
            std::fs::create_dir_all(&scratch)?;
            let file = scratch.join(if cfg!(windows) { "installer.ps1" } else { "installer.sh" });
            let url = channel::installer_url(version);
            if let Err(f) = crate::net::download(&url, &file, 4 << 20, &mut |_| {})? {
                return Err(crate::refusal(
                    format!("cannot fetch the release installer: {} ({})", f.class, f.host),
                    vec![f.detail],
                    format!("nothing was replaced; run `adage update` again later, or `{shown}` yourself"),
                ));
            }
            let f = file.to_string_lossy().into_owned();
            if cfg!(windows) {
                (
                    "powershell".into(),
                    vec!["-NoProfile".into(), "-ExecutionPolicy".into(), "Bypass".into(), "-File".into(), f],
                )
            } else {
                ("sh".into(), vec![f])
            }
        }
        Step::Companion { name, version } => {
            let into = exe.parent().context("the running adage has no directory")?;
            let scratch = paths.cache().join(format!("companion-{}", std::process::id()));
            let got = crate::net::fetch_companion(name, version, into, &scratch);
            let _ = std::fs::remove_dir_all(&scratch);
            return match got? {
                crate::net::Companion::Placed(p) => {
                    Out::stdout()
                        .kv("fetched", Line::new().text(detect::tilde(&p)).quiet(" · sha256 matches the release"))?;
                    Ok(())
                }
                crate::net::Companion::NoTarget => Ok(()),
                crate::net::Companion::NotPublished(u) => {
                    Out::stdout()
                        .warn(&format!("release v{version} has no {name} for this machine ({u}); the old one stays"))?;
                    Ok(())
                }
                crate::net::Companion::Failed(f) => Err(crate::refusal(
                    format!("cannot fetch {name} from release v{version}: {} ({})", f.class, f.host),
                    vec![f.detail],
                    back.to_string(),
                )),
                crate::net::Companion::Mismatch { archive, want, got } => Err(crate::refusal(
                    format!("{archive} does not match the release's checksum; it was not installed"),
                    vec![format!("the release publishes sha256 {want}; the download is {got}")],
                    back.to_string(),
                )),
            };
        }
    };
    let status = std::process::Command::new(&program).args(&argv).status();
    let _ = std::fs::remove_dir_all(paths.cache().join(format!("installer-{}", std::process::id())));
    let status = match status {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let help = match program.as_str() {
                "cargo" => "install Rust from https://rustup.rs, or `adage update --from release`".to_string(),
                "git" => "install git, then run `adage update` again".into(),
                "npm" => "install Node.js 18 or later, then run `adage update` again".into(),
                "brew" => "install Homebrew from https://brew.sh, then run `adage update` again".into(),
                _ => format!("run `{shown}` yourself"),
            };
            return Err(crate::hint(
                format!("cannot run {program}: it is not on PATH"),
                format!("nothing was replaced; {help}"),
            ));
        }
        Err(e) => return Err(e).with_context(|| format!("cannot run {program}")),
    };
    if !status.success() {
        return Err(crate::hint(
            format!("{program} failed ({status})"),
            format!("its output above says why; run `{shown}` yourself to retry; {back}"),
        ));
    }
    Ok(())
}

/// What checking the installed binary's digest came to.
enum Digest {
    Matched,
    Mismatch {
        want: String,
        got: String,
    },
    /// Not checked here, and why (the channel checks the bytes it fetches itself).
    Unchecked(String),
}

/// Compare the binary npm or the release installer put in place with the release's published
/// `SHA256SUMS` (`<sha256>  <target>/adage`), when the release publishes one.
fn verify_digest(channel: Channel, version: &str, bin: &Path) -> Digest {
    let by_channel = match channel {
        Channel::Npm => "npm checks the package's integrity",
        Channel::Release => "the installer checks the archive's .sha256",
        _ => return Digest::Unchecked("built or upgraded by the channel".into()),
    };
    let Some(target) = crate::net::release_target() else {
        return Digest::Unchecked(format!("no release target for this machine; {by_channel}"));
    };
    let url = format!("{}/v{version}/SHA256SUMS", crate::net::release_base());
    let body = match crate::net::get(&url, latest::TIMEOUT * 3, 64 << 10) {
        Ok(Ok(b)) => b,
        Ok(Err(f)) if f.not_found() => {
            return Digest::Unchecked(format!("the release publishes no SHA256SUMS; {by_channel}"))
        }
        _ => return Digest::Unchecked(format!("the release's SHA256SUMS could not be read; {by_channel}")),
    };
    let name = format!("{target}/{}", channel::exe_name("adage"));
    let want = String::from_utf8_lossy(&body).lines().find_map(|l| {
        let mut it = l.split_whitespace();
        let (sum, file) = (it.next()?, it.next()?);
        (file.trim_start_matches('*') == name).then(|| sum.to_lowercase())
    });
    let Some(want) = want else {
        return Digest::Unchecked(format!("the release's SHA256SUMS names no {name}; {by_channel}"));
    };
    match crate::net::sha256_file(bin) {
        Ok(got) if got == want => Digest::Matched,
        Ok(got) => Digest::Mismatch { want, got },
        Err(e) => Digest::Unchecked(format!("{} cannot be read ({e})", detect::tilde(bin))),
    }
}

/// The installed Claude Code plugin Adage runs from, if `claude` lists one.
fn installed_plugin(cli: &dyn ClaudeCli) -> Option<claude_plugin::Installed> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    claude_plugin::list(cli, &cwd).ok()?.into_iter().find(claude_plugin::Installed::is_adage)
}

/// The JSON result line `claude … --json` prints last, and whether it says `ok`.
fn claude_step(cli: &dyn ClaudeCli, args: &[&str]) -> std::result::Result<serde_json::Value, String> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let ran = cli.run(args, &cwd).map_err(|e| format!("could not run: {e}"))?;
    let line = ran
        .stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<serde_json::Value>(l.trim()).ok().filter(|v| v.is_object()));
    match line {
        Some(v) if ran.code == Some(0) && v["outcome"] == "ok" => Ok(v),
        v => {
            let said = v
                .as_ref()
                .and_then(|v| v["message"].as_str().map(str::to_string))
                .or_else(|| ran.stderr.lines().rev().find(|l| !l.trim().is_empty()).map(str::to_string))
                .unwrap_or_default();
            Err(format!("failed (exit {}): {said}", ran.code.map(|c| c.to_string()).unwrap_or_else(|| "?".into())))
        }
    }
}

/// The plugin channel: Claude Code updates its own plugin. With `claude` on `PATH`, `claude plugin
/// marketplace update <marketplace>` and `claude plugin update <id>`; without it, the `/plugin`
/// steps and nothing run. The running session keeps the binary it started with: the new one takes
/// effect in the next session.
fn plugin_update<W: std::io::Write>(o: &mut Out<W>, paths: &Paths, print: bool, to: Option<&str>) -> Result<()> {
    let manual = |o: &mut Out<W>| -> Result<()> {
        o.blank()?;
        o.note("This adage came with the Claude Code plugin, which Claude Code updates.")?;
        o.next("/plugin", "in Claude Code: choose adage, then Update")?;
        o.also("adage update --from source", "or install a separate copy on PATH")?;
        Ok(())
    };
    let Some(cli) = claude_plugin::System::on_path() else { return manual(o) };
    let installed = installed_plugin(&cli);
    // The plugin `adage init` staged from a terminal install: that install is what updates.
    if installed.as_ref().is_some_and(|p| p.id == claude_plugin::PLUGIN_ID) {
        o.blank()?;
        o.note(&format!(
            "This plugin was staged by adage init from the adage installed in a terminal ({}); updating that one stages it again.",
            detect::tilde(&paths.plugin())
        ))?;
        o.next("adage update", "in a terminal, with the adage on PATH")?;
        return Ok(());
    }
    let id = installed.as_ref().map(|p| p.id.clone()).unwrap_or_else(|| "adage@adage".into());
    let market = id.split_once('@').map(|(_, m)| m.to_string()).unwrap_or_else(|| "adage".into());
    let refresh = ["plugin", "marketplace", "update", market.as_str(), "--json"];
    let update = ["plugin", "update", id.as_str(), "--json"];
    for s in [&refresh[..], &update[..]] {
        o.kv("runs", Line::new().cmd(format!("claude {}", s[..s.len() - 1].join(" "))))?;
    }
    if print {
        o.blank()?;
        o.note("printed only: nothing was run")?;
        return Ok(());
    }
    o.flush()?;
    if let Err(e) = claude_step(&cli, &refresh) {
        o.warn(&format!("claude plugin marketplace update {market} {e}"))?;
    }
    let done = claude_step(&cli, &update).map_err(|e| {
        crate::hint(format!("claude plugin update {id} {e}"), "in Claude Code: /plugin, choose adage, then Update")
    })?;
    let (from, now) = (
        done["oldVersion"].as_str().unwrap_or(VERSION).to_string(),
        done["newVersion"].as_str().map(str::to_string).or(to.map(str::to_string)).unwrap_or_else(|| VERSION.into()),
    );
    if done["updateOutcome"] == "up_to_date" {
        o.kv("plugin", Line::new().text(format!("adage {now} is the latest the marketplace has")))?;
        return Ok(());
    }
    o.kv("plugin", Line::new().text(format!("adage {from} → {now}")))?;
    o.kv("effect", Line::new().text("next session"))?;
    o.blank()?;
    o.kv("done", Line::new().bold(Role::Ink, format!("adage {now}")).sep().quiet("takes effect in your next session"))?;
    Ok(())
}

/// `adage update --finish <from> [--rollback] [--yes]`: run by the binary an update (or a
/// rollback) installed, from `previous/<from>/previous.json`: re-point the registrations, stage
/// the Claude Code plugin again, doctor's summary, the release notes, `done`.
fn finish(from: &str, args: &[String], paths: &Paths) -> Result<()> {
    let has = |f: &str| args.iter().any(|a| a == f);
    let rollback = has("--rollback") || has("--back");
    let dir = keep::dir(paths, from);
    let prev = keep::read(&dir)
        .with_context(|| format!("no update from {from} to finish ({} is missing)", dir.join(keep::FILE).display()))?;
    // After an update, registrations follow the new binary from the old paths; after a rollback,
    // the old binary back from where the update put it.
    let new_adage = prev.bin.clone().unwrap_or(running()?);
    let (target, old) = if rollback {
        let target = prev.paths.get("adage").cloned().unwrap_or_else(|| new_adage.clone());
        let dir = new_adage.parent().map(Path::to_path_buf).unwrap_or_default();
        let old = channel::BINARIES.iter().map(|n| (n.to_string(), dir.join(channel::exe_name(n)))).collect();
        (target, old)
    } else {
        (new_adage.clone(), prev.paths.clone())
    };
    let version = if rollback { prev.from.clone() } else { prev.to.clone().unwrap_or_else(|| VERSION.into()) };
    let root = std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d));
    let mut o = Out::stdout();

    // (6) Registrations.
    let found = repoint::plan(&old, &target, root.as_deref());
    let mode = match person_at_terminal() {
        Some(true) => repoint::Mode::Write { ask: !(has("--yes") || has("-y")) },
        Some(false) => repoint::Mode::Write { ask: false },
        None => repoint::Mode::Report,
    };
    repoint::apply(&mut o, &found, mode)?;
    for r in detect::registrations(root.as_deref()).into_iter().filter(|r| r.kind != "plugin-data") {
        if r.binary.as_deref().is_some_and(|b| {
            same_file(b, &target) || same_file(b, &target.with_file_name(channel::exe_name("adage-exec")))
        }) {
            o.kv(
                "hooks",
                Line::new().text(format!("{} {}", r.harness, detect::tilde(&r.location))).quiet(" runs this binary"),
            )?;
        }
    }

    restage_plugin(&mut o, paths, &target, root.as_deref())?;

    // (7) Doctor's summary, the notes, done.
    let checks = crate::doctor::checks(paths, root.as_deref());
    let count = |s: crate::doctor::Status| checks.iter().filter(|c| c.status == s).count();
    let (w, f) = (count(crate::doctor::Status::Warn), count(crate::doctor::Status::Fail));
    let first = checks
        .iter()
        .find(|c| c.status == crate::doctor::Status::Fail)
        .or_else(|| checks.iter().find(|c| c.status == crate::doctor::Status::Warn));
    let lamp = match (f, w) {
        (0, 0) => Line::new().push(Role::Fire, "● ").text("healthy"),
        (0, w) => Line::new().push(Role::Prime, "● ").text(format!("{w} to look at")),
        (f, _) => Line::new().push(Role::Inhibit, "● ").text(format!("{f} failing")),
    };
    let lamp = match first.and_then(|c| c.fix.as_ref().map(|fx| (c, fx))) {
        Some((c, fx)) => lamp.sep().quiet(format!("{}: ", c.label)).cmd(fx.next()),
        None => lamp,
    };
    o.kv("doctor", lamp)?;
    if !rollback {
        if let Some(notes) =
            Latest::read(paths).and_then(|l| l.latest).filter(|r| r.version == version).and_then(|r| r.notes)
        {
            for (i, n) in notes.lines().enumerate() {
                o.kv(if i == 0 { "changed" } else { "" }, Line::new().quiet(format!("· {n}")))?;
            }
        }
        keep::prune(paths, from);
    }
    o.blank()?;
    let what = if rollback { format!("adage {version} restored") } else { format!("adage {version}") };
    o.kv("done", Line::new().bold(Role::Ink, what).sep().quiet("takes effect in new sessions"))?;
    if rollback {
        o.next("adage update", &format!("when you want {} again", prev.to.as_deref().unwrap_or("the newer release")))?;
    } else {
        o.next("adage update --rollback", &format!("if anything is wrong: puts {} back", prev.from))?;
    }
    Ok(())
}

/// Is there a Claude Code plugin `adage init` staged on this machine (`$ADAGE_HOME/plugin`)?
fn local_plugin(paths: &Paths) -> bool {
    paths.plugin().join(".claude-plugin/marketplace.json").is_file()
}

/// Does the local plugin bundle another `adage` than `bin`?
fn local_plugin_stale(paths: &Paths, bin: &Path) -> bool {
    let staged =
        claude_plugin::plugin_dir(paths).join("dist").join(claude_plugin::target()).join(channel::exe_name("adage"));
    local_plugin(paths) && crate::net::sha256_file(&staged).ok() != crate::net::sha256_file(bin).ok()
}

/// Stage the Claude Code plugin `adage init` staged (`$ADAGE_HOME/plugin`) again from `bin` and its
/// companions, and refresh Claude Code's copy of the marketplace (`claude plugin marketplace
/// update adage-local`, when `claude` is on `PATH`; otherwise the command is named). Nothing when
/// there is no local plugin. Never fails the update: the plugin keeps working with the binaries it
/// has until it is staged again.
fn restage_plugin<W: std::io::Write>(o: &mut Out<W>, paths: &Paths, bin: &Path, root: Option<&Path>) -> Result<()> {
    if !local_plugin(paths) {
        return Ok(());
    }
    let cwd =
        root.map(Path::to_path_buf).or_else(|| std::env::current_dir().ok()).unwrap_or_else(|| PathBuf::from("."));
    let again = |o: &mut Out<W>, s: &claude_plugin::Staged| {
        o.kv(
            "plugin",
            Line::new()
                .text(format!("staged again in {}", detect::tilde(&s.plugin)))
                .quiet(" · takes effect in the next Claude Code session"),
        )
    };
    match claude_plugin::System::on_path() {
        Some(cli) => match claude_plugin::refresh(&cli, paths, bin, &cwd) {
            Ok(Some(s)) => again(o, &s)?,
            Ok(None) => {}
            Err(e) => {
                o.warn(&format!("the Claude Code plugin was not staged again ({e:#}); `adage init` stages it"))?
            }
        },
        None => match claude_plugin::stage(paths, bin) {
            Ok(s) => {
                again(o, &s)?;
                o.kv(
                    "",
                    Line::new()
                        .quiet("then, where claude runs: ")
                        .cmd(format!("claude plugin marketplace update {}", claude_plugin::MARKETPLACE)),
                )?;
            }
            Err(e) => {
                o.warn(&format!("the Claude Code plugin was not staged again ({e:#}); `adage init` stages it"))?
            }
        },
    }
    Ok(())
}

/// `adage update --rollback [--print] [--yes]`: put back the binaries the last update kept, byte
/// for byte, then let them finish (re-point the registrations, doctor).
fn rollback(args: &[String], paths: &Paths) -> Result<()> {
    let has = |f: &str| args.iter().any(|a| a == f);
    let Some((dir, mut prev)) = keep::newest(paths) else {
        return Err(crate::hint(
            format!("nothing to roll back to: no update has kept binaries in {}", detect::tilde(&paths.previous())),
            "`adage update` keeps the binaries it replaces there",
        ));
    };
    let mut o = Out::stdout();
    let now_version = prev.to.clone().unwrap_or_else(|| VERSION.to_string());
    o.header("update --rollback", &[&format!("{now_version} → {}", prev.from), &prev.channel])?;
    o.blank()?;
    for (name, to) in &prev.paths {
        o.kv("restores", Line::new().text(format!("{name} ")).quiet(format!("to {}", detect::tilde(to))))?;
    }
    o.kv("from", Line::new().text(format!("{}/", detect::tilde(&dir))))?;
    if has("--print") {
        o.blank()?;
        o.note("printed only: nothing was restored")?;
        o.next("adage update --rollback", "restores them")?;
        return Ok(());
    }
    let restored = keep::restore(&dir, &prev)?;
    prev.restored_at = Some(now());
    keep::write(&dir, &prev)?;
    let adage = restored.iter().find(|(n, _)| n == "adage").map(|(_, p)| p.clone());
    let answered = adage.as_deref().and_then(probe::version_of);
    o.kv(
        "version",
        Line::new()
            .text(format!("adage {} ", answered.as_deref().unwrap_or("?")))
            .push(Role::Fire, "✓")
            .quiet(" · byte for byte as kept"),
    )?;
    o.flush()?;
    let extra: Vec<&str> = if has("--yes") || has("-y") { vec!["--rollback", "--yes"] } else { vec!["--rollback"] };
    let finished = match &adage {
        Some(a) if answered.as_deref() == Some(prev.from.as_str()) => Some(probe::hand_over(a, &prev.from, &extra)),
        _ => None,
    };
    match finished {
        Some(Ok(s)) if s.success() => Ok(()),
        Some(Ok(s)) if s.code() != Some(2) => Err(crate::hint(
            format!("adage {} is back, but finishing failed ({s})", prev.from),
            "its output above says why; `adage doctor` checks the install",
        )),
        _ => {
            let mut a: Vec<String> = args.to_vec();
            a.push("--rollback".into());
            finish(&prev.from, &a, paths)
        }
    }
}
