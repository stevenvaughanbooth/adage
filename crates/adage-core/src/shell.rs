//! Shell → canonical actions, via `tree-sitter` + `tree-sitter-bash`.
//!
//! Compared to the TypeScript proof of concept (`research/poc/src/normalize/shell.ts`, a
//! dependency-free hand-rolled tokenizer), this uses a real shell grammar so that compound
//! constructs (`for`, `while`, `if`, `case`, subshells, `$(…)`) are parsed rather than treated
//! as one opaque low-confidence blob. See SPIKE.md's method section.

use tree_sitter::{Node, Parser};

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
        p.set_timeout_micros(PARSE_TIMEOUT_MICROS);
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

/// Parsing gives up after this long, and the command is recorded as unknown. tree-sitter's error
/// recovery is quadratic on some malformed input: 16 KB of `a|a|a|…` or `)))…` took 0.6–0.7 s to
/// parse in a release build (the hook's budget is 25 ms). Real commands, heredocs included, parse
/// in well under a millisecond per kilobyte.
pub const PARSE_TIMEOUT_MICROS: u64 = 50_000;

/// Maximum depth of the syntax-tree walk (statements within statements, including `bash -c`
/// scripts). `too_deep` bounds bracket nesting before parsing; this bounds everything else a
/// grammar can nest (`if` in `if`, `!` in `!`, …) so no input can exhaust the stack.
pub const MAX_DEPTH: usize = 96;

/// More pipe operators than this (outside quotes) and a command is treated as unknown: a long run
/// of `a|a|…|` sends tree-sitter's error recovery quadratic in a phase its timeout does not cover
/// (16 KB took 0.6 s in a release build). No real pipeline has this many stages.
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
    normalize_at(command, cwd, table, 0)
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

fn normalize_at(command: &str, cwd: &str, table: &CommandTable, depth: usize) -> Vec<Normalized> {
    let command = command.trim();
    if command.is_empty() {
        return vec![];
    }
    if command.len() > MAX_COMMAND || too_deep(command) || too_many_pipes(command) || depth > MAX_DEPTH {
        return vec![unknown("not normalised: exceeds size or nesting bounds")];
    }

    let tree = PARSER.with(|p| {
        let mut p = p.borrow_mut();
        let t = p.parse(command, None);
        if t.is_none() {
            // A timed-out parse would otherwise resume on the next, unrelated command.
            p.reset();
        }
        t
    });
    let tree = match tree {
        Some(t) => t,
        None => {
            return vec![Normalized {
                action: CanonicalAction::Unknown { raw: command.to_string() },
                effect: Effect::Environment,
                mutating: false,
                confidence: 0.0,
                note: "tree-sitter produced no tree (parse timed out or failed)".to_string(),
            }]
        }
    };
    let root = tree.root_node();

    let ctx = Ctx { src: command, table, depth: std::cell::Cell::new(depth) };
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
    if p.starts_with('/') || p.starts_with('~') {
        return p.to_string();
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
        // `list_services`) is classified observe by name, at reduced confidence. The miner still
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
            head_actions
        }

        "redirected_statement" => {
            let Some(body) = node.child_by_field_name("body") else { return vec![] };
            let mut actions = walk_statement(body, cwd, ctx);

            let mut c = node.walk();
            let mut output_target: Option<String> = None;
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
                    subs.extend(normalize_at(&script, cwd, ctx.table, ctx.depth.get()));
                    return subs;
                }
            }

            let ops = operands(&cmd, &argv, ctx.table);
            let action_result = shape_command(&cmd, &argv, &ops, cwd, ctx.table);
            subs.push(action_result);
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
