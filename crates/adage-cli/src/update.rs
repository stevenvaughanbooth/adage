//! `adage update [--from source|npm|brew] [--branch <name>] [--print]` — replace this install of
//! Adage with the latest one, the same way it was installed.
//!
//! **Where from.** The install is read from where the running binary lives ([`detect`]): inside
//! `node_modules` it came from npm, under a Homebrew `Cellar` from the tap, inside Claude Code's
//! plugin directory from the marketplace, and anywhere else it was built from source. `--from`
//! overrides the guess. Until the first release is published (README, Install), building from
//! source is the channel that works: `cargo install --locked --git <repo> --branch main` for
//! `adage`, `adage-exec` and `adage-train`, which cargo skips when the branch has not moved.
//!
//! **What runs.** One package manager command, printed first, with the terminal attached, so its
//! own progress and errors show as they are. `--print` prints it without running it. A plugin
//! install is updated from Claude Code, not from here. This is the one command that replaces
//! Adage's own binaries, and only when a person runs it: no hook or learner calls it.

use adage_term::{Line, Out};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

const REPO: &str = "https://github.com/stevenvaughanbooth/adage";
const BRANCH: &str = "main";
/// npm/config.mjs `PACKAGE_NAME`.
const NPM_PACKAGE: &str = "@adage-sh/cli";
/// The Homebrew tap (dist-workspace.toml `tap`) and the formula for each binary.
const TAP: &str = "stevenvaughanbooth/tap";
/// The crates (and binaries) a source install builds: `adage`, then its companions.
const CRATES: &[(&str, &str)] = &[("adage-cli", "adage"), ("adage-exec", "adage-exec"), ("adage-train", "adage-train")];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channel {
    Source,
    Npm,
    Brew,
    Plugin,
}

impl Channel {
    fn name(self) -> &'static str {
        match self {
            Channel::Source => "source",
            Channel::Npm => "npm",
            Channel::Brew => "brew",
            Channel::Plugin => "plugin",
        }
    }
}

/// How the binary at `exe` (symlinks resolved) was installed.
fn detect(exe: &Path) -> Channel {
    let parts: Vec<String> = exe.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    let has = |p: &str| parts.iter().any(|c| c == p);
    let pair = |a: &str, b: &str| parts.windows(2).any(|w| w[0] == a && w[1] == b);
    if has("node_modules") {
        Channel::Npm
    } else if has("cellar") {
        Channel::Brew
    } else if pair(".claude", "plugins") {
        Channel::Plugin
    } else {
        Channel::Source
    }
}

fn exe_name(bin: &str) -> String {
    format!("{bin}{}", std::env::consts::EXE_SUFFIX)
}

/// The package manager command that updates `channel` (program, arguments). `beside` says which
/// companion binaries sit next to this one, for a Homebrew install (one formula each).
fn plan(channel: Channel, branch: &str, beside: &dyn Fn(&str) -> bool) -> Option<(String, Vec<String>)> {
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    match channel {
        Channel::Source => {
            let mut args = s(&["install", "--locked", "--git", REPO, "--branch", branch]);
            args.extend(CRATES.iter().map(|(c, _)| c.to_string()));
            Some(("cargo".into(), args))
        }
        Channel::Npm => Some(("npm".into(), s(&["install", "--global", &format!("{NPM_PACKAGE}@latest")]))),
        Channel::Brew => {
            let mut args = s(&["upgrade"]);
            args.extend(CRATES.iter().filter(|(_, b)| *b == "adage" || beside(b)).map(|(_, b)| format!("{TAP}/{b}")));
            Some(("brew".into(), args))
        }
        Channel::Plugin => None,
    }
}

fn flag(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// Where `cargo install` puts binaries: `$CARGO_HOME/bin`, else `~/.cargo/bin`.
fn cargo_bin() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join(".cargo"))
    })?;
    Some(home.join("bin"))
}

pub fn run(args: &[String]) -> Result<()> {
    let exe = std::env::current_exe().context("cannot find the running adage binary")?;
    let exe = exe.canonicalize().unwrap_or(exe);
    let channel = match flag(args, "--from").as_deref() {
        Some("source") => Channel::Source,
        Some("npm") => Channel::Npm,
        Some("brew") => Channel::Brew,
        Some(other) => {
            return Err(crate::hint(format!("unknown install channel {other:?}"), "use --from source, npm or brew"))
        }
        None => detect(&exe),
    };
    let branch = flag(args, "--branch").unwrap_or_else(|| BRANCH.to_string());
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let beside = |b: &str| dir.join(exe_name(b)).is_file();

    let mut o = Out::stdout();
    let from = if channel == Channel::Source { format!("{REPO} ({branch})") } else { channel.name().to_string() };
    o.header("update", &[&format!("adage {}", env!("CARGO_PKG_VERSION")), channel.name()])?;
    o.blank()?;
    o.kv("installed", Line::new().text(crate::detect::tilde(&exe)))?;
    o.kv("from", Line::new().text(&from))?;

    let Some((program, argv)) = plan(channel, &branch, &beside) else {
        o.blank()?;
        o.note("This adage came with the Claude Code plugin, which Claude Code updates.")?;
        o.next("/plugin", "in Claude Code: update adage from the marketplace")?;
        o.also("adage update --from source", "or install a separate copy on PATH from source")?;
        return Ok(());
    };
    let shown = std::iter::once(program.as_str()).chain(argv.iter().map(String::as_str)).collect::<Vec<_>>().join(" ");
    o.kv("runs", Line::new().cmd(&shown))?;
    if args.iter().any(|a| a == "--print") {
        return Ok(());
    }
    o.blank()?;
    o.flush()?;

    let status = std::process::Command::new(&program).args(&argv).status();
    let status = match status {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let help = match channel {
                Channel::Source => "install Rust from https://rustup.rs, then run `adage update` again",
                Channel::Npm => "install Node.js 18 or later, then run `adage update` again",
                _ => "install Homebrew from https://brew.sh, then run `adage update` again",
            };
            return Err(crate::hint(format!("cannot run {program}: it is not on PATH"), help));
        }
        Err(e) => return Err(e).with_context(|| format!("cannot run {program}")),
    };
    if !status.success() {
        return Err(crate::hint(
            format!("{program} failed ({status})"),
            format!("its output above says why; run `{shown}` yourself to retry"),
        ));
    }

    let mut o = Out::stdout();
    o.blank()?;
    o.done("adage updated", &from)?;
    // cargo installs into its own bin directory; if this adage runs from somewhere else, the one
    // on PATH may still be the old build.
    if channel == Channel::Source {
        if let Some(bin) = cargo_bin() {
            let bin = bin.canonicalize().unwrap_or(bin);
            if exe.parent() != Some(bin.as_path()) {
                o.warn(&format!(
                    "this adage runs from {}, not {}: check which one is first on PATH",
                    crate::detect::tilde(&dir),
                    crate::detect::tilde(&bin)
                ))?;
            }
        }
    }
    o.next("adage version", "the version now installed")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_channel_is_read_from_where_the_binary_lives() {
        let d = |p: &str| detect(Path::new(p));
        assert_eq!(d("/Users/a/.cargo/bin/adage"), Channel::Source);
        assert_eq!(d("/repo/target/release/adage"), Channel::Source);
        assert_eq!(d("/usr/local/lib/node_modules/@adage-sh/cli-darwin-arm64/bin/adage"), Channel::Npm);
        assert_eq!(d("/opt/homebrew/Cellar/adage/0.1.0/bin/adage"), Channel::Brew);
        assert_eq!(d("/Users/a/.claude/plugins/cache/adage/adage/0.1.0/bin/adage"), Channel::Plugin);
    }

    #[test]
    fn each_channel_runs_its_package_manager() {
        let none = |_: &str| false;
        let (p, a) = plan(Channel::Source, "main", &none).unwrap();
        assert_eq!(p, "cargo");
        assert_eq!(
            a.join(" "),
            format!("install --locked --git {REPO} --branch main adage-cli adage-exec adage-train")
        );
        let (p, a) = plan(Channel::Npm, "main", &none).unwrap();
        assert_eq!((p.as_str(), a.join(" ")), ("npm", "install --global @adage-sh/cli@latest".to_string()));
        let (_, a) = plan(Channel::Brew, "main", &none).unwrap();
        assert_eq!(a.join(" "), "upgrade stevenvaughanbooth/tap/adage");
        let (_, a) = plan(Channel::Brew, "main", &|b: &str| b == "adage-train").unwrap();
        assert_eq!(a.join(" "), "upgrade stevenvaughanbooth/tap/adage stevenvaughanbooth/tap/adage-train");
        assert!(plan(Channel::Plugin, "main", &none).is_none());
    }
}
