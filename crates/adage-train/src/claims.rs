//! Deterministic claim extraction: what a skill (or runbook, CONTRIBUTING.md, Makefile,
//! package.json, CI workflow) tells an agent to do, as typed claims. No LLM, no execution.
//!
//! From Markdown, commands, paths and globs come from fenced and inline code; a sentence counts
//! only if it is imperative (run, use, always, never, after, before, instead of, …) or the command
//! sits in a shell code block. Each command is normalised and effect-classified by adage-core, and
//! a claim whose directed command classifies above *regenerate* (environment, author, publish) is
//! `unsafe`: it is reported and never exercised.
//!
//! From a Makefile or package.json, claims come from adage-instincts' declared-fact extraction
//! (the same bodies, verdicts and generator links instincts bind from); from a GitHub Actions
//! workflow, from its `run:` steps.

use adage_core::{keys, shell, CanonicalAction, CommandTable, Effect};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::Path;

/// What a claim says.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClaimKind {
    /// "Run `make test` to test."
    Verify { command: String },
    /// "After editing `schema/*.json`, run `make gen`." `glob` is `None` when the source names no
    /// input (a CI step, a Makefile target without declared inputs): nothing can trigger it.
    RegenerateAfter { glob: Option<String>, command: String },
    /// "Look up work items with `scripts/wi-lookup W-2042`." `tool` is the command without the id.
    Lookup { tool: String, command: String, id_example: Option<String> },
    /// "Use `make test` instead of `pytest`."
    Avoid { avoid: String, instead: String },
    /// Anything that changes the environment or more: `pip install`, `make deploy`, `git push`.
    Environment { command: String },
}

impl ClaimKind {
    pub fn label(&self) -> &'static str {
        match self {
            ClaimKind::Verify { .. } => "verify",
            ClaimKind::RegenerateAfter { .. } => "regenerate_after",
            ClaimKind::Lookup { .. } => "lookup",
            ClaimKind::Avoid { .. } => "avoid",
            ClaimKind::Environment { .. } => "environment",
        }
    }

    /// The command an agent would be directed to run.
    pub fn directed(&self) -> &str {
        match self {
            ClaimKind::Verify { command }
            | ClaimKind::RegenerateAfter { command, .. }
            | ClaimKind::Lookup { command, .. }
            | ClaimKind::Environment { command } => command,
            ClaimKind::Avoid { instead, .. } => instead,
        }
    }

    /// One line, for reports: `regenerate_after(schema/*.json) → make gen`.
    pub fn describe(&self) -> String {
        match self {
            ClaimKind::Verify { command } => format!("verify: `{command}`"),
            ClaimKind::RegenerateAfter { glob: Some(g), command } => format!("regenerate_after({g}): `{command}`"),
            ClaimKind::RegenerateAfter { glob: None, command } => format!("regenerate_after(?): `{command}`"),
            ClaimKind::Lookup { tool, id_example, .. } => {
                format!("lookup: `{tool}` (e.g. {})", id_example.as_deref().unwrap_or("no example id"))
            }
            ClaimKind::Avoid { avoid, instead } => format!("avoid: `{avoid}` → `{instead}`"),
            ClaimKind::Environment { command } => format!("environment: `{command}`"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Claim {
    /// `c1`, `c2`, … in source order.
    pub id: String,
    #[serde(flatten)]
    pub kind: ClaimKind,
    /// adage-core's effect class of the directed command.
    pub effect: Effect,
    /// Why the claim is never exercised, if it is unsafe.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#unsafe: Option<String>,
    /// Where it came from: `SKILL.md:12`, `Makefile:test`, `ci.yml:run`.
    pub at: String,
    /// The sentence (or entry) it was read from, trimmed to 200 characters.
    pub text: String,
}

impl Claim {
    pub fn is_unsafe(&self) -> bool {
        self.r#unsafe.is_some()
    }
}

/// What kind of file a claim source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Markdown,
    Makefile,
    PackageJson,
    Workflow,
}

impl SourceKind {
    pub fn of(path: &Path) -> SourceKind {
        let name = path.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
        if matches!(name.as_str(), "makefile" | "gnumakefile") {
            SourceKind::Makefile
        } else if name == "package.json" {
            SourceKind::PackageJson
        } else if name.ends_with(".yml") || name.ends_with(".yaml") {
            SourceKind::Workflow
        } else {
            SourceKind::Markdown
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Commands

static PLACEHOLDER: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"<[A-Za-z_][A-Za-z0-9_ -]{0,30}>|\{[A-Za-z_][A-Za-z0-9_]{0,30}\}|\$[A-Z][A-Z0-9_]{0,30}").unwrap()
});
static TICKET: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b").unwrap());
static INLINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`([^`\n]{1,300})`").unwrap());

/// A command as adage-core sees it: its first executed argv, and the strongest effect of every
/// action it normalises to. `None` if it does not normalise to a command.
pub struct Classified {
    pub argv: Vec<String>,
    pub effect: Effect,
    /// Why it may never be exercised, beyond its effect: an unknown action, a write or a pipe.
    pub refused: Option<String>,
}

fn rank(e: Effect) -> u8 {
    match e {
        Effect::Observe => 0,
        Effect::Verify => 1,
        Effect::Regenerate => 2,
        Effect::Environment => 3,
        Effect::Author => 4,
        Effect::Publish => 5,
    }
}

pub fn above_regenerate(e: Effect) -> bool {
    rank(e) > rank(Effect::Regenerate)
}

/// Normalise and classify a command. Placeholders (`<id>`, `{id}`, `$ID`) are filled with
/// `example` (or `X-1`) first, as track validation fills slots.
pub fn classify(command: &str, table: &CommandTable, example: Option<&str>) -> Option<Classified> {
    let filled = PLACEHOLDER.replace_all(command, example.unwrap_or("X-1"));
    let items = keys::flatten(&shell::normalize_shell(&filled, "/repo", table));
    let mut argv = None;
    let mut effect = Effect::Observe;
    let mut refused = None;
    for n in &items {
        if rank(n.effect) > rank(effect) {
            effect = n.effect;
        }
        match &n.action {
            CanonicalAction::Exec { argv: a, .. } if argv.is_none() => argv = Some(a.clone()),
            CanonicalAction::Unknown { .. } => refused = Some("does not normalise to a known action".to_string()),
            _ => {}
        }
        // A regenerate step writes files by nature; anything else that writes, redirects or
        // pipes into another command is not a plain step.
        if n.mutating && n.effect != Effect::Regenerate {
            refused.get_or_insert_with(|| "writes, redirects or pipes".to_string());
        }
    }
    if items.len() > 1 {
        refused.get_or_insert_with(|| "is more than one command".to_string());
    }
    Some(Classified { argv: argv?, effect, refused })
}

/// The first argv a command runs, placeholders filled.
pub fn argv_of(command: &str, table: &CommandTable) -> Option<Vec<String>> {
    classify(command, table, None).map(|c| c.argv)
}

fn looks_path(s: &str) -> bool {
    !s.contains(char::is_whitespace)
        && (s.contains('*')
            || s.contains('/')
            || s.rsplit_once('.').is_some_and(|(a, e)| !a.is_empty() && e.len() <= 6))
}

fn is_command(s: &str, table: &CommandTable) -> bool {
    let s = s.trim();
    if s.is_empty() || s.starts_with('-') || s.starts_with('<') {
        return false;
    }
    if looks_path(s) {
        return false;
    }
    let first = s.split_whitespace().next().unwrap_or("");
    let known = table.get(first).is_some() || first.contains('/') || first.contains('=');
    known && argv_of(s, table).is_some()
}

fn safe_glob(g: &str) -> bool {
    !g.is_empty()
        && g.len() <= 120
        && !g.starts_with('/')
        && !g.contains("..")
        && g.chars().all(|c| c.is_ascii_alphanumeric() || "_./*-".contains(c))
}

/// A path or glob named in the text, as a glob: `schema/` → `schema/*`.
fn as_glob(s: &str) -> Option<String> {
    let s = s.trim().trim_start_matches("./");
    let g = if s.ends_with('/') { format!("{s}*") } else { s.to_string() };
    safe_glob(&g).then_some(g)
}

// ---------------------------------------------------------------------------------------------
// Markdown

/// One sentence of prose, with its inline code spans (and position), and any shell code block
/// that follows it.
#[derive(Debug, Default)]
struct Sentence {
    text: String,
    line: usize,
    block: Vec<String>,
}

const SHELL_LANGS: &[&str] = &["", "bash", "sh", "shell", "console", "zsh", "terminal", "shell-session"];

/// Split prose into sentences on `. `, `! ` and `? ` outside backticks (not on `;`: "`x` fails;
/// use `y`" is one instruction).
fn split_sentences(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut in_code = false;
    let chars: Vec<char> = text.chars().collect();
    for (i, &c) in chars.iter().enumerate() {
        cur.push(c);
        if c == '`' {
            in_code = !in_code;
        }
        let end = matches!(c, '.' | '!' | '?') && !in_code && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if end {
            out.push(std::mem::take(&mut cur).trim().to_string());
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out.into_iter().filter(|s| !s.is_empty()).collect()
}

/// The skill's name: `name:` in its front matter, else the file's stem.
pub fn skill_name(text: &str, path: &Path) -> String {
    let mut lines = text.lines();
    if lines.next().map(str::trim) == Some("---") {
        for l in lines {
            if l.trim() == "---" {
                break;
            }
            if let Some(v) = l.strip_prefix("name:") {
                let v = v.trim().trim_matches('"').trim_matches('\'');
                if !v.is_empty() {
                    return v.to_string();
                }
            }
        }
    }
    path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "skill".into())
}

fn sentences(text: &str) -> Vec<Sentence> {
    let mut out: Vec<Sentence> = Vec::new();
    let mut para = String::new();
    let mut para_line = 0usize;
    let flush = |para: &mut String, line: usize, out: &mut Vec<Sentence>| {
        for s in split_sentences(&para.replace('\n', " ")) {
            out.push(Sentence { text: s, line, block: vec![] });
        }
        para.clear();
    };
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    // Front matter: skipped (only `name:` is read, by `skill_name`).
    if lines.first().map(|l| l.trim()) == Some("---") {
        i = 1;
        while i < lines.len() && lines[i].trim() != "---" {
            i += 1;
        }
        i += 1;
    }
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            let fence = &t[..3];
            let lang = t[3..].trim().to_lowercase();
            let mut block = Vec::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with(fence) {
                block.push(lines[i].to_string());
                i += 1;
            }
            i += 1;
            let cmds: Vec<String> = if SHELL_LANGS.contains(&lang.as_str()) {
                block
                    .iter()
                    .map(|l| l.trim())
                    .map(|l| l.strip_prefix("$ ").unwrap_or(l).trim())
                    .filter(|l| !l.is_empty() && !l.starts_with('#'))
                    .map(str::to_string)
                    .collect()
            } else {
                vec![]
            };
            if !para.trim().is_empty() {
                flush(&mut para, para_line, &mut out);
            }
            match out.last_mut() {
                Some(s) if s.block.is_empty() && s.line + 3 >= para_line => s.block = cmds,
                _ => out.push(Sentence { text: String::new(), line: i, block: cmds }),
            }
            continue;
        }
        let item = t.starts_with("- ")
            || t.starts_with("* ")
            || t.starts_with("+ ")
            || t.split_once(". ").is_some_and(|(n, _)| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()));
        if t.is_empty() || t.starts_with('#') || item {
            flush(&mut para, para_line, &mut out);
        }
        if t.starts_with('#') {
            i += 1;
            continue;
        }
        if !t.is_empty() {
            if para.is_empty() {
                para_line = i + 1;
            }
            let body = if item { t.split_once(' ').map(|(_, r)| r).unwrap_or("") } else { t };
            para.push_str(body.trim_start_matches(['>', ' ']));
            para.push('\n');
        }
        i += 1;
    }
    flush(&mut para, para_line, &mut out);
    out
}

const CUES: &[&str] = &[
    "run",
    "use",
    "execute",
    "invoke",
    "always",
    "never",
    "after",
    "before",
    "instead",
    "don't",
    "do not",
    "must",
    "should",
    "test",
    "check",
    "regenerate",
    "look up",
    "lookup",
    "look-up",
    "avoid",
    "whenever",
    "install",
    "build",
    "deploy",
    "generate",
    "format",
    "lint",
    "release",
    "publish",
];
const AFTER: &[&str] = &["after", "whenever", "when you", "if you", "every time", "each time", "once you"];
const NEG: &[&str] = &["never", "don't", "do not", "avoid", "doesn't work", "does not work", "not work", "fails"];
const LOOKUP: &[&str] = &["look up", "lookup", "look-up", "details", "fetch", "describe", "ticket", "work item"];

/// Claims from a Markdown document (SKILL.md, CONTRIBUTING.md, a runbook).
pub fn from_markdown(text: &str, file: &str, table: &CommandTable) -> Vec<Claim> {
    let doc_ids: Vec<String> = TICKET.find_iter(text).map(|m| m.as_str().to_string()).collect();
    let mut kinds: Vec<(ClaimKind, String, String)> = Vec::new();
    for s in sentences(text) {
        let lower = s.text.to_lowercase();
        let at = format!("{file}:{}", s.line);
        // (command, position in the sentence)
        let mut cmds: Vec<(String, usize)> = Vec::new();
        let mut globs: Vec<String> = Vec::new();
        for c in INLINE.captures_iter(&s.text) {
            let g = c.get(1).unwrap();
            let v = g.as_str().trim();
            if is_command(v, table) {
                if CUES.iter().any(|w| lower.contains(w)) {
                    cmds.push((v.to_string(), g.start()));
                }
            } else if looks_path(v) {
                if let Some(g) = as_glob(v) {
                    globs.push(g);
                }
            }
        }
        for b in &s.block {
            if is_command(b, table) {
                cmds.push((b.clone(), s.text.len()));
            }
        }
        if cmds.is_empty() {
            continue;
        }
        let text: String = if s.text.is_empty() { s.block.join("; ") } else { s.text.clone() };
        let text: String = text.chars().take(200).collect();
        let mut push = |k: ClaimKind| kinds.push((k, at.clone(), text.clone()));
        // 1. avoid(x → y)
        if cmds.len() >= 2 && (lower.contains("instead of") || NEG.iter().any(|w| lower.contains(w))) {
            let (x, y) = match lower.find("instead of") {
                Some(io) => {
                    let xi = cmds.iter().position(|(_, p)| *p >= io).unwrap_or(0);
                    let yi = if xi == 0 { 1 } else { 0 };
                    (cmds[xi].0.clone(), cmds[yi].0.clone())
                }
                None => (cmds[0].0.clone(), cmds[1].0.clone()),
            };
            push(ClaimKind::Avoid { avoid: x, instead: y });
            continue;
        }
        let after = AFTER.iter().any(|w| lower.contains(w));
        for (c, _) in &cmds {
            let id = TICKET.find(c).map(|m| m.as_str().to_string()).or_else(|| doc_ids.first().cloned());
            let Some(cl) = classify(c, table, id.as_deref()) else { continue };
            let has_id = PLACEHOLDER.is_match(c) || TICKET.is_match(c);
            if cl.effect == Effect::Observe && (has_id || LOOKUP.iter().any(|w| lower.contains(w))) {
                let command = PLACEHOLDER.replace_all(c, id.as_deref().unwrap_or("{id}")).to_string();
                let tool: Vec<&str> =
                    c.split_whitespace().filter(|a| !PLACEHOLDER.is_match(a) && !TICKET.is_match(a)).collect();
                push(ClaimKind::Lookup { tool: tool.join(" "), command, id_example: id.clone() });
                continue;
            }
            match cl.effect {
                Effect::Observe => {}
                Effect::Verify => push(ClaimKind::Verify { command: c.clone() }),
                Effect::Regenerate => {
                    let glob = if after { globs.first().cloned() } else { None };
                    push(ClaimKind::RegenerateAfter { glob, command: c.clone() })
                }
                _ => push(ClaimKind::Environment { command: c.clone() }),
            }
        }
    }
    finish(kinds, table)
}

/// Number claims, drop duplicates, classify and mark unsafe ones.
fn finish(kinds: Vec<(ClaimKind, String, String)>, table: &CommandTable) -> Vec<Claim> {
    let mut seen = BTreeSet::new();
    let mut out = Vec::new();
    for (kind, at, text) in kinds {
        if !seen.insert(kind.clone()) {
            continue;
        }
        let example = match &kind {
            ClaimKind::Lookup { id_example, .. } => id_example.clone(),
            _ => None,
        };
        let c = classify(kind.directed(), table, example.as_deref());
        let effect = c.as_ref().map(|c| c.effect).unwrap_or(Effect::Environment);
        let r#unsafe = match (&kind, &c) {
            (_, None) => Some("does not normalise to a command".to_string()),
            (ClaimKind::Environment { .. }, _) | (_, Some(_)) if above_regenerate(effect) => Some(format!(
                "`{}` classifies as {}: above regenerate, never exercised",
                kind.directed(),
                effect.as_str()
            )),
            (_, Some(Classified { refused: Some(r), .. })) => Some(format!("`{}` {r}", kind.directed())),
            _ => None,
        };
        out.push(Claim { id: format!("c{}", out.len() + 1), kind, effect, r#unsafe, at, text });
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Declared files

/// Claims from a Makefile or package.json, through adage-instincts' declared facts of the
/// repository at `root` (the file's directory).
pub fn from_declared(root: &Path, kind: SourceKind, table: &CommandTable) -> Vec<Claim> {
    use adage_instincts::declared::{Source, Verdict};
    let facts = adage_instincts::extract(root);
    let want = match kind {
        SourceKind::Makefile => Source::Makefile,
        SourceKind::PackageJson => Source::PackageJson,
        _ => return vec![],
    };
    let file = if kind == SourceKind::Makefile { "Makefile" } else { "package.json" };
    let mut kinds = Vec::new();
    let mut refused: Vec<(usize, String)> = Vec::new();
    for e in facts.entries.iter().filter(|e| e.source == want) {
        let Some(command) = e.command.clone() else { continue };
        let at = format!("{file}:{}", e.name);
        let text: String = format!("{}: {}", e.name, e.body.join(" && ")).chars().take(200).collect();
        if let Verdict::Refused { reason } = &e.verdict {
            refused.push((kinds.len(), format!("its body is refused: {reason}")));
            kinds.push((ClaimKind::Environment { command }, at, text));
            continue;
        }
        match e.effect() {
            Some(Effect::Observe) | None => {}
            Some(Effect::Verify) => kinds.push((ClaimKind::Verify { command }, at, text)),
            Some(Effect::Regenerate) => {
                let inputs: Vec<String> = facts
                    .generators
                    .iter()
                    .filter(|g| g.entry == e.name && g.source == want)
                    .flat_map(|g| g.inputs.clone())
                    .collect();
                if inputs.is_empty() {
                    kinds.push((ClaimKind::RegenerateAfter { glob: None, command }, at, text));
                    continue;
                }
                for i in inputs {
                    let glob = as_glob(&i);
                    kinds.push((
                        ClaimKind::RegenerateAfter { glob, command: command.clone() },
                        at.clone(),
                        text.clone(),
                    ));
                }
            }
            Some(_) => kinds.push((ClaimKind::Environment { command }, at, text)),
        }
    }
    let mut claims = finish(kinds, table);
    for (i, why) in refused {
        if let Some(c) = claims.get_mut(i) {
            c.r#unsafe.get_or_insert(why);
        }
    }
    claims
}

/// Claims from a GitHub Actions workflow's `run:` steps. A regenerate step takes its inputs from
/// the repository's declared generator links, when one runs the same command.
pub fn from_workflow(text: &str, file: &str, root: Option<&Path>, table: &CommandTable) -> Vec<Claim> {
    let facts = root.map(adage_instincts::extract).unwrap_or_default();
    let mut kinds = Vec::new();
    for (n, cmd) in adage_instincts::declared::run_steps(text).into_iter().enumerate() {
        let Some(c) = classify(&cmd, table, None) else { continue };
        let at = format!("{file}:run[{}]", n + 1);
        match c.effect {
            Effect::Observe => {}
            Effect::Verify => kinds.push((ClaimKind::Verify { command: cmd.clone() }, at, cmd)),
            Effect::Regenerate => {
                let inputs: Vec<String> =
                    facts.generators.iter().filter(|g| g.command == cmd).flat_map(|g| g.inputs.clone()).collect();
                if inputs.is_empty() {
                    kinds.push((ClaimKind::RegenerateAfter { glob: None, command: cmd.clone() }, at, cmd));
                } else {
                    for i in inputs {
                        kinds.push((
                            ClaimKind::RegenerateAfter { glob: as_glob(&i), command: cmd.clone() },
                            at.clone(),
                            cmd.clone(),
                        ));
                    }
                }
            }
            _ => kinds.push((ClaimKind::Environment { command: cmd.clone() }, at, cmd)),
        }
    }
    finish(kinds, table)
}

/// Claims from any supported source file. `root` is the repository (for declared facts).
pub fn extract(path: &Path, text: &str, root: &Path, table: &CommandTable) -> Vec<Claim> {
    let file = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
    match SourceKind::of(path) {
        SourceKind::Markdown => from_markdown(text, &file, table),
        k @ (SourceKind::Makefile | SourceKind::PackageJson) => {
            let dir = path.parent().filter(|p| !p.as_os_str().is_empty()).unwrap_or(root);
            from_declared(dir, k, table)
        }
        SourceKind::Workflow => from_workflow(text, &file, Some(root), table),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(md: &str) -> Vec<(String, Option<String>)> {
        from_markdown(md, "SKILL.md", &CommandTable::load())
            .into_iter()
            .map(|c| (c.kind.describe(), c.r#unsafe.map(|_| "unsafe".to_string())))
            .collect()
    }

    #[test]
    fn imperative_sentences_become_typed_claims() {
        let md = "---\nname: acme\ndescription: x\n---\n# Acme\n\nRun the tests with `python3 -m unittest`.\n\
                  After editing `schema/*.json`, run `make gen`.\n\n- Look up work items such as W-2042 with \
                  `python3 scripts/wi-lookup W-2042`.\n- Install dependencies with `pip install -r requirements.txt`.\n\
                  - Use `make test` instead of `pytest`.\n\nThe `acme/` package holds the code.\n";
        let got = kinds(md);
        assert_eq!(
            got,
            vec![
                ("verify: `python3 -m unittest`".to_string(), None),
                ("regenerate_after(schema/*.json): `make gen`".to_string(), None),
                ("lookup: `python3 scripts/wi-lookup` (e.g. W-2042)".to_string(), None),
                ("environment: `pip install -r requirements.txt`".to_string(), Some("unsafe".into())),
                ("avoid: `pytest` → `make test`".to_string(), None),
            ],
            "{got:#?}"
        );
        assert_eq!(skill_name(md, Path::new("x/SKILL.md")), "acme");
    }

    #[test]
    fn shell_blocks_attach_to_their_sentence() {
        let md = "Whenever you change `proto/*.proto`, regenerate:\n\n```bash\n$ make gen\n# comment\n```\n\n\
                  ```json\n{\"make\": \"test\"}\n```\n";
        assert_eq!(kinds(md), vec![("regenerate_after(proto/*.proto): `make gen`".to_string(), None)]);
    }

    #[test]
    fn prose_without_an_instruction_is_not_a_claim() {
        assert!(kinds("The history of `make` is long.\n").is_empty());
        assert!(kinds("See `schema/a.json`.\n").is_empty());
    }

    #[test]
    fn publishing_and_deleting_are_unsafe() {
        let got = kinds("Always run `git push --force` after `rm -rf build`.\n");
        assert!(!got.is_empty() && got.iter().all(|(_, u)| u.is_some()), "{got:?}");
        let got = kinds("Before committing, run `make test | tee log.txt`.\n");
        assert!(got.iter().all(|(_, u)| u.is_some()), "{got:?}");
    }

    #[test]
    fn avoid_claims_read_either_order() {
        for md in [
            "`python3 -m unittest` fails here; use `make test`.\n",
            "Never run `python3 -m unittest`, use `make test`.\n",
            "Instead of `python3 -m unittest`, run `make test`.\n",
            "Run `make test` instead of `python3 -m unittest`.\n",
        ] {
            assert_eq!(kinds(md), vec![("avoid: `python3 -m unittest` → `make test`".to_string(), None)], "{md}");
        }
    }
}
