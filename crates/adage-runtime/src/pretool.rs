//! `PreToolUse` delivery for U11 (the project's environment): before a shell command runs, a test
//! run or scratch repro under the system Python is pointed at the project's own interpreter, and a
//! `pip install` of the project or its test dependencies into the system Python gets advice (never
//! a refusal) naming the command that reaches the project.
//!
//! Only the interpreter prefix changes: `pytest -x tests/a.py` becomes
//! `<root>/.venv/bin/python -m pytest -x tests/a.py`, `python -m pytest …` and `python3 repro.py`
//! keep every argument. The environment was verified from its files when the instincts were bound
//! (`adage_instincts::verify_env`: pytest installed, the project importable from this checkout),
//! never here. The replay metric of this lever is `adage eval replay-savings --mode pre-emptive`.
//!
//! **What is rewritten.** The command is read as a shell reads it ([`parse`]): a list of simple
//! commands joined by `;`, `&&`, `||`, `|`, `&` and newlines, with quotes, escapes, comments,
//! redirections and heredocs. Only the command word of a simple command in that top-level list
//! is ever rewritten. A heredoc body, a quoted string and a `-c` payload are data: never
//! rewritten, never advised on. A command whose structure this reader does not follow (a
//! subshell, a group, `if`/`for`/`while`/`case`, a command or process substitution, an
//! unterminated quote or heredoc, a line continuation) is left alone as a whole, and so is one
//! that activates an environment, `cd`s outside the checkout, or names an interpreter version
//! other than the environment's (`python3.9`, `python2`).

use std::path::{Component, Path, PathBuf};

/// The project's verified environment, as the hook hands it over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    /// The interpreter, as it goes into the command (absolute, shell-quoted if needed).
    pub interpreter: String,
    /// The same, as a person reads it (`.venv/bin/python`).
    pub shown: String,
    /// pytest is installed in the environment.
    pub pytest: bool,
    /// Modules it imports that the system Python may lack: the project's own packages and the
    /// declared test dependencies installed in it.
    pub modules: Vec<String>,
    /// The checkout the environment belongs to: a command that `cd`s outside it is left alone.
    pub root: PathBuf,
    /// Where the command runs (the payload's `cwd`): relative `cd` targets resolve against it.
    pub cwd: PathBuf,
    /// The environment's Python version, major.minor (`3.12`, from `pyvenv.cfg`): `python3.12` is
    /// the same interpreter family and is rewritten; any other pinned version is not.
    pub version: Option<String>,
    /// The project is managed by uv (`uv.lock`): uv-made environments have no pip.
    pub uv: bool,
}

/// What to do before the command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pre {
    /// Run this command instead (the interpreter prefix swapped); `kind` is `test` or `repro`.
    /// `segments` are the simple commands rewritten, before and after, for permission rules.
    Rewrite { command: String, kind: &'static str, segments: Vec<(String, String)> },
    /// Let the command run as written, and tell the agent this (`kind`: `pip-install`, or
    /// `permissions` for a rewrite the person's permission rules would not admit).
    Advise { reason: String, kind: &'static str },
}

/// A shell word: its byte range in the command, its text with quotes and escapes removed, and
/// whether any of it was quoted or escaped.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Word {
    start: usize,
    end: usize,
    text: String,
    quoted: bool,
}

/// A simple command of the top-level list: its words (redirections left out), how many
/// redirections it has, and the byte range of its heredoc's body, if it reads one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Simple {
    words: Vec<Word>,
    redirs: usize,
    heredoc: Option<(usize, usize)>,
}

/// Words that open or close a compound command, or change how the next command runs: a command
/// that uses one in command position is left alone as a whole.
const RESERVED: &[&str] = &[
    "if", "then", "elif", "else", "fi", "for", "while", "until", "do", "done", "case", "esac", "select", "function",
    "time", "coproc", "{", "}", "!", "[[", "]]",
];

fn finish(cur: &mut Simple, out: &mut Vec<Simple>) {
    if !cur.words.is_empty() || cur.redirs > 0 {
        out.push(std::mem::take(cur));
    }
}

/// One shell word at `i`, and where it ends. `None` for what this reader does not follow: a
/// command substitution (`$(…)`, backquotes), an unterminated quote, a line continuation.
fn word(s: &str, mut i: usize) -> Option<(Word, usize)> {
    let b = s.as_bytes();
    let start = i;
    let mut text = String::new();
    let mut quoted = false;
    let ch = |i: usize| s[i..].chars().next();
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\r' | b'\n' | b';' | b'&' | b'|' | b'(' | b')' | b'<' | b'>' => break,
            b'`' => return None,
            b'$' if b.get(i + 1) == Some(&b'(') => return None,
            b'\'' => {
                quoted = true;
                let end = i + 1 + s[i + 1..].find('\'')?;
                text.push_str(&s[i + 1..end]);
                i = end + 1;
            }
            b'"' => {
                quoted = true;
                i += 1;
                loop {
                    match *b.get(i)? {
                        b'"' => {
                            i += 1;
                            break;
                        }
                        b'`' => return None,
                        b'$' if b.get(i + 1) == Some(&b'(') => return None,
                        b'\\' => match b.get(i + 1) {
                            Some(b'\n') => return None,
                            Some(&c @ (b'"' | b'\\' | b'$' | b'`')) => {
                                text.push(c as char);
                                i += 2;
                            }
                            _ => {
                                text.push('\\');
                                i += 1;
                            }
                        },
                        _ => {
                            let c = ch(i)?;
                            text.push(c);
                            i += c.len_utf8();
                        }
                    }
                }
            }
            b'\\' => {
                let c = ch(i + 1)?;
                if c == '\n' {
                    return None;
                }
                quoted = true;
                text.push(c);
                i += 1 + c.len_utf8();
            }
            _ => {
                let c = ch(i)?;
                text.push(c);
                i += c.len_utf8();
            }
        }
    }
    (i > start).then_some((Word { start, end: i, text, quoted }, i))
}

/// The word a redirection names, after blanks.
fn target(s: &str, mut i: usize) -> Option<(Word, usize)> {
    let b = s.as_bytes();
    while i < b.len() && matches!(b[i], b' ' | b'\t') {
        i += 1;
    }
    word(s, i)
}

/// A redirection at `i` (`<`, `>`, `>>`, `>&`, `<&`, `>|`, `<>`, `<<<`, `<<`, `<<-`), its target
/// read. A heredoc's delimiter is queued; its body is read at the end of the line.
fn redirection(
    s: &str,
    i: usize,
    cur: &mut Simple,
    pending: &mut Vec<(String, bool, usize)>,
    owner: usize,
) -> Option<usize> {
    let b = s.as_bytes();
    cur.redirs += 1;
    if b[i] == b'<' && b.get(i + 1) == Some(&b'<') {
        if b.get(i + 2) == Some(&b'<') {
            return target(s, i + 3).map(|(_, n)| n);
        }
        let strip = b.get(i + 2) == Some(&b'-');
        let (w, next) = target(s, i + 2 + usize::from(strip))?;
        if w.text.is_empty() {
            return None;
        }
        pending.push((w.text, strip, owner));
        return Some(next);
    }
    if b.get(i + 1) == Some(&b'(') {
        return None; // a process substitution
    }
    let two = match b[i] {
        b'<' => matches!(b.get(i + 1), Some(b'&' | b'>')),
        _ => matches!(b.get(i + 1), Some(b'>' | b'&' | b'|')),
    };
    target(s, i + 1 + usize::from(two)).map(|(_, n)| n)
}

/// The command's top-level list of simple commands, or `None` when its structure is not one
/// this reader follows (see the module documentation). Heredoc bodies are skipped as data and
/// recorded on the command that reads them.
fn parse(s: &str) -> Option<Vec<Simple>> {
    let b = s.as_bytes();
    let mut out: Vec<Simple> = Vec::new();
    let mut cur = Simple::default();
    let mut pending: Vec<(String, bool, usize)> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b' ' | b'\t' | b'\r' => i += 1,
            b'\n' => {
                finish(&mut cur, &mut out);
                i += 1;
                for (delim, strip, owner) in pending.drain(..) {
                    let start = i;
                    loop {
                        if i >= b.len() {
                            return None; // unterminated
                        }
                        let end = s[i..].find('\n').map_or(b.len(), |k| i + k);
                        let line = s[i..end].trim_end_matches('\r');
                        let line = if strip { line.trim_start_matches('\t') } else { line };
                        let next = (end + 1).min(b.len());
                        if line == delim {
                            out.get_mut(owner)?.heredoc = Some((start, i));
                            i = next;
                            break;
                        }
                        i = next;
                    }
                }
            }
            b'#' => {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            b';' => {
                if b.get(i + 1) == Some(&b';') {
                    return None;
                }
                finish(&mut cur, &mut out);
                i += 1;
            }
            b'&' => match b.get(i + 1) {
                Some(b'&') => {
                    finish(&mut cur, &mut out);
                    i += 2;
                }
                Some(b'>') => {
                    cur.redirs += 1;
                    let at = if b.get(i + 2) == Some(&b'>') { i + 3 } else { i + 2 };
                    i = target(s, at)?.1;
                }
                _ => {
                    finish(&mut cur, &mut out);
                    i += 1;
                }
            },
            b'|' => {
                finish(&mut cur, &mut out);
                i += if matches!(b.get(i + 1), Some(b'|' | b'&')) { 2 } else { 1 };
            }
            b'(' | b')' | b'`' => return None,
            b'<' | b'>' => i = redirection(s, i, &mut cur, &mut pending, out.len())?,
            _ => {
                let (w, next) = word(s, i)?;
                // A file descriptor before a redirection (`2>&1`).
                if !w.quoted && w.text.bytes().all(|c| c.is_ascii_digit()) && matches!(b.get(next), Some(b'<' | b'>')) {
                    i = redirection(s, next, &mut cur, &mut pending, out.len())?;
                    continue;
                }
                if !w.quoted && command_index(s, &cur).is_none() && RESERVED.contains(&w.text.as_str()) {
                    return None;
                }
                cur.words.push(w);
                i = next;
            }
        }
    }
    if !pending.is_empty() {
        return None;
    }
    finish(&mut cur, &mut out);
    Some(out)
}

/// A leading environment assignment (`PYTHONPATH=src`), by its unquoted name.
fn assignment(s: &str, w: &Word) -> bool {
    let raw = &s[w.start..w.end];
    raw.find('=').is_some_and(|e| {
        e > 0
            && !raw[..1].bytes().all(|c| c.is_ascii_digit())
            && raw[..e].bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
    })
}

/// The index of a simple command's command word (after any assignments).
fn command_index(s: &str, c: &Simple) -> Option<usize> {
    c.words.iter().position(|w| !assignment(s, w))
}

/// `a` normalised lexically (`.` and `..` resolved).
fn normal(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            c => out.push(c),
        }
    }
    out
}

/// A path a word names, resolved against `dir`; `None` when the shell would expand it (`~`,
/// `$VAR`, a glob) or it is an option.
fn resolve(dir: &Path, w: &str) -> Option<PathBuf> {
    if w.is_empty() || w.starts_with(['-', '~']) || w.contains(['$', '*', '?', '[']) {
        return None;
    }
    let p = Path::new(w);
    Some(normal(&if p.is_absolute() { p.to_path_buf() } else { dir.join(p) }))
}

/// `python`, `python3`, or the environment's own version (`python3.12`): a pinned other version
/// (`python3.9`, `python2`) is a deliberate choice, and not the same interpreter.
fn same_python(w: &str, env: &Env) -> bool {
    match w.strip_prefix("python") {
        Some("" | "3") => true,
        Some(v) => env.version.as_deref() == Some(v),
        None => false,
    }
}

/// Whether Python source imports one of `modules` (`import click`, `from click.core import …`).
pub fn imports(code: &str, modules: &[String]) -> bool {
    let known = |name: &str| {
        let top = name.trim().split([' ', '.']).next().unwrap_or("");
        modules.iter().any(|m| m == top)
    };
    code.split([';', '\n']).take(2_000).any(|stmt| {
        let t = stmt.trim();
        if let Some(r) = t.strip_prefix("import ") {
            r.split(',').any(known)
        } else if let Some(r) = t.strip_prefix("from ") {
            known(r)
        } else {
            false
        }
    })
}

/// A distribution or module name, normalised (`Pytest-Cov` → `pytest_cov`).
fn norm_name(n: &str) -> String {
    n.to_ascii_lowercase().replace(['-', '.'], "_")
}

/// Options of `pip install` that change nothing about where it installs.
const PIP_PLAIN_OPTIONS: &[&str] = &[
    "-q",
    "-qq",
    "--quiet",
    "-v",
    "--verbose",
    "-U",
    "--upgrade",
    "--pre",
    "--no-deps",
    "--force-reinstall",
    "--no-cache-dir",
    "--no-build-isolation",
    "--disable-pip-version-check",
    "--no-input",
];

/// Advice for a `pip install` into the system Python (review B2): only a plain `pip`/`pip3`/
/// `python -m pip` install, with no option that sends it elsewhere or that this reader does not
/// know (`--user`, `--target`, `--prefix`, `--root`, `-r`, …), of the project itself or of its
/// test dependencies installed in the environment (the harm measured: a second copy of them in
/// the system Python, which the tests never use). Anything else gets nothing. `dir` is where it
/// runs.
fn pip_advice(word: &str, args: &[&str], env: &Env, dir: &Path) -> Option<String> {
    let rest = if matches!(word, "pip" | "pip3") && args.first() == Some(&"install") {
        &args[1..]
    } else if same_python(word, env) && args.len() >= 3 && args[..3] == ["-m", "pip", "install"] {
        &args[3..]
    } else {
        return None;
    };
    let mut targets = Vec::new();
    let mut it = rest.iter();
    while let Some(a) = it.next() {
        match *a {
            "-e" | "--editable" => targets.push(*it.next()?),
            a if PIP_PLAIN_OPTIONS.contains(&a) => {}
            a if a.starts_with('-') => return None,
            a => targets.push(a),
        }
    }
    let project = |t: &str| {
        let path = t.split_once('[').map_or(t, |(p, _)| p);
        (path.starts_with('.') || path.starts_with('/'))
            && !env.root.as_os_str().is_empty()
            && resolve(dir, path).is_some_and(|p| p == normal(&env.root))
    };
    let test_dep = |t: &str| {
        let name = t.split(['[', '<', '>', '=', '!', '~', ';', '@', ' ']).next().unwrap_or("");
        let n = norm_name(name);
        !n.is_empty()
            && !name.contains('/')
            && (env.modules.iter().any(|m| norm_name(m) == n) || (n == "pytest" && env.pytest))
    };
    if targets.is_empty() || !targets.iter().all(|t| project(t) || test_dep(t)) {
        return None;
    }
    let install = if env.uv {
        format!("uv pip install --python {} <package>", env.interpreter)
    } else {
        format!("{} -m pip install <package>", env.interpreter)
    };
    Some(format!(
        "Adage: this repository's Python environment is {shown}, and the project and its test dependencies are already installed there (checked when Adage bound it). `{word} install` here installs into the system Python instead, which the tests do not use. To run the tests: `{interp} -m pytest`. If a package is really missing, install it into the environment: `{install}`{uv}. The command was not changed or blocked.",
        shown = env.shown,
        interp = env.interpreter,
        uv = if env.uv { " (a uv-made environment has no pip)" } else { "" },
    ))
}

/// Plan a `PreToolUse` for a shell command. `read` reads a script the command runs (relative to
/// the session's working directory), for a repro's imports; it may return `None`.
pub fn plan(command: &str, env: &Env, read: impl Fn(&str) -> Option<String>) -> Option<Pre> {
    if command.len() > 8_000 {
        return None;
    }
    let list = parse(command)?;
    // An activated environment already runs its interpreter; a command that goes to another
    // directory is not this checkout's. Either way: nothing to rewrite, nothing to advise.
    let mut dir = normal(&env.cwd);
    // The directory each simple command runs in.
    let mut dirs = Vec::with_capacity(list.len());
    for c in &list {
        dirs.push(dir.clone());
        let Some(k) = command_index(command, c) else { continue };
        let name = c.words[k].text.as_str();
        if c.words.iter().any(|w| w.text.contains("activate"))
            || matches!(name, "source" | "." | "conda" | "mamba" | "micromamba" | "workon" | "pyenv" | "popd")
        {
            return None;
        }
        if matches!(name, "cd" | "pushd") {
            dir = resolve(&dir, c.words.get(k + 1).map_or("", |w| w.text.as_str()))?;
            if env.root.as_os_str().is_empty() || !dir.starts_with(normal(&env.root)) {
                return None;
            }
        }
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut segments = Vec::new();
    let mut kind = "";
    for (c, dir) in list.iter().zip(&dirs) {
        let Some(k) = command_index(command, c) else { continue };
        let w = &c.words[k];
        if w.quoted {
            continue;
        }
        let word = w.text.as_str();
        let args: Vec<&str> = c.words[k + 1..].iter().map(|x| x.text.as_str()).collect();
        if let Some(reason) = pip_advice(word, &args, env, dir) {
            return Some(Pre::Advise { reason, kind: "pip-install" });
        }
        let first = args.first().copied();
        let second = args.get(1).copied();
        let with = if matches!(word, "pytest" | "py.test") && env.pytest {
            kind = "test";
            format!("{} -m pytest", env.interpreter)
        } else if same_python(word, env) {
            let test = first == Some("-m") && second == Some("pytest") && env.pytest;
            let repro = !env.modules.is_empty()
                && match first {
                    Some("-c") => second.is_some_and(|c| imports(c, &env.modules)),
                    Some("-m") => {
                        second.is_some_and(|m| env.modules.iter().any(|x| Some(x.as_str()) == m.split('.').next()))
                    }
                    // A heredoc (`python3 - <<'EOF'`, `python3 <<EOF`): its body is the script.
                    Some("-") | None => c.heredoc.is_some_and(|(s, e)| imports(&command[s..e], &env.modules)),
                    Some(script) if script.ends_with(".py") && !script.starts_with('-') => {
                        read(script).is_some_and(|code| imports(&code, &env.modules))
                    }
                    _ => false,
                };
            if !(test || repro) {
                continue;
            }
            if kind != "test" {
                kind = if test { "test" } else { "repro" };
            }
            env.interpreter.clone()
        } else {
            continue;
        };
        let (s, e) = (c.words[0].start, c.words.last().map_or(w.end, |l| l.end));
        let before = &command[s..e];
        segments.push((before.to_string(), format!("{}{with}{}", &command[s..w.start], &command[w.end..e])));
        edits.push((w.start, w.end, with));
    }
    if edits.is_empty() {
        return None;
    }
    let mut out = command.to_string();
    for (s, e, with) in edits.into_iter().rev() {
        out.replace_range(s..e, &with);
    }
    Some(Pre::Rewrite { command: out, kind, segments })
}

/// Whether a Claude Code `permissions.allow` rule admits a shell command: `Bash` or `Bash(*)`
/// (any), `Bash(prefix:*)` (the prefix, then nothing or a blank), `Bash(… * …)` (a wildcard
/// pattern), or `Bash(exact command)`.
pub fn rule_allows(rule: &str, cmd: &str) -> bool {
    let Some(inner) = rule.trim().strip_prefix("Bash") else { return false };
    let inner = inner.trim();
    if inner.is_empty() {
        return true;
    }
    let Some(p) = inner.strip_prefix('(').and_then(|x| x.strip_suffix(')')) else { return false };
    let cmd = cmd.trim();
    if let Some(prefix) = p.strip_suffix(":*") {
        return cmd.strip_prefix(prefix).is_some_and(|r| r.is_empty() || r.starts_with(char::is_whitespace));
    }
    if p.contains('*') {
        wildcard(p.as_bytes(), cmd.as_bytes())
    } else {
        p == cmd
    }
}

/// `*` matches any run of bytes.
fn wildcard(p: &[u8], s: &[u8]) -> bool {
    let (mut pi, mut si) = (0, 0);
    let mut back: Option<(usize, usize)> = None;
    while si < s.len() {
        if pi < p.len() && p[pi] == b'*' {
            back = Some((pi, si));
            pi += 1;
        } else if pi < p.len() && p[pi] == s[si] {
            pi += 1;
            si += 1;
        } else if let Some((bp, bs)) = back {
            pi = bp + 1;
            si = bs + 1;
            back = Some((bp, bs + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == b'*')
}

/// Whether a rewrite keeps what the person's permission rules decided (review S4): Claude Code
/// checks the rewritten command against the same rules, and they match command text. A command
/// the rules allowed as written must still be allowed rewritten; under `dontAsk` (only allowed
/// commands run), the rewritten one must be allowed. `bypassPermissions` checks nothing.
pub fn keeps_permissions(segments: &[(String, String)], allow: &[String], mode: Option<&str>) -> bool {
    if mode == Some("bypassPermissions") {
        return true;
    }
    let allowed = |c: &str| allow.iter().any(|r| rule_allows(r, c));
    segments.iter().all(|(before, after)| allowed(after) || (!allowed(before) && mode != Some("dontAsk")))
}

/// The advice given instead of a rewrite the permission rules would not admit.
pub fn permission_advice(env: &Env, kind: &str, command: &str) -> String {
    let what = if kind == "test" { "test run" } else { "Python run" };
    format!(
        "Adage: this {what} uses the system Python, which lacks the project and its test dependencies. The project's environment is {}: `{command}` runs it there. (Adage did not rewrite the command: your permission rules allow it as written, not rewritten.)",
        env.shown
    )
}

/// The interpreter as it goes into a shell command: quoted when it holds a character the shell
/// would read.
pub fn shell_word(p: &str) -> String {
    if p.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+@:".contains(c)) {
        p.to_string()
    } else {
        format!("'{}'", p.replace('\'', "'\\''"))
    }
}

/// The line the agent (and the person) reads beside a rewritten command.
pub fn rewrite_note(env: &Env, kind: &str) -> String {
    let what = if kind == "test" { "test run" } else { "Python run" };
    format!(
        "Adage ran this {what} with the project's environment ({}): the system Python lacks the project and its test dependencies. Nothing else in the command changed.",
        env.shown
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env() -> Env {
        Env {
            interpreter: "/w/click/.venv/bin/python".into(),
            shown: ".venv/bin/python".into(),
            pytest: true,
            modules: vec!["click".into(), "pytest".into()],
            root: "/w/click".into(),
            cwd: "/w/click".into(),
            version: Some("3.12".into()),
            uv: false,
        }
    }

    fn rw(cmd: &str) -> Option<String> {
        match plan(cmd, &env(), |_| None) {
            Some(Pre::Rewrite { command, .. }) => Some(command),
            _ => None,
        }
    }

    fn advice(cmd: &str) -> Option<String> {
        match plan(cmd, &env(), |_| None) {
            Some(Pre::Advise { reason, kind: "pip-install" }) => Some(reason),
            _ => None,
        }
    }

    #[test]
    fn only_the_interpreter_prefix_changes() {
        // Verbatim from RepoBench v2 click control sessions (the U11 dead end).
        assert_eq!(
            rw("python -m pytest tests/test_options.py::test_shadowed_help_option_error_hint -xvs").as_deref(),
            Some(
                "/w/click/.venv/bin/python -m pytest tests/test_options.py::test_shadowed_help_option_error_hint -xvs"
            )
        );
        assert_eq!(
            rw("cd /w/click && python -m pytest tests/test_formatting.py -xvs 2>&1 | head -50").as_deref(),
            Some("cd /w/click && /w/click/.venv/bin/python -m pytest tests/test_formatting.py -xvs 2>&1 | head -50")
        );
        assert_eq!(rw("pytest -q tests").as_deref(), Some("/w/click/.venv/bin/python -m pytest -q tests"));
        assert_eq!(
            rw("PYTHONPATH=src python3 -c \"import click; print(click.__version__)\"").as_deref(),
            Some("PYTHONPATH=src /w/click/.venv/bin/python -c \"import click; print(click.__version__)\"")
        );
        assert_eq!(
            rw("python3 - <<'EOF'\nfrom click.core import Command\nprint(1)\nEOF").as_deref(),
            Some("/w/click/.venv/bin/python - <<'EOF'\nfrom click.core import Command\nprint(1)\nEOF")
        );
        assert_eq!(rw("python3.12 -m pytest").as_deref(), Some("/w/click/.venv/bin/python -m pytest"));
        assert_eq!(rw("cd tests && pytest -q").as_deref(), Some("cd tests && /w/click/.venv/bin/python -m pytest -q"));
        let with_script = plan("python repro.py", &env(), |p| (p == "repro.py").then(|| "import click\n".into()));
        assert!(
            matches!(with_script, Some(Pre::Rewrite { ref command, kind: "repro", .. }) if command == "/w/click/.venv/bin/python repro.py")
        );
        let Some(Pre::Rewrite { segments, .. }) = plan("git status; pytest -q | tail", &env(), |_| None) else {
            panic!()
        };
        assert_eq!(segments, [("pytest -q".to_string(), "/w/click/.venv/bin/python -m pytest -q".to_string())]);
    }

    #[test]
    fn anything_else_is_left_alone() {
        for cmd in [
            ".venv/bin/python -m pytest tests",
            "/usr/bin/python3 -m pytest",
            "uv run pytest",
            "python -c \"print(1)\"",
            "python -c 'import requests'",
            "python setup.py --version",
            "echo python -m pytest",
            "grep -rn 'python -m pytest' docs",
            "git commit -m 'run pytest'",
            "make test",
            "$PY -m pytest",
            "\"python\" -m pytest",
            "source .venv/bin/activate && python -m pytest",
        ] {
            assert_eq!(rw(cmd), None, "{cmd}");
        }
        // Without pytest in the environment, a test run is not rewritten.
        let no_pytest = Env { pytest: false, ..env() };
        assert_eq!(plan("pytest tests", &no_pytest, |_| None), None);
    }

    /// Review B1, verbatim: heredoc bodies, quoted strings and `-c` payloads are data.
    #[test]
    fn heredoc_bodies_and_payloads_are_never_rewritten() {
        for cmd in [
            // A CI script written with a heredoc: the file must not get the machine's venv path.
            "cat > ci.sh <<'EOF'\n#!/bin/sh\npytest -q\nEOF",
            "cat > ci.sh <<EOF\npython -m pytest tests\nEOF",
            "cat <<-EOF > setup.sh\n\tpip install -e .\n\tpytest\n\tEOF",
            // A commit message that mentions the test run.
            "git commit -F - <<'EOF'\nFix rounding\n\npytest tests passes now\nEOF",
            "git commit -m \"$(cat <<'EOF'\npytest passes\nEOF\n)\"",
            "bash -c 'pytest -q'",
            "sh -c \"python -m pytest\"",
            "echo 'pytest' > notes.txt",
        ] {
            assert_eq!(plan(cmd, &env(), |_| None), None, "{cmd}");
        }
        // The command before a heredoc is still read; the body after it is not.
        assert_eq!(
            rw("pytest -q && cat > log.md <<'EOF'\npytest -q\nEOF").as_deref(),
            Some("/w/click/.venv/bin/python -m pytest -q && cat > log.md <<'EOF'\npytest -q\nEOF")
        );
        // A heredoc body with a `pip install` line is not advised on.
        assert_eq!(plan("cat > setup.sh <<'EOF'\npip install -e .\nEOF", &env(), |_| None), None);
    }

    /// Ambiguous structure: left alone as a whole.
    #[test]
    fn a_structure_this_reader_does_not_follow_is_left_alone() {
        for cmd in [
            "cat <<EOF\npytest", // unterminated heredoc
            "pytest 'tests",     // unterminated quote
            "(cd tests && pytest)",
            "{ pytest; }",
            "if true; then pytest; fi",
            "for f in a b; do python -m pytest $f; done",
            "while true; do pytest; done",
            "x=$(pytest)",
            "echo `pytest`",
            "diff <(pytest) out",
            "pytest \\\n -q",
            "time pytest",
            "case x in x) pytest;; esac",
        ] {
            assert_eq!(plan(cmd, &env(), |_| None), None, "{cmd}");
        }
    }

    /// Review S1 and S2: another directory's command, or a pinned other interpreter.
    #[test]
    fn another_directory_or_interpreter_is_left_alone() {
        for cmd in [
            "cd /home/me/otherproj && pytest",
            "cd ../otherproj && python -m pytest",
            "pushd /tmp && pytest",
            "cd && pytest",
            "cd ~/x && pytest",
            "cd $OTHER && pytest",
            "cd tests && cd ../.. && pytest",
            "python3.9 -m pytest tests",
            "python2 -m pytest tests",
            "python3.11 -m pytest tests",
        ] {
            assert_eq!(rw(cmd), None, "{cmd}");
        }
        let unknown_root = Env { root: PathBuf::new(), ..env() };
        assert_eq!(plan("cd /w/click && pytest", &unknown_root, |_| None), None);
    }

    /// Review B2: advice, only for a plain install of the project or its test dependencies.
    #[test]
    fn pip_advice_only_for_the_project_or_its_test_deps() {
        for cmd in [
            "pip install pytest",
            "pip3 install -e .",
            "python -m pip install -e '.[test]'",
            "pip install -q -U pytest click",
            "cd /w/click && pip install -e .",
            "pip install /w/click",
        ] {
            let reason = advice(cmd).unwrap_or_else(|| panic!("{cmd}"));
            assert!(reason.contains("/w/click/.venv/bin/python -m pip install <package>"), "{reason}");
            assert!(reason.contains("/w/click/.venv/bin/python -m pytest"), "{reason}");
            assert!(reason.contains("not changed or blocked"), "{reason}");
        }
        for cmd in [
            // The reviewer's refusals: none of them is advised on.
            "pip install --user httpie",
            "pip install pre-commit && pre-commit install",
            "conda activate py311 && pip install numpy",
            "python -m pip install --target /tmp/x foo",
            "if true; then pip install x; fi",
            "pip install --prefix /opt/x pytest",
            "pip install --root /r pytest",
            "pip install --target=/tmp/x pytest",
            "pip install -r requirements.txt",
            "pip install numpy",
            "pip install pytest numpy",
            "sudo pip install pytest",
            "python3.9 -m pip install pytest",
            ".venv/bin/python -m pip install pytest",
            "source venv/bin/activate && pip install pytest",
            "cd tests && pip install -e .",
            "pip list",
            "pip install",
        ] {
            assert_eq!(plan(cmd, &env(), |_| None), None, "{cmd}");
        }
        let uv = Env { uv: true, ..env() };
        let Some(Pre::Advise { reason, .. }) = plan("pip install pytest", &uv, |_| None) else { panic!() };
        assert!(reason.contains("uv pip install --python /w/click/.venv/bin/python <package>"), "{reason}");
    }

    /// Review S4: a rewrite that would lose the person's allow rule is advice instead.
    #[test]
    fn permission_rules_are_matched_as_claude_code_does() {
        assert!(rule_allows("Bash(pytest:*)", "pytest -q"));
        assert!(rule_allows("Bash(pytest:*)", "pytest"));
        assert!(!rule_allows("Bash(pytest:*)", "pytest-cov"));
        assert!(rule_allows("Bash(python -m pytest *)", "python -m pytest tests"));
        assert!(rule_allows("Bash", "anything"));
        assert!(rule_allows("Bash(*)", "anything"));
        assert!(rule_allows("Bash(make test)", "make test") && !rule_allows("Bash(make test)", "make test2"));
        assert!(!rule_allows("Read(**)", "pytest"));
        let segs = vec![("pytest -q".to_string(), "/w/click/.venv/bin/python -m pytest -q".to_string())];
        let allow = |r: &[&str]| r.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert!(!keeps_permissions(&segs, &allow(&["Bash(pytest:*)"]), Some("default")), "allowed as written only");
        assert!(keeps_permissions(&segs, &allow(&["Bash(pytest:*)", "Bash(/w/click/.venv/bin/python:*)"]), None));
        assert!(keeps_permissions(&segs, &allow(&["Bash(git:*)"]), Some("default")), "it would prompt either way");
        assert!(!keeps_permissions(&segs, &allow(&["Bash(git:*)"]), Some("dontAsk")), "dontAsk denies it");
        assert!(keeps_permissions(&segs, &allow(&["Bash(pytest:*)"]), Some("bypassPermissions")));
    }
}
