//! Stable keys for canonical actions.
//!
//! A *template key* generalises every literal argument into a typed placeholder, so instances of
//! the same behaviour can be counted together (Design §11). The command itself (`argv[0]`) and
//! subcommands stay literal: `make gen` and `./scripts/wi-lookup {id}` must remain distinguishable.

use crate::template::placeholder;
use std::path::Path;
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
    // For interpreters, the script is part of the command's identity: `python3 scripts/a {id}`,
    // not `python3 {path} {id}`, which would match every script.
    const INTERPRETERS: [&str; 9] = ["python", "python3", "node", "bash", "sh", "zsh", "ruby", "perl", "deno"];
    let head = argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)).unwrap_or("");
    let script_at = if INTERPRETERS.contains(&head) {
        argv.iter().skip(1).position(|a| !a.starts_with('-')).map(|p| p + 1)
    } else {
        None
    };
    argv.iter()
        .enumerate()
        .map(|(i, t)| {
            if i == 0 || Some(i) == script_at {
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

/// Canonicalise command paths against the repository root, so the same behaviour in different
/// checkouts produces the same key: `/tmp/x/repo/scripts/a`, `./scripts/a` and `scripts/a` all
/// become `scripts/a`.
pub fn canonicalise(n: &mut Normalized, root: &Path) {
    // macOS: /tmp is a symlink to /private/tmp; compare both sides without the prefix.
    fn strip(s: &str) -> &str {
        s.strip_prefix("/private").unwrap_or(s)
    }
    let root_s = strip(&root.to_string_lossy()).to_string();
    let fix = |t: &mut String| {
        if let Some(rest) = strip(t).strip_prefix(root_s.as_str()) {
            if rest.is_empty() || rest.starts_with('/') {
                *t = rest.trim_start_matches('/').to_string();
            }
        }
        if let Some(rest) = t.strip_prefix("./") {
            *t = rest.to_string();
        }
    };
    match &mut n.action {
        CanonicalAction::Exec { argv, .. } => argv.iter_mut().filter(|t| t.contains('/')).for_each(fix),
        CanonicalAction::Compound { actions, .. } => actions.iter_mut().for_each(|a| canonicalise(a, root)),
        _ => {}
    }
}

/// The identity of a command, independent of its arguments: tokens up to and including the first
/// path-like token (the script), else the first two. `python3 scripts/wi-lookup W-1 --json`,
/// `scripts/wi-lookup W-1` and `./scripts/wi-lookup W-1` share the head `scripts/wi-lookup` after
/// dropping interpreters.
pub fn command_head(argv: &[String]) -> String {
    const INTERPRETERS: [&str; 9] = ["python", "python3", "node", "bash", "sh", "zsh", "ruby", "perl", "deno"];
    let toks: Vec<&str> = argv
        .iter()
        .map(|t| t.strip_prefix("./").unwrap_or(t))
        .filter(|t| !INTERPRETERS.contains(&t.rsplit('/').next().unwrap_or(t)))
        .collect();
    match toks.iter().position(|t| t.contains('/')) {
        Some(i) => toks[i].to_string(),
        None => toks.iter().take(2).copied().collect::<Vec<_>>().join(" "),
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
    fn canonicalises_paths() {
        let mut n = Normalized {
            action: CanonicalAction::Exec {
                argv: vec!["/tmp/r/repo/scripts/wi-lookup".into(), "W-1".into()],
                cwd: "/tmp/r/repo".into(),
            },
            effect: crate::Effect::Observe,
            mutating: false,
            confidence: 1.0,
            note: String::new(),
        };
        canonicalise(&mut n, Path::new("/tmp/r/repo"));
        assert_eq!(template_key(&n.action), "exec scripts/wi-lookup {id}");
        let mut m = n.clone();
        if let CanonicalAction::Exec { argv, .. } = &mut m.action {
            argv[0] = "./scripts/wi-lookup".into();
        }
        canonicalise(&mut m, Path::new("/tmp/r/repo"));
        assert_eq!(template_key(&m.action), template_key(&n.action));
    }

    #[test]
    fn command_heads_ignore_interpreter_and_arguments() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = command_head(&v(&["python3", "scripts/wi-lookup", "W-1", "--json"]));
        assert_eq!(a, "scripts/wi-lookup");
        assert_eq!(command_head(&v(&["./scripts/wi-lookup", "W-2"])), a);
        assert_eq!(command_head(&v(&["make", "test"])), "make test");
    }

    #[test]
    fn interpreter_scripts_stay_literal() {
        let argv: Vec<String> = ["python3", "scripts/wi-lookup", "W-1"].iter().map(|s| s.to_string()).collect();
        assert_eq!(template_command(&argv), vec!["python3", "scripts/wi-lookup", "{id}"]);
        let argv: Vec<String> = ["python3", "-m", "unittest"].iter().map(|s| s.to_string()).collect();
        assert_eq!(template_command(&argv)[0], "python3");
    }

    #[test]
    fn command_word_stays_literal() {
        let argv: Vec<String> =
            ["./scripts/wi-lookup", "W-2042", "--json"].iter().map(|s| s.to_string()).collect();
        assert_eq!(template_command(&argv), vec!["./scripts/wi-lookup", "{id}", "--json"]);
    }
}
