//! Shell → canonical actions, via `tree-sitter` + `tree-sitter-bash`.
//!
//! Compared to the TypeScript proof of concept (`research/poc/src/normalize/shell.ts`, a
//! dependency-free hand-rolled tokenizer), this uses a real shell grammar so that compound
//! constructs (`for`, `while`, `if`, `case`, subshells, `$(…)`) are parsed rather than treated
//! as one opaque low-confidence blob. See SPIKE.md's method section.

use std::sync::atomic::{AtomicUsize, Ordering};
use tree_sitter::{Node, Parser, Tree};

use crate::table::CommandTable;
use crate::types::{CanonicalAction, Effect, Normalized};

/// Trailing pipeline stages that only filter keep a pipeline read-only (ported from the PoC's
/// `PURE_FILTERS`, extended slightly).
const PURE_FILTERS: &[&str] = &[
    "head", "tail", "grep", "rg", "jq", "sed", "wc", "sort", "uniq", "cut", "awk", "tr", "column", "cat", "less",
    "more", "fold", "tac",
    "rev",
    // deliberately NOT here: "xargs" — its purity depends entirely on the command it runs.
];

/// Shell built-ins / assignment-only forms that consume no worldly action of their own.
const NOOP_BUILTINS: &[&str] = &[
    "cd", "export", "local", "declare", "typeset", "readonly", "unset", "set", "shift", "return", "exit", "alias",
    "unalias", "trap", "ulimit", "umask", "wait", "fg", "bg", "jobs", "type",
];

thread_local! {
    static PARSER: std::cell::RefCell<Parser> = std::cell::RefCell::new({
        let mut p = Parser::new();
        p.set_language(&tree_sitter_bash::LANGUAGE.into()).expect("load tree-sitter-bash");
        p.set_timeout_micros(PARSE_BACKSTOP_MICROS);
        p
    });
}

/// Entry point: normalise one shell command string (the value of a `Bash` tool call, or the
/// unwrapped body of `bash -lc "…"`) into zero or more canonical actions.
/// Commands longer than this are not normalised (recorded as unknown): the hot path must stay
/// bounded, and no real hand-typed agent command is this long.
pub const MAX_COMMAND: usize = 16 * 1024;
/// Maximum nesting of `$(`, `(`, `{` and backticks before a command is treated as unknown; the
/// syntax-tree walk is recursive, so this bounds stack depth.
pub const MAX_NESTING: usize = 48;

fn too_deep(s: &str) -> bool {
    let (mut depth, mut max, mut tick) = (0usize, 0usize, false);
    for c in s.chars() {
        match c {
            '(' | '{' => depth += 1,
            ')' | '}' => depth = depth.saturating_sub(1),
            '`' => {
                tick = !tick;
                if tick {
                    depth += 1
                } else {
                    depth = depth.saturating_sub(1)
                }
            }
            _ => {}
        }
        max = max.max(depth);
        if max > MAX_NESTING {
            return true;
        }
    }
    false
}

/// `s` with every non-ASCII character replaced by as many `x` bytes as it occupies, so byte
/// offsets are unchanged. tree-sitter-bash's C scanner calls `isdigit()` on full code points
/// (scanner.c, still so in 0.25.1), which is undefined behaviour above 255 and reads out of bounds
/// in glibc's table: a `{` followed by a large code point crashed the process on Linux. Bash
/// itself treats every non-ASCII character as an ordinary word character, which `x` also is.
pub(crate) fn ascii_shadow(s: &str) -> std::borrow::Cow<'_, str> {
    if s.is_ascii() {
        return s.into();
    }
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            out.extend(std::iter::repeat_n('x', c.len_utf8()));
        }
    }
    out.into()
}

/// The parse budget: how many bytes tree-sitter's lexer may read, counting every re-read, before
/// the parse is abandoned and the command recorded as unknown. It makes the cut-off a property of
/// the command, not of the machine: the same command is cut off, or not, everywhere.
///
/// tree-sitter's error recovery is quadratic on some malformed input, and the quadratic part is
/// re-lexing: 16 KB of `)))…` makes the lexer read 128 million bytes (0.76 s in a release build;
/// the hook's budget is 25 ms), `>>>…` 56 million, `|||…` 64 million. Real commands read each byte
/// one to seven times (heredocs are the most), so every real command up to [`MAX_COMMAND`] fits in
/// this budget with room to spare, and a cut-off parse costs about 7 ms in a release build.
///
/// The one known quadratic case that is not lexing, a long run of `a|a|…|`, is bounded by
/// [`MAX_PIPES`] before parsing.
pub const PARSE_BUDGET: u64 = 64 * MAX_COMMAND as u64;

/// A wall-clock backstop, far above anything the budget allows: a cut-off parse takes about 7 ms in
/// a release build and 40–60 ms in a debug one (macOS arm64), and the worst real command well
/// under it. It is not how pathological commands are cut off, and no known input reaches it. It
/// exists so that an input shape not yet known, whose cost is neither lexing nor pipes, still
/// cannot stall a hook without limit. If it ever fires, the result for that command depends on the
/// machine, which is why it sits two orders of magnitude above the budget's worst case.
pub const PARSE_BACKSTOP_MICROS: u64 = 1_000_000;

/// Why a command has no syntax tree: the note recorded on its unknown action.
pub const NOTE_BUDGET_SPENT: &str = "not normalised: parse budget spent";
pub const NOTE_NO_TREE: &str = "tree-sitter produced no tree (backstop timeout or parse failure)";

/// Parse `src` (ASCII) within [`PARSE_BUDGET`]. `Err` carries the note: the budget was spent, or
/// the backstop fired or tree-sitter failed.
///
/// The input is fed one byte per read, so each read is one byte of lexing and the read count
/// measures the lexer's work exactly. (Chunk boundaries fall between characters because the parser
/// only sees the ASCII shadow, and all 315 normaliser fixtures parse to the same tree as a
/// whole-string parse.) When the
/// budget is spent, the read returns end of input and raises the parser's cancellation flag, which
/// tree-sitter checks every 100 parse operations: where it stops is fixed by the input too.
fn parse_within_budget(p: &mut Parser, src: &[u8]) -> Result<Tree, &'static str> {
    let cancel = AtomicUsize::new(0);
    let mut reads: u64 = 0;
    // SAFETY: `cancel` outlives every use of the flag: it is unset again below, before `cancel`
    // is dropped, and the parser reads it only inside `parse_with`.
    unsafe { p.set_cancellation_flag(Some(&cancel)) };
    let tree = p.parse_with(
        &mut |offset: usize, _| {
            reads += 1;
            if reads > PARSE_BUDGET {
                cancel.store(1, Ordering::Relaxed);
                return &[][..];
            }
            src.get(offset..offset + 1).unwrap_or(&[])
        },
        None,
    );
    unsafe { p.set_cancellation_flag(None) };
    match tree {
        Some(t) if reads <= PARSE_BUDGET => Ok(t),
        _ => {
            // A cancelled or timed-out parse would otherwise resume on the next, unrelated command.
            p.reset();
            Err(if reads > PARSE_BUDGET { NOTE_BUDGET_SPENT } else { NOTE_NO_TREE })
        }
    }
}

/// Maximum depth of the syntax-tree walk (statements within statements, including `bash -c`
/// scripts). `too_deep` bounds bracket nesting before parsing; this bounds everything else a
/// grammar can nest (`if` in `if`, `!` in `!`, …) so no input can exhaust the stack.
pub const MAX_DEPTH: usize = 96;

/// More pipe operators than this (outside quotes) and a command is treated as unknown: a long run
/// of `a|a|…|` sends tree-sitter's error recovery quadratic in a phase that is not lexing, so
/// [`PARSE_BUDGET`] does not see it (16 KB took 0.6 s in a release build, reading only 144 thousand
/// bytes). No real pipeline has this many stages.
pub const MAX_PIPES: usize = 128;

fn too_many_pipes(s: &str) -> bool {
    let (mut pipes, mut quote, mut escaped, mut prev) = (0usize, None::<char>, false, ' ');
    for c in s.chars() {
        if escaped {
            escaped = false;
        } else if c == '\\' && quote != Some('\'') {
            escaped = true;
        } else if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '\'' || c == '"' {
            quote = Some(c);
        } else if c == '|' && prev != '|' {
            pipes += 1;
            if pipes > MAX_PIPES {
                return true;
            }
        }
        prev = c;
    }
    false
}

pub fn normalize_shell(command: &str, cwd: &str, table: &CommandTable) -> Vec<Normalized> {
    normalize_at(command, cwd, table, 0, false)
}

/// [`normalize_shell`], as the runtime observes a command that ran: the same actions, and with
/// them what the command was seen to change and run beyond them, from the same parse:
///
/// - each file it writes, as a `Write` (an `Edit` when it appends), right after the action that
///   wrote it: a redirection's target (`printf … > f`, `cat > f <<'EOF'`, `cat <<'EOF' >> f`),
///   `tee`'s operands (`… | tee f`, `tee -a f <<'EOF'`), and the files an inline Python program
///   opens for writing (`python3 - <<'EOF'`, `python3 -c '…'`; [`python_writes`]);
/// - the commands written after a heredoc's start (`cat > f <<'EOF' && make gen`), which the
///   grammar nests inside the heredoc, where the plain walk never looks.
///
/// So an edit made through Bash counts as an edit of those paths (triggers, contexts, the finish
/// line), and the command that meets it in the same call is seen. The plain form stays what
/// learning, the graph and the classifiers read.
pub fn normalize_shell_observed(command: &str, cwd: &str, table: &CommandTable) -> Vec<Normalized> {
    normalize_at(command, cwd, table, 0, true)
}

fn unknown(note: &str) -> Normalized {
    Normalized {
        action: CanonicalAction::Unknown { raw: "oversized-or-deeply-nested-command".into() },
        effect: Effect::Environment,
        mutating: false,
        confidence: 0.0,
        note: note.into(),
    }
}

fn normalize_at(command: &str, cwd: &str, table: &CommandTable, depth: usize, observed: bool) -> Vec<Normalized> {
    let command = command.trim();
    if command.is_empty() {
        return vec![];
    }
    if command.len() > MAX_COMMAND || too_deep(command) || too_many_pipes(command) || depth > MAX_DEPTH {
        return vec![unknown("not normalised: exceeds size or nesting bounds")];
    }

    // The parser only ever sees ASCII: see [`ascii_shadow`]. Text is still read from `command`
    // by the same byte ranges, so non-ASCII paths and arguments come through unchanged.
    let shadow = ascii_shadow(command);
    let tree = PARSER.with(|p| parse_within_budget(&mut p.borrow_mut(), shadow.as_bytes()));
    let tree = match tree {
        Ok(t) => t,
        Err(note) => {
            return vec![Normalized {
                action: CanonicalAction::Unknown { raw: command.to_string() },
                effect: Effect::Environment,
                mutating: false,
                confidence: 0.0,
                note: note.to_string(),
            }]
        }
    };
    let root = tree.root_node();

    let ctx = Ctx { src: command, table, depth: std::cell::Cell::new(depth), observed };
    let mut cwd = cwd.to_string();
    let mut out = Vec::new();
    let mut cursor = root.walk();
    for child in root.children(&mut cursor) {
        out.extend(walk_statement(child, &mut cwd, &ctx));
    }
    out
}

struct Ctx<'a> {
    src: &'a str,
    table: &'a CommandTable,
    /// Current depth of the recursive walk (see [`MAX_DEPTH`]).
    depth: std::cell::Cell<usize>,
    /// The walk for [`normalize_shell_observed`]: the files written and the commands after a
    /// heredoc's start besides.
    observed: bool,
}

/// Holds one level of walk depth; released on drop.
struct Level<'a>(&'a std::cell::Cell<usize>);

impl Drop for Level<'_> {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}

impl Ctx<'_> {
    /// Enter one level of the walk, or `None` past [`MAX_DEPTH`].
    fn enter(&self) -> Option<Level<'_>> {
        if self.depth.get() >= MAX_DEPTH {
            return None;
        }
        self.depth.set(self.depth.get() + 1);
        Some(Level(&self.depth))
    }
}

fn text<'a>(node: Node, src: &'a str) -> &'a str {
    src.get(node.byte_range()).unwrap_or("")
}

/// Best-effort literal text for one argument/word node: strips surrounding quotes, keeps
/// expansions and concatenations as their raw source (good enough to template as a path/id
/// later; we are not a shell interpreter).
fn word_text(node: Node, src: &str) -> String {
    match node.kind() {
        "string" => {
            // string -> '"' string_content? '"' (with possible expansions as children)
            let mut s = String::new();
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if ch.kind() != "\"" {
                    s.push_str(text(ch, src));
                }
            }
            s
        }
        "raw_string" => {
            let t = text(node, src);
            t.trim_matches('\'').to_string()
        }
        _ => text(node, src).to_string(),
    }
}

/// basename("/usr/bin/foo") == "foo"
pub fn basename(p: &str) -> String {
    p.rsplit('/').next().unwrap_or(p).to_string()
}

/// Minimal path resolution — enough to group templates, not a realpath. Ported from the PoC.
pub fn resolve_path(cwd: &str, p: &str) -> String {
    if p.is_empty() {
        return cwd.to_string();
    }
    if p.starts_with('/') || p.starts_with('~') || windows_absolute(p) {
        return p.to_string();
    }
    // A Windows working directory (`C:\x`, `\\?\C:\x`): join and normalise on both separators,
    // keeping the drive, rather than as a Unix path (which would prefix `/` to `C:`).
    if windows_absolute(cwd) {
        return crate::repo::path_parts(&format!("{cwd}/{p}")).join("/");
    }
    if p == "." {
        return cwd.to_string();
    }
    let joined = format!("{}/{}", cwd, p);
    let mut stack: Vec<&str> = Vec::new();
    for part in joined.split('/') {
        if part.is_empty() || part == "." {
            continue;
        }
        if part == ".." {
            stack.pop();
        } else {
            stack.push(part);
        }
    }
    format!("/{}", stack.join("/"))
}

/// A Windows absolute path: a drive (`C:\x`, `c:/x`), a `\\?\` verbatim path, or a UNC or
/// root-relative path starting with `\\`.
fn windows_absolute(p: &str) -> bool {
    p.starts_with('\\') || matches!(p.as_bytes(), [d, b':', b'\\' | b'/', ..] if d.is_ascii_alphabetic())
}

/// Extract (argv, has_var_prefix) for a `command` node: variable_assignment children are
/// skipped (VAR=val prefix before argv[0]) but their command_substitution values, if any, are
/// walked for embedded actions by the caller.
fn command_argv(node: Node, src: &str) -> Vec<String> {
    let mut argv = Vec::new();
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        match ch.kind() {
            "command_name" => {
                if let Some(w) = ch.child(0) {
                    argv.push(word_text(w, src));
                } else {
                    argv.push(text(ch, src).to_string());
                }
            }
            "variable_assignment" => continue,
            "word" | "string" | "raw_string" | "number" | "concatenation" | "simple_expansion" | "expansion"
            | "ansi_c_string" => {
                argv.push(word_text(ch, src));
            }
            _ => {}
        }
    }
    argv
}

/// Find every `command_substitution` / `process_substitution` reachable from `node` without
/// descending into another `command` node's own nested substitutions twice, and normalise each
/// one's inner statement. Order is left-to-right source order (substitutions run before the
/// command that uses their output, which is close enough for our purposes).
fn extract_substitutions(node: Node, cwd: &str, ctx: &Ctx) -> Vec<Normalized> {
    let mut out = Vec::new();
    fn walk(node: Node, cwd: &str, ctx: &Ctx, out: &mut Vec<Normalized>) {
        let Some(_level) = ctx.enter() else { return };
        let mut c = node.walk();
        for ch in node.children(&mut c) {
            if ch.kind() == "command_substitution" || ch.kind() == "process_substitution" {
                // child is the wrapped statement directly (command/list/pipeline/…)
                let mut cc = ch.walk();
                for inner in ch.children(&mut cc) {
                    match inner.kind() {
                        "(" | ")" | "$(" | "<(" | ">(" => continue,
                        _ => {
                            let mut scratch_cwd = cwd.to_string();
                            out.extend(walk_statement(inner, &mut scratch_cwd, ctx));
                        }
                    }
                }
            } else {
                walk(ch, cwd, ctx, out);
            }
        }
    }
    walk(node, cwd, ctx, &mut out);
    out
}

/// `sed -n '10,40p' f` / `head -n 40 f` / `tail -n +10 f` → a line range.
fn read_range(cmd: &str, argv: &[String]) -> Option<(i64, i64)> {
    match cmd {
        "sed" => {
            let joined = argv.join(" ");
            let re = regex::Regex::new(r"(\d+),(\d+)p").unwrap();
            let c = re.captures(&joined)?;
            Some((c[1].parse().ok()?, c[2].parse().ok()?))
        }
        "head" => {
            let i = argv.iter().position(|a| a == "-n")?;
            let v: i64 = argv.get(i + 1)?.trim_start_matches('+').parse().ok()?;
            Some((1, v))
        }
        "tail" => {
            let i = argv.iter().position(|a| a == "-n")?;
            let v = argv.get(i + 1)?;
            if let Some(rest) = v.strip_prefix('+') {
                Some((rest.parse().ok()?, i64::MAX))
            } else {
                None
            }
        }
        _ => None,
    }
}

/// Non-flag operands, respecting the command's own value-taking-flag grammar (data-driven via
/// `CommandTable`) — the reason a shared flag table is wrong: `-n` is boolean for `rg` but takes
/// a value for `head`.
fn operands(cmd: &str, argv: &[String], table: &CommandTable) -> Vec<String> {
    let value_flags: Vec<&str> =
        table.get(cmd).map(|s| s.value_flags.iter().map(|s| s.as_str()).collect()).unwrap_or_default();
    let mut out = Vec::new();
    let mut i = 1;
    while i < argv.len() {
        let t = &argv[i];
        if t.starts_with('-') {
            if value_flags.contains(&t.as_str()) {
                i += 1;
            }
        } else {
            out.push(t.clone());
        }
        i += 1;
    }
    out
}

/// Find a `-c` / `-lc` / `-xc` style flag (any run of lowercase letters ending in `c`) among
/// `args` and return the script text that follows it.
fn extract_dash_c(args: &[String]) -> Option<String> {
    let re = regex::Regex::new(r"^-[a-z]*c$").unwrap();
    let i = args.iter().position(|a| re.is_match(a))?;
    args.get(i + 1).cloned()
}

fn has_flag(argv: &[String], flag: &str) -> bool {
    argv.iter().any(|a| a == flag)
}

/// Effect classification for one `exec`-shaped command, table-driven per Design Spec §8.6:
/// "Classification is data, per command template… Unknown commands default to Environment."
/// Whether `argv` runs an interpreter on the program it reads from stdin: an interpreter named
/// with no script operand (only options, or `-`).
fn stdin_program(argv: &[String]) -> bool {
    let Some(head) = argv.first().map(|a| a.rsplit('/').next().unwrap_or(a)) else { return false };
    SCRIPT_INTERPRETERS.contains(&head)
        && argv[1..].iter().all(|a| a == "-" || (a.starts_with('-') && !matches!(a.as_str(), "-c" | "-e" | "-m")))
}

fn classify_effect(cmd: &str, args: &[String], table: &CommandTable) -> (Effect, f64, String) {
    // `python3 scripts/wi-lookup W-1` — classify the script, not the interpreter.
    const INTERPRETERS: [&str; 9] = ["python", "python3", "node", "bash", "sh", "zsh", "ruby", "perl", "deno"];
    if INTERPRETERS.contains(&cmd) {
        if let Some(script) = args.first().filter(|a| !a.starts_with('-') && a.contains('/')) {
            let base = basename(script);
            if table.get(&base).is_none() {
                return classify_effect(&base, &args[1..], table);
            }
        }
    }
    let cmd = &basename(cmd);
    let Some(spec) = table.get(cmd) else {
        // Unlisted commands default to environment (Design §8.6). One narrow exception: a
        // project script whose name leads with a read verb (`wi-lookup`, `show-config`,
        // `list_services`) is classified observe by name, at reduced confidence. The learner still
        // needs session evidence before such a step can be directed.
        const READ_VERBS: [&str; 13] = [
            "lookup", "show", "get", "list", "view", "status", "info", "describe", "search", "find", "query", "print",
            "fetch",
        ];
        let lower = cmd.to_ascii_lowercase();
        let parts: Vec<&str> = lower.split(['-', '_', '.']).collect();
        let read_by_name = parts.iter().take(2).any(|p| READ_VERBS.contains(p));
        if read_by_name {
            return (Effect::Observe, 0.6, format!("'{cmd}' classified observe by name; unverified"));
        }
        return (Effect::Environment, 0.5, format!("'{cmd}' has no flag-grammar entry — defaulted to environment"));
    };

    // `pnpm --filter pkg test`, `npm --prefix web test`: skip the flags before the subcommand.
    let mut args = args;
    if !spec.leading_value_flags.is_empty() {
        let mut i = 0;
        while i < args.len() && args[i].starts_with('-') && args[i] != "--" {
            i += if spec.leading_value_flags.iter().any(|f| *f == args[i]) { 2 } else { 1 };
        }
        args = &args[i.min(args.len())..];
    }

    // `uv run pytest`, `uvx ruff check`, `pnpm exec vitest`: the program a wrapper runs.
    if spec.wraps {
        if let Some(r) = wrapped_effect(cmd, spec, args, table) {
            return r;
        }
    }
    if let Some(sub) = args.first().and_then(|s| spec.subcommands.get(s)).filter(|s| s.wraps) {
        if let Some(r) = wrapped_effect(cmd, sub, &args[1..], table) {
            return r;
        }
    }
    // `pnpm check`, `yarn vitest`: a package manager's first word runs a script or a local binary.
    if spec.run_shorthand {
        if let Some(word) = args.first().filter(|w| !w.starts_with('-') && !spec.subcommands.contains_key(*w)) {
            let base = basename(word);
            if base != *cmd && table.get(&base).is_some() {
                let (effect, _, _) = classify_effect(&base, &args[1..], table);
                return (effect, 0.8, format!("'{cmd} {base}' runs {base}"));
            }
            if let Some(run) = spec.subcommands.get("run") {
                let mut effect = run.effect;
                for (re, eff) in &run.pattern_effects {
                    if re.is_match(word) {
                        effect = *eff;
                    }
                }
                return (effect, 0.8, format!("'{cmd} {word}' classified as '{cmd} run {word}'"));
            }
        }
    }

    let mut effect = spec.effect;
    let mut confidence = 0.85;
    let mut note = String::new();

    let sub = args.first().cloned().unwrap_or_default();
    if let Some(subspec) = spec.subcommands.get(&sub) {
        effect = subspec.effect;
        confidence = 0.95;
        if let Some(next) = args.get(1) {
            for (re, eff) in &subspec.pattern_effects {
                if re.is_match(next) {
                    effect = *eff;
                }
            }
        }
        if subspec.check_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Verify;
        }
        if subspec.mutating_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Author;
        }
        if subspec.regenerate_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Regenerate;
        }
        if subspec.publish_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Publish;
        }
    } else {
        if !spec.subcommands.is_empty() {
            note = format!("'{cmd} {sub}' subcommand not in table — used {cmd}'s default effect");
        } else {
            confidence = 0.95;
        }
        for (re, eff) in &spec.pattern_effects {
            if args.iter().any(|a| re.is_match(a)) || (!sub.is_empty() && re.is_match(&sub)) {
                effect = *eff;
                confidence = 0.9;
            }
        }
        if spec.check_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Verify;
            confidence = 0.9;
        }
        if spec.mutating_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Author;
            confidence = 0.9;
        }
        if spec.regenerate_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Regenerate;
            confidence = 0.9;
        }
        if spec.publish_flags.iter().any(|f| args.iter().any(|a| a == f)) {
            effect = Effect::Publish;
            confidence = 0.9;
        }
    }
    (effect, confidence, note)
}

/// The effect of the program a wrapper runs (`uv run pytest -x` → pytest's), if the table knows
/// that program. `args` follow the wrapper (and its subcommand); `spec.value_flags` are the
/// wrapper's own flags that take a value. `None` leaves the wrapper to its own classification.
fn wrapped_effect(
    wrapper: &str,
    spec: &crate::table::CommandSpec,
    args: &[String],
    table: &CommandTable,
) -> Option<(Effect, f64, String)> {
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--" {
            i += 1;
            break;
        }
        if !a.starts_with('-') {
            break;
        }
        i += if !a.contains('=') && spec.value_flags.iter().any(|f| f == a) { 2 } else { 1 };
    }
    let program = args.get(i)?;
    let base = basename(program);
    if base == wrapper || table.get(&base).is_none() {
        return None;
    }
    let (effect, _, _) = classify_effect(program, &args[i + 1..], table);
    Some((effect, 0.8, format!("'{wrapper}' runs '{base}'")))
}

/// The head command name of a pipeline stage, without a full walk — used only to decide whether
/// a trailing stage is a pure filter.
fn stage_head_name(node: Node, src: &str, depth: usize) -> Option<String> {
    if depth > MAX_DEPTH {
        return None;
    }
    match node.kind() {
        "command" => {
            let argv = command_argv(node, src);
            argv.first().map(|a| basename(a))
        }
        "redirected_statement" => node.child_by_field_name("body").and_then(|b| stage_head_name(b, src, depth + 1)),
        "negated_command" => node.child(1).and_then(|b| stage_head_name(b, src, depth + 1)),
        _ => None,
    }
}

/// Walk one statement-shaped node, threading `cwd` through what follows in the same sequence.
fn walk_statement(node: Node, cwd: &mut String, ctx: &Ctx) -> Vec<Normalized> {
    let src = ctx.src;
    let Some(_level) = ctx.enter() else {
        return vec![unknown("not normalised: statements nested too deeply")];
    };
    match node.kind() {
        "ERROR" => vec![Normalized {
            action: CanonicalAction::Unknown { raw: text(node, src).to_string() },
            effect: Effect::Environment,
            mutating: false,
            confidence: 0.0,
            note: "shell grammar could not parse this fragment".to_string(),
        }],

        "list" => {
            // binary &&/|| node: children are [left, operator, right], left may itself be a
            // nested `list` for chains of 3+. Flatten the chain iteratively: a long `a && b && …`
            // is as deep as it is long, and recursing on it exhausted the stack.
            let mut operands: Vec<Node> = Vec::new();
            let mut cur = node;
            loop {
                let mut c = cur.walk();
                let kids: Vec<Node> = cur.children(&mut c).filter(|ch| !matches!(ch.kind(), "&&" | "||")).collect();
                let Some((first, rest)) = kids.split_first() else { break };
                operands.extend(rest.iter().rev().copied());
                if first.kind() == "list" {
                    cur = *first;
                } else {
                    operands.push(*first);
                    break;
                }
            }
            let mut out = Vec::new();
            for ch in operands.into_iter().rev() {
                out.extend(walk_statement(ch, cwd, ctx));
            }
            out
        }

        "pipeline" => {
            let stages: Vec<Node> = {
                let mut c = node.walk();
                node.children(&mut c).filter(|ch| ch.kind() != "|" && ch.kind() != "|&").collect()
            };
            if stages.is_empty() {
                return vec![];
            }
            let tail_pure = stages[1..]
                .iter()
                .all(|s| stage_head_name(*s, src, 0).map(|n| PURE_FILTERS.contains(&n.as_str())).unwrap_or(false));
            let mut head_actions = walk_statement(stages[0], cwd, ctx);
            if !tail_pure {
                for a in head_actions.iter_mut() {
                    a.mutating = true;
                }
            }
            // A later stage is not an action of its own, but what it writes is a write
            // (`… | tee schema/x.sql`, `… | sort > out.txt`).
            if ctx.observed {
                for stage in &stages[1..] {
                    head_actions.extend(stage_writes(*stage, cwd, ctx));
                }
            }
            head_actions
        }

        "redirected_statement" => {
            let Some(body) = node.child_by_field_name("body") else { return vec![] };
            let mut actions = walk_statement(body, cwd, ctx);

            let mut c = node.walk();
            let mut output_target: Option<String> = None;
            // An interpreter fed its program by a heredoc (`python3 - <<'EOF'`) runs inline code,
            // as `python3 -c '…'` does: it gets the same argv shape, so one-off scripts do not
            // all share one bare `python3` identity.
            let inline = node
                .children(&mut node.walk())
                .any(|ch| matches!(ch.kind(), "heredoc_redirect" | "herestring_redirect"));
            // Python reading its program from this heredoc: the files the program writes.
            let mut python_fed = false;
            if inline {
                for a in actions.iter_mut() {
                    if let CanonicalAction::Exec { argv, .. } = &mut a.action {
                        if stdin_program(argv) {
                            python_fed |= is_python(&argv[0]);
                            argv.retain(|x| x != "-");
                            argv.push("-c".into());
                            argv.push("<stdin>".into());
                        }
                    }
                }
            }
            for ch in node.children(&mut c) {
                if ch.kind() == "file_redirect" {
                    let op = ch.child(0).map(|o| text(o, src)).unwrap_or("");
                    let is_output = op.starts_with('>') || op == ">&" || op.contains(">&");
                    if is_output {
                        if let Some(dest) = ch.child_by_field_name("destination") {
                            let d = word_text(dest, src);
                            // fd duplication like `2>&1` — not a real file
                            if !d.chars().all(|c| c.is_ascii_digit()) && d != "/dev/null" {
                                output_target = Some(d);
                            }
                        }
                    }
                }
            }
            if let Some(target) = output_target {
                let path = resolve_path(cwd, &target);
                for a in actions.iter_mut() {
                    a.mutating = true;
                    if !matches!(a.effect, Effect::Publish) {
                        a.effect = Effect::Author;
                    }
                    a.note = format!("redirected to {path}");
                }
                if actions.is_empty() {
                    // pure `> file` truncation with no command (rare) — no body actions to
                    // annotate, so synthesize a write action.
                    actions.push(Normalized {
                        action: CanonicalAction::Write { path },
                        effect: Effect::Author,
                        mutating: true,
                        confidence: 0.9,
                        note: String::new(),
                    });
                }
            }
            if ctx.observed {
                observe_redirected(node, python_fed, &mut actions, cwd, ctx);
            }
            actions
        }

        "subshell" => {
            // `( … )` — a new cwd scope that does not leak `cd` to the parent.
            let mut inner_cwd = cwd.clone();
            let mut out = Vec::new();
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                match ch.kind() {
                    "(" | ")" => continue,
                    _ => out.extend(walk_statement(ch, &mut inner_cwd, ctx)),
                }
            }
            out
        }

        "compound_statement" => {
            // `{ … }` grouping — shares the caller's cwd (unlike a subshell).
            let mut out = Vec::new();
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                match ch.kind() {
                    "{" | "}" => continue,
                    _ => out.extend(walk_statement(ch, cwd, ctx)),
                }
            }
            out
        }

        "negated_command" => {
            let mut c = node.walk();
            let inner = node.children(&mut c).find(|ch| ch.kind() != "!");
            inner.map(|i| walk_statement(i, cwd, ctx)).unwrap_or_default()
        }

        "for_statement" | "c_style_for_statement" => {
            let body = node.child_by_field_name("body");
            let Some(body) = body else { return vec![] };
            let mut loop_cwd = cwd.clone();
            let inner = normalize_group_body(body, &mut loop_cwd, ctx);
            wrap_loop("for", inner)
        }

        "while_statement" => {
            let body = node.child_by_field_name("body");
            let Some(body) = body else { return vec![] };
            let mut loop_cwd = cwd.clone();
            let inner = normalize_group_body(body, &mut loop_cwd, ctx);
            wrap_loop("while", inner)
        }

        "if_statement" => {
            let mut branch_actions = Vec::new();
            for ch in statements_after_keyword(node, "then") {
                branch_actions.extend(walk_statement(ch, cwd, ctx));
            }
            wrap_branch("if", branch_actions)
        }

        "elif_clause" => {
            let mut out = Vec::new();
            for ch in statements_after_keyword(node, "then") {
                out.extend(walk_statement(ch, cwd, ctx));
            }
            out
        }

        "else_clause" => {
            let mut out = Vec::new();
            for ch in statements_after_keyword(node, "else") {
                out.extend(walk_statement(ch, cwd, ctx));
            }
            out
        }

        "case_statement" => {
            let mut branch_actions = Vec::new();
            let mut c = node.walk();
            for ch in node.children(&mut c) {
                if ch.kind() == "case_item" {
                    for gch in statements_after_keyword(ch, ")") {
                        branch_actions.extend(walk_statement(gch, cwd, ctx));
                    }
                }
            }
            wrap_branch("case", branch_actions)
        }

        "function_definition" => {
            // Defining a function executes nothing; a later call to it (if any) is a separate
            // `command` node we will see and classify on its own (usually as an unrecognised
            // exec, since we do not resolve function bodies at the call site).
            vec![]
        }

        "declaration_command" => {
            // `local x=1` / `export FOO=bar` / `declare -a arr` — variable-scoped, no filesystem
            // or process action of interest. Still walk for embedded command substitutions.
            extract_substitutions(node, cwd, ctx)
        }

        "variable_assignment" => extract_substitutions(node, cwd, ctx),

        "test_command" | "unary_expression" | "binary_expression" => vec![],

        "command" => {
            let mut subs = extract_substitutions(node, cwd, ctx);
            let argv = command_argv(node, src);
            if argv.is_empty() {
                return subs;
            }
            let cmd = basename(&argv[0]);

            if NOOP_BUILTINS.contains(&cmd.as_str()) {
                if cmd == "cd" {
                    if let Some(target) = argv.get(1) {
                        *cwd = resolve_path(cwd, target);
                    }
                }
                return subs;
            }

            // unwrap `bash -lc "…"` / `sh -c "…"` / `zsh -c "…"` — a new shell instance, so `cd`
            // inside it does not leak to the caller (subshell-like scoping).
            if matches!(cmd.as_str(), "bash" | "sh" | "zsh") {
                if let Some(script) = extract_dash_c(&argv[1..]) {
                    subs.extend(normalize_at(&script, cwd, ctx.table, ctx.depth.get(), ctx.observed));
                    return subs;
                }
            }

            let ops = operands(&cmd, &argv, ctx.table);
            let action_result = shape_command(&cmd, &argv, &ops, cwd, ctx.table);
            subs.push(action_result);
            if ctx.observed {
                subs.extend(command_writes(&cmd, &argv, cwd));
            }
            subs
        }

        // Comments, heredoc bodies encountered directly, punctuation, etc.
        _ => vec![],
    }
}

/// Children of `node` that come after the first child of kind `keyword` (e.g. every child
/// after the `then` token in an `if_statement`, or after `)` in a `case_item`). Used to drop a
/// clause's condition/pattern, which are not statements we want to normalise.
fn statements_after_keyword<'a>(node: Node<'a>, keyword: &str) -> Vec<Node<'a>> {
    let mut out = Vec::new();
    let mut seen = false;
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        if seen {
            out.push(ch);
        } else if ch.kind() == keyword {
            seen = true;
        }
    }
    out
}

/// What a redirected statement writes and runs besides its body, for [`normalize_shell_observed`]:
/// each file a redirection writes (one the grammar nests in a heredoc too: `cat <<'EOF' > f`), the
/// files the Python program a heredoc feeds writes, and the commands after the heredoc's start
/// (`cat > f <<'EOF' && make gen`), in that order, after the body's actions.
fn observe_redirected(node: Node, python_fed: bool, actions: &mut Vec<Normalized>, cwd: &mut String, ctx: &Ctx) {
    let at = cwd.clone();
    let mut c = node.walk();
    let heredocs: Vec<Node> = node.children(&mut c).filter(|ch| ch.kind() == "heredoc_redirect").collect();
    for (path, append) in redirect_writes(node, ctx.src) {
        let known = actions.iter().any(|a| {
            matches!(&a.action, CanonicalAction::Write { path: p } | CanonicalAction::Edit { path: p } if *p == resolve_path(&at, &path))
        });
        if !known {
            actions.push(written(&at, &path, append, "a redirection"));
        }
    }
    if python_fed {
        for h in &heredocs {
            let mut hc = h.walk();
            let body = h.children(&mut hc).find(|ch| ch.kind() == "heredoc_body");
            for (path, append) in body.map(|b| python_writes(text(b, ctx.src))).unwrap_or_default() {
                actions.push(written(&at, &path, append, "its Python program"));
            }
        }
    }
    for h in &heredocs {
        let mut hc = h.walk();
        let after: Vec<Node> = h
            .named_children(&mut hc)
            .filter(|ch| !matches!(ch.kind(), "heredoc_start" | "heredoc_body" | "heredoc_end" | "file_redirect"))
            .collect();
        for ch in after {
            actions.extend(walk_statement(ch, cwd, ctx));
        }
    }
}

/// The files a redirected statement's redirections write (its own, and those nested in a
/// heredoc), each with whether it appends.
fn redirect_writes(node: Node, src: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut c = node.walk();
    for ch in node.children(&mut c) {
        match ch.kind() {
            "file_redirect" => out.extend(redirect_target(ch, src)),
            "heredoc_redirect" => {
                let mut hc = ch.walk();
                out.extend(
                    ch.children(&mut hc)
                        .filter(|r| r.kind() == "file_redirect")
                        .filter_map(|r| redirect_target(r, src)),
                );
            }
            _ => {}
        }
    }
    out
}

/// The file one redirection writes, and whether it appends: `>`, `>|`, `&>` and `>&` to a name
/// write it, `>>` and `&>>` append; an input, a descriptor (`2>&1`) or a device writes none.
fn redirect_target(r: Node, src: &str) -> Option<(String, bool)> {
    let mut c = r.walk();
    let op = r.children(&mut c).find(|ch| !ch.is_named())?;
    let append = match op.kind() {
        ">" | ">|" | "&>" | ">&" => false,
        ">>" | "&>>" => true,
        _ => return None,
    };
    let dest = r.child_by_field_name("destination").filter(|d| d.kind() != "number")?;
    let path = word_text(dest, src);
    literal_file(&path).then_some((path, append))
}

/// What a later stage of a pipeline writes ([`normalize_shell_observed`]): `tee`'s operands, an
/// inline Python program's files, and its own redirections.
fn stage_writes(stage: Node, cwd: &str, ctx: &Ctx) -> Vec<Normalized> {
    let (command, redirects) = match stage.kind() {
        "command" => (Some(stage), Vec::new()),
        "redirected_statement" => {
            (stage.child_by_field_name("body").filter(|b| b.kind() == "command"), redirect_writes(stage, ctx.src))
        }
        _ => (None, Vec::new()),
    };
    let mut out = Vec::new();
    if let Some(node) = command {
        let argv = command_argv(node, ctx.src);
        if let Some(head) = argv.first() {
            out.extend(command_writes(&basename(head), &argv, cwd));
        }
    }
    out.extend(redirects.into_iter().map(|(path, append)| written(cwd, &path, append, "a redirection")));
    out
}

/// What one command writes by itself ([`normalize_shell_observed`]): `tee`'s operands, and the
/// files a Python program given with `-c` writes.
fn command_writes(cmd: &str, argv: &[String], cwd: &str) -> Vec<Normalized> {
    if cmd == "tee" {
        let append = argv[1..]
            .iter()
            .take_while(|a| *a != "--")
            .any(|a| a == "--append" || (a.starts_with('-') && !a.starts_with("--") && a.contains('a')));
        let mut operands = false;
        let mut out = Vec::new();
        for a in &argv[1..] {
            if !operands && a == "--" {
                operands = true;
            } else if (operands || !a.starts_with('-')) && literal_file(a) {
                out.push(written(cwd, a, append, "tee"));
            }
        }
        return out;
    }
    if is_python(cmd) {
        if let Some(code) = argv.iter().position(|a| a == "-c").and_then(|i| argv.get(i + 1)) {
            return python_writes(code)
                .into_iter()
                .map(|(p, append)| written(cwd, &p, append, "its Python program"))
                .collect();
        }
    }
    Vec::new()
}

fn is_python(head: &str) -> bool {
    matches!(basename(head).as_str(), "python" | "python3")
}

/// A target the command names literally: not empty, not `-`, a descriptor or a device, nothing
/// for the shell to expand (a variable, a command substitution, a glob).
fn literal_file(p: &str) -> bool {
    !p.is_empty()
        && p != "-"
        && !p.bytes().all(|b| b.is_ascii_digit())
        && !p.starts_with("/dev/")
        && !p.contains(['$', '`', '*', '?', '[', '\n'])
}

/// A file written, as a canonical action: `Write`, or `Edit` when it appends to the file.
fn written(cwd: &str, path: &str, append: bool, by: &str) -> Normalized {
    let path = resolve_path(cwd, path);
    Normalized {
        action: if append { CanonicalAction::Edit { path } } else { CanonicalAction::Write { path } },
        effect: Effect::Author,
        mutating: true,
        confidence: 0.9,
        note: format!("written by {by}"),
    }
}

/// The most files one inline program is read as writing.
const MAX_PROGRAM_WRITES: usize = 32;

/// The files an inline Python program writes, in program order, each with whether it changes the
/// file in place (opened to append, or `r+`): `open(<file>, <mode>)` with a writing mode (`mode=`
/// too), and `Path(<file>).write_text(…)` or `.write_bytes(…)`, where `<file>` is a string literal
/// or a name an earlier statement bound to one (`p = 'schema/invoice.json'`, `p = Path('…')`; any
/// other assignment unbinds it). The program is read, never run: a path it computes is not
/// followed.
fn python_writes(code: &str) -> Vec<(String, bool)> {
    use regex::Regex;
    use std::sync::OnceLock;
    const LIT: &str = r#"(?:'([^'\\\n]*)'|"([^"\\\n]*)")"#;
    static ASSIGN: OnceLock<Regex> = OnceLock::new();
    static RHS: OnceLock<Regex> = OnceLock::new();
    static OPEN: OnceLock<Regex> = OnceLock::new();
    static PATH_WRITE: OnceLock<Regex> = OnceLock::new();
    let assign = ASSIGN.get_or_init(|| Regex::new(r"(?m)(?:^|;)[ \t]*([A-Za-z_]\w*)[ \t]*=([^=\n;][^\n;]*)").unwrap());
    let rhs = RHS
        .get_or_init(|| Regex::new(&format!(r"^\s*(?:(?:pathlib\.)?Path\(\s*{LIT}\s*\)|{LIT})\s*(?:#.*)?$")).unwrap());
    let open = OPEN.get_or_init(|| {
        Regex::new(&format!(r"\bopen\(\s*(?:{LIT}|([A-Za-z_]\w*))\s*,\s*(?:mode\s*=\s*)?{LIT}")).unwrap()
    });
    let path_write = PATH_WRITE.get_or_init(|| {
        Regex::new(&format!(
            r"(?:\b(?:pathlib\.)?Path\(\s*(?:{LIT}|([A-Za-z_]\w*))\s*\)|\b([A-Za-z_]\w*))\.write_(?:text|bytes)\("
        ))
        .unwrap()
    });
    let text = |c: &regex::Captures, i: usize| c.get(i).map(|m| m.as_str().to_string());
    // Statements in program order: (offset, a binding or an unbinding, or a write).
    enum Seen {
        Bind(String, Option<String>),
        Write(Option<String>, Option<String>, bool),
    }
    let mut seen: Vec<(usize, Seen)> = Vec::new();
    for c in assign.captures_iter(code) {
        let value = rhs.captures(&c[2]).and_then(|v| (1..=4).find_map(|i| text(&v, i)));
        seen.push((c.get(1).map_or(0, |m| m.start()), Seen::Bind(c[1].to_string(), value)));
    }
    for c in open.captures_iter(code) {
        let mode = text(&c, 4).or_else(|| text(&c, 5)).unwrap_or_default();
        let append = mode.contains('a') || mode.contains('+');
        if mode.contains(['w', 'x', 'a', '+']) {
            let file = text(&c, 1).or_else(|| text(&c, 2));
            seen.push((c.get(0).map_or(0, |m| m.start()), Seen::Write(file, text(&c, 3), append)));
        }
    }
    for c in path_write.captures_iter(code) {
        let file = text(&c, 1).or_else(|| text(&c, 2));
        let name = text(&c, 3).or_else(|| text(&c, 4));
        seen.push((c.get(0).map_or(0, |m| m.start()), Seen::Write(file, name, false)));
    }
    seen.sort_by_key(|(at, _)| *at);
    let mut bound: std::collections::BTreeMap<String, String> = Default::default();
    let mut out: Vec<(String, bool)> = Vec::new();
    for (_, s) in seen {
        match s {
            Seen::Bind(name, Some(value)) => {
                bound.insert(name, value);
            }
            Seen::Bind(name, None) => {
                bound.remove(&name);
            }
            Seen::Write(file, name, append) => {
                let Some(file) = file.or_else(|| name.and_then(|n| bound.get(&n).cloned())) else { continue };
                if literal_file(&file) && !out.iter().any(|(f, _)| *f == file) && out.len() < MAX_PROGRAM_WRITES {
                    out.push((file, append));
                }
            }
        }
    }
    out
}

/// `do_group` / `compound_statement` body of a loop.
fn normalize_group_body(body: Node, cwd: &mut String, ctx: &Ctx) -> Vec<Normalized> {
    let mut out = Vec::new();
    let mut c = body.walk();
    for ch in body.children(&mut c) {
        match ch.kind() {
            "do" | "done" | "{" | "}" => continue,
            _ => out.extend(walk_statement(ch, cwd, ctx)),
        }
    }
    out
}

/// Wrap a loop body's actions. Per SPIKE.md: "decide what a loop over reads should become"
/// rather than emitting one opaque low-confidence blob. A loop whose body is entirely
/// observe/verify is still low-risk even though it repeats; a loop with any mutation inherits
/// that mutation, since some iteration count triggered it.
fn wrap_loop(kind: &str, actions: Vec<Normalized>) -> Vec<Normalized> {
    if actions.is_empty() {
        return vec![];
    }
    let mutating = actions.iter().any(|a| a.mutating);
    let base_conf = actions.iter().map(|a| a.confidence).sum::<f64>() / actions.len() as f64;
    let effect = actions.iter().map(|a| a.effect).max_by_key(effect_rank).unwrap_or(Effect::Environment);
    vec![Normalized {
        action: CanonicalAction::Compound { kind: kind.to_string(), actions },
        effect,
        mutating,
        confidence: (base_conf * 0.85).max(0.3),
        note: format!("{kind} loop body parsed; runs an unknown number of times"),
    }]
}

/// Wrap an if/case's branch actions: only one branch runs, but we can't know which, so we
/// flatten every branch's actions into the compound and discount confidence accordingly —
/// this is still far more informative than PoC's single opaque token per compound statement.
fn wrap_branch(kind: &str, actions: Vec<Normalized>) -> Vec<Normalized> {
    if actions.is_empty() {
        return vec![];
    }
    let mutating = actions.iter().any(|a| a.mutating);
    let base_conf = actions.iter().map(|a| a.confidence).sum::<f64>() / actions.len() as f64;
    let effect = actions.iter().map(|a| a.effect).max_by_key(effect_rank).unwrap_or(Effect::Environment);
    vec![Normalized {
        action: CanonicalAction::Compound { kind: kind.to_string(), actions },
        effect,
        mutating,
        confidence: (base_conf * 0.8).max(0.3),
        note: format!("{kind}: only one branch actually runs"),
    }]
}

fn effect_rank(e: &Effect) -> u8 {
    match e {
        Effect::Observe => 0,
        Effect::Verify => 1,
        Effect::Regenerate => 2,
        Effect::Environment => 3,
        Effect::Author => 4,
        Effect::Publish => 5,
    }
}

/// The op-shape decision for one fully-resolved `command` node: read/search/list/fetch/edit vs
/// generic exec, mirroring the PoC's shape rules but table-driven for flags and effects.
fn shape_command(cmd: &str, argv: &[String], ops: &[String], cwd: &str, table: &CommandTable) -> Normalized {
    let has_redirect = false; // redirects are handled by the enclosing `redirected_statement`

    if cmd == "sed" && argv.iter().any(|a| a == "-i" || (a.starts_with("-i") && a.len() > 2)) {
        let path = resolve_path(cwd, ops.last().map(|s| s.as_str()).unwrap_or(""));
        return Normalized {
            action: CanonicalAction::Edit { path },
            effect: Effect::Author,
            mutating: true,
            confidence: 0.95,
            note: String::new(),
        };
    }

    if (cmd == "cat" || cmd == "nl") && !ops.is_empty() && !has_redirect {
        return Normalized {
            action: CanonicalAction::Read { path: resolve_path(cwd, &ops[0]), range: None },
            effect: Effect::Observe,
            mutating: false,
            confidence: 0.95,
            note: String::new(),
        };
    }

    if matches!(cmd, "sed" | "head" | "tail") && !ops.is_empty() {
        let range = read_range(cmd, argv);
        return Normalized {
            action: CanonicalAction::Read { path: resolve_path(cwd, ops.last().unwrap()), range },
            effect: Effect::Observe,
            mutating: false,
            confidence: 0.95,
            note: String::new(),
        };
    }

    if matches!(cmd, "grep" | "rg" | "ag" | "ack") {
        if has_flag(argv, "--files") {
            return Normalized {
                action: CanonicalAction::Search {
                    kind: "filename".into(),
                    pattern: "*".into(),
                    paths: ops.iter().map(|p| resolve_path(cwd, p)).collect(),
                },
                effect: Effect::Observe,
                mutating: false,
                confidence: 0.9,
                note: String::new(),
            };
        }
        let pattern = ops.first().cloned().unwrap_or_default();
        let paths =
            if ops.len() > 1 { ops[1..].iter().map(|p| resolve_path(cwd, p)).collect() } else { vec![cwd.to_string()] };
        return Normalized {
            action: CanonicalAction::Search { kind: "content".into(), pattern, paths },
            effect: Effect::Observe,
            mutating: false,
            confidence: 0.95,
            note: String::new(),
        };
    }

    if matches!(cmd, "find" | "fd") {
        let ni = argv.iter().position(|a| a == "-name" || a == "-iname");
        let pattern = ni
            .and_then(|i| argv.get(i + 1))
            .cloned()
            .unwrap_or_else(|| ops.first().cloned().unwrap_or_else(|| "*".to_string()));
        return Normalized {
            action: CanonicalAction::Search {
                kind: "filename".into(),
                pattern,
                paths: vec![resolve_path(cwd, ops.first().map(|s| s.as_str()).unwrap_or("."))],
            },
            effect: Effect::Observe,
            mutating: false,
            confidence: 0.9,
            note: String::new(),
        };
    }

    if matches!(cmd, "ls" | "tree" | "eza" | "exa") {
        return Normalized {
            action: CanonicalAction::List { path: resolve_path(cwd, ops.first().map(|s| s.as_str()).unwrap_or(".")) },
            effect: Effect::Observe,
            mutating: false,
            confidence: 0.95,
            note: String::new(),
        };
    }

    if cmd == "curl" || cmd == "wget" {
        if let Some(url) = ops.iter().find(|o| o.starts_with("http://") || o.starts_with("https://")) {
            let (effect, confidence, note) = classify_effect(cmd, &argv[1..], table);
            return Normalized {
                action: CanonicalAction::Fetch { url: url.clone() },
                effect,
                mutating: matches!(effect, Effect::Publish),
                confidence,
                note,
            };
        }
    }

    // `npx <tool> …` / `yarn dlx <tool> …` / `pnpm dlx <tool> …` delegate to an arbitrary
    // installed binary; classify by the *wrapped* command when we recognise it, since "npx" or
    // "dlx" alone tells us nothing (unlike the PoC, which had no such unwrap at all).
    if matches!(cmd, "npx") || (matches!(cmd, "yarn" | "pnpm") && argv.get(1).map(|a| a == "dlx").unwrap_or(false)) {
        let rest_start = if cmd == "npx" { 1 } else { 2 };
        if let Some(rel_pos) = argv[rest_start..].iter().position(|a| !a.starts_with('-')) {
            let pos = rest_start + rel_pos;
            let real_base = basename(&argv[pos]);
            if table.get(&real_base).is_some() {
                let (effect, _, _) = classify_effect(&real_base, &argv[pos + 1..], table);
                return Normalized {
                    action: CanonicalAction::Exec { argv: argv.to_vec(), cwd: cwd.to_string() },
                    effect,
                    mutating: matches!(effect, Effect::Publish | Effect::Author | Effect::Regenerate),
                    confidence: 0.75,
                    note: format!("npx/dlx-wrapped '{real_base}'"),
                };
            }
        }
    }

    let (effect, confidence, note) = classify_effect(cmd, &argv[1..], table);
    // "mutating" tracks whether this action closes a discovery window (PoC semantics): source or
    // shared-state changes do, machine-environment setup does not by itself.
    let mutating = matches!(effect, Effect::Publish | Effect::Author | Effect::Regenerate);
    Normalized {
        action: CanonicalAction::Exec { argv: argv.to_vec(), cwd: cwd.to_string() },
        effect,
        mutating,
        confidence,
        note,
    }
}

#[cfg(test)]
mod resolve_tests {
    use super::resolve_path;

    #[test]
    fn windows_paths_resolve_on_any_platform() {
        let cwd = r"\\?\C:\Users\a\acme-billing";
        // Absolute Windows paths are kept as given (relativize reads every spelling).
        let abs = r"\\?\C:\Users\a\acme-billing/schema/invoice.json";
        assert_eq!(resolve_path(cwd, abs), abs);
        assert_eq!(resolve_path(cwd, r"C:\Users\a\acme-billing\x.py"), r"C:\Users\a\acme-billing\x.py");
        // Relative paths join onto a Windows cwd, keeping the drive.
        assert_eq!(resolve_path(cwd, "schema/invoice.json"), "c:/Users/a/acme-billing/schema/invoice.json");
        assert_eq!(resolve_path(r"C:\r\repo", r"src\..\schema\x.json"), "c:/r/repo/schema/x.json");
        // Unix is unchanged.
        assert_eq!(resolve_path("/r/repo", "schema/x.json"), "/r/repo/schema/x.json");
        assert_eq!(resolve_path("/r/repo", "/etc/hosts"), "/etc/hosts");
        assert_eq!(resolve_path("/r/repo", "a/../b"), "/r/repo/b");
    }

    #[test]
    fn a_windows_edit_relativizes_to_the_glob_path() {
        let cwd = r"\\?\C:\Users\a\acme-billing";
        let root = std::path::Path::new(cwd);
        for p in [r"\\?\C:\Users\a\acme-billing/schema/invoice.json", "schema/invoice.json", r"schema\invoice.json"] {
            let resolved = resolve_path(cwd, p);
            assert_eq!(crate::repo::relativize(root, &resolved), "schema/invoice.json", "{p} → {resolved}");
        }
    }
}

#[cfg(test)]
mod shadow_tests {
    use super::{ascii_shadow, normalize_shell};
    use crate::CommandTable;

    #[test]
    fn the_parser_only_sees_ascii_with_offsets_unchanged() {
        for s in ["{\u{10FFFF}..9}", "cat résumé.txt", "echo 日本 && ls", "\u{1F600}{\u{2028}", "plain"] {
            let sh = ascii_shadow(s);
            assert!(sh.is_ascii(), "{s:?}");
            assert_eq!(sh.len(), s.len(), "{s:?}");
            for (a, b) in s.char_indices().filter(|(_, c)| c.is_ascii()).map(|(i, c)| (sh.as_bytes()[i], c)) {
                assert_eq!(a as char, b, "ASCII characters are kept in place");
            }
        }
    }

    /// An interpreter fed its program by a heredoc runs inline code: `python3 - <<'EOF'` is shaped
    /// as `python3 -c …`, so its key is `python3 -c {code}`, never a bare `python3` that every
    /// one-off script would share. A script operand keeps the script as the identity.
    #[test]
    fn a_heredoc_program_is_inline_code() {
        let t = CommandTable::load();
        let key = |cmd: &str| {
            let out = normalize_shell(cmd, "/repo", t);
            crate::keys::template_key(&out[0].action)
        };
        assert_eq!(key("python3 - <<'EOF'\nprint(1)\nEOF"), "exec python3 -c {code}");
        assert_eq!(key("python3 <<EOF\nprint(1)\nEOF"), "exec python3 -c {code}");
        assert_eq!(key("python3 tools/gen.py <<EOF\nx\nEOF"), "exec python3 tools/gen.py");
        assert_eq!(key("cat <<EOF\nhello\nEOF"), "exec cat");
    }

    /// A rewriting tool asked only to check is a check: `cargo fmt --all --check` verifies,
    /// `cargo fmt --all` regenerates.
    #[test]
    fn check_flags_make_a_formatter_a_check() {
        let t = CommandTable::load();
        let effect = |cmd: &str| normalize_shell(cmd, "/repo", t)[0].effect;
        use crate::Effect;
        assert_eq!(effect("cargo fmt --all --check"), Effect::Verify);
        assert_eq!(effect("cargo fmt --all"), Effect::Regenerate);
        assert_eq!(effect("black --check ."), Effect::Verify);
        assert_eq!(effect("black ."), Effect::Regenerate);
    }

    /// Python project managers, task runners and package-manager shorthands classify by what they
    /// run: a wrapper as its program, an environment sync as environment, tox and nox as verify.
    #[test]
    fn wrappers_and_task_runners_classify_by_what_they_run() {
        let t = CommandTable::load();
        let effect = |cmd: &str| normalize_shell(cmd, "/repo", t)[0].effect;
        use crate::Effect::*;
        for (cmd, want) in [
            ("uv run pytest -x tests/test_a.py", Verify),
            ("uv run --group dev tox run", Verify),
            ("uv run python -m pytest", Verify),
            ("uv run -- pytest", Verify),
            ("uv run --with rich scripts/x.py", Environment),
            ("uv run black .", Regenerate),
            ("uv run rm -rf build", Publish),
            ("uv sync", Environment),
            ("uv pip install -e .", Environment),
            ("uv pip list", Observe),
            ("uv lock", Environment),
            ("uv publish", Publish),
            ("uvx ruff check .", Verify),
            ("tox", Verify),
            ("tox -e py", Verify),
            ("tox run -e py312", Verify),
            ("tox list", Observe),
            ("tox exec -e py -- pytest", Verify),
            ("nox -s tests", Verify),
            ("poetry run pytest", Verify),
            ("poetry install", Environment),
            ("poetry publish", Publish),
            ("hatch run test", Verify),
            ("hatch run pytest", Verify),
            ("hatch test", Verify),
            ("hatch fmt --check", Verify),
            ("pdm run pytest", Verify),
            ("pdm install", Environment),
            ("pipenv run pytest", Verify),
            ("just test", Verify),
            ("just gen", Regenerate),
            ("just deploy", Environment),
            ("just --list", Observe),
            ("task test", Verify),
            ("task release", Environment),
            ("coverage run -m pytest", Verify),
            ("pytest -q", Verify),
            ("vitest run", Verify),
            ("jest --ci", Verify),
            ("go test ./...", Verify),
            ("cargo nextest run", Verify),
            ("pnpm vitest run", Verify),
            ("pnpm exec vitest", Verify),
            ("pnpm test-unit", Verify),
            ("pnpm check", Verify),
            ("pnpm --filter runtime-core test", Verify),
            ("pnpm -r build", Verify),
            ("pnpm deploy out", Environment),
            ("pnpm publish", Publish),
            ("yarn lint", Verify),
            ("yarn npm publish", Publish),
            ("npm run lint", Verify),
            ("npm --prefix web test", Verify),
            ("npm exec -- jest", Verify),
            ("bun run test", Verify),
            ("bun publish", Publish),
            ("bunx vitest", Verify),
            ("make test", Verify),
        ] {
            assert_eq!(effect(cmd), want, "{cmd}");
        }
    }

    #[test]
    fn non_ascii_arguments_survive_normalisation() {
        let out = normalize_shell("cat résumé.txt && ls 日本", "/repo", CommandTable::load());
        let text = format!("{out:?}");
        assert!(text.contains("résumé.txt") && text.contains("日本"), "{text}");
        // The input shape that crashed tree-sitter-bash's scanner on glibc: no crash, some result.
        let _ = normalize_shell("{\u{10FFFF}", "/repo", CommandTable::load());
        let _ = normalize_shell("echo {\u{1F600}..}", "/repo", CommandTable::load());
    }
}

/// Interpreters whose first path argument is the script they run.
pub const SCRIPT_INTERPRETERS: [&str; 9] = ["python", "python3", "node", "bash", "sh", "zsh", "ruby", "perl", "deno"];

/// Words in a script's name that say it changes something, reaches the network, or runs other
/// programs: such a script is never classified observe from the traces.
const MUTATING_WORDS: [&str; 34] = [
    "set", "update", "write", "delete", "del", "remove", "rm", "create", "add", "apply", "deploy", "push", "publish",
    "install", "sync", "migrate", "gen", "generate", "build", "fix", "format", "fmt", "commit", "reset", "clean",
    "init", "post", "send", "upload", "download", "curl", "wget", "http", "exec",
];

/// The repository script a plain command runs, when the command table does not know it:
/// `python3 tools/case {item}` and `./tools/case {item}` → `tools/case`. `None` for a known
/// program, anything with shell syntax, a path outside the repository, or a script whose name
/// says it changes something ([`MUTATING_WORDS`]).
pub fn unlisted_repo_script(command: &str, table: &CommandTable) -> Option<String> {
    let words: Vec<&str> = command.split_whitespace().collect();
    if words.is_empty()
        || words
            .iter()
            .any(|w| w.contains(['|', ';', '&', '<', '>', '$', '`', '(', ')', '\'', '"', '\\']) || w.contains("://"))
    {
        return None;
    }
    let script = if SCRIPT_INTERPRETERS.contains(&words[0]) {
        let s = words.get(1)?;
        if s.starts_with('-') {
            return None;
        }
        *s
    } else {
        words[0]
    };
    let rel = script.strip_prefix("./").unwrap_or(script);
    if !script.contains('/')
        || rel.starts_with('/')
        || rel.split('/').any(|p| p.is_empty() || p == "..")
        || rel.contains('{')
    {
        return None;
    }
    let base = basename(rel);
    if table.get(&base).is_some() {
        return None;
    }
    let lower = base.to_ascii_lowercase();
    if lower.split(['-', '_', '.']).any(|p| MUTATING_WORDS.contains(&p)) {
        return None;
    }
    Some(rel.to_string())
}

#[cfg(test)]
mod repo_script_tests {
    use super::*;

    #[test]
    fn unlisted_repo_scripts_are_found_and_known_programs_are_not() {
        let t = CommandTable::load();
        assert_eq!(unlisted_repo_script("python3 tools/case {item}", t).as_deref(), Some("tools/case"));
        assert_eq!(unlisted_repo_script("./tools/case {item}", t).as_deref(), Some("tools/case"));
        assert_eq!(unlisted_repo_script("node bin/fetch-ticket {item} --json", t).as_deref(), Some("bin/fetch-ticket"));
        assert_eq!(unlisted_repo_script("make test", t), None);
        assert_eq!(unlisted_repo_script("python3 -m pytest", t), None);
        assert_eq!(unlisted_repo_script("cat tools/case", t), None);
        assert_eq!(unlisted_repo_script("python3 tools/case {item} | sh", t), None);
        assert_eq!(unlisted_repo_script("python3 /usr/bin/case x", t), None);
        assert_eq!(unlisted_repo_script("python3 ../case x", t), None);
        assert_eq!(unlisted_repo_script("python3 scripts/update-case x", t), None);
        assert_eq!(unlisted_repo_script("python3 tools/gen.py", t), None);
        assert_eq!(unlisted_repo_script("./scripts/deploy x", t), None);
    }
}

#[cfg(test)]
mod observed_tests {
    use super::{normalize_shell, normalize_shell_observed};
    use crate::types::CanonicalAction;
    use crate::CommandTable;

    /// Each action of a command as the runtime observes it, flattened: `exec <argv>`, `write
    /// <path>` or `edit <path>`.
    fn seen(cmd: &str) -> Vec<String> {
        crate::keys::flatten(&normalize_shell_observed(cmd, "/repo", CommandTable::load()))
            .into_iter()
            .map(|n| match n.action {
                CanonicalAction::Exec { argv, .. } => format!("exec {}", argv.join(" ")),
                CanonicalAction::Write { path } => format!("write {path}"),
                CanonicalAction::Edit { path } => format!("edit {path}"),
                CanonicalAction::List { path } => format!("list {path}"),
                other => format!("{other:?}"),
            })
            .collect()
    }

    /// The shapes the first live run's agents edited files with through Bash: a heredoc into a
    /// redirection, `tee`, `printf … >`, and a Python program fed by a heredoc. Each writes its
    /// file as an action right after the one that wrote it, an append as an edit.
    #[test]
    fn redirections_tee_and_heredocs_write_their_files() {
        let x = "/repo/schema/x.sql";
        for (cmd, want) in [
            ("cat > schema/x.sql <<'EOF'\nCREATE TABLE x;\nEOF", vec!["exec cat".to_string(), format!("write {x}")]),
            ("cat <<'EOF' > schema/x.sql\nCREATE TABLE x;\nEOF", vec!["exec cat".into(), format!("write {x}")]),
            ("cat >> schema/x.sql <<'EOF'\nCREATE TABLE y;\nEOF", vec!["exec cat".into(), format!("edit {x}")]),
            (
                "printf '%s\\n' 'CREATE TABLE x;' > schema/x.sql",
                vec!["exec printf %s\\n CREATE TABLE x;".into(), format!("write {x}")],
            ),
            ("tee schema/x.sql <<'EOF'\nx\nEOF", vec!["exec tee schema/x.sql".into(), format!("write {x}")]),
            (
                "cat <<'EOF' | tee -a schema/x.sql > /dev/null\nx\nEOF",
                vec!["exec cat".into(), "exec tee -a schema/x.sql".into(), format!("edit {x}")],
            ),
            (
                "echo x | tee schema/x.sql schema/y.sql",
                vec!["exec echo x".into(), format!("write {x}"), "write /repo/schema/y.sql".into()],
            ),
            ("ls | sort > out.txt", vec!["list /repo".into(), "write /repo/out.txt".into()]),
            ("cd schema && cat > x.sql <<'EOF'\nx\nEOF", vec!["exec cat".into(), format!("write {x}")]),
            ("bash -c 'printf x > schema/x.sql'", vec!["exec printf x".into(), format!("write {x}")]),
        ] {
            assert_eq!(seen(cmd), want, "{cmd}");
        }
    }

    /// `python3 - <<'E'` with the file in a variable, as every schema edit in the live run was
    /// made, then `make gen` in the same call: the program's write, then the commands after it.
    #[test]
    fn a_python_program_writes_the_files_it_opens_for_writing() {
        let live = "python3 - <<'E'\np='schema/invoice.json'\ns=open(p).read()\ns=s.replace('a','b')\nopen(p,'w').write(s)\nE\nmake gen && make ci";
        assert_eq!(
            seen(live),
            ["exec python3 -c <stdin>", "write /repo/schema/invoice.json", "exec make gen", "exec make ci"]
        );
        // Two files through one name, rebound; an append; `Path` writes; `-c` code.
        let two = "python3 - <<'E'\np='schema/invoice.json'\nopen(p,'w').write(s)\np='acme/billing.py'\nwith open(p, mode='a') as f: f.write(s)\nE";
        assert_eq!(
            seen(two),
            ["exec python3 -c <stdin>", "write /repo/schema/invoice.json", "edit /repo/acme/billing.py"]
        );
        let paths = "python3 <<'E'\nfrom pathlib import Path\nPath('schema/a.json').write_text('x')\nq = Path(\"schema/b.json\")\nq.write_bytes(b'y')\nE";
        assert_eq!(seen(paths), ["exec python3 -c <stdin>", "write /repo/schema/a.json", "write /repo/schema/b.json"]);
        assert_eq!(
            seen("python3 -c \"open('schema/a.json','w').write('x')\""),
            ["exec python3 -c open('schema/a.json','w').write('x')", "write /repo/schema/a.json"]
        );
        // Reads, a computed path, a name bound to anything but a literal, and another
        // interpreter: no write.
        for cmd in [
            "python3 - <<'E'\np='schema/a.json'\nprint(open(p).read(), open(p, 'r').read(), open(p, 'rb').read())\nE",
            "python3 - <<'E'\nopen('schema/' + name, 'w')\nE",
            "python3 - <<'E'\np='schema/a.json'\np = choose()\nopen(p,'w')\nE",
            "node - <<'E'\nrequire('fs').writeFileSync('schema/a.json', 'x')\nE",
        ] {
            assert!(seen(cmd).iter().all(|a| a.starts_with("exec ")), "{cmd}: {:?}", seen(cmd));
        }
    }

    /// The commands written after a heredoc's start are seen (the grammar nests them in the
    /// heredoc): the edit and the step that meets it, in one call.
    #[test]
    fn commands_after_a_heredoc_start_are_seen() {
        assert_eq!(
            seen("cat > schema/a.json <<'EOF' && make gen\n{}\nEOF"),
            ["exec cat", "write /repo/schema/a.json", "exec make gen"]
        );
        assert_eq!(
            seen("python3 - <<'E' && make gen\nopen('schema/a.json','w')\nE"),
            ["exec python3 -c <stdin>", "write /repo/schema/a.json", "exec make gen"]
        );
    }

    /// No file: a descriptor, a device, an expansion, an input; and the plain normalisation, which
    /// learning and the graph read, is what it was for every shape above.
    #[test]
    fn only_literal_files_are_written_and_the_plain_form_is_unchanged() {
        for cmd in ["make test 2>&1", "make test > /dev/null 2>&1", "echo x > \"$OUT\"", "sort < in.txt", "echo x >&2"]
        {
            assert!(seen(cmd).iter().all(|a| a.starts_with("exec ")), "{cmd}: {:?}", seen(cmd));
        }
        let t = CommandTable::load();
        for cmd in [
            "cat <<'EOF' > schema/x.sql\nx\nEOF",
            "echo x | tee schema/x.sql",
            "python3 - <<'E'\nopen('schema/a.json','w')\nE",
            "cat > schema/a.json <<'EOF' && make gen\n{}\nEOF",
        ] {
            let plain = crate::keys::flatten(&normalize_shell(cmd, "/repo", t));
            assert!(
                plain.iter().all(|n| !matches!(n.action, CanonicalAction::Write { .. } | CanonicalAction::Edit { .. })),
                "{cmd}: {plain:?}"
            );
            assert!(plain.iter().all(|n| !format!("{:?}", n.action).contains("make")), "{cmd}: {plain:?}");
        }
    }
}
