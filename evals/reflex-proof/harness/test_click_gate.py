"""Click held-out runner (r13): prompts, control-prompt checks, grants, BEST drop, budget and the person's script."""
import hashlib
import json
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import click_analysis
import click_gate as runner
import click_tasks
import click_training
import real_repo_pairs as pairs
import split_audit

HERE = Path(__file__).resolve().parent
SCRIPT = HERE / "results/20261001-click-gate/run-click-gate.sh"
COMMAND = click_training.COMMAND
ROUTE = "github-com-pallets-click/learned/run-0abc12"


def study(tmp, mode):
    root = Path(tmp)
    for task in click_tasks.AUTHORED:
        d = root / "private/tasks" / task
        d.mkdir(parents=True)
        v = click_tasks.variant(task.split("--", 1)[1])
        (d / "prompt.txt").write_text(click_tasks.prompt(v) if task in click_tasks.EVALUATION else click_tasks.base_prompt(v))
    rows = runner.prompts(root, click_analysis.matrix(click_tasks.EVALUATION, mode), COMMAND if mode == "execution" else "")
    m = dict(mode=mode, matrix=rows, route=ROUTE if mode == "execution" else None, command=COMMAND if mode == "execution" else None,
             caches=list(click_analysis.CACHES), binaries=dict(adage="/bin/adage", **{"adage-exec": "/bin/adage-exec"}))
    return root, m


class Prompts(unittest.TestCase):
    def test_blocks_use_the_frozen_prompt_and_controls_the_composed_one(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, m = study(tmp, "execution")
            for row in m["matrix"]:
                if row["condition"] == "positive":
                    self.assertEqual(row["prompt"], (root / "private/tasks" / row["task"] / "prompt.txt").read_text())
                else:
                    self.assertEqual(row["prompt"], click_tasks.control_prompt(row["condition"], COMMAND))
            checks = runner.check_control_prompts(root, m["matrix"], COMMAND)
            self.assertEqual(len(checks), 5)
            self.assertTrue(all(c["passed"] for c in checks))
            m["matrix"][4]["prompt"] = m["matrix"][4]["prompt"].replace(COMMAND, "pytest")
            with self.assertRaises(ValueError):
                runner.check_control_prompts(root, m["matrix"], COMMAND)

    def test_every_block_prompt_ends_with_the_realism_line_and_names_no_adage(self):
        with tempfile.TemporaryDirectory() as tmp:
            _, m = study(tmp, "advisory")
            for row in m["matrix"]:
                self.assertTrue(row["prompt"].endswith("Do not create git commits."))
                self.assertNotIn("adage", row["prompt"].lower())


class Grants(unittest.TestCase):
    def test_the_approval_script_grants_best_homes_only(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, m = study(tmp, "execution")
            with patch.object(runner, "verify", return_value=m):
                out = runner.approval_script(root)
                text = Path(out["path"]).read_text()
                self.assertEqual(out["homes"], 25)
                self.assertEqual(text.count("review delegate"), 25)
                self.assertIn("review delegate github-com-pallets-click/learned run-0abc12 --level e2 --effect verify", text)
                self.assertIn("--timeout-ms 20000", text)
                for cache in click_analysis.CACHES:
                    self.assertIn("--cache " + shlex.quote(cache), text)
                for row in m["matrix"]:
                    home = str(runner.session_dir(root, row) / "adage-home")
                    self.assertEqual(("ADAGE_HOME=" + home) in text, row["arm"] == "BEST")
                self.assertEqual(out["sha256"], hashlib.sha256(text.encode()).hexdigest())
                self.assertEqual(runner.approval_script(root), out)  # idempotent, never rewritten

    def test_advisory_mode_has_no_grant_to_issue(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, m = study(tmp, "advisory")
            with patch.object(runner, "verify", return_value=m):
                with self.assertRaises(ValueError):
                    runner.approval_script(root)
                self.assertTrue(runner.verify_grants(root)["passed"])
                home = runner.session_dir(root, m["matrix"][0]) / "adage-home"
                home.mkdir(parents=True)
                (home / "policy.toml").write_text(runner.POLICY)
                self.assertFalse(runner.verify_grants(root)["passed"])

    def test_verify_grants_checks_the_exact_reviewed_terms(self):
        with tempfile.TemporaryDirectory() as tmp:
            root, m = study(tmp, "execution")
            grant = dict(route=ROUTE, by=runner.GRANTOR, auto=False,
                         key=dict(level="e2", effect="verify", argv_templates=[shlex.split(COMMAND)]),
                         terms=dict(cache=list(click_analysis.CACHES), timeout_ms=runner.TIMEOUT_MS))
            for row in runner.granted(m):
                home = runner.session_dir(root, row) / "adage-home"
                home.mkdir(parents=True)
                pairs.write(home / "approvals.json", dict(approvals=[grant]))
                (home / "policy.toml").write_text(runner.POLICY)
            with patch.object(runner, "verify", return_value=m):
                self.assertTrue(runner.verify_grants(root)["passed"])
                home = runner.session_dir(root, runner.granted(m)[3]) / "adage-home"
                pairs.write(home / "approvals.json", dict(approvals=[dict(grant, terms=dict(grant["terms"], timeout_ms=30000))]))
                result = runner.verify_grants(root)
                self.assertFalse(result["passed"])
                self.assertEqual(len(result["problems"]), 1)


class Drop(unittest.TestCase):
    def test_best_is_dropped_once_and_stays_dropped(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            self.assertIsNone(runner.dropped(root))
            first = runner.drop_best(root, 5, "a control was unrealized")
            second = runner.drop_best(root, 9, "another reason")
            self.assertEqual(first, second)
            self.assertEqual(runner.dropped(root)["order"], 5)


class Budget(unittest.TestCase):
    def test_the_check_reads_the_ledger_and_never_raises_a_ceiling(self):
        with tempfile.TemporaryDirectory() as tmp:
            authority = Path(tmp).resolve()
            pairs.write(authority / "evaluation-allocation.json", dict(kind="human_evaluation_budget", authority=str(authority),
                                                                       ledger_ceiling_usd=60, statement="test"))
            attempts = [dict(run="r", status="settled", cost_usd=39.8915551, cap_usd=50)]
            pairs.write(authority / "paid-state.json", dict(runs={}, accounting=dict(version=1, attempts=attempts, blocked_reason=None)))
            start = runner.budget_check(authority, "start")
            self.assertFalse(start["fits"])
            self.assertAlmostEqual(start["shortfall_usd"], 13.3915551, places=6)
            self.assertFalse(runner.budget_check(authority, "heldout", "execution")["fits"])
            pairs.write(authority / "paid-state.json", dict(runs={}, accounting=dict(version=1, attempts=[], blocked_reason=None)))
            self.assertTrue(runner.budget_check(authority, "start")["fits"])
            self.assertTrue(runner.budget_check(authority, "heldout", "execution")["fits"])
            pairs.write(authority / "evaluation-allocation.json", dict(kind="human_evaluation_budget", authority=str(authority),
                                                                       ledger_ceiling_usd=90, statement="test"))
            with self.assertRaises(ValueError):  # the $60 limit in real_repo_gate.budget_limit is the person's to change
                runner.budget_check(authority, "start")


class Script(unittest.TestCase):
    def test_the_person_script_is_valid_bash_and_bound_to_the_registered_inputs(self):
        subprocess.run(["bash", "-n", str(SCRIPT)], check=True)
        text = SCRIPT.read_text()
        value = lambda name: text.split("\n" + name + "=", 1)[1].split("\n", 1)[0]  # noqa: E731
        self.assertEqual(value("PROTOCOL_SHA"), pairs.sha(split_audit.PROTOCOL))
        manifest = json.loads((HERE / "results/20261001-click-tasks/manifest.json").read_text())
        self.assertEqual(value("TASKS_VALIDATION_SHA"), manifest["validation_sha256"])
        spec = json.loads(split_audit.PROTOCOL.read_text())["click_heldout_analysis"]
        self.assertIn(value("ADAGE_SHA"), spec["arm_definitions"]["BEST"])
        self.assertIn(value("EXEC_SHA"), spec["arm_definitions"]["BEST"])
        self.assertTrue(spec["arm_definitions"]["BEST"].count(value("BUILD")[:7]) >= 1)
        for guard in ("CLAUDECODE", "[ -t 0 ]", "git status --porcelain", "budget-check", "'advisory'", "'advisory-only'",
                      "APPROVE_SHA:0:12", "verify-grants", "click_gate.py live", "click_gate.py report"):
            self.assertIn(guard, text)
        self.assertLess(text.index("click_training.py train"), text.index("click_training.py learn"))
        self.assertLess(text.index("click_training.py learn"), text.index("click_gate.py freeze"))
        self.assertLess(text.index("approve-click.sh\""), text.index("click_gate.py live"))


class Sources(unittest.TestCase):
    def test_the_frozen_sources_cover_the_runner_analysis_tasks_training_and_protocol(self):
        names = {Path(p).name for p in runner.sources()}
        for name in ("click_gate.py", "click_analysis.py", "click_tasks.py", "click_training.py", "click_writeup.py",
                     "heldout_cohort.py", "heldout_analysis.py", "launch_gate_protocol.json", "real_repo_gate.py",
                     "real_repo_pairs.py", "probe.py", "budget.py", "receipt_replay.py", "split_audit.py", "cohort_v5.py"):
            self.assertIn(name, names)


if __name__ == "__main__":
    unittest.main()
