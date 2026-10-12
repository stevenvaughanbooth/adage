//! The terminal UI: `adage watch` (a compact live pane for a split terminal), `adage ui` (the
//! full-screen app), and the interactive `adage init` wizard. Built on ratatui and crossterm.
//!
//! Nothing here runs on the hook path: `main` handles `adage hook` before any command dispatch,
//! and no terminal is touched until one of these commands starts.
//!
//! Every screen is drawn by a pure function from state to frame (`view`), so frames are tested
//! with ratatui's `TestBackend` against fixture data. State comes from files only:
//!
//! * hook activity: `$ADAGE_HOME/sessions/*.spool`, followed incrementally ([`tail`]);
//! * conditioning: `$ADAGE_HOME/learn.log`, one JSON object per line ([`learnlog`]);
//! * health: `adage doctor`'s checks, re-run every few seconds on a worker thread ([`health`]);
//! * settings: `$ADAGE_HOME/config.toml`, through `adage_store::config` (as `adage config`).
//!
//! The terminal is restored on every exit path, including a panic ([`term`]). `NO_COLOR` is
//! honoured: colour is dropped and strengths and lamps are spelled out. Words follow the lexicon
//! (docs/lexicon.md); older spellings in spools and learn.log are still read ([`words`]).

mod app;
mod brain;
mod conditioned;
pub(crate) mod demo;
mod diff;
mod feed;
mod follow;
mod forms;
mod fuzzy;
pub(crate) mod health;
mod jobs;
mod keys;
mod kit;
mod learnlog;
mod motion;
mod palette;
mod routes;
mod scope;
mod splash;
mod tabs;
pub(crate) mod tail;
mod term;
mod theme;
mod tools;
mod view;
mod watch;
mod wizard;
pub(crate) mod words;

#[cfg(test)]
mod tests;

use crate::helpers::flag;
use adage_store::Paths;
use anyhow::{bail, Result};
use std::io::IsTerminal;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Whether a terminal UI owns the screen: warnings are then queued instead of written to stderr,
/// which would scribble over the alternate screen.
static ACTIVE: AtomicBool = AtomicBool::new(false);
static NOTICES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// A warning from shared code (an unreadable track, say): stderr normally, a notice in the UI
/// while one is running.
pub fn warn(msg: String) {
    if ACTIVE.load(Ordering::SeqCst) {
        if let Ok(mut n) = NOTICES.lock() {
            if n.len() < 50 && !n.contains(&msg) {
                n.push(msg);
            }
        }
    } else {
        eprintln!("{msg}");
    }
}

/// Warnings queued while the UI was running.
pub(crate) fn take_notices() -> Vec<String> {
    NOTICES.lock().map(|mut n| std::mem::take(&mut *n)).unwrap_or_default()
}

/// Is a person at a terminal? Both stdin and stdout are terminals, and no agent-session marker is
/// set (an agent's shell must never get an interactive screen).
pub fn interactive() -> bool {
    agent_marker().is_none() && std::io::stdin().is_terminal() && std::io::stdout().is_terminal()
}

pub(crate) fn agent_marker() -> Option<&'static str> {
    crate::install::AGENT_ENV.iter().copied().find(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty()))
}

/// `WxH`, e.g. `80x24`.
fn size(args: &[String]) -> Result<Option<(u16, u16)>> {
    let Some(s) = flag(args, "--size") else { return Ok(None) };
    let parsed = s.split_once('x').and_then(|(w, h)| Some((w.parse::<u16>().ok()?, h.parse::<u16>().ok()?)));
    match parsed {
        Some((w, h)) if (20..=500).contains(&w) && (6..=300).contains(&h) => Ok(Some((w, h))),
        _ => bail!("--size takes WIDTHxHEIGHT, e.g. 80x24"),
    }
}

const WATCH_USAGE: &str = "usage: adage watch [--repo <path>] [--demo] [--once [--size WxH]] | adage watch --follow --json [--repo <path>] [--session <id>] [--since <cursor>] [--stdin eof|ignore]";

/// `adage watch [--repo <path>] [--demo] [--once [--size WxH]]` (and `adage doctor --watch`), and
/// `adage watch --follow --json [--repo <path>] [--session <id>] [--since <cursor>]`, the same feed
/// as JSON lines for a program ([`follow`]).
pub fn watch(args: &[String], paths: &Paths) -> Result<()> {
    if let Some(bad) = args.iter().find(|a| {
        a.starts_with("--")
            && !matches!(
                a.as_str(),
                "--repo" | "--demo" | "--once" | "--size" | "--follow" | "--json" | "--session" | "--since" | "--stdin"
            )
    }) {
        bail!("unknown option {bad}\n{WATCH_USAGE}");
    }
    let follow = args.iter().any(|a| a == "--follow");
    if follow || args.iter().any(|a| a == "--json") {
        if !(follow && args.iter().any(|a| a == "--json")) {
            bail!("`--follow` and `--json` go together: the stream is JSON lines\n{WATCH_USAGE}");
        }
        // `--stdin ignore`: a parent that closes the child's stdin at once (the Claude Code mod's
        // spawn with no input) must not end the stream; SIGTERM and the parent's death still do.
        let watch_stdin = match flag(args, "--stdin").as_deref() {
            None | Some("eof") => true,
            Some("ignore") => false,
            Some(other) => bail!("--stdin takes eof or ignore, not {other:?}\n{WATCH_USAGE}"),
        };
        let ctx = view::Ctx::new(paths.clone(), flag(args, "--repo").map(PathBuf::from))?;
        return follow::run(
            paths,
            ctx.root,
            flag(args, "--session").as_deref(),
            flag(args, "--since").as_deref(),
            watch_stdin,
        );
    }
    let once = args.iter().any(|a| a == "--once");
    let size = size(args)?;
    if args.iter().any(|a| a == "--demo") {
        return demo::run(once, size, false);
    }
    let ctx = view::Ctx::new(paths.clone(), flag(args, "--repo").map(PathBuf::from))?;
    if once {
        print!("{}", watch::snapshot(&ctx, size));
        return Ok(());
    }
    if !std::io::stdout().is_terminal() {
        bail!("`adage watch` needs a terminal; `adage watch --once` prints a single frame");
    }
    watch::run(ctx, None)
}

/// `adage ui [--repo <path>] [--demo]`: the full-screen app.
pub fn app(args: &[String], paths: &Paths) -> Result<()> {
    if let Some(bad) = args.iter().find(|a| a.starts_with("--") && !matches!(a.as_str(), "--repo" | "--demo")) {
        bail!("unknown option {bad}\nusage: adage ui [--repo <path>] [--demo]");
    }
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        bail!(
            "`adage ui` needs a terminal; `adage doctor`, `adage status` and `adage review` print the same information"
        );
    }
    if args.iter().any(|a| a == "--demo") {
        return demo::run(false, None, true);
    }
    let ctx = view::Ctx::new(paths.clone(), flag(args, "--repo").map(PathBuf::from))?;
    app::run(ctx, None)
}

/// The interactive `adage init`.
pub fn wizard(repo: Option<PathBuf>, paths: &Paths) -> Result<()> {
    let ctx = view::Ctx::new(paths.clone(), repo)?;
    wizard::run(ctx)
}
