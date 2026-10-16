//! How this adage was installed, and the commands that replace it with a given release
//! ([`plan`]), pinned to that release's version wherever the channel can pin one.

use adage_store::Paths;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub(crate) const REPO: &str = "https://github.com/stevenvaughanbooth/adage";
/// npm/config.mjs `PACKAGE_NAME`.
pub(crate) const NPM_PACKAGE: &str = "@adage-sh/cli";
/// The Homebrew tap (dist-workspace.toml `tap`) and the formula for each binary.
const TAP: &str = "stevenvaughanbooth/tap";
/// The crates (and binaries) a source install builds: `adage`, then its companions.
pub(crate) const CRATES: &[(&str, &str)] =
    &[("adage-cli", "adage"), ("adage-exec", "adage-exec"), ("adage-train", "adage-train")];
/// The binaries an install carries, in the order they are kept and restored.
pub(crate) const BINARIES: [&str; 3] = ["adage", "adage-exec", "adage-train"];
/// The release installer dist publishes with every release (`dist-workspace.toml` `installers`).
const INSTALLER: &str = "adage-cli-installer";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Channel {
    /// `cargo install --git`: rebuilt from the repository at the release's tag.
    Source,
    /// Built in a git checkout of this workspace: pulled and rebuilt in place.
    Checkout,
    Npm,
    Brew,
    /// The release's shell (or PowerShell) installer, with `adage-exec` and `adage-train` fetched
    /// beside it by `adage init`'s Binaries step.
    Release,
    /// The Claude Code plugin's own binary: Claude Code updates the plugin.
    Plugin,
}

impl Channel {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Channel::Source => "source",
            Channel::Checkout => "checkout",
            Channel::Npm => "npm",
            Channel::Brew => "brew",
            Channel::Release => "release",
            Channel::Plugin => "plugin",
        }
    }

    pub(crate) fn parse(s: &str) -> Option<Channel> {
        [Channel::Source, Channel::Checkout, Channel::Npm, Channel::Brew, Channel::Release, Channel::Plugin]
            .into_iter()
            .find(|c| c.name() == s)
    }

    /// Does this channel install a release by its version (so `adage update` needs to know the
    /// newest one before it can run)? A checkout follows its branch, Homebrew its tap, and the
    /// plugin Claude Code's marketplace.
    pub(crate) fn pins(self) -> bool {
        matches!(self, Channel::Source | Channel::Npm | Channel::Release)
    }
}

/// How the binary at `exe` (symlinks resolved) was installed: inside Claude Code's plugin
/// directories it is the plugin's (whatever `CLAUDE_CONFIG_DIR` is), inside `node_modules`
/// npm's, under a Homebrew `Cellar` the tap's, under `<checkout>/target/<profile>` of a git
/// checkout of this workspace a checkout build; where the release installer put it (its receipt,
/// or `adage init`'s `channel.json`) a release; anywhere else a source install.
pub(crate) fn detect(exe: &Path, paths: &Paths) -> Channel {
    let parts: Vec<String> = exe.components().map(|c| c.as_os_str().to_string_lossy().to_lowercase()).collect();
    let has = |p: &str| parts.iter().any(|c| c == p);
    if adage_store::release::plugin_binary(exe) {
        Channel::Plugin
    } else if has("node_modules") {
        Channel::Npm
    } else if has("cellar") {
        Channel::Brew
    } else if checkout(exe).is_some() {
        Channel::Checkout
    } else if released_into(paths, exe.parent().unwrap_or(exe)) {
        Channel::Release
    } else {
        Channel::Source
    }
}

/// `$ADAGE_HOME/channel.json`, written by `adage init`'s Binaries step.
#[derive(Debug, Default, Deserialize)]
struct ChannelFile {
    #[serde(default)]
    channel: String,
    /// The directory the release's `adage` was installed into, when recorded.
    #[serde(default)]
    bin_dir: Option<PathBuf>,
}

/// dist's install receipt (`<config>/adage-cli/adage-cli-receipt.json`): where its installer put
/// the binaries.
#[derive(Debug, Default, Deserialize)]
struct Receipt {
    #[serde(default)]
    install_prefix: Option<PathBuf>,
}

/// Was `dir` filled by the release installer? `channel.json` says `release` (for `dir`, when it
/// names one), or dist's receipt names `dir` (or its parent, `~/.cargo` for `~/.cargo/bin`).
fn released_into(paths: &Paths, dir: &Path) -> bool {
    let same =
        |a: &Path, b: &Path| a == b || a.canonicalize().ok().zip(b.canonicalize().ok()).is_some_and(|(x, y)| x == y);
    if let Some(c) = read::<ChannelFile>(&paths.channel_file()).filter(|c| c.channel == "release") {
        return c.bin_dir.as_deref().is_none_or(|b| same(b, dir));
    }
    receipt_dirs()
        .into_iter()
        .filter_map(|r| read::<Receipt>(&r)?.install_prefix)
        .any(|p| same(&p, dir) || same(&p.join("bin"), dir))
}

/// Where dist's installer writes its receipt: `$XDG_CONFIG_HOME` (else `~/.config`) on Unix,
/// `%LOCALAPPDATA%` on Windows.
fn receipt_dirs() -> Vec<PathBuf> {
    let file = |d: PathBuf| d.join("adage-cli").join("adage-cli-receipt.json");
    let env = |v: &str| std::env::var_os(v).filter(|s| !s.is_empty()).map(PathBuf::from);
    let mut out = Vec::new();
    if cfg!(windows) {
        out.extend(env("LOCALAPPDATA").map(file));
    } else {
        out.extend(env("XDG_CONFIG_HOME").or_else(|| adage_store::home_dir().map(|h| h.join(".config"))).map(file));
    }
    out
}

fn read<T: serde::de::DeserializeOwned>(p: &Path) -> Option<T> {
    let bytes = std::fs::read(p).ok()?;
    (bytes.len() < 1 << 20).then(|| serde_json::from_slice(&bytes).ok()).flatten()
}

/// A git checkout of this workspace that built the binary at `exe`.
#[derive(Debug, Clone)]
pub(crate) struct Checkout {
    pub(crate) root: PathBuf,
    pub(crate) profile: String,
}

/// `exe` is `<root>/target/<profile>/adage`, and `<root>` is a git checkout with a `Cargo.toml`.
pub(crate) fn checkout(exe: &Path) -> Option<Checkout> {
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

/// The version a checkout builds: `[workspace.package] version` (else `[package] version`) of its
/// `Cargo.toml`.
pub(crate) fn checkout_version(root: &Path) -> Option<String> {
    let t: toml::Table = std::fs::read_to_string(root.join("Cargo.toml")).ok()?.parse().ok()?;
    let ws = t.get("workspace").and_then(|w| w.get("package")).and_then(|p| p.get("version"));
    ws.or_else(|| t.get("package").and_then(|p| p.get("version"))).and_then(|v| v.as_str()).map(str::to_string)
}

pub(crate) fn exe_name(bin: &str) -> String {
    format!("{bin}{}", std::env::consts::EXE_SUFFIX)
}

/// One step of an update: a program and its arguments, run with the terminal attached; or the
/// release installer, fetched first and then run (shown as the one line a person would type).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Step {
    Run {
        program: String,
        args: Vec<String>,
    },
    Installer {
        version: String,
    },
    /// `adage-exec` or `adage-train` from the release, verified against its `.sha256`, into the
    /// directory `adage` lives in.
    Companion {
        name: String,
        version: String,
    },
}

impl Step {
    fn run(program: &str, args: Vec<String>) -> Step {
        Step::Run { program: program.into(), args }
    }

    /// The step as one line a person could type.
    pub(crate) fn shown(&self) -> String {
        match self {
            Step::Run { program, args } => {
                std::iter::once(program.as_str()).chain(args.iter().map(String::as_str)).collect::<Vec<_>>().join(" ")
            }
            Step::Installer { version } => installer_line(version),
            Step::Companion { name, version } => {
                format!("fetch {name} from release v{version} (checked against its .sha256)")
            }
        }
    }
}

/// The release installer's URL for `version`, for this platform.
pub(crate) fn installer_url(version: &str) -> String {
    let ext = if cfg!(windows) { "ps1" } else { "sh" };
    format!("{}/v{version}/{INSTALLER}.{ext}", crate::net::release_base())
}

/// The one line that runs the release installer, as the README prints it.
pub(crate) fn installer_line(version: &str) -> String {
    let url = installer_url(version);
    if cfg!(windows) {
        format!("powershell -ExecutionPolicy Bypass -c \"irm {url} | iex\"")
    } else {
        format!("curl --proto '=https' --tlsv1.2 -LsSf {url} | sh")
    }
}

/// What a plan is made from.
pub(crate) struct Inputs<'a> {
    pub(crate) channel: Channel,
    /// The release to install, for a channel that pins one.
    pub(crate) version: Option<&'a str>,
    /// `--branch`: a source install builds this branch instead of a release's tag.
    pub(crate) branch: Option<&'a str>,
    pub(crate) checkout: Option<&'a Checkout>,
    /// Is a companion binary beside this adage (Homebrew upgrades one formula per binary; a
    /// release install fetches the companions it had)?
    pub(crate) beside: &'a dyn Fn(&str) -> bool,
}

/// The steps that update `channel`, in order: `None` for the plugin (Claude Code updates it), and
/// for a channel that pins a version when none is known.
pub(crate) fn plan(i: &Inputs) -> Option<Vec<Step>> {
    let s = |v: &[&str]| v.iter().map(|a| a.to_string()).collect::<Vec<_>>();
    let crates = || CRATES.iter().map(|(c, _)| c.to_string());
    match i.channel {
        Channel::Source => {
            let mut args = s(&["--config", "net.git-fetch-with-cli=true", "install", "--locked", "--git", REPO]);
            match (i.branch, i.version) {
                (Some(b), _) => args.extend(s(&["--branch", b])),
                (None, Some(v)) => args.extend(s(&["--tag", &format!("v{v}")])),
                (None, None) => return None,
            }
            args.extend(crates());
            Some(vec![Step::run("cargo", args)])
        }
        Channel::Checkout => {
            let co = i.checkout?;
            let root = co.root.to_string_lossy().into_owned();
            let manifest = co.root.join("Cargo.toml").to_string_lossy().into_owned();
            let mut build = s(&["build", "--locked", "--manifest-path", &manifest]);
            match co.profile.as_str() {
                "release" => build.push("--release".into()),
                "debug" => {}
                other => build.extend(s(&["--profile", other])),
            }
            build.extend(crates().flat_map(|c| ["-p".to_string(), c]));
            Some(vec![Step::run("git", s(&["-C", &root, "pull", "--ff-only"])), Step::run("cargo", build)])
        }
        // Never `@latest`: the version the check found, so what is verified is what was asked for.
        Channel::Npm => {
            Some(vec![Step::run("npm", s(&["install", "--global", &format!("{NPM_PACKAGE}@{}", i.version?)]))])
        }
        Channel::Brew => {
            let mut args = s(&["upgrade"]);
            args.extend(
                CRATES.iter().filter(|(_, b)| *b == "adage" || (i.beside)(b)).map(|(_, b)| format!("{TAP}/{b}")),
            );
            Some(vec![Step::run("brew", args)])
        }
        Channel::Release => {
            let v = i.version?;
            let mut steps = vec![Step::Installer { version: v.to_string() }];
            // The installer brings `adage` alone; the companions this install had come from the
            // same release.
            for name in ["adage-exec", "adage-train"].into_iter().filter(|n| (i.beside)(n)) {
                steps.push(Step::Companion { name: name.into(), version: v.to_string() });
            }
            Some(steps)
        }
        Channel::Plugin => None,
    }
}

/// Where `cargo install` puts binaries: `$CARGO_HOME/bin`, else `~/.cargo/bin`.
pub(crate) fn cargo_bin() -> Option<PathBuf> {
    let home = std::env::var_os("CARGO_HOME").map(PathBuf::from).or_else(|| {
        std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(|h| PathBuf::from(h).join(".cargo"))
    })?;
    Some(home.join("bin"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: &str) -> bool {
        false
    }

    /// T-O7-01: the channel is read from where the binary lives, the plugin's whatever
    /// `CLAUDE_CONFIG_DIR` is; `channel.json` (or dist's receipt) makes a release install.
    #[test]
    fn the_channel_is_read_from_where_the_binary_lives() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("data"));
        let c = |p: &str| detect(Path::new(p), &paths);
        assert_eq!(c("/Users/a/.cargo/bin/adage"), Channel::Source);
        assert_eq!(c("/repo/target/release/adage"), Channel::Source, "a checkout only if the repo is on disk");
        assert_eq!(c("/usr/local/lib/node_modules/@adage-sh/cli-darwin-arm64/bin/adage"), Channel::Npm);
        assert_eq!(c("/opt/homebrew/Cellar/adage/0.1.0/bin/adage"), Channel::Brew);
        assert_eq!(c("/Users/a/.claude/plugins/cache/adage/adage/0.1.0/bin/adage"), Channel::Plugin);
        assert_eq!(c("/x/cfg/plugins/cache/adage/adage/0.1.0/bin/adage"), Channel::Plugin);
        assert_eq!(c("/x/cfg/plugins/data/adage-adage/bin/adage-0123abcdef"), Channel::Plugin);
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(paths.channel_file(), r#"{"channel":"release","version":"0.1.0","at":1}"#).unwrap();
        assert_eq!(c("/Users/a/.cargo/bin/adage"), Channel::Release);
        std::fs::write(paths.channel_file(), r#"{"channel":"release","bin_dir":"/elsewhere"}"#).unwrap();
        assert_eq!(c("/Users/a/.cargo/bin/adage"), Channel::Source, "recorded for another directory");
    }

    /// T-O7-02: each channel runs its package manager with the version pinned (never `@latest`,
    /// never the main branch unless asked); a pinned channel with no version known has no plan.
    #[test]
    fn each_channel_pins_the_release() {
        let one = |channel: Channel, version: Option<&str>, beside: &dyn Fn(&str) -> bool| {
            plan(&Inputs { channel, version, branch: None, checkout: None, beside })
                .map(|steps| steps.iter().map(Step::shown).collect::<Vec<_>>())
        };
        assert_eq!(
            one(Channel::Source, Some("0.2.0"), &none).unwrap(),
            vec![format!("cargo --config net.git-fetch-with-cli=true install --locked --git {REPO} --tag v0.2.0 adage-cli adage-exec adage-train")]
        );
        assert_eq!(one(Channel::Npm, Some("0.2.0"), &none).unwrap(), vec!["npm install --global @adage-sh/cli@0.2.0"]);
        assert_eq!(one(Channel::Brew, None, &none).unwrap(), vec!["brew upgrade stevenvaughanbooth/tap/adage"]);
        assert_eq!(
            one(Channel::Brew, None, &|b: &str| b == "adage-train").unwrap(),
            vec!["brew upgrade stevenvaughanbooth/tap/adage stevenvaughanbooth/tap/adage-train"]
        );
        let release = one(Channel::Release, Some("0.2.0"), &|b: &str| b == "adage-exec").unwrap();
        assert_eq!(release.len(), 2);
        assert!(release[0].ends_with("/v0.2.0/adage-cli-installer.sh | sh") || cfg!(windows), "{release:?}");
        assert!(release[1].starts_with("fetch adage-exec from release v0.2.0"), "{release:?}");
        for pinned in [Channel::Source, Channel::Npm, Channel::Release] {
            assert!(one(pinned, None, &none).is_none(), "{pinned:?} needs a version");
        }
        assert!(one(Channel::Plugin, Some("0.2.0"), &none).is_none());
        assert!(one(Channel::Checkout, None, &none).is_none(), "a checkout plan needs the checkout");
        // `--branch` is the person's explicit choice: unpinned.
        let b = plan(&Inputs {
            channel: Channel::Source,
            version: Some("0.2.0"),
            branch: Some("main"),
            checkout: None,
            beside: &none,
        })
        .unwrap();
        assert!(b[0].shown().contains("--branch main") && !b[0].shown().contains("--tag"));
    }

    #[test]
    fn a_checkout_build_pulls_and_rebuilds_in_place() {
        let dir = tempfile::tempdir().unwrap();
        let dir = dir.path();
        std::fs::create_dir_all(dir.join("target/release")).unwrap();
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        std::fs::write(dir.join("Cargo.toml"), "[workspace.package]\nversion = \"0.3.1\"\n").unwrap();
        let exe = dir.join("target/release/adage");
        let co = checkout(&exe).expect("a checkout");
        assert_eq!(co.profile, "release");
        assert_eq!(checkout_version(dir).as_deref(), Some("0.3.1"));
        let steps = plan(&Inputs {
            channel: Channel::Checkout,
            version: None,
            branch: None,
            checkout: Some(&co),
            beside: &none,
        })
        .unwrap();
        let root = dir.to_string_lossy();
        assert_eq!(steps[0].shown(), format!("git -C {root} pull --ff-only"));
        assert_eq!(
            steps[1].shown(),
            format!("cargo build --locked --manifest-path {root}/Cargo.toml --release -p adage-cli -p adage-exec -p adage-train")
        );
        assert!(checkout(&dir.join("target/release/sub/adage")).is_none());
    }
}
