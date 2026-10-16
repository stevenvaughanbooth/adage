//! `adage off` and `adage on`: the repository's kill switch, `<root>/.adage/disabled`.
//!
//! With the file present every hook in the repository prints nothing and records nothing
//! (`adage_hook`, before any other work), whichever harness runs it. `off` turns something down,
//! so it is allowed anywhere, inside an agent session too (docs/live-learning.md, "Turning it off
//! in a repository"); `on` turns Adage back on, a person's decision (at a terminal, or a press from
//! the session). Outside a repository both say so in one line and exit 0: there is nothing to
//! switch.

use adage_term::{Line, Out};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// The switch's file, relative to the repository root.
pub const MARKER: &str = adage_store::paths::REPO_DISABLED;

pub fn marker(root: &Path) -> PathBuf {
    adage_store::Paths::repo_disabled(root)
}

/// Whether hooks do nothing here: the repository's switch is off, or `ADAGE_DISABLE=1`.
pub fn is_off(root: Option<&Path>) -> bool {
    std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") || root.is_some_and(|r| marker(r).exists())
}

fn here() -> Option<PathBuf> {
    std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d))
}

fn say(line: Line) -> Result<()> {
    let mut o = Out::stdout();
    o.line(&line)?;
    o.flush()?;
    Ok(())
}

/// The line `adage off` prints, and the status card's first line says the same.
pub fn off_line() -> Line {
    Line::new()
        .text(format!("adage is off for this repository ({MARKER}). "))
        .cmd("adage on")
        .text(" turns it back on.")
}

/// `adage off`.
pub fn off() -> Result<()> {
    let Some(root) = here() else {
        return say(
            Line::new().quiet("not inside a repository: `adage off` turns Adage off for the repository it runs in")
        );
    };
    let m = marker(&root);
    if !m.exists() {
        if let Some(dir) = m.parent() {
            std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
        }
        std::fs::write(&m, "Adage is off in this repository: hooks do nothing here. `adage on` turns it back on.\n")
            .with_context(|| format!("writing {}", m.display()))?;
    }
    say(off_line())
}

/// `adage on`.
pub fn on() -> Result<()> {
    let Some(root) = here() else {
        return say(
            Line::new().quiet("not inside a repository: `adage on` turns Adage on for the repository it runs in")
        );
    };
    let m = marker(&root);
    if m.exists() {
        // A person's decision: at a terminal, or with a press from the session (mod architecture
        // §3.12, `/adage on`).
        crate::review::person_or_press(adage_app::person::PersonVerb::On, &adage_store::Paths::from_env())?;
        std::fs::remove_file(&m).with_context(|| format!("removing {}", m.display()))?;
        // The directory is Adage's own; leave it if anything else lives there.
        if let Some(dir) = m.parent() {
            let _ = std::fs::remove_dir(dir);
        }
    }
    let mut l = Line::new().text("adage is on for this repository.");
    if std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
        l = l.quiet(" ADAGE_DISABLE=1 still keeps every hook off where it is set.");
    }
    say(l)
}
