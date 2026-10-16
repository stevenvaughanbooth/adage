//! Numeric Python versions retain their identity while direct known test modules classify alike.

use adage_core::{keys::template_key, outcome, shell::normalize_shell, CanonicalAction, CommandTable, Effect};

#[test]
fn direct_python_test_modules_classify_consistently_without_rewriting_the_command() {
    for program in ["python", "python3", "python3.11", ".venv/bin/python", ".venv/bin/python3.12"] {
        for (module, output) in [("pytest", "=== 1 passed in 0.01s ==="), ("unittest", "Ran 1 test in 0.01s\n\nOK")] {
            for startup in ["", "-B ", "-I -E -s -u "] {
                let command = format!("{program} {startup}-m {module} tests/test_example.py");
                let actions = normalize_shell(&command, "/repo", CommandTable::load());
                let [action] = actions.as_slice() else { panic!("unexpected actions: {command}") };
                assert_eq!(action.effect, Effect::Verify, "{command}");
                assert!(!action.mutating, "{command}");
                let CanonicalAction::Exec { argv, cwd } = &action.action else { panic!("not exec: {command}") };
                assert_eq!(argv, &command.split_whitespace().map(str::to_owned).collect::<Vec<_>>());
                assert_eq!(cwd, "/repo");
                assert_eq!(template_key(&action.action), format!("exec {program} {startup}-m {module} {{path}}"));
                assert_eq!(
                    outcome::verification_for_command(argv, &command, output, false).unwrap().outcome,
                    outcome::VerificationOutcome::Passed,
                    "{command}"
                );
            }
        }
    }
}

#[test]
fn a_versioned_python_name_does_not_promote_inline_code_scripts_or_unknown_modules() {
    for program in ["python3.11", ".venv/bin/python3.12"] {
        for tail in [
            "-c 'print(1)'",
            "-c 'print(1)' -m pytest",
            "-cpass -m pytest",
            "-Icpass -m unittest",
            "scripts/check.py",
            "scripts/pytest_check.py",
            "scripts/check.py -m pytest",
            "-m custom pytest",
            "-m pytest_wrapper",
            "-m unittest_extra",
            "-W error -m pytest",
            "-- -m pytest",
            "-m",
        ] {
            let command = format!("{program} {tail}");
            let actions = normalize_shell(&command, "/repo", CommandTable::load());
            let [action] = actions.as_slice() else { panic!("unexpected actions: {command}") };
            assert_eq!(action.effect, Effect::Environment, "{command}");
            assert!(!action.mutating, "{command}");
        }
    }
}

#[test]
fn unknown_shells_dynamic_heads_and_nonversion_names_do_not_gain_test_effects() {
    for command in [
        "unknown-shell -c 'python3.11 -m pytest'",
        "$PYTHON -m pytest",
        "python-wrapper -m pytest",
        "python. -m pytest",
        "python3..11 -m pytest",
        "python.3 -m pytest",
    ] {
        let actions = normalize_shell(command, "/repo", CommandTable::load());
        assert!(!actions.is_empty(), "{command}");
        assert!(actions.iter().all(|action| action.effect == Effect::Environment), "{command}: {actions:?}");
    }
    // Classification is not an automatic-approval entry or a new interpreter alias in the table.
    assert!(CommandTable::load().get("python3.11").is_none());
    assert!(!CommandTable::load().get("python").unwrap().inert);
}
