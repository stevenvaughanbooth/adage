//! `PreToolUse` delivery for U11 (the project's environment): before a shell command runs, a test
//! run or scratch repro under the system Python is pointed at the project's own interpreter, and a
//! `pip install` into the system Python is refused with the command that reaches the project.
//!
//! Only the interpreter prefix changes: `pytest -x tests/a.py` becomes
//! `<root>/.venv/bin/python -m pytest -x tests/a.py`, `python -m pytest …` and `python3 repro.py`
//! keep every argument. The environment was verified from its files when the instincts were bound
//! (`adage_instincts::verify_env`: pytest installed, the project importable from this checkout),
//! never here. A command this reader cannot place (a variable, a substitution in command position)
//! is left alone. The replay metric of this lever is `adage eval replay-savings --mode
//! pre-emptive`.

/// The project's verified environment, as the hook hands it over.
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

/// What to do before the command runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pre {
    /// Run this command instead (the interpreter prefix swapped); `kind` is `test` or `repro`.
    Rewrite { command: String, kind: &'static str },
    /// Refuse it, with the reason the agent reads.
    Deny { reason: String },
}

/// Where a word starts a simple command (after an operator or at the start).
fn command_starts(s: &str) -> Vec<(usize, usize)> {
    // (start, end) byte ranges of the first word of each simple command, outside quotes, after
    // any leading environment assignments.
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    let mut at_start = true;
    let mut quote: Option<u8> = None;
    while i < b.len() {
        let c = b[i];
        if let Some(q) = quote {
            if c == b'\\' && q == b'"' {
                i += 2;
                continue;
            }
            if c == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        match c {
            b'\'' | b'"' => {
                quote = Some(c);
                at_start = false;
                i += 1;
            }
            b'\\' => {
                i += 2;
                at_start = false;
            }
            b';' | b'&' | b'|' | b'(' | b'\n' | b'{' => {
                at_start = true;
                i += 1;
            }
            b' ' | b'\t' => i += 1,
            _ => {
                let start = i;
                while i < b.len() && !b" \t\n;&|()<>'\"".contains(&b[i]) {
                    i += 1;
                }
                if i == start {
                    // A redirection or a closing parenthesis: not a word.
                    i += 1;
                    at_start = false;
                    continue;
                }
                let word = &s[start..i];
                if at_start {
                    let assignment = word
                        .find('=')
                        .is_some_and(|e| e > 0 && word[..e].chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
                    if assignment {
                        continue; // still at the command's start
                    }
                    if matches!(word, "then" | "do" | "else" | "time" | "exec") {
                        continue;
                    }
                    out.push((start, i));
                }
                at_start = false;
            }
        }
    }
    out
}

/// The words after position `from` up to the end of its simple command (unquoted where quoted).
fn args_after(s: &str, from: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut have = false;
    for c in s[from..].chars() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                have = true;
            }
            (None, ' ' | '\t') => {
                if have || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    have = false;
                }
            }
            (None, ';' | '&' | '|' | '\n' | ')' | '<' | '>') => break,
            (None, c) => cur.push(c),
        }
    }
    if have || !cur.is_empty() {
        out.push(cur);
    }
    out
}

fn is_system_python(w: &str) -> bool {
    w.strip_prefix("python").is_some_and(|v| v.chars().all(|c| c.is_ascii_digit() || c == '.'))
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

/// Plan a `PreToolUse` for a shell command. `read` reads a script the command runs (relative to
/// the session's working directory), for a repro's imports; it may return `None`.
pub fn plan(command: &str, env: &Env, read: impl Fn(&str) -> Option<String>) -> Option<Pre> {
    // An activated environment (`source .venv/bin/activate && python …`) already runs the
    // project's interpreter: nothing to rewrite, nothing to refuse.
    if command.len() > 8_000 || command.contains("bin/activate") {
        return None;
    }
    let mut edits: Vec<(usize, usize, String)> = Vec::new();
    let mut kind = "";
    for (s, e) in command_starts(command) {
        let word = &command[s..e];
        let args = args_after(command, e);
        let first = args.first().map(String::as_str);
        let second = args.get(1).map(String::as_str);
        // pip into the system Python: refused, with the command that reaches the project.
        let pip = matches!(word, "pip" | "pip3") && first == Some("install")
            || is_system_python(word)
                && first == Some("-m")
                && second == Some("pip")
                && args.get(2).is_some_and(|a| a == "install");
        if pip {
            return Some(Pre::Deny {
                reason: format!(
                    "This repository's Python environment is {shown} (the project and its test dependencies are installed there); installing into the system Python does not reach it. If a package is really missing, install it into the environment: `{shown} -m pip install ...` (or `uv pip install ...`).",
                    shown = env.shown
                ),
            });
        }
        if matches!(word, "pytest" | "py.test") && env.pytest {
            edits.push((s, e, format!("{} -m pytest", env.interpreter)));
            kind = "test";
            continue;
        }
        if !is_system_python(word) {
            continue;
        }
        let test = first == Some("-m") && second == Some("pytest") && env.pytest;
        let repro = !env.modules.is_empty()
            && match first {
                Some("-c") => second.is_some_and(|c| imports(c, &env.modules)),
                Some("-m") => {
                    second.is_some_and(|m| env.modules.iter().any(|x| Some(x.as_str()) == m.split('.').next()))
                }
                // A heredoc (`python3 - <<'EOF'`, `python3 <<EOF`): its body follows.
                Some("-") | None if command[e..].contains("<<") => imports(&command[e..], &env.modules),
                Some(script) if script.ends_with(".py") && !script.starts_with('-') => {
                    read(script).is_some_and(|code| imports(&code, &env.modules))
                }
                _ => false,
            };
        if test || repro {
            edits.push((s, e, env.interpreter.clone()));
            if kind != "test" {
                kind = if test { "test" } else { "repro" };
            }
        }
    }
    if edits.is_empty() {
        return None;
    }
    let mut out = command.to_string();
    for (s, e, with) in edits.into_iter().rev() {
        out.replace_range(s..e, &with);
    }
    Some(Pre::Rewrite { command: out, kind })
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
        }
    }

    fn rw(cmd: &str) -> Option<String> {
        match plan(cmd, &env(), |_| None) {
            Some(Pre::Rewrite { command, .. }) => Some(command),
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
        let with_script = plan("python repro.py", &env(), |p| (p == "repro.py").then(|| "import click\n".into()));
        assert!(
            matches!(with_script, Some(Pre::Rewrite { ref command, kind: "repro" }) if command == "/w/click/.venv/bin/python repro.py")
        );
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
        ] {
            assert_eq!(rw(cmd), None, "{cmd}");
        }
        // Without pytest in the environment, a test run is not rewritten.
        let no_pytest = Env { pytest: false, ..env() };
        assert_eq!(plan("pytest tests", &no_pytest, |_| None), None);
    }

    #[test]
    fn pip_into_the_system_python_is_refused() {
        for cmd in [
            "pip install pytest",
            "pip3 install -e .",
            "python -m pip install hypothesis",
            "cd x && pip install -q pytest",
        ] {
            let Some(Pre::Deny { reason }) = plan(cmd, &env(), |_| None) else { panic!("{cmd}") };
            assert!(reason.contains(".venv/bin/python -m pip install"), "{reason}");
        }
        assert_eq!(plan("pip list", &env(), |_| None), None);
        assert_eq!(plan(".venv/bin/python -m pip install x", &env(), |_| None), None);
    }
}
