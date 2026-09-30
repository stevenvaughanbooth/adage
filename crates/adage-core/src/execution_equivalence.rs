//! Narrow equivalence for successfully observed, direct project-Python pytest commands.
//!
//! Only pytest verbosity is erased. Environment, runner, targets, selectors, other options and
//! their order remain exact. Callers must separately establish the same repository/cwd, a real
//! successful outcome, and safe raw command provenance; this key is neither execution authority
//! nor a shell interpreter. Unknown option arity fails closed rather than erasing its value.

/// An equivalence key for plain argv in the same repository working-directory context.
pub fn project_pytest_key(command: &str) -> Option<Vec<String>> {
    if command.is_empty()
        || command.len() > 200
        || !command.chars().all(|c| c.is_ascii_alphanumeric() || " _./:=@,+-".contains(c))
    {
        return None;
    }
    let argv: Vec<&str> = command.split(' ').collect();
    if argv.iter().any(|a| a.is_empty() || a.starts_with('/') || a.split('/').any(|p| p == ".."))
        || argv.len() < 3
        || !crate::action::is_python(argv[0])
        || crate::action::venv_of(argv[0]).is_none()
        || argv[1..3] != ["-m", "pytest"]
    {
        return None;
    }
    let mut key: Vec<String> = argv[..3].iter().map(|s| s.to_string()).collect();
    let mut i = 3;
    while i < argv.len() {
        let arg = argv[i];
        i += 1;
        if arg == "--" {
            key.push(arg.into());
            key.extend(argv[i..].iter().map(|s| s.to_string()));
            break;
        }
        let repeated = arg
            .strip_prefix('-')
            .is_some_and(|s| !s.is_empty() && (s.bytes().all(|c| c == b'q') || s.bytes().all(|c| c == b'v')));
        if repeated || matches!(arg, "--quiet" | "--verbose") {
            continue;
        }
        key.push(arg.into());
        if matches!(
            arg,
            "-k" | "-m"
                | "-n"
                | "-p"
                | "-c"
                | "-o"
                | "--rootdir"
                | "--maxfail"
                | "--tb"
                | "--junitxml"
                | "--override-ini"
                | "--deselect"
                | "--ignore"
                | "--ignore-glob"
        ) {
            key.push((*argv.get(i)?).into());
            i += 1;
        } else if arg.starts_with('-')
            && !arg.contains('=')
            && !matches!(arg, "-x" | "-s" | "--exitfirst" | "--disable-warnings" | "--collect-only" | "--co")
        {
            return None;
        }
    }
    Some(key)
}

/// Only a recognized pair can be equivalent; two unrecognized commands are not evidence.
pub fn project_pytest_equivalent(a: &str, b: &str) -> bool {
    project_pytest_key(a).is_some_and(|key| project_pytest_key(b).is_some_and(|other| key == other))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_output_verbosity_disappears() {
        let base = ".venv/bin/python -m pytest tests/test_a.py -x";
        for flags in ["-q", "-qqq", "-v", "-vv", "--quiet", "--verbose"] {
            assert!(project_pytest_equivalent(base, &format!("{base} {flags}")));
        }
        for other in [
            ".venv/bin/python -m pytest tests/test_b.py -x",
            ".venv/bin/python -m pytest tests/test_a.py",
            ".venv/bin/python -m pytest tests/test_a.py -s",
            ".venv/bin/python -m pytest tests/test_a.py -x -k one",
            "other/bin/python -m pytest tests/test_a.py -x",
            ".venv/bin/python3 -m pytest tests/test_a.py -x",
            "python -m pytest tests/test_a.py -x",
            ".venv/bin/python -m pytest -x tests/test_a.py",
            ".venv/bin/python -m pytest tests/test_a.py -x --rootdir other",
        ] {
            assert!(!project_pytest_equivalent(base, other), "{other}");
        }
    }

    #[test]
    fn option_values_compounds_and_unknown_arity_are_not_verbosity() {
        assert!(!project_pytest_equivalent(".venv/bin/python -m pytest -k -q", ".venv/bin/python -m pytest -k -v"));
        assert!(!project_pytest_equivalent(".venv/bin/python -m pytest -m -q", ".venv/bin/python -m pytest -m -v"));
        assert!(!project_pytest_equivalent(".venv/bin/python -m pytest -- -q", ".venv/bin/python -m pytest -- -v"));
        for command in [
            "source .venv/bin/activate && python -m pytest",
            ".venv/bin/python -m pytest | tail",
            ".venv/bin/python -m pytest && true",
            ".venv/bin/python -m pytest; true",
            ".venv/bin/python -m pytest --plugin-option -q",
            ".venv/bin/python -m pytest {tests}",
            ".venv/bin/python -m pytest ../tests",
            ".venv/bin/python -m pytest -k",
        ] {
            assert!(project_pytest_key(command).is_none(), "{command}");
        }
    }
}
