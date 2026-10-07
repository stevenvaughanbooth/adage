//! How a tool call is read: its canonical template keys (what triggers, follows and obligations
//! match on; for a Bash call in a batch, with the files it wrote), the argv of each command it
//! ran, its command line, and the test functions an edit adds.

use crate::{orient, Call, Ctx};
use adage_core::keys::{self, template_key};
use adage_core::tools::normalize_tool;
use adage_core::{repo, CommandTable};
use serde_json::Value;
use std::path::Path;

const MAX_KEY: usize = 256;

/// A key cut to [`MAX_KEY`] bytes, at a character boundary: what is observed and kept in history.
pub(crate) fn cap(mut s: String) -> String {
    if s.len() > MAX_KEY {
        let mut i = MAX_KEY;
        while !s.is_char_boundary(i) {
            i -= 1;
        }
        s.truncate(i);
    }
    s
}

/// The canonical template key of a command string, for matching the agent's calls.
pub fn command_key(command: &str, table: &CommandTable) -> String {
    let n = adage_core::shell::normalize_shell(command, "/", table);
    keys::flatten(&n)
        .first()
        .map(|x| {
            let mut x = x.clone();
            keys::canonicalise(&mut x, Path::new("/"));
            template_key(&x.action)
        })
        .unwrap_or_default()
}

pub(crate) fn call_keys(call: &Call, ctx: &Ctx) -> Vec<(String, adage_core::Normalized)> {
    let cwd = ctx.root.to_string_lossy();
    keyed(normalize_tool(&call.tool, &call.input, &cwd, ctx.table), ctx)
}

/// [`call_keys`] for a call a tool batch reports as made: a Bash command's actions with what it
/// was seen to write and run beyond them ([`adage_core::shell::normalize_shell_observed`]: each
/// file a redirection, `tee` or an inline Python program writes, as an edit of that path right
/// after the action that wrote it, and the commands after a heredoc's start). So an edit made
/// through Bash arms what an edit tool's would (triggers, contexts, the finish line), and the step
/// that meets it in the same command is seen after it. One parse, as before.
pub(crate) fn batch_keys(call: &Call, ctx: &Ctx) -> Vec<(String, adage_core::Normalized)> {
    let cwd = ctx.root.to_string_lossy();
    match (call.tool.as_str(), call.input.get("command").and_then(Value::as_str)) {
        ("Bash", Some(command)) => keyed(adage_core::shell::normalize_shell_observed(command, &cwd, ctx.table), ctx),
        _ => call_keys(call, ctx),
    }
}

fn keyed(normalized: Vec<adage_core::Normalized>, ctx: &Ctx) -> Vec<(String, adage_core::Normalized)> {
    keys::flatten(&normalized)
        .into_iter()
        .map(|mut n| {
            keys::canonicalise(&mut n, ctx.root);
            (template_key(&n.action), n)
        })
        .collect()
}

/// The argv of each command a call ran, in order (an activation, the run, a filter), for the
/// action-level follow judgement ([`crate::policy::follows_call`]).
pub(crate) fn exec_segments(keyed: &[(String, adage_core::Normalized)]) -> Vec<Vec<String>> {
    keyed
        .iter()
        .filter_map(|(_, n)| match &n.action {
            adage_core::CanonicalAction::Exec { argv, .. } => Some(argv.clone()),
            _ => None,
        })
        .collect()
}

/// The command line of a shell tool call (Claude Code's `Bash`, Codex's `shell`), if it has one.
pub(crate) fn call_command(call: &Call) -> Option<&str> {
    match call.input.get("command")? {
        Value::String(s) => Some(s.as_str()),
        // Codex: an argv, `["bash", "-lc", "…"]`.
        Value::Array(a) => a.last().and_then(Value::as_str),
        _ => None,
    }
}

/// `def test…` names (sync or async, at any indentation) in a text an edit wrote.
fn test_names(text: &str) -> Vec<&str> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim_start();
            let r = l.strip_prefix("async def ").or_else(|| l.strip_prefix("def "))?;
            let end = r.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(r.len());
            let name = &r[..end];
            (name.starts_with("test") && name.len() <= 100).then_some(name)
        })
        .collect()
}

/// The test functions a successful edit adds to a test file ([`orient::is_test_file`]): names in
/// what it wrote that were not in what it replaced (an Edit's or MultiEdit's `new_string`s over
/// their `old_string`s; a Write's whole content).
pub(crate) fn added_tests(call: &Call, root: &std::path::Path) -> Vec<String> {
    if call.failed || !matches!(call.tool.as_str(), "Edit" | "MultiEdit" | "Write") {
        return Vec::new();
    }
    let Some(path) = call.input.get("file_path").and_then(Value::as_str) else { return Vec::new() };
    if !orient::is_test_file(&repo::relativize(root, path)) {
        return Vec::new();
    }
    let field = |v: &Value, k: &str| v.get(k).and_then(Value::as_str).map(str::to_string);
    let mut pairs: Vec<(String, String)> = Vec::new();
    if let Some(t) = field(&call.input, "content") {
        pairs.push((t, String::new()));
    }
    if let Some(t) = field(&call.input, "new_string") {
        pairs.push((t, field(&call.input, "old_string").unwrap_or_default()));
    }
    for e in call.input.get("edits").and_then(Value::as_array).into_iter().flatten() {
        if let Some(t) = field(e, "new_string") {
            pairs.push((t, field(e, "old_string").unwrap_or_default()));
        }
    }
    let mut out: Vec<String> = Vec::new();
    for (new, old) in &pairs {
        let before = test_names(old);
        for name in test_names(new) {
            if !before.contains(&name) && !out.iter().any(|n| n == name) {
                out.push(name.to_string());
            }
        }
    }
    out
}
