//! Stable keys for canonical actions.
//!
//! A *template key* generalises every literal argument into a typed placeholder, so instances of
//! the same behaviour can be counted together (Design §11). The command itself (`argv[0]`) and
//! subcommands stay literal: `make gen` and `./scripts/wi-lookup {id}` must remain distinguishable.

use crate::template::placeholder;
use crate::types::{CanonicalAction, Normalized};
use std::path::Path;

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
    // Inline code (`python3 -c '…'`, `node -e '…'`) is not an identity: it becomes `{code}`, so
    // one-off scripts neither split keys nor carry their text into reports.
    const CODE_FLAGS: [&str; 5] = ["-c", "-e", "--eval", "-p", "--print"];
    let head = argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)).unwrap_or("");
    let interpreter = INTERPRETERS.contains(&head);
    let first_operand =
        if interpreter { argv.iter().skip(1).position(|a| !a.starts_with('-')).map(|p| p + 1) } else { None };
    let code_at = first_operand.filter(|&i| CODE_FLAGS.contains(&argv[i - 1].as_str()));
    let script_at = first_operand.filter(|_| code_at.is_none());
    argv.iter()
        .enumerate()
        .map(|(i, t)| {
            if Some(i) == code_at {
                "{code}".to_string()
            } else if i == 0 || Some(i) == script_at {
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
            if url.starts_with("search:") {
                "fetch {query}".into()
            } else {
                "fetch {url}".into()
            }
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
/// become `scripts/a`. Windows paths (`C:\x\repo\scripts\a`, `c:/x/repo/scripts/a`, and Git Bash's
/// `/c/x/repo/scripts/a`) canonicalise the same way, whichever platform the transcript is read on.
pub fn canonicalise(n: &mut Normalized, root: &Path) {
    // macOS: /tmp is a symlink to /private/tmp; compare both sides without the prefix.
    fn strip(s: &str) -> &str {
        s.strip_prefix("/private").unwrap_or(s)
    }
    let root_s = strip(&windows_slashed(&root.to_string_lossy())).to_string();
    // Git Bash spells `c:/x` as `/c/x`.
    let root_bash = match root_s.as_bytes() {
        [d, b':', b'/', ..] => Some(format!("/{}{}", *d as char, &root_s[2..])),
        _ => None,
    };
    let fix = |t: &mut String| {
        let slashed = windows_slashed(t);
        let s = strip(&slashed);
        let rest = s.strip_prefix(root_s.as_str()).or_else(|| root_bash.as_deref().and_then(|b| s.strip_prefix(b)));
        if let Some(rest) = rest {
            if rest.is_empty() || rest.starts_with('/') {
                *t = rest.trim_start_matches('/').to_string();
            }
        }
        if let Some(rest) = t.strip_prefix("./") {
            *t = rest.to_string();
        }
    };
    match &mut n.action {
        CanonicalAction::Exec { argv, .. } => {
            argv.iter_mut().filter(|t| t.contains('/') || t.contains('\\')).for_each(fix)
        }
        CanonicalAction::Compound { actions, .. } => actions.iter_mut().for_each(|a| canonicalise(a, root)),
        _ => {}
    }
}

/// A Windows drive path (`C:\x`, `C:/x`, `\\?\C:\x`) in `/` form with a lower-case drive letter;
/// anything else unchanged (a backslash in a Unix path is an ordinary character).
fn windows_slashed(s: &str) -> std::borrow::Cow<'_, str> {
    let s = s.strip_prefix(r"\\?\").unwrap_or(s);
    match s.as_bytes() {
        [d, b':', b'\\' | b'/', ..] if d.is_ascii_alphabetic() => {
            format!("{}{}", d.to_ascii_lowercase() as char, s[1..].replace('\\', "/")).into()
        }
        _ => s.into(),
    }
}

/// The identity of a command, independent of its arguments: its program and the subcommand it
/// multiplexes to ([`crate::pattern`]). `python3 scripts/wi-lookup W-1 --json`,
/// `scripts/wi-lookup W-1` and `./scripts/wi-lookup W-1` share the head `scripts/wi-lookup`;
/// `npm run build` and `npm run check` do not share one.
pub fn command_head(argv: &[String]) -> String {
    crate::pattern::Shape::of_argv(argv).map(|s| s.identity.join(" ")).unwrap_or_default()
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

/// Whether sub-action `index` of `count` (from [`flatten`] of one tool call) failed, when the call
/// as a whole did. A transcript records one result per call, not per sub-action: `make gen &&
/// pytest` failing says nothing about which one failed.
///
/// - In an `&&` chain the last sub-action that ran is the one that failed, and the likeliest to
///   have run last is the last one written; in a `;` chain which one failed is ambiguous. Either
///   way the failure is attributed to the last sub-action only, and the earlier ones count as not
///   failed. This can credit an earlier step that did fail (`make gen` failing stops `make gen &&
///   pytest` before `pytest` runs), but never blames a step that succeeded for its successor's
///   failure, which is what taught dead ends about commands that work.
/// - A command with `||` (`or_chain`) keeps the failure on every sub-action: `a || b` fails only
///   when `a` failed and `b` then ran and failed too.
pub fn sub_action_failed(index: usize, count: usize, call_failed: bool, or_chain: bool) -> bool {
    call_failed && (or_chain || index + 1 == count)
}

/// Whether a tool call's shell command has an `||` (see [`sub_action_failed`]).
pub fn has_or_chain(input: &serde_json::Value) -> bool {
    input.get("command").and_then(|c| c.as_str()).is_some_and(|c| c.contains("||"))
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
    fn canonicalises_windows_paths_on_any_platform() {
        let root = Path::new(r"C:\work\repo");
        for tok in
            [r"C:\work\repo\scripts\wi-lookup", "c:/work/repo/scripts/wi-lookup", "/c/work/repo/scripts/wi-lookup"]
        {
            let mut n = Normalized {
                action: CanonicalAction::Exec { argv: vec![tok.into(), "W-1".into()], cwd: r"C:\work\repo".into() },
                effect: crate::Effect::Observe,
                mutating: false,
                confidence: 1.0,
                note: String::new(),
            };
            canonicalise(&mut n, root);
            assert_eq!(template_key(&n.action), "exec scripts/wi-lookup {id}", "{tok}");
        }
        // A Unix backslash is just a character.
        assert_eq!(windows_slashed(r"a\b"), r"a\b");
    }

    #[test]
    fn command_heads_ignore_interpreter_and_arguments() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        let a = command_head(&v(&["python3", "scripts/wi-lookup", "W-1", "--json"]));
        assert_eq!(a, "scripts/wi-lookup");
        assert_eq!(command_head(&v(&["./scripts/wi-lookup", "W-2"])), a);
        assert_eq!(command_head(&v(&["make", "test"])), "make test");
        assert_eq!(command_head(&v(&["python3", "-m", "pytest", "-q"])), "python3 -m pytest");
        assert_ne!(command_head(&v(&["npm", "run", "build"])), command_head(&v(&["npm", "run", "check"])));
    }

    #[test]
    fn a_failed_chain_is_attributed_to_its_last_sub_action() {
        assert_eq!((0..3).map(|i| sub_action_failed(i, 3, true, false)).collect::<Vec<_>>(), [false, false, true]);
        assert!((0..3).all(|i| !sub_action_failed(i, 3, false, false)));
        assert!(sub_action_failed(0, 1, true, false));
        assert!((0..3).all(|i| sub_action_failed(i, 3, true, true)), "a || b failed: both did");
        assert!(
            has_or_chain(&serde_json::json!({"command": "a || b"}))
                && !has_or_chain(&serde_json::json!({"command": "a && b"}))
        );
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
        let argv: Vec<String> = ["./scripts/wi-lookup", "W-2042", "--json"].iter().map(|s| s.to_string()).collect();
        assert_eq!(template_command(&argv), vec!["./scripts/wi-lookup", "{id}", "--json"]);
    }
}

#[cfg(test)]
mod code_tests {
    use super::template_command;

    fn t(argv: &[&str]) -> String {
        template_command(&argv.iter().map(|s| s.to_string()).collect::<Vec<_>>()).join(" ")
    }

    #[test]
    fn inline_code_is_a_placeholder_scripts_and_modules_are_not() {
        assert_eq!(t(&["python3", "-c", "import json; print(json.load(open('/x')))"]), "python3 -c {code}");
        assert_eq!(t(&["node", "-e", "console.log(1)"]), "node -e {code}");
        assert_eq!(t(&["python3", "scripts/wi-lookup", "W-1234"]), "python3 scripts/wi-lookup {id}");
        assert_eq!(t(&["python3", "-m", "pytest", "tests/test_a.py"]), "python3 -m pytest {path}");
    }
}
