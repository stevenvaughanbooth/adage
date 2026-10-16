"""Offline regression tests for cohort isolation, permission parity and shared authority."""
import json
from pathlib import Path
import tempfile
import unittest

import cohort_v5 as cohort
import real_repo_gate as gate


class CohortBoundary(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.authority = Path(self.temp.name).resolve()
        self.root = self.authority / "cohorts/v5"
        self.session = self.root / "sessions/train"
        self.session.mkdir(parents=True)
        gate.write(self.root / "manifest.json", {"budget_authority":str(self.authority)})
        self.native = self.root / "sessions/native-confinement/manifest.json"
        checkout = self.native.parent / "repo"
        settings = cohort.policy(self.authority, checkout, self.root / "runtime/python", [checkout / "native_canary.py"])
        settings["permissions"]["allow"] = ["Read", "Bash"]
        gate.write(self.native.parent / "settings.json", settings)
        gate.write(self.native, {"protocol_revision":5, "root":str(self.native.parent), "execution_root":str(checkout),
            "settings_sha256":gate.sha(self.native.parent / "settings.json"),
            "source_sha256":gate.sha(Path(gate.__file__)),
            "shared_sources":{str(Path(cohort.__file__)):gate.sha(Path(cohort.__file__))}})
        self.review = self.root / "native-confinement-reviewed.json"
        gate.write(self.review, {"passed":True, "cohort_root":str(self.root),
                                 "manifest_sha256":gate.sha(self.native)})
        self.value = dict(root=str(self.session), experiment_root=str(self.authority), cohort_root=str(self.root),
                          native_review_path=str(self.review), native_review_sha256=gate.sha(self.review))

    def test_shared_authority_is_canonical_ancestor_not_new_cohort_budget(self):
        gate.validate_authority(self.value)
        for bad in (self.root, self.authority.parent, self.authority / "unrelated"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                gate.validate_authority(dict(self.value, experiment_root=str(bad)))

    def test_old_review_and_tampered_current_review_cannot_admit_new_training(self):
        gate.require_native_review(self.value)
        old = self.authority / "native-confinement-reviewed.json"
        gate.write(old, {"passed":True})
        with self.assertRaises(ValueError):
            gate.require_native_review(dict(self.value, native_review_path=str(old)))
        gate.write(self.review, {"passed":True, "cohort_root":str(self.root), "manifest_sha256":"old-recipe"})
        with self.assertRaises(ValueError):
            gate.require_native_review(self.value)
        gate.write(self.review, {"passed":True, "cohort_root":str(self.root),
                                "manifest_sha256":gate.sha(self.native), "changed":True})
        with self.assertRaisesRegex(ValueError, "review changed"):
            gate.require_native_review(self.value)

    def test_generic_bash_preserves_confinement_and_protected_dependencies(self):
        checkout = self.session / "repo"
        runtime = self.authority / "runtime/python"
        settings = cohort.policy(self.authority, checkout, runtime)
        self.assertIn("Bash", settings["permissions"]["allow"])
        self.assertFalse(any(rule.startswith("Bash(") for rule in settings["permissions"]["allow"]))
        sandbox = settings["sandbox"]
        self.assertTrue(sandbox["enabled"])
        self.assertTrue(sandbox["failIfUnavailable"])
        self.assertFalse(sandbox["allowUnsandboxedCommands"])
        self.assertFalse(sandbox["autoAllowBashIfSandboxed"])
        self.assertEqual(sandbox["excludedCommands"], [])
        self.assertIn(str(self.authority), sandbox["filesystem"]["denyRead"])
        self.assertEqual(sandbox["filesystem"]["allowRead"], [str(checkout), str(runtime)])
        self.assertIn(str(checkout / ".venv"), sandbox["filesystem"]["denyWrite"])
        self.assertEqual(sandbox["network"]["deniedDomains"], ["*"])
        self.assertTrue(settings["permissions"]["blockReadsOutsideWorkingDirectories"])
        self.assertTrue(settings["disableAllHooks"])

    def test_review_of_changed_policy_code_cannot_admit_training(self):
        native = json.loads(self.native.read_text())
        native["shared_sources"][str(Path(cohort.__file__))] = "previous-policy-code"
        gate.write(self.native, native)
        gate.write(self.review, {"passed":True, "cohort_root":str(self.root), "manifest_sha256":gate.sha(self.native)})
        self.value["native_review_sha256"] = gate.sha(self.review)
        with self.assertRaisesRegex(ValueError, "different runner or sandbox policy source"):
            gate.require_native_review(self.value)

    def test_actual_training_policy_must_equal_reviewed_policy(self):
        checkout = self.session / "repo"
        self.value["execution_root"] = str(checkout)
        settings = cohort.policy(self.authority, checkout, self.root / "runtime/python")
        gate.write(self.session / "settings.json", settings)
        gate.require_native_review(self.value)
        settings["sandbox"]["allowUnsandboxedCommands"] = True
        gate.write(self.session / "settings.json", settings)
        with self.assertRaisesRegex(ValueError, "assignment permissions differ"):
            gate.require_native_review(self.value)

    def test_startup_is_fresh_preserves_prompt_and_leaves_path_unchanged(self):
        argv = ["claude", "--print", "task prompt"]
        first, second = cohort.startup_argv(argv), cohort.startup_argv(argv)
        self.assertEqual(first[:3], ["/usr/bin/env", "CLAUDE_CODE_PROFILE_STARTUP=1", "claude"])
        self.assertEqual(first[-1], "task prompt")
        self.assertNotEqual(first[-2], second[-2])
        self.assertEqual(Path(first[-2]).parent.stat().st_mode & 0o777, 0o700)
        self.assertFalse(any(arg.startswith("PATH=") for arg in first))
        self.assertEqual(argv, ["claude", "--print", "task prompt"])


if __name__ == "__main__":
    unittest.main()
