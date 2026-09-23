//! Stable keys for canonical actions.
//!
//! A *template key* generalises every literal argument into a typed placeholder, so instances of
//! the same behaviour can be counted together (Design §11). The command itself (`argv[0]`) and
//! subcommands stay literal: `make gen` and `./scripts/wi-lookup {id}` must remain distinguishable.

use crate::template::placeholder;
use crate::types::{CanonicalAction, Normalized};

pub fn op_name(a: &CanonicalAction) -> &'static str {
    match a {
        CanonicalAction::Read { .. } => "read",
        CanonicalAction::Search { .. } => "search",
        CanonicalAction::List { .. } => "list",
        CanonicalAction::Exec { .. } => "exec",
        CanonicalAction::Edit { .. } => "edit",
        CanonicalAction::Write { .. } => "write",
        CanonicalAction::Fetch { .. } => "fetch",
        CanonicalAction::Unknown { .. } => "unknown",
        CanonicalAction::Control { .. } => "control",
        CanonicalAction::Compound { .. } => "compound",
    }
}

/// Template an argv, keeping the command word literal.
pub fn template_command(argv: &[String]) -> Vec<String> {
    argv.iter()
        .enumerate()
        .map(|(i, t)| {
            if i == 0 {
                t.clone()
            } else {
                placeholder(t).map(str::to_string).unwrap_or_else(|| t.clone())
            }
        })
        .collect()
}

pub fn template_key(a: &CanonicalAction) -> String {
    match a {
        CanonicalAction::Read { range, .. } => {
            format!("read {{path}}{}", if range.is_some() { " [range]" } else { "" })
        }
        CanonicalAction::Search { kind, .. } => format!("search:{kind} {{pattern}}"),
        CanonicalAction::List { .. } => "list {path}".to_string(),
        CanonicalAction::Exec { argv, .. } => format!("exec {}", template_command(argv).join(" ")),
        CanonicalAction::Edit { .. } => "edit {path}".to_string(),
        CanonicalAction::Write { .. } => "write {path}".to_string(),
        CanonicalAction::Fetch { url } => {
            if url.starts_with("search:") { "fetch {query}".into() } else { "fetch {url}".into() }
        }
        CanonicalAction::Unknown { .. } => "unknown".to_string(),
        CanonicalAction::Control { tool } => format!("control {tool}"),
        CanonicalAction::Compound { kind, actions } => format!(
            "compound:{kind} [{}]",
            actions.iter().map(|n| template_key(&n.action)).collect::<Vec<_>>().join(";")
        ),
    }
}

/// Expand compound actions into their body actions, in order.
pub fn flatten(items: &[Normalized]) -> Vec<Normalized> {
    let mut out = Vec::new();
    for n in items {
        match &n.action {
            CanonicalAction::Compound { actions, .. } => out.extend(flatten(actions)),
            _ => out.push(n.clone()),
        }
    }
    out
}

/// The path an action reads or writes, if any.
pub fn action_path(a: &CanonicalAction) -> Option<&str> {
    match a {
        CanonicalAction::Read { path, .. }
        | CanonicalAction::Edit { path }
        | CanonicalAction::Write { path }
        | CanonicalAction::List { path } => Some(path),
        _ => None,
    }
}

pub fn is_author(a: &CanonicalAction) -> bool {
    matches!(a, CanonicalAction::Edit { .. } | CanonicalAction::Write { .. })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_word_stays_literal() {
        let argv: Vec<String> =
            ["./scripts/wi-lookup", "W-2042", "--json"].iter().map(|s| s.to_string()).collect();
        assert_eq!(template_command(&argv), vec!["./scripts/wi-lookup", "{id}", "--json"]);
    }
}
