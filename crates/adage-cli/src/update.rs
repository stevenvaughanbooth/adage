//! `adage update [--from source|npm|brew] [--branch <name>] [--print]` — replace this install of
//! Adage with the latest one, the same way it was installed.
//!
//! **Where from.** The install is read from where the running binary lives ([`detect`]): inside
//! `node_modules` it came from npm, under a Homebrew `Cellar` from the tap, inside Claude Code's
//! plugin directory from the marketplace, under `<checkout>/target/<profile>` of a git checkout of
//! this workspace it was built there, and anywhere else it was installed from source. `--from`
//! overrides the guess. Until the first release is published (README, Install), building from
//! source is the channel that works: `cargo install --locked --git <repo> --branch main` for
//! `adage`, `adage-exec` and `adage-train`, which cargo skips when the branch has not moved. It
//! fetches with the git CLI (`net.git-fetch-with-cli`), so a private repository authenticates the
//! way `git` does. A checkout build updates in place instead: `git pull --ff-only` on its current
//! branch, then `cargo build` of the same crates and profile, so hooks registered with that
//! binary's path run the new build.
//!
//! **What runs.** Package manager commands, printed first, with the terminal attached, so their
//! own progress and errors show as they are. `--print` prints them without running them. A plugin
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
    Checkout,
    Npm,
    Brew,
    Plugin,
}

impl Channel {
    fn name(self) -> &'static str {
        match self {
            Channel::Source => "source",
            Channel::Checkout => "checkout",
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

/// A git checkout of this workspace that built the binary at `exe`.
struct Checkout {
    root: PathBuf,
    profile: String,
}

/// `exe` is `<root>/target/<profile>/adage`, and `<root>` is a git checkout with a `Cargo.toml`.
fn checkout(exe: &Path) -> Option<Checkout> {
    let profile_dir = exe.parent()?;
    let target = profile_dir.parent()?;
    if target.file_name()? != "target" {
        return None;
    }
    let root = target.parent()?;
    if !root.join("Cargo.toml").is_file() || !root.join(".git").exists() {
        return None;
    }
    Some(Checkout { root: root.to_path_buf(), profile: profile_dir.file_name()?.to_string_lossy().into_owned() })
}

fn exe_name(bin: &str) -> String {
    format!("{bin}{}", std::env::consts::EXE_SUFFIX)
}

type Step = (String, Vec<String>);

/// The package manager commands that update `channel`, in order. `beside` says which companion
/// binaries sit next to this one, for a Homebrew install (one formula each).
fn plan(channel: Channel, branch: &str, co: Option<&Checkout>, beside: &dyn Fn(&str) -> bool) -> Option<Vec<Step>> {
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let crates = || CRATES.iter().map(|(c, _)| c.to_string());
    match channel {
        Channel::Source => {
            let mut args = s(&[
                "--config",
                "net.git-fetch-with-cli=true",
                "install",
                "--locked",
                "--git",
                REPO,
                "--branch",
                branch,
            ]);
            args.extend(crates());
            Some(vec![("cargo".into(), args)])
        }
        Channel::Checkout => {
            let co = co?;
            let root = co.root.to_string_lossy().into_owned();
            let manifest = co.root.join("Cargo.toml").to_string_lossy().into_owned();
            let mut build = s(&["build", "--locked", "--manifest-path", &manifest]);
            match co.profile.as_str() {
                "release" => build.push("--release".into()),
                "debug" => {}
                other => build.extend(s(&["--profile", other])),
            }
            build.extend(crates().flat_map(|c| ["-p".to_string(), c]));
            Some(vec![("git".into(), s(&["-C", &root, "pull", "--ff-only"])), ("cargo".into(), build)])
        }
        Channel::Npm => Some(vec![("npm".into(), s(&["install", "--global", &format!("{NPM_PACKAGE}@latest")]))]),
        Channel::Brew => {
            let mut args = s(&["upgrade"]);
            args.extend(CRATES.iter().filter(|(_, b)| *b == "adage" || beside(b)).map(|(_, b)| format!("{TAP}/{b}")));
            Some(vec![("brew".into(), args)])
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
        None => match detect(&exe) {
            Channel::Source if checkout(&exe).is_some() => Channel::Checkout,
            c => c,
        },
    };
    let co = if channel == Channel::Checkout { checkout(&exe) } else { None };
    if let (Some(co), Some(b)) = (&co, flag(args, "--branch")) {
        return Err(crate::hint(
            format!(
                "this adage is built from the checkout at {}, so --branch {b} does not apply",
                crate::detect::tilde(&co.root)
            ),
            "switch branches there with git and run `adage update` again, or pass --from source",
        ));
    }
    let branch = flag(args, "--branch").unwrap_or_else(|| BRANCH.to_string());
    let dir = exe.parent().map(Path::to_path_buf).unwrap_or_default();
    let beside = |b: &str| dir.join(exe_name(b)).is_file();

    let mut o = Out::stdout();
    let from = match (&co, channel) {
        (Some(co), _) => format!("{} (git pull, cargo build)", crate::detect::tilde(&co.root)),
        (None, Channel::Source) => format!("{REPO} ({branch})"),
        _ => channel.name().to_string(),
    };
    o.header("update", &[&format!("adage {}", env!("CARGO_PKG_VERSION")), channel.name()])?;
    o.blank()?;
    o.kv("installed", Line::new().text(crate::detect::tilde(&exe)))?;
    o.kv("from", Line::new().text(&from))?;

    let Some(steps) = plan(channel, &branch, co.as_ref(), &beside) else {
        o.blank()?;
        o.note("This adage came with the Claude Code plugin, which Claude Code updates.")?;
        o.next("/plugin", "in Claude Code: update adage from the marketplace")?;
        o.also("adage update --from source", "or install a separate copy on PATH from source")?;
        return Ok(());
    };
    let shown: Vec<String> = steps
        .iter()
        .map(|(program, argv)| {
            std::iter::once(program.as_str()).chain(argv.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
        })
        .collect();
    for cmd in &shown {
        o.kv("runs", Line::new().cmd(cmd))?;
    }
    if args.iter().any(|a| a == "--print") {
        return Ok(());
    }
    o.blank()?;
    o.flush()?;

    for ((program, argv), shown) in steps.iter().zip(&shown) {
        let status = std::process::Command::new(program).args(argv).status();
        let status = match status {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let help = match program.as_str() {
                    "cargo" => "install Rust from https://rustup.rs, then run `adage update` again",
                    "git" => "install git, then run `adage update` again",
                    "npm" => "install Node.js 18 or later, then run `adage update` again",
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
        assert_eq!(d("/repo/target/release/adage"), Channel::Source); // a checkout only if the repo is on disk
        assert_eq!(d("/usr/local/lib/node_modules/@adage-sh/cli-darwin-arm64/bin/adage"), Channel::Npm);
        assert_eq!(d("/opt/homebrew/Cellar/adage/0.1.0/bin/adage"), Channel::Brew);
        assert_eq!(d("/Users/a/.claude/plugins/cache/adage/adage/0.1.0/bin/adage"), Channel::Plugin);
    }

    #[test]
    fn each_channel_runs_its_package_manager() {
        let none = |_: &str| false;
        let one = |c: Channel, beside: &dyn Fn(&str) -> bool| {
            let mut steps = plan(c, "main", None, beside).unwrap();
            assert_eq!(steps.len(), 1);
            let (p, a) = steps.remove(0);
            (p, a.join(" "))
        };
        let (p, a) = one(Channel::Source, &none);
        assert_eq!(p, "cargo");
        assert_eq!(
            a,
            format!("--config net.git-fetch-with-cli=true install --locked --git {REPO} --branch main adage-cli adage-exec adage-train")
        );
        assert_eq!(one(Channel::Npm, &none), ("npm".into(), "install --global @adage-sh/cli@latest".to_string()));
        assert_eq!(one(Channel::Brew, &none).1, "upgrade stevenvaughanbooth/tap/adage");
        assert_eq!(
            one(Channel::Brew, &|b: &str| b == "adage-train").1,
            "upgrade stevenvaughanbooth/tap/adage stevenvaughanbooth/tap/adage-train"
        );
        assert!(plan(Channel::Plugin, "main", None, &none).is_none());
        assert!(plan(Channel::Checkout, "main", None, &none).is_none());
    }

    #[test]
    fn a_checkout_build_pulls_and_rebuilds_in_place() {
        let dir = std::env::temp_dir().join(format!("adage-update-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("target/release")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "").unwrap();
        let exe = dir.join("target/release/adage");
        let co = checkout(&exe).expect("a checkout");
        assert_eq!(co.profile, "release");
        let steps = plan(Channel::Checkout, "main", Some(&co), &|_: &str| false).unwrap();
        let root = dir.to_string_lossy();
        assert_eq!(steps[0], ("git".into(), vec!["-C".into(), root.to_string(), "pull".into(), "--ff-only".into()]));
        assert_eq!(
            steps[1].1.join(" "),
            format!(
                "build --locked --manifest-path {root}/Cargo.toml --release -p adage-cli -p adage-exec -p adage-train"
            )
        );
        assert!(checkout(&dir.join("target/release/sub/adage")).is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
