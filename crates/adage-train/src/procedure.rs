//! Skill → procedure graph, deterministically (no model, no execution).
//!
//! A procedure graph is the compile-time view of a skill (a `SKILL.md`, a runbook): its steps in
//! order, each with a **role** (lookup, read, search, edit, regenerate, verify, run, install, vcs,
//! remove, note), the **commands** it directs as templates with typed slots (`scripts/wi-lookup
//! {id}`), the commands it **forbids** and what to run instead (with the failure signature the
//! skill gives), a **precondition** (always, after an edit to a glob, before finishing, or a
//! condition only an agent can judge) and what it **expects** to see. Blocks that direct no
//! command are kept as **advice**. [`crate::compile`] turns the graph into routes.
//!
//! The parsing rules, each found by a wrong first parse in the skill lab (evals/lab/skills):
//!
//! - A command counts only in an imperative sentence (run, use, never, after, before, …) or in a
//!   shell code block; a mention is not an instruction ("fails if you skipped `make gen`").
//! - In a sentence that forbids (never, do not, instead of), the commands before *instead of* are
//!   forbidden, and the one after it is what to run instead.
//! - `**bold**` is stripped before sentences are split.
//! - A concrete command that instantiates a template is its example (`scripts/wi-lookup W-2042`
//!   gives slot `id` the example `W-2042` and the pattern `^W-\d{4}$`).
//! - A quoted literal in an example command (`grep -rn "prompt_suffix" src/click`) is the agent's
//!   choice: a judgment slot.

use adage_core::CommandTable;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// What a step (or a command) is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Lookup,
    Read,
    Search,
    Edit,
    Regenerate,
    Verify,
    Run,
    Install,
    Vcs,
    Remove,
    Note,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Lookup => "lookup",
            Role::Read => "read",
            Role::Search => "search",
            Role::Edit => "edit",
            Role::Regenerate => "regenerate",
            Role::Verify => "verify",
            Role::Run => "run",
            Role::Install => "install",
            Role::Vcs => "vcs",
            Role::Remove => "remove",
            Role::Note => "note",
        }
    }
}

/// Where a slot's value comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SlotFrom {
    /// The user's prompt (a work item id).
    Prompt,
    /// The edited file (its module name, its path).
    Edit,
    /// The agent's own choice (a search term): nothing Adage sees binds it.
    Judgment,
}

/// A typed placeholder of a command template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SlotDef {
    pub name: String,
    /// `work-item-id`, `module`, `repo-path`, `symbol`, `string`.
    pub kind: String,
    /// The anchored pattern a value must match.
    pub pattern: String,
    pub from: SlotFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example: Option<String>,
}

/// A command a step directs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Command {
    /// As the skill writes it, placeholders as `{name}`: `scripts/wi-lookup {id}`.
    pub template: String,
    pub role: Role,
    pub slots: Vec<String>,
}

/// A command a step forbids.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forbidden {
    pub command: String,
    /// What to run instead (empty when the skill names nothing).
    pub instead: String,
    /// The failure signature the skill gives (`RuntimeError: ACME_ENV is not set`), if any.
    pub signature: String,
}

/// When a step applies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "glob", rename_all = "snake_case")]
pub enum Pre {
    Always,
    /// After an edit to a file matching this glob.
    AfterEdit(String),
    /// Before the agent finishes.
    BeforeFinish,
    /// A condition only the agent can judge ("if the item changes a record field").
    Judgment,
}

impl Pre {
    pub fn describe(&self) -> String {
        match self {
            Pre::Always => "always".into(),
            Pre::AfterEdit(g) => format!("after_edit {g}"),
            Pre::BeforeFinish => "before_finish".into(),
            Pre::Judgment => "judgment".into(),
        }
    }
}

/// One step of the procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Step {
    /// `s1`, `s2`, … in the skill's order.
    pub id: String,
    pub title: String,
    pub role: Role,
    /// The step's text, whitespace collapsed.
    pub text: String,
    pub commands: Vec<Command>,
    pub avoid: Vec<Forbidden>,
    pub pre: Pre,
    /// Strings the step says its command's output shows (`OK`, `passed`).
    pub expect: Vec<String>,
    /// Globs of the files an edit step changes (`acme/` → `acme/**`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<String>,
}

/// The skill file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillInfo {
    pub name: String,
    pub description: String,
    /// The file's name (never a path: a pack holds no absolute path).
    pub file: String,
    pub sha256: String,
    pub bytes: u64,
    /// About 4 characters a token: what the skill costs in context, every call.
    pub approx_tokens: u64,
}

/// A skill's procedure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Graph {
    pub skill: SkillInfo,
    pub slots: BTreeMap<String, SlotDef>,
    pub steps: Vec<Step>,
    /// Blocks that direct no command, whitespace collapsed.
    pub advice: Vec<String>,
}

// ---------------------------------------------------------------------------------------------
// Commands

static PLACEHOLDER: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"<([A-Za-z_][A-Za-z0-9_-]{0,30})>|\{([a-z_][a-z0-9_]{0,30})\}").unwrap());
static INLINE: Lazy<Regex> = Lazy::new(|| Regex::new(r"`([^`\n]{1,300})`").unwrap());
static QUOTED: Lazy<Regex> = Lazy::new(|| Regex::new(r#""[^"]+"|'[^']+'"#).unwrap());
static REPO_SCRIPT: Lazy<Regex> = Lazy::new(|| Regex::new(r"^(scripts|bin|tools)/[A-Za-z0-9_.-]+$").unwrap());
static ENV_ASSIGN: Lazy<Regex> = Lazy::new(|| Regex::new(r"^[A-Z_][A-Z0-9_]*=").unwrap());

fn words(cmd: &str) -> Vec<&str> {
    cmd.split_whitespace().collect()
}

/// The argv after leading `NAME=value` assignments.
fn program_words(cmd: &str) -> Vec<&str> {
    words(cmd).into_iter().skip_while(|w| ENV_ASSIGN.is_match(w)).collect()
}

/// Whether an inline code span or a shell line is a command (not a path, a flag, an identifier
/// or an error message): its program is in the command table, or a repository script or
/// environment path (`./x`, `scripts/x`, `.venv/bin/python`).
pub fn is_command(code: &str, table: &CommandTable) -> bool {
    let filled = PLACEHOLDER.replace_all(code, "X");
    let a = program_words(&filled);
    let Some(p) = a.first() else { return false };
    if p.starts_with('-') || p.ends_with(':') {
        return false;
    }
    if p.starts_with("./") || adage_core::action::venv_of(p).is_some() {
        return true;
    }
    if p.contains('/') {
        return REPO_SCRIPT.is_match(p);
    }
    table.get(p).is_some()
}

const VERIFY: [&str; 15] = [
    "make test",
    "make ci",
    "make check",
    "make lint",
    "pytest",
    "unittest",
    "tox",
    "nox",
    "npm test",
    "npm run test",
    "pnpm test",
    "go test",
    "cargo test",
    "py_compile",
    "cargo check",
];

/// A command's role, from its words.
pub fn role_of(cmd: &str) -> Role {
    let c = format!(" {cmd} ");
    let a = program_words(cmd);
    let p = a.first().copied().unwrap_or("");
    let base = p.rsplit('/').next().unwrap_or(p);
    let inline = adage_core::action::is_python(base) && a.contains(&"-c");
    match base {
        "grep" | "rg" | "find" | "ls" | "fd" => return Role::Search,
        "cat" | "head" | "tail" | "less" => return Role::Read,
        "sed" if a.contains(&"-n") => return Role::Read,
        "git" | "gh" if !c.contains(" view ") => return Role::Vcs,
        "rm" => return Role::Remove,
        _ => {}
    }
    if c.contains("lookup") || c.contains(" --help ") || c.contains(" issue view ") || c.contains(" pr view ") {
        return Role::Lookup;
    }
    let gen_script = a.get(1).is_some_and(|x| {
        let n = x.rsplit('/').next().unwrap_or(x);
        n == "gen.py" || n == "generate.py"
    });
    if (base == "make" && a.get(1).is_some_and(|x| x.starts_with("gen")))
        || a.contains(&"generate")
        || a.contains(&"codegen")
        || (!inline && gen_script)
    {
        return Role::Regenerate;
    }
    if !inline && VERIFY.iter().any(|k| c.contains(&format!(" {k}")) || c.contains(&format!("/{k}"))) {
        return Role::Verify;
    }
    if matches!(base, "pip" | "pip3" | "npm" | "pnpm" | "yarn" | "bun") && a.contains(&"install")
        || (base == "uv" && a.contains(&"sync"))
    {
        return Role::Install;
    }
    if (base == "sed" || base == "perl") && a.iter().any(|x| x.starts_with("-i")) {
        return Role::Edit;
    }
    Role::Run
}

/// A slot's definition from its name and an example value.
pub fn slot_for(name: &str, example: Option<&str>) -> SlotDef {
    let n = name.to_lowercase().replace('-', "_");
    let ex = example.map(str::to_string);
    let def = |name: &str, kind: &str, pattern: String, from| SlotDef {
        name: name.into(),
        kind: kind.into(),
        pattern,
        from,
        example: ex.clone(),
    };
    match n.as_str() {
        "id" | "item" | "ticket" => {
            let mut pat = r"^[A-Z]{1,5}-\d{1,9}$".to_string();
            if let Some((letters, digits)) = example.and_then(|e| e.split_once('-')) {
                let ok = !letters.is_empty()
                    && letters.len() <= 5
                    && letters.bytes().all(|c| c.is_ascii_uppercase())
                    && !digits.is_empty()
                    && digits.len() <= 9
                    && digits.bytes().all(|c| c.is_ascii_digit());
                if ok {
                    pat = format!(r"^{letters}-\d{{{}}}$", digits.len());
                }
            }
            def("id", "work-item-id", pat, SlotFrom::Prompt)
        }
        "module" | "record" => def(&n, "module", r"^[a-z_][a-z0-9_]{0,63}$".into(), SlotFrom::Edit),
        "path" | "file" => def(&n, "repo-path", r"^[A-Za-z0-9_][A-Za-z0-9_./-]{0,199}$".into(), SlotFrom::Edit),
        "symbol" | "name" | "x" => def(&n, "symbol", r"^[A-Za-z_][A-Za-z0-9_]{0,63}$".into(), SlotFrom::Judgment),
        _ => def(&n, "string", r"^[A-Za-z0-9_.-]{1,64}$".into(), SlotFrom::Judgment),
    }
}

/// Placeholders to `{name}` (`<ID>`, `<item>`, `{ticket}` → `{id}`), and the names found.
pub fn templatize(code: &str) -> (String, Vec<String>) {
    let mut names = Vec::new();
    let t = PLACEHOLDER
        .replace_all(code, |c: &regex::Captures| {
            let raw = c.get(1).or_else(|| c.get(2)).map(|m| m.as_str()).unwrap_or("x").to_lowercase().replace('-', "_");
            let n = if matches!(raw.as_str(), "id" | "item" | "ticket") { "id".to_string() } else { raw };
            names.push(n.clone());
            format!("{{{n}}}")
        })
        .to_string();
    (t, names)
}

/// The names a template uses.
pub fn slot_names(template: &str) -> Vec<String> {
    let mut v: Vec<String> = PLACEHOLDER
        .captures_iter(template)
        .filter_map(|c| c.get(2).map(|m| m.as_str().to_string()))
        .collect();
    v.sort();
    v.dedup();
    v
}

// ---------------------------------------------------------------------------------------------
// Markdown blocks

#[derive(Debug, Clone, PartialEq)]
enum Kind {
    Heading,
    Item,
    Para,
    Fence,
}

#[derive(Debug, Clone)]
struct Fence {
    lang: String,
    lines: Vec<String>,
}

#[derive(Debug, Clone)]
struct Block {
    kind: Kind,
    text: String,
    fences: Vec<Fence>,
}

fn is_numbered(s: &str) -> Option<&str> {
    let (n, rest) = s.split_once(". ")?;
    (!n.is_empty() && n.len() <= 3 && n.bytes().all(|c| c.is_ascii_digit())).then_some(rest)
}

fn bullet(s: &str) -> Option<&str> {
    ["- ", "* ", "+ "].iter().find_map(|b| s.strip_prefix(b))
}

/// Front matter (`name:`, `description:`) and the body after it.
fn front_matter(text: &str) -> (BTreeMap<String, String>, &str) {
    let mut meta = BTreeMap::new();
    if let Some(rest) = text.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some((k, v)) = line.split_once(':') {
                    meta.insert(k.trim().to_string(), v.trim().trim_matches('"').trim_matches('\'').to_string());
                }
            }
            let after = &rest[end + 4..];
            return (meta, after.strip_prefix('\n').unwrap_or(after));
        }
    }
    (meta, text)
}

/// Numbered items (with their continuation lines and fences), bullets, headings, paragraphs and
/// fences, in order.
fn blocks(body: &str) -> Vec<Block> {
    let mut out: Vec<Block> = Vec::new();
    // Index into `out` of the block continuation lines join.
    let mut cur: Option<usize> = None;
    let mut fence: Option<Fence> = None;
    for line in body.lines() {
        let s = line.trim();
        if let Some(f) = fence.as_mut() {
            if s.starts_with("```") || s.starts_with("~~~") {
                let f = fence.take().unwrap();
                match cur.filter(|&i| out[i].kind == Kind::Item) {
                    Some(i) => out[i].fences.push(f),
                    None => out.push(Block { kind: Kind::Fence, text: String::new(), fences: vec![f] }),
                }
            } else {
                f.lines.push(line.to_string());
            }
            continue;
        }
        if s.starts_with("```") || s.starts_with("~~~") {
            fence = Some(Fence { lang: s[3..].trim().to_lowercase(), lines: Vec::new() });
            continue;
        }
        if s.starts_with('#') {
            out.push(Block { kind: Kind::Heading, text: s.trim_start_matches('#').trim().to_string(), fences: vec![] });
            cur = None;
            continue;
        }
        let indented = line.starts_with(' ') || line.starts_with('\t');
        if let Some(rest) = is_numbered(s).filter(|_| !indented) {
            out.push(Block { kind: Kind::Item, text: rest.to_string(), fences: vec![] });
            cur = Some(out.len() - 1);
        } else if let Some(rest) = bullet(s).filter(|_| !indented) {
            out.push(Block { kind: Kind::Para, text: rest.to_string(), fences: vec![] });
            cur = Some(out.len() - 1);
        } else if s.is_empty() {
            if cur.is_some_and(|i| out[i].kind != Kind::Item) {
                cur = None;
            }
        } else if let Some(i) = cur.filter(|&i| indented || out[i].kind == Kind::Para) {
            out[i].text.push(' ');
            out[i].text.push_str(s);
        } else {
            out.push(Block { kind: Kind::Para, text: s.to_string(), fences: vec![] });
            cur = Some(out.len() - 1);
        }
    }
    out
}

const SHELL_LANGS: [&str; 8] = ["", "bash", "sh", "shell", "console", "zsh", "terminal", "shell-session"];

/// The commands of shell code blocks (a `\` continuation joins lines; `$ ` prompts dropped).
fn shell_lines(fences: &[Fence], table: &CommandTable) -> Vec<String> {
    let mut out = Vec::new();
    for f in fences.iter().filter(|f| SHELL_LANGS.contains(&f.lang.as_str())) {
        let joined = f.lines.join("\n").replace("\\\n", " ");
        for l in joined.lines() {
            let l = l.trim();
            let l = l.strip_prefix("$ ").unwrap_or(l).trim();
            if !l.is_empty() && !l.starts_with('#') && is_command(l, table) {
                out.push(l.split_whitespace().collect::<Vec<_>>().join(" "));
            }
        }
    }
    out
}

/// Commands named in a plain-text decision tree (`Run: python x.py --help`).
fn tree_commands(fences: &[Fence], table: &CommandTable) -> Vec<String> {
    static RUN: Lazy<Regex> = Lazy::new(|| Regex::new(r"\bRun:\s*(.+)$").unwrap());
    fences
        .iter()
        .flat_map(|f| f.lines.iter())
        .filter_map(|l| RUN.captures(l).map(|c| c[1].trim().to_string()))
        .filter(|c| is_command(c, table))
        .collect()
}

// ---------------------------------------------------------------------------------------------
// Sentences

static IMPERATIVE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(run|use|always|never|after|before|instead|install|invoke|execute|call|start|search|look up|check)\b")
        .unwrap()
});
static MENTION: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(fails?|if you skipped|it prints|is generated)\b").unwrap());
static EXAMPLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(for example|e\.g\.|such as)\b").unwrap());
static FORBIDS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(never|do not|don't|instead of|avoid)\b").unwrap());
static INSTEAD: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\binstead of\b|\binstead\b").unwrap());
static AFTER_EDIT: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"(?i)\b(after|whenever|when you)\s+(editing|changing|you edit|you change|modifying)\s+`([^`]+)`").unwrap()
});
static FINISH: Lazy<Regex> =
    Lazy::new(|| Regex::new(r"(?i)\bbefore (you )?(finish|finishing|commit|committing|are done|you are done)\b").unwrap());
static JUDGE: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)^\s*if\b|\bif the (item|report|task)\b").unwrap());
static EXPECT: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\bexpects?\s+`([^`]+)`").unwrap());
static FAILS_WITH: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\bfails? with\s+`([^`]+)`").unwrap());
static EDIT_GLOBS: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r"`([A-Za-z0-9_./*-]+/)`|`([A-Za-z0-9_./-]*\*[A-Za-z0-9_./*-]*)`|under `([A-Za-z0-9_./-]+)`").unwrap()
});
static ID_IN_PROSE: Lazy<Regex> = Lazy::new(|| Regex::new(r"\b([A-Z]{1,5}-\d{3,9})\b").unwrap());
static EDIT_WORDS: Lazy<Regex> = Lazy::new(|| Regex::new(r"(?i)\b(edit|change|fix|add)\b").unwrap());
static BOLD_TITLE: Lazy<Regex> = Lazy::new(|| Regex::new(r"^\*\*(.+?)\*\*").unwrap());

/// Split prose into sentences at `.`, `!` or `?` followed by whitespace and a capital, `*` or a
/// backtick, outside code spans.
fn sentences(text: &str) -> Vec<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut code = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        cur.push(c);
        if c == '`' {
            code = !code;
        }
        if matches!(c, '.' | '!' | '?') && !code {
            let mut j = i + 1;
            while j < chars.len() && chars[j].is_whitespace() {
                j += 1;
            }
            if j > i + 1 && j < chars.len() && (chars[j].is_uppercase() || chars[j] == '*' || chars[j] == '`') {
                out.push(std::mem::take(&mut cur).trim().to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

fn inline_commands(s: &str, table: &CommandTable) -> Vec<String> {
    INLINE.captures_iter(s).map(|c| c[1].trim().to_string()).filter(|c| is_command(c, table)).collect()
}

/// The commands an imperative sentence directs. A quoted literal in an example becomes a
/// judgment slot.
fn commands_in(s: &str, table: &CommandTable) -> Vec<String> {
    if !IMPERATIVE.is_match(s) || MENTION.is_match(s) {
        return Vec::new();
    }
    inline_commands(s, table)
        .into_iter()
        .map(|c| if EXAMPLE.is_match(s) && QUOTED.is_match(&c) { QUOTED.replace_all(&c, "<symbol>").to_string() } else { c })
        .collect()
}

fn directs(text: &str, table: &CommandTable) -> bool {
    sentences(&text.replace("**", "")).iter().any(|s| !commands_in(s, table).is_empty())
}

fn title_of(text: &str) -> String {
    let t = match BOLD_TITLE.captures(text) {
        Some(c) => c[1].to_string(),
        None => sentences(&text.replace("**", "")).into_iter().next().unwrap_or_default(),
    };
    t.trim().trim_end_matches('.').to_string()
}

fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Globs of the files a step's text names (`acme/` → `acme/**`, `schema/*.json`).
fn edit_globs(text: &str) -> Vec<String> {
    EDIT_GLOBS
        .captures_iter(text)
        .filter_map(|c| c.get(1).or_else(|| c.get(2)).or_else(|| c.get(3)).map(|m| m.as_str().to_string()))
        .map(|g| if g.ends_with('/') { format!("{g}**") } else if !g.contains('*') { format!("{g}/**") } else { g })
        .collect()
}

/// A regex matching a template's instances (placeholders as `[A-Za-z0-9_.-]+` groups).
fn instance_re(template: &str) -> Option<(Regex, Vec<String>)> {
    let mut pat = String::from("^");
    let mut names = Vec::new();
    let mut last = 0;
    for c in PLACEHOLDER.captures_iter(template) {
        let m = c.get(0)?;
        pat.push_str(&regex::escape(&template[last..m.start()]));
        pat.push_str("([A-Za-z0-9_.-]+)");
        names.push(c.get(2).or_else(|| c.get(1)).map(|x| x.as_str().to_string())?);
        last = m.end();
    }
    pat.push_str(&regex::escape(&template[last..]));
    pat.push('$');
    Some((Regex::new(&pat).ok()?, names))
}

struct Parsed {
    step: Step,
    examples: BTreeMap<String, String>,
    names: Vec<String>,
}

fn parse_step(n: usize, text: &str, fences: &[Fence], table: &CommandTable) -> Parsed {
    let plain = text.replace("**", "");
    let sents = sentences(&plain);
    let mut cmds: Vec<String> = Vec::new();
    let mut forbidden: Vec<String> = Vec::new();
    let mut generic: Vec<String> = Vec::new();
    for s in &sents {
        if FORBIDS.is_match(s) {
            let mut parts = INSTEAD.splitn(s, 2);
            let before = parts.next().unwrap_or("");
            let after = parts.next();
            let avoided = inline_commands(before, table);
            if !avoided.is_empty() {
                forbidden.extend(avoided);
                if let Some(a) = after {
                    generic.extend(inline_commands(a, table));
                }
                continue;
            }
        }
        cmds.extend(commands_in(s, table));
    }
    cmds.extend(shell_lines(fences, table));
    cmds.extend(tree_commands(fences, table));

    // Templates, deduplicated; a concrete command that instantiates one is its example.
    let mut templates: Vec<String> = Vec::new();
    let mut names: Vec<String> = Vec::new();
    for c in &cmds {
        let (t, ns) = templatize(c);
        names.extend(ns);
        if !templates.contains(&t) {
            templates.push(t);
        }
    }
    let mut examples: BTreeMap<String, String> = BTreeMap::new();
    let concrete: Vec<String> = templates.iter().filter(|t| !t.contains('{')).cloned().collect();
    for t in templates.clone().iter().filter(|t| t.contains('{')) {
        let Some((re, ns)) = instance_re(t) else { continue };
        for c in &concrete {
            if let Some(m) = re.captures(c) {
                for (i, n) in ns.iter().enumerate() {
                    if let Some(v) = m.get(i + 1) {
                        examples.insert(n.clone(), v.as_str().to_string());
                    }
                }
                templates.retain(|x| x != c);
            }
        }
    }
    // A command that is a prefix of a more specific one of the step is the generic form.
    let specific = templates.clone();
    templates.retain(|t| !specific.iter().any(|o| o != t && o.starts_with(&format!("{t} "))));
    let commands: Vec<Command> =
        templates.iter().map(|t| Command { template: t.clone(), role: role_of(t), slots: slot_names(t) }).collect();

    let pre = if let Some(c) = AFTER_EDIT.captures(&plain) {
        Pre::AfterEdit(c[3].to_string())
    } else if FINISH.is_match(&plain) {
        Pre::BeforeFinish
    } else if JUDGE.is_match(&plain) {
        Pre::Judgment
    } else {
        Pre::Always
    };
    let mut expect = Vec::new();
    let mut signature = String::new();
    for s in &sents {
        if let Some(c) = FAILS_WITH.captures(s) {
            if signature.is_empty() {
                signature = c[1].to_string();
            }
        }
        if let Some(c) = EXPECT.captures(s) {
            expect.push(c[1].to_string());
        }
    }
    let instead = generic
        .iter()
        .filter(|g| !g.contains('<') && !g.contains('{'))
        .cloned()
        .chain(commands.iter().filter(|c| c.slots.is_empty()).map(|c| c.template.clone()))
        .next()
        .or_else(|| commands.first().map(|c| c.template.clone()))
        .unwrap_or_default();
    let mut avoid: Vec<Forbidden> = Vec::new();
    for f in forbidden {
        if !avoid.iter().any(|a| a.command == f) {
            avoid.push(Forbidden { command: f, instead: instead.clone(), signature: signature.clone() });
        }
    }
    let role = if let Some(main) = commands.iter().find(|c| c.role != Role::Lookup || !c.template.contains("--help")) {
        main.role
    } else if let Some(c) = commands.first() {
        c.role
    } else if !avoid.is_empty() {
        Role::Verify
    } else if EDIT_WORDS.is_match(text) {
        Role::Edit
    } else {
        Role::Note
    };
    let edits = if role == Role::Edit { edit_globs(text) } else { Vec::new() };
    Parsed {
        step: Step {
            id: format!("s{n}"),
            title: collapse(&title_of(text)),
            role,
            text: collapse(text),
            commands,
            avoid,
            pre,
            expect,
            edits,
        },
        examples,
        names,
    }
}

/// Parse a skill (Markdown, optionally with front matter) into its procedure graph. `file` is the
/// file's name, for the record; `fallback_name` names a skill whose front matter does not.
pub fn parse(bytes: &[u8], file: &str, fallback_name: &str, table: &CommandTable) -> Graph {
    let raw = String::from_utf8_lossy(bytes).replace("\r\n", "\n");
    let (meta, body) = front_matter(&raw);
    let bl = blocks(body);
    let items: Vec<&Block> = bl.iter().filter(|b| b.kind == Kind::Item).collect();
    let mut parsed: Vec<Parsed> = Vec::new();
    let mut advice: Vec<String> = Vec::new();
    let numbered = items.iter().any(|b| directs(&b.text, table) || !shell_lines(&b.fences, table).is_empty());
    let mut next = 1;
    for b in &bl {
        let is_step = match b.kind {
            Kind::Heading => false,
            Kind::Item if numbered => true,
            Kind::Item | Kind::Para | Kind::Fence => {
                directs(&b.text, table)
                    || !shell_lines(&b.fences, table).is_empty()
                    || !tree_commands(&b.fences, table).is_empty()
            }
        };
        if is_step {
            parsed.push(parse_step(next, if b.text.is_empty() { "commands" } else { &b.text }, &b.fences, table));
            next += 1;
        } else if matches!(b.kind, Kind::Para | Kind::Item) && !b.text.is_empty() {
            advice.push(collapse(&b.text));
        } else if let (Kind::Fence, Some(f)) = (&b.kind, b.fences.first()) {
            advice.push(format!(
                "[{} block: {} lines of code to imitate]",
                if f.lang.is_empty() { "text" } else { &f.lang },
                f.lines.len()
            ));
        }
    }
    // Slots: every name a step uses, with the first example given for it.
    let mut slots: BTreeMap<String, SlotDef> = BTreeMap::new();
    for p in &parsed {
        for n in &p.names {
            let ex = p.examples.get(n).map(String::as_str);
            let replace = match slots.get(n) {
                None => true,
                Some(s) => s.example.is_none() && ex.is_some(),
            };
            if replace {
                slots.insert(n.clone(), slot_for(n, ex));
            }
        }
    }
    // An id in the prose ("such as W-2042") is the id slot's example.
    if slots.get("id").is_some_and(|s| s.example.is_none()) {
        if let Some(m) = ID_IN_PROSE.captures(body) {
            slots.insert("id".into(), slot_for("id", Some(&m[1])));
        }
    }
    let name = meta.get("name").filter(|n| !n.is_empty()).cloned().unwrap_or_else(|| fallback_name.to_string());
    let skill = SkillInfo {
        name,
        description: meta.get("description").cloned().unwrap_or_default(),
        file: file.to_string(),
        sha256: Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect(),
        bytes: bytes.len() as u64,
        approx_tokens: (raw.chars().count() as u64).div_ceil(4),
    };
    Graph { skill, slots, steps: parsed.into_iter().map(|p| p.step).collect(), advice }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ACME: &str = include_str!("../../../evals/lab/skills/skills/acme-work-item/SKILL.md");
    const CLICK: &str = include_str!("../../../evals/lab/skills/skills/click-bugfix/SKILL.md");

    fn g(text: &str) -> Graph {
        parse(text.as_bytes(), "SKILL.md", "skill", &CommandTable::load())
    }

    #[test]
    fn the_acme_skill_parses_into_its_procedure() {
        let g = g(ACME);
        assert_eq!(g.skill.name, "acme-work-item");
        assert_eq!(g.steps.len(), 5, "{:#?}", g.steps);
        let s = |i: usize| &g.steps[i];
        assert_eq!(s(0).role, Role::Lookup);
        assert_eq!(s(0).commands.iter().map(|c| c.template.as_str()).collect::<Vec<_>>(), ["scripts/wi-lookup {id}"]);
        assert_eq!(g.slots["id"].example.as_deref(), Some("W-2042"));
        assert_eq!(g.slots["id"].pattern, r"^W-\d{4}$");
        assert_eq!(s(1).role, Role::Regenerate);
        assert_eq!(s(1).pre, Pre::AfterEdit("schema/*.json".into()));
        assert_eq!(s(1).commands[0].template, "make gen");
        assert_eq!(s(2).role, Role::Edit);
        assert!(s(2).commands.is_empty());
        assert_eq!(s(2).edits, ["acme/**"]);
        assert_eq!(s(3).role, Role::Verify);
        assert_eq!(s(3).commands[0].template, "make test");
        let av: Vec<&str> = s(3).avoid.iter().map(|a| a.command.as_str()).collect();
        assert_eq!(av, ["python3 -m unittest", "pytest"]);
        assert!(s(3).avoid.iter().all(|a| a.instead == "make test" && a.signature == "RuntimeError: ACME_ENV is not set"));
        assert_eq!(s(3).expect, ["OK"]);
        assert_eq!(s(4).pre, Pre::BeforeFinish);
        assert_eq!(s(4).commands[0].template, "make ci");
        assert_eq!(g.advice.len(), 3, "{:?}", g.advice);
        assert!(g.advice.iter().any(|a| a.contains("Do not commit")));
    }

    #[test]
    fn the_click_skill_has_judgment_and_edit_slots() {
        let g = g(CLICK);
        assert_eq!(g.steps.len(), 6, "{:#?}", g.steps);
        assert_eq!(g.slots["symbol"].from, SlotFrom::Judgment);
        assert_eq!(g.slots["module"].from, SlotFrom::Edit);
        let s3 = &g.steps[2];
        assert_eq!(s3.commands[0].template, ".venv/bin/python -m pytest tests/test_{module}.py -q");
        assert_eq!(s3.avoid.len(), 3);
        assert!(s3.avoid.iter().all(|a| a.signature == "No module named pytest"));
        assert_eq!(g.steps[4].pre, Pre::AfterEdit("src/click/*.py".into()));
        assert_eq!(g.steps[5].pre, Pre::BeforeFinish);
        assert_eq!(g.steps[3].role, Role::Edit);
    }

    #[test]
    fn mentions_and_paths_are_not_commands() {
        let t = CommandTable::load();
        assert!(is_command("make gen", &t));
        assert!(is_command("scripts/wi-lookup <ID>", &t));
        assert!(is_command(".venv/bin/python -m pytest", &t));
        assert!(!is_command("schema/*.json", &t));
        assert!(!is_command("acme/generated/models.py", &t));
        assert!(!is_command("RuntimeError: ACME_ENV is not set", &t));
        assert!(!is_command("OK", &t));
        assert!(commands_in("It fails with `generated models are stale` if you skipped `make gen`.", &t).is_empty());
        assert_eq!(sentences("Run `a`. Then b! `c` d. e.g. x"), ["Run `a`.", "Then b!", "`c` d. e.g. x"]);
    }

    #[test]
    fn the_same_skill_parses_the_same_way() {
        assert_eq!(g(ACME), g(ACME));
        assert_eq!(g(ACME).skill.sha256.len(), 64);
    }
}
