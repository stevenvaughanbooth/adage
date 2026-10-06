//! `adage ledger note`: record one session's usage as its harness reported it
//! (`adage_app::harness_ledger`), then refresh the repository's savings estimate.
//!
//! Reads one JSON object from stdin:
//!
//! ```text
//! { "session_id", "repo", "harness": "claude-code", "harness_version", "started_at", "ended_at",
//!   "steps": [{ "index", "model", "input", "output", "cache_read", "cache_write", "predicted" }],
//!   "cost_usd"?, "hook_ms"?: [..] }
//! ```
//!
//! `repo` is the session's working directory (resolved to its repository's key, as hooks resolve
//! it) or the key itself. Allowed inside an agent session: it changes no reflex and no setting,
//! and a note is accepted only for a session the spool shows started in that repository. Exit 0
//! with nothing on stdout when recorded; exit 2, with the reason on stderr and nothing written,
//! when the note is malformed or names a session that did not start there.

use adage_app::harness_ledger::{self, Refused};
use adage_store::Paths;
use anyhow::Result;
use std::io::Read;
use std::path::Path;

/// `repo` as a key: a directory is resolved to its repository's key; anything else is a key.
fn resolve(paths: &Paths, repo: &str) -> String {
    let p = Path::new(repo);
    if p.is_absolute() && p.is_dir() {
        let root = adage_core::repo::find_root(p).unwrap_or_else(|| p.to_path_buf());
        let index = adage_tracks::Index::load(&paths.index());
        return crate::ui::health::repo_key(paths, &root, &index);
    }
    repo.to_string()
}

fn note(paths: &Paths, input: &str) -> Result<(), Refused> {
    let mut n = harness_ledger::parse(input)?;
    n.repo = resolve(paths, &n.repo);
    harness_ledger::check_session(paths, &n)?;
    let repo = n.repo.clone();
    harness_ledger::record(paths, n, adage_core::time::now_ms())
        .map_err(|e| Refused(format!("the note could not be written: {e}")))?;
    let ledger = adage_learn::ledger::Ledger::new(&paths.ledger(), &repo).read();
    crate::savings::refresh(paths, &repo, &ledger);
    Ok(())
}

/// `adage ledger note`.
pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        Some("note") => {}
        _ => anyhow::bail!("usage: adage ledger note < note.json"),
    }
    let mut input = String::new();
    let read = std::io::stdin().take(harness_ledger::MAX_NOTE_BYTES as u64 + 1).read_to_string(&mut input);
    let result = match read {
        Ok(_) => note(paths, &input),
        Err(e) => Err(Refused(format!("stdin could not be read ({e})"))),
    };
    if let Err(Refused(why)) = result {
        let mut err = adage_term::Out::stderr();
        if err
            .error(
                &format!("note not recorded: {why}"),
                &[],
                Some("`adage ledger note` takes one session's usage as JSON on stdin"),
            )
            .is_err()
        {
            eprintln!("adage ledger note: {why}");
        }
        std::process::exit(2);
    }
    Ok(())
}
