import unittest

import bash_action_audit as audit


def call(command, identity="tool-1"):
    return {"id":identity,"name":"Bash","input":{"command":command}}


class BashActionAuditTests(unittest.TestCase):
    def category(self, command, trusted=True):
        return audit.classify_bash_action(call(command), trusted_inspection_environment=trusted)["classification"]

    def test_direct_verification_attempts_are_definite_without_environment_attestation(self):
        for command in ("pytest", ".venv/bin/pytest -q tests", "python -m pytest tests", ".venv/bin/python -m pytest --quiet",
                        "python3.12 -m unittest discover", "python -m doctest file.py", "cargo test", "go test ./...",
                        "npm test", "yarn test", "pnpm test", "bun test", "make check"):
            with self.subTest(command=command):
                self.assertEqual(self.category(command, False), audit.VERIFICATION)

    def test_wrappers_compounds_substitutions_and_code_are_never_inspection(self):
        for command in (
            "env pytest tests", "timeout 30 pytest", "command pytest", "uv run pytest", "sh -c 'pytest tests'",
            "python -c 'import pytest; pytest.main()'", "python -c 'import subprocess; subprocess.run([\"pytest\"])'",
            "python -c 'import os; os.system(\"pytest\")'", "python check.py", "./check.sh", "bash checks.sh",
            "make", "make build", "npm run check", "node check.js", "source checks.sh", "eval testcmd", "$CHECK",
            "ls; pytest", "pwd && pytest", "ls | sh", "cat <(pytest)", "cat `pytest`", "cat $(pytest)",
            "cat \"$(pytest)\"", "cat ${CHECK}", "cat $'x'", "X=pytest ls", "alias ls=pytest", "function ls() { pytest; }",
            "ls > result", "ls\npytest", "ls # comment", "ls *.py", "ls [ab]", "ls !(x)", "ls &", "ls || pytest",
        ):
            with self.subTest(command=command):
                self.assertEqual(self.category(command), audit.UNKNOWN)

    def test_executable_flags_or_git_overrides_cannot_masquerade_as_reads(self):
        for command in (
            "rg --pre pytest x", "rg --pre=pytest x", "rg --pre-glob='*' x", "rg -z x", "rg --search-zip x",
            "rg --hostname-bin=pytest x", "rg --config x", "grep --mystery x", "find . -exec pytest \\;",
            "find . -execdir pytest +", "find . -ok pytest \\;", "find . -delete", "find . -fprint out",
            "find . -fprintf out '%p'", "find . -files0-from input", "git -c core.pager=pytest diff",
            "git --exec-path=/tmp status", "git --paginate diff", "git diff --ext-diff", "git diff --textconv",
            "git diff --output=out", "git status --whatever", "tail -f file", "/tmp/ls", "./rg x", "busybox ls",
        ):
            with self.subTest(command=command):
                self.assertEqual(self.category(command), audit.UNKNOWN)

    def test_only_enumerated_simple_inspection_is_admitted_under_attested_context(self):
        for command in (
            "pwd", "pwd -P", "ls -la src", "/bin/ls -1 tests", "cat -- README.md", "head -n 40 src/file.py",
            "tail -n20 src/file.py", "wc -l src/file.py", "grep -n -e pytest README.md", "grep -nE 'pytest|unittest' README.md",
            "rg --no-config -n pytest src", "rg --files src", "rg -g '*.py' 'pytest.main' .", "rg '$(pytest)' README.md",
            "find . -maxdepth 2 -name '*.py' -type f -print", "git --no-pager status --short",
            "git diff --no-ext-diff --no-textconv --stat", "git diff HEAD~1 -- src/file.py", "git status --porcelain=v1",
        ):
            with self.subTest(command=command):
                self.assertEqual(self.category(command), audit.INSPECTION)
                self.assertEqual(self.category(command, False), audit.UNKNOWN)

    def test_malformed_commands_and_tool_input_block(self):
        for command in (None, [], {}, "", "  ", "ls '", 'cat "unterminated', "ls \\", "ls\x00", "x"*32769):
            self.assertEqual(self.category(command), audit.UNKNOWN)
        self.assertEqual(audit.classify_bash_action(call("ls", ""), trusted_inspection_environment=True)["classification"], audit.UNKNOWN)
        with self.assertRaises(ValueError):
            audit.classify_bash_action({"name":"Read","input":{"file_path":"pytest"}})

    def test_missing_option_values_and_unrecognized_options_block(self):
        for command in ("head -n", "head -nabc file", "rg --glob", "grep -e", "find . -name", "find . -maxdepth all",
                        "git diff --unified=bad", "git status --porcelain=anything", "pwd extra", "ls --unknown"):
            self.assertEqual(self.category(command), audit.UNKNOWN)

    def test_audit_preserves_original_identity_reason_and_every_unknown_blocker(self):
        actions = [call("ls", "read"), call("python check.py", "unknown"), call("pytest", "test"),
                   {"id":"read-tool","name":"Read","input":{"file_path":"pytest.ini"}}]
        result = audit.audit_bash_actions(actions, trusted_inspection_environment=True)
        self.assertEqual([r["tool_id"] for r in result["actions"]], ["read","unknown","test"])
        self.assertEqual([r["tool_id"] for r in result["blocking_actions"]], ["unknown","test"])
        self.assertEqual(result["actions"][1]["command"], "python check.py")
        self.assertTrue(all(r["reason"] for r in result["actions"]))
        self.assertEqual(len(audit.audit_bash_actions(actions)["blocking_actions"]), 3)
        self.assertEqual(audit.audit_bash_actions([])["blocking_actions"], [])


if __name__ == "__main__":
    unittest.main()
