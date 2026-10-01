import json
from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch

import probe as gate_probe
import real_repo_gate as gate


class PreparationTests(unittest.TestCase):
    def test_profile_is_allowlisted_and_never_grants_parent_eval_data(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            p = gate.sandbox_profile(root / "repo", root / "scratch", root / "python")
            self.assertIn("(deny default)", p)
            self.assertIn("(deny network*)", p)
            self.assertNotIn('(allow file-read*)', p)
            self.assertNotIn('(subpath ' + json.dumps(str(root)) + ')', p)
            self.assertIn('(subpath ' + json.dumps(str(root / "repo")) + ')', p)
            self.assertNotIn('(subpath "/private/tmp")', p)
            self.assertNotIn('(subpath "/tmp")', p)

    def test_native_credentials_denial_is_fixed_and_independent_of_the_freezing_shell(self):
        freeze = {name: "not-read" for name in ("CLAUDECODE", "CODEX_SANDBOX", "CLAUDE_CODE_MESSAGING_TOKEN", "GOCACHE")}
        other = {"HOME": "/Users/person", "PATH": "/usr/bin", "GITHUB_TOKEN": "not-read", "ANTHROPIC_API_KEY": "not-read"}
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            with patch.dict("os.environ", freeze, clear=True):
                settings = gate.native_settings(root, root / "repo", ["Read"])
            with patch.dict("os.environ", other, clear=True):
                self.assertEqual(gate.native_settings(root, root / "repo", ["Read"]), settings)
                # A different launching shell still confines: every passed credential name is denied.
                names = gate_probe.check_launch(gate_probe.launch_env(), settings)
            self.assertNotIn("GITHUB_TOKEN", names)
            self.assertIn("ANTHROPIC_API_KEY", names)
            scrubbed = {x["name"] for x in settings["sandbox"]["credentials"]["envVars"]}
            self.assertEqual(scrubbed, set(gate_probe.DENIED_ENV))
            self.assertTrue({"ANTHROPIC_API_KEY", "CLAUDE_CODE_OAUTH_TOKEN", gate_probe.SENTINEL} <= scrubbed)
            self.assertNotIn("not-read", json.dumps(settings))
            self.assertTrue(settings["sandbox"]["failIfUnavailable"])
            self.assertFalse(settings["sandbox"]["allowUnsandboxedCommands"])
            self.assertFalse(settings["sandbox"]["autoAllowBashIfSandboxed"])

    def test_native_exact_protected_file_and_directory_are_denied(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d).resolve()
            script = root / "repo/native_canary.py"
            settings = gate.native_settings(root, root / "repo", ["Read"], [script])
            self.assertIn(f"Edit(/{script})", settings["permissions"]["deny"])
            self.assertIn(str(script), settings["sandbox"]["filesystem"]["denyWrite"])
            self.assertIn(str(root), settings["sandbox"]["filesystem"]["denyRead"])
            self.assertNotIn(str(root), settings["sandbox"]["filesystem"]["allowRead"])

    def test_missing_or_ambiguous_native_init_identity_never_searches_home(self):
        with tempfile.TemporaryDirectory() as d:
            session = Path(d)
            for values in ([], [{"type":"system","subtype":"init","session_id":"bad"}],
                           [{"type":"system","subtype":"init","session_id":"a" * 36},
                            {"type":"system","subtype":"init","session_id":"b" * 36}]):
                (session / "stdout.jsonl").write_text("\n".join(json.dumps(x) for x in values))
                with patch.object(gate.shutil, "copyfile") as copy:
                    with self.assertRaises(ValueError):
                        gate.retain_transcript(session, session / "repo")
                    copy.assert_not_called()

    def test_environment_does_not_forward_host_credentials_or_python_injection(self):
        with tempfile.TemporaryDirectory() as d, patch.dict("os.environ", {"API_KEY": "secret", "PYTHONPATH": "/evil"}):
            env = gate.environment(Path(d))
            self.assertNotIn("API_KEY", env)
            self.assertNotIn("PYTHONPATH", env)
            self.assertEqual(env["PYTHONNOUSERSITE"], "1")
            self.assertTrue(env["HOME"].startswith(d))

    def test_failed_attempt_is_preserved_and_never_records_paid_calls(self):
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            with self.assertRaises(RuntimeError):
                gate.command(root, "intentional-failure", ["/bin/sh", "-c", "exit 7"])
            records = [json.loads(s) for s in (root / "attempts.jsonl").read_text().splitlines()]
            self.assertEqual([r["status"] for r in records], ["started", "failed"])
            self.assertEqual(records[-1]["returncode"], 7)
            self.assertTrue(all(r["paid_calls"] == 0 for r in records))

    def test_unsupported_platform_never_runs_unconfined(self):
        with tempfile.TemporaryDirectory() as d, patch.object(gate.sys, "platform", "linux"), patch.object(gate, "command") as run:
            with self.assertRaisesRegex(RuntimeError, "never fall back"):
                gate.confined(Path(d), "test", ["python", "-m", "pytest"], Path(d))
            run.assert_not_called()


if __name__ == "__main__":
    unittest.main()
