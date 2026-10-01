import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import probe


class FixtureTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="reflex-offline-test-")
        self.root = Path(self.temp.name)
        (self.root / ".reflex-fixture").write_text("test")
        self.payload = {"hook_event_name": "UserPromptSubmit", "session_id": "offline", "prompt": "fixture"}

    def tearDown(self):
        self.temp.cleanup()

    def start(self, delay=0):
        child = subprocess.Popen([sys.executable, "-B", str(probe.HERE / "fixture_hook.py"), str(self.root), str(delay)],
                                 stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True)
        child.stdin.write(json.dumps(self.payload))
        child.stdin.close()
        child.stdin = None
        return child

    def events(self):
        path = self.root / "events.jsonl"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def test_real_subprocess_order_and_receipt(self):
        child = self.start()
        out, err = child.communicate(timeout=5)
        self.assertEqual(child.returncode, 0, err)
        self.assertIn("REFLEX_FIXTURE_OK", out)
        events = self.events()
        times = {item["kind"]: item["monotonic_ns"] for item in events}
        for left, right in zip(["hook_enter", "script_start", "script_end", "receipt"],
                               ["script_start", "script_end", "receipt", "hook_exit"]):
            self.assertLess(times[left], times[right])
        self.assertEqual((self.root / "effects.txt").read_text(), "fixture-effect\n")

    def test_concurrent_duplicate_runs_once(self):
        a, b = self.start(.1), self.start(.1)
        for child in (a, b):
            _out, err = child.communicate(timeout=5)
            self.assertEqual(child.returncode, 0, err)
        self.assertEqual((self.root / "effects.txt").read_text(), "fixture-effect\n")
        self.assertEqual(sum(item["kind"] == "duplicate_suppressed" for item in self.events()), 1)

    def test_sigterm_kills_fixture_child_and_leaves_unknown(self):
        child = self.start(10)
        deadline = time.monotonic() + 3
        while not any(item["kind"] == "script_start" for item in self.events()):
            if time.monotonic() > deadline:
                child.kill()
                child.communicate()
                self.fail("fixture child did not start")
            time.sleep(.01)
        child.terminate()
        child.communicate(timeout=3)
        self.assertFalse((self.root / "effects.txt").exists())
        receipt = next(item for item in self.events() if item["kind"] == "receipt")
        self.assertEqual(receipt["state"], "unknown")
        retry = self.start()
        retry.communicate(timeout=3)
        self.assertFalse((self.root / "effects.txt").exists(), "unknown action must not retry")

    def test_message_ids_are_not_request_counts(self):
        value = {"type": "assistant", "message": {"id": "msg_123"}}
        self.assertEqual(probe.explicit_request_ids(value), [])
        self.assertEqual(probe.explicit_request_ids({"request_id": "req_real", "message": value}), ["req_real"])

    def test_missing_accounting_is_unknown_not_zero(self):
        self.assertIsNone(probe.summary(self.root)["total_cost_usd"])
        (self.root / "stream-observations.jsonl").write_text(json.dumps({"value": {
            "type": "result", "subtype": "success", "total_cost_usd": .02, "num_turns": 3,
            "usage": {"input_tokens": 123}}}) + "\n")
        result = probe.summary(self.root)
        self.assertEqual(result["total_cost_usd"], .02)
        self.assertEqual(result["num_turns"], 3)
        self.assertIsNone(result["api_request_count"])

    def test_live_refuses_nested_marker_before_reading_manifest(self):
        with patch.dict(os.environ, {"CLAUDECODE": "1"}):
            with self.assertRaisesRegex(ValueError, "Nested Claude"):
                probe.execute(self.root / "missing.json", 1, 10)

    @unittest.skipIf(bool(os.environ.get("CLAUDECODE")), "preserve nested-session guard")
    def test_launcher_stdin_cannot_become_extra_model_input(self):
        # A real child process models print-mode reading stdin; no provider is contacted.
        fake = ("import json,sys; print(json.dumps({'type':'result','subtype':'success',"
                "'is_error':False,'total_cost_usd':0,'result':sys.stdin.read()}))")
        manifest = self.root / "manifest.json"
        # The launch check reads --settings; deny every name the launch may pass, as native settings do.
        settings = self.root / "settings.json"
        settings.write_text(json.dumps({"sandbox": {"credentials": {"envVars": [
            {"name": n, "mode": "deny"} for n in probe.DENIED_ENV]}}}))
        manifest.write_text(json.dumps(dict(root=str(self.root), budget_usd=1,
            expected_claude_version=probe.EXPECTED_VERSION,
            argv=[sys.executable, "-c", fake, "--max-budget-usd", "1", "--settings", str(settings)])))
        launcher = ("import sys; from pathlib import Path; from unittest.mock import patch; "
            f"sys.path.insert(0,{str(probe.HERE)!r}); import probe\n"
            f"with patch('probe.subprocess.check_output',return_value={probe.EXPECTED_VERSION!r}):\n"
            f" probe.execute(Path({str(manifest)!r}),1,5)\n")
        result = subprocess.run([sys.executable, "-c", launcher], input="UNINTENDED LAUNCHER INSTRUCTIONS",
            text=True, capture_output=True, timeout=10)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(probe.summary(self.root)["result_text"], "")


class LaunchEnvironment(unittest.TestCase):
    def test_extra_secret_in_the_launching_shell_never_reaches_claude(self):
        shell = {"PATH": "/usr/bin:/bin", "HOME": "/tmp", "GITHUB_TOKEN": "x", "MY_SERVICE_PASSWORD": "y",
                 "ANTHROPIC_API_KEY": "z", "PYTHONPATH": "/evil"}
        env = probe.launch_env(shell)
        self.assertEqual(set(env), {"PATH", "HOME", "ANTHROPIC_API_KEY", probe.SENTINEL})
        # A real child process sees exactly the constructed names.
        seen = subprocess.run(["/usr/bin/env"], env=env, capture_output=True, text=True, check=True).stdout
        names = {line.split("=", 1)[0] for line in seen.splitlines()}
        self.assertEqual(names, set(env))
        self.assertNotEqual(probe.launch_env(shell)[probe.SENTINEL], env[probe.SENTINEL])

    def test_launch_refuses_settings_that_do_not_deny_a_passed_credential(self):
        env = probe.launch_env({"PATH": "/usr/bin", "CLAUDE_CODE_OAUTH_TOKEN": "x"})
        deny = lambda names: {"sandbox": {"credentials": {"envVars": [{"name": n, "mode": "deny"} for n in names]}}}
        self.assertIn("PATH", probe.check_launch(env, deny(probe.DENIED_ENV)))
        with self.assertRaises(ValueError):
            probe.check_launch(env, deny([probe.SENTINEL]))
        with self.assertRaises(ValueError):
            probe.check_launch(env, {})


class RuntimeInjectedNames(unittest.TestCase):
    OBSERVED = ["CLAUDECODE", "CLAUDE_CODE_MESSAGING_SOCKET", "CLAUDE_CODE_MESSAGING_TOKEN", "CLOUDSDK_PROXY_PASSWORD",
                "CLOUDSDK_PROXY_USERNAME", "HTTPS_PROXY", "PATH"]

    def test_exemptions_are_named_and_conditional_on_a_passing_proof(self):
        proven = {"inbox": True, "proxy_denies_external": True}
        self.assertEqual(probe.unexplained_credentials(self.OBSERVED, proven), [])
        self.assertEqual(probe.unexplained_credentials(self.OBSERVED, {"inbox": True}), ["CLOUDSDK_PROXY_PASSWORD"])
        self.assertEqual(probe.unexplained_credentials(self.OBSERVED, {"proxy_denies_external": True}),
                         ["CLAUDE_CODE_MESSAGING_TOKEN"])
        # Host secrets, the sentinel and look-alike names are never exempt.
        for name in ("ANTHROPIC_API_KEY", "GITHUB_TOKEN", "CLAUDE_CODE_OAUTH_TOKEN", "CLOUDSDK_PROXY_PASSWORD_2",
                     "MY_MESSAGING_TOKEN", "SSH_AUTH_SOCK"):
            self.assertEqual(probe.unexplained_credentials([name], proven), [name])
        self.assertTrue(set(probe.RUNTIME_INJECTED) <= set(probe.DENIED_ENV))
        self.assertIn("CLAUDE_CODE_MESSAGING_SOCKET", probe.DENIED_ENV)
        self.assertNotIn(probe.SENTINEL, probe.RUNTIME_INJECTED)


class NativeCanaryOutput(unittest.TestCase):
    """Built from the archived launch-gate-v3 tool result: an exit-code prefix, the two canary lines,
    then a <sandbox_violations> block that Claude Code appends. Fixture, not confinement evidence."""
    FIXTURE = Path(__file__).parent / "results/20260930-launch-canary/canary-v4-aborted/native-canary-tool-result.json"

    def fixture(self):
        return json.loads(self.FIXTURE.read_text())

    def test_parser_tolerates_prefixes_and_appended_blocks(self):
        content = self.fixture()["content"]
        self.assertTrue(content.startswith("Exit code 1\n") and content.rstrip().endswith("</sandbox_violations>"))
        with self.assertRaises(json.JSONDecodeError):
            json.loads(content.split("NATIVE_SANDBOX_CHECKS=", 1)[1])  # The original parser's failure.
        names, checks = probe.canary_output(content)
        self.assertIn("CLOUDSDK_PROXY_PASSWORD", names)
        self.assertIs(checks["inbox"], True)
        names, _ = probe.canary_output([{"type": "text", "text": content}])
        self.assertIn("PATH", names)
        for bad in (content + "\nNATIVE_SANDBOX_CHECKS={}", content.replace("NATIVE_ENV_NAMES=", "X="),
                    content.replace("NATIVE_ENV_NAMES=[", "NATIVE_ENV_NAMES={")):
            with self.assertRaises((ValueError, json.JSONDecodeError)):
                probe.canary_output(bad)

    def test_dry_run_on_the_archived_shape_passes_only_with_the_proven_exemption(self):
        content = self.fixture()["content"]
        verdict = probe.assess_canary(content)
        self.assertTrue(verdict["passed"], verdict)
        self.assertEqual(verdict["tolerated_runtime_names"], ["CLOUDSDK_PROXY_PASSWORD"])
        self.assertEqual((verdict["visible_denied_names"], verdict["unexplained_credential_names"]), ([], []))
        unproven = probe.assess_canary(content.replace('"proxy_denies_external": true', '"proxy_denies_external": false'))
        self.assertFalse(unproven["passed"])
        self.assertEqual(unproven["visible_denied_names"], ["CLOUDSDK_PROXY_PASSWORD"])
        self.assertEqual(unproven["unexplained_credential_names"], ["CLOUDSDK_PROXY_PASSWORD"])
        leaked = probe.assess_canary(content.replace('"PATH", ', '"PATH", "GITHUB_TOKEN", "' + probe.SENTINEL + '", '))
        self.assertFalse(leaked["passed"])
        self.assertEqual(leaked["visible_denied_names"], [probe.SENTINEL, "GITHUB_TOKEN"])
        no_inbox = probe.assess_canary(content.replace('"inbox": true', '"inbox": false'))
        self.assertEqual(no_inbox["failed_checks"], ["inbox"])

    def test_canary_program_applies_the_same_exemption_rule(self):
        import real_repo_gate as gate
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            work = root / "repo"
            for sub in (".venv", ".git"):
                (work / sub).mkdir(parents=True)
            (work / "native_canary.py").write_text(gate.native_canary_source(root / "p", root / "o", 9, root / "i.sock"))
            def run(extra):
                out = subprocess.run([sys.executable, "native_canary.py"], cwd=work, capture_output=True, text=True,
                                     env={"PATH": "/usr/bin:/bin", "CLOUDSDK_PROXY_PASSWORD": "x", **extra})
                return probe.canary_output(out.stdout)[1]
            # A closed local proxy refuses the external request, which proves the proxy credential grants nothing.
            proven = run({"HTTP_PROXY": "http://127.0.0.1:9"})
            self.assertIs(proven["proxy_denies_external"], True)
            self.assertIs(proven["env_denied_names_absent"], True)
            self.assertIs(run({})["env_denied_names_absent"], False)
            self.assertIs(run({"HTTP_PROXY": "http://127.0.0.1:9", "GITHUB_TOKEN": "t"})["env_denied_names_absent"], False)


if __name__ == "__main__":
    unittest.main()
