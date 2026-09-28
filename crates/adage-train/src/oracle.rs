//! The trajectory oracle: what a session did about a claim, read from its transcript (parsed and
//! normalised by adage-learn, exactly as `adage condition` reads it).

use crate::claims::{argv_of, Claim, ClaimKind};
use adage_core::{keys, CanonicalAction, CommandTable, Effect};
use adage_learn::import::Session;
use std::path::Path;

/// Parse a transcript as `adage condition` would, under `repo` (the forced repository key).
pub fn read(path: &Path, repo: &str, table: &CommandTable) -> Option<Session> {
    adage_learn::read_session(path, table, Some(repo))
}

/// Either argv is a prefix of the other: `make test` and `make test -k x` are one command.
pub fn same_command(a: &[String], b: &[String]) -> bool {
    !a.is_empty() && !b.is_empty() && (a.starts_with(b) || b.starts_with(a))
}

/// The executed commands of a session, in order: (argv, effect, failed).
pub fn execs(s: &Session) -> Vec<(&[String], Effect, bool)> {
    s.steps
        .iter()
        .filter_map(|st| match &st.norm.action {
            CanonicalAction::Exec { argv, .. } => Some((argv.as_slice(), st.norm.effect, st.call.failed)),
            _ => None,
        })
        .collect()
}

pub fn glob_matches(glob: &str, rel: &str) -> bool {
    glob::Pattern::new(glob).is_ok_and(|p| {
        p.matches_with(rel, glob::MatchOptions { require_literal_separator: true, ..Default::default() })
    })
}

/// Runs of `command` in a session: (runs, failed runs).
pub fn runs(s: &Session, command: &[String]) -> (usize, usize) {
    let r: Vec<bool> = execs(s).into_iter().filter(|(a, _, _)| a.starts_with(command)).map(|(_, _, f)| f).collect();
    (r.len(), r.iter().filter(|f| **f).count())
}

/// Whether the claim could apply in this session (`None`: never), and if so whether the session
/// did what it says. `corrected` is the working command of a contradicted claim: the session then
/// counts as following when its first verification is that command, and it passes.
pub fn followed(c: &Claim, corrected: Option<&str>, s: &Session, table: &CommandTable) -> Option<bool> {
    let ex = execs(s);
    let first_verify = ex.iter().find(|(_, e, _)| *e == Effect::Verify);
    let first_is = |cmd: &str| -> bool {
        let want = argv_of(cmd, table).unwrap_or_default();
        first_verify.is_some_and(|(a, _, f)| !f && same_command(a, &want))
    };
    match &c.kind {
        ClaimKind::RegenerateAfter { glob: Some(g), command } => {
            let last = s.steps.iter().rposition(|st| {
                keys::is_author(&st.norm.action) && st.rel_path.as_deref().is_some_and(|p| glob_matches(g, p))
            })?;
            let want = argv_of(command, table)?;
            Some(s.steps[last + 1..].iter().any(|st| match &st.norm.action {
                CanonicalAction::Exec { argv, .. } => !st.call.failed && same_command(argv, &want),
                _ => false,
            }))
        }
        ClaimKind::Verify { command } => {
            first_verify?;
            if let Some(y) = corrected {
                return Some(first_is(y));
            }
            let want = argv_of(command, table)?;
            Some(ex.iter().any(|(a, _, f)| !f && same_command(a, &want)))
        }
        ClaimKind::Avoid { instead, .. } => {
            first_verify?;
            Some(first_is(corrected.unwrap_or(instead)))
        }
        ClaimKind::Lookup { tool, .. } => {
            let ticket = regex::Regex::new(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b").unwrap();
            let id = ticket.find(&s.prompt)?.as_str().to_string();
            let want = argv_of(tool, table)?;
            Some(ex.iter().any(|(a, _, f)| !f && a.starts_with(&want) && a.contains(&id)))
        }
        _ => None,
    }
}

/// Whether a session gave the claim a chance at all (exploration): it ran a verification, edited
/// a matching file, or was asked about a ticket.
pub fn exercised(c: &Claim, s: &Session, table: &CommandTable) -> bool {
    match &c.kind {
        ClaimKind::Verify { command } => {
            let want = argv_of(command, table).unwrap_or_default();
            execs(s).iter().any(|(a, e, _)| *e == Effect::Verify || same_command(a, &want))
        }
        _ => followed(c, None, s, table).is_some(),
    }
}
