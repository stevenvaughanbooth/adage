use adage_core::shell::{observational_commands, MAX_COMMAND};

#[test]
fn literal_readers_and_discovery_lists_are_observational_evidence() {
    for command in [
        "ls -la",
        "/bin/ls -la .venv .venv/bin",
        "/usr/bin/head -n 30 README.md | /usr/bin/wc -l",
        "ls .venv/bin 2>/dev/null | head -30",
        "ls .venv/bin 2> /dev/null | head -n 30",
        "cat README.md 2>&1 | tail -5",
        "pwd; ls -la && cat 'file with spaces' || ls .venv/bin",
        "cat Cargo.toml src/lib.rs | wc -l",
        "head -c 80 README.md; tail -n 5 README.md",
        "cat -- -filename",
        "pwd -P\nls .venv/bin",
        "ls -la; ls -la .venv* venv* 2>/dev/null; ls .claude .devcontainer",
        "/bin/ls src/*.py .venv?/bin ./?",
        "ls 'literal*glob'",
    ] {
        assert!(observational_commands(command), "{command}");
    }
}

#[test]
fn mutation_opaque_work_and_shell_expansion_are_not_observational_evidence() {
    for command in [
        "cat source.py > target.py",
        "cat source.py >> target.py",
        "cat source.py 2> errors.txt",
        "cat source.py >/dev/null",
        "cat source.py 1>&2",
        "cat source.py < input.txt",
        "cat <<'EOF'\ntext\nEOF",
        "rg --pre program needle .",
        "find . -exec program {} \\;",
        "cat $(program)",
        "cat `program`",
        "cat <(program)",
        "cat $FILE",
        "ls *.py",
        "cat {a,b}.py",
        "ls ~/src",
        "SETTING=1 ls -la",
        "export SETTING=1; ls",
        "cd src && ls",
        "sh -c 'ls -la'",
        "./cat source.py",
        "/custom/bin/cat source.py",
        "ls; program",
        "ls &",
        "(ls)",
        "{ ls; }",
        "ls # comment",
        "ls |& head",
        "head -n source.py",
        "head -n",
        "head --unknown source.py",
        "wc --files0-from=names",
        "pwd source.py",
        "cat /proc/self/mem",
        "cat /dev/stdin",
        "cat /sys/kernel/notes",
        "ls *",
        "ls -l*",
        "ls ?",
        "ls .venv*/$(program)",
        "ls .venv*${NAME}",
        "ls .venv* > listing.txt",
        "cat src/*.py",
        "ls /dev/std*",
        "ls /proc/*",
        "",
        ";",
    ] {
        assert!(!observational_commands(command), "{command}");
    }
    assert!(!observational_commands(&format!("ls {}", "a".repeat(MAX_COMMAND))));
}
