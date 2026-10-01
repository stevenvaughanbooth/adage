"""Held-out cohort runner, offline dry runs: prompts, arms, grants, hook routing, binary binding, task
material, the control gate and the live preflight. Nothing launches claude."""
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import curve_tasks
import dev_arms
import heldout_analysis as analysis
import heldout_cohort as runner
import heldout_tasks

FINITE = next(v for v in curve_tasks.VARIANTS if v["id"] == "finite")


def frozen(root, best="A3"):
    for task in heldout_tasks.EVALUATION + [analysis.CONTROL_TASK]:
        d = root / "private/tasks" / task
        d.mkdir(parents=True)
        v = FINITE if task == analysis.CONTROL_TASK else heldout_tasks.variant(task.split("--", 1)[1])
        (d / "prompt.txt").write_text(curve_tasks.prompt(v) if task == analysis.CONTROL_TASK else heldout_tasks.prompt(v))
    m = dict(protocol=runner.PROTOCOL, root=str(root), cohort_root=str(root), budget_authority=str(root),
             source_sha256=runner.sources(), frozen_inputs={}, matrix=runner.prompts(root, analysis.matrix(heldout_tasks.EVALUATION)),
             route=runner.canary.ROUTE, command=runner.canary.COMMAND, caches=list(runner.canary.CACHES),
             binaries={"adage": str(root / "bin/adage"), "adage-exec": str(root / "bin/adage-exec")},
             best=dict(arm=best, card_checks=dev_arms.ARMS[best]["card_checks"]), learned_files={"index.json": "0" * 64},
             preparation_python="/usr/bin/python3", model="claude-haiku-4-5-20251001", budget_allocation_sha256="0" * 64,
             budget=dict(study_ceiling_usd=runner.STUDY_CEILING_USD))
    runner.pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(runner.pairs.sha(root / "manifest.json") + "\n")
    return m


def grant():
    return dict(approvals=[dict(route=runner.canary.ROUTE, by="launch-evaluation-human", auto=False,
                                key=dict(level="e2", effect="verify", argv_templates=[shlex.split(runner.canary.COMMAND)]),
                                terms=dict(cache=list(runner.canary.CACHES), timeout_ms=30000))])


def grant_best(root, m):
    for row in m["matrix"]:
        home = runner.session_dir(root, row) / "adage-home"
        home.mkdir(parents=True)
        if row["arm"] == "BEST":
            (home / "policy.toml").write_text(runner.POLICY)
            runner.pairs.write(home / "approvals.json", grant())


class Prompts(unittest.TestCase):
    def test_blocks_use_the_frozen_natural_prompt_and_controls_the_validated_suffix(self):
        with tempfile.TemporaryDirectory() as tmp:
            m = frozen(Path(tmp))
        for row in m["matrix"]:
            if row["condition"] == "positive":
                v = heldout_tasks.variant(row["task"].split("--", 1)[1])
                self.assertEqual(row["prompt"], heldout_tasks.prompt(v))
                self.assertTrue(row["prompt"].endswith("Do not create git commits."))
                self.assertNotIn("```", row["prompt"])
                self.assertNotIn("Work in this order", row["prompt"])
            else:
                self.assertEqual(row["prompt"], curve_tasks.prompt(FINITE) + runner.canary.CONDITIONS[row["condition"]][0])

    def test_session_directories_are_unique_and_name_arm_and_block(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            dirs = [runner.session_dir(root, r) for r in m["matrix"]]
        self.assertEqual(len(dirs), len(set(dirs)))
        self.assertEqual(dirs[0].name, "00-BEST-failed")
        self.assertRegex(dirs[5].name, r"^05-(A0|NOEXEC|BEST)-b01-has-keys$")


class Arms(unittest.TestCase):
    def test_each_arm_gets_only_its_registered_hooks_binaries_and_runtime_label(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root, best="A2")
            for arm in analysis.ARMS:
                row = next(r for r in m["matrix"] if r["arm"] == arm and r["condition"] == "positive")
                session = runner.session_dir(root, row)
                settings, value = runner.assignment(m, row, session, session / "repo", session / "adage-home",
                                                    {"disableAllHooks": True, "sandbox": {}}, root / "hidden.json", root / "review.json")
                argv = value["argv"]
                self.assertEqual(argv[argv.index("--max-budget-usd") + 1], "0.4")
                self.assertEqual(argv[argv.index("--max-turns") + 1], "60")
                self.assertEqual(argv[-1], row["prompt"])
                self.assertEqual(value["heldout_arm"], arm)
                self.assertEqual(value["arm"], runner.RUNTIME_ARM[arm])
                if arm == "A0":
                    self.assertTrue(settings["disableAllHooks"])
                    self.assertNotIn("hooks", settings)
                    self.assertEqual((value["binaries"], value["learned_files"], value["card_checks"]), ({}, {}, None))
                else:
                    self.assertFalse(settings["disableAllHooks"])
                    self.assertEqual(set(settings["hooks"]), {e for e, _ in dev_arms.EVENTS})
                    self.assertEqual(settings["hooks"]["PreToolUse"][0]["matcher"], "Bash")
                    self.assertEqual(value["binaries"], m["binaries"])
                    self.assertIs(value["card_checks"], False)  # BEST's design (A2) in both hook arms
        # real_repo_pairs.hook runs adage-exec only for "automatic": NOEXEC never executes.
        self.assertEqual([a for a, label in runner.RUNTIME_ARM.items() if label == "automatic"], ["BEST"])

    def test_hook_refuses_a0_and_sets_the_card_knob_from_the_best_design(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.json"
            path.write_text(json.dumps(dict(arm="agent")))
            with self.assertRaises(ValueError):
                runner.hook(path, {})
            with patch.object(runner.pairs, "hook", return_value="out") as h, patch.dict(os.environ, {}, clear=False):
                path.write_text(json.dumps(dict(arm="noexec", card_checks=False)))
                self.assertEqual(runner.hook(path, {}), "out")
                self.assertEqual(os.environ.get("ADAGE_CARD_CHECKS"), "0")
                path.write_text(json.dumps(dict(arm="automatic", card_checks=True)))
                runner.hook(path, {})
                self.assertNotIn("ADAGE_CARD_CHECKS", os.environ)
                self.assertEqual(h.call_count, 2)


class Grants(unittest.TestCase):
    def test_script_grants_only_best_homes(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            result = runner.approval_script(root)
            text = Path(result["path"]).read_text()
            self.assertEqual(result["homes"], 25)
            self.assertEqual(text.count("review delegate"), 25)
            self.assertEqual(text.count("--level e2 --effect verify"), 25)
            self.assertIn('auto_approve = "off"', text)
            self.assertNotIn("approvals.json >", text)
            homes = [line.split()[0].split("=", 1)[1] for line in text.splitlines() if line.startswith("ADAGE_HOME=")]
            best = {str(runner.session_dir(root, r) / "adage-home") for r in m["matrix"] if r["arm"] == "BEST"}
            self.assertEqual(set(homes), best)
            for r in m["matrix"]:
                if r["arm"] != "BEST":
                    self.assertNotIn(str(runner.session_dir(root, r)), text)
            self.assertEqual(runner.approval_script(root)["sha256"], result["sha256"])

    def test_grant_check_requires_exactly_the_reviewed_grant_in_best_and_nothing_elsewhere(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            grant_best(root, m)
            self.assertTrue(runner.verify_grants(root)["passed"])
            noexec = next(r for r in m["matrix"] if r["arm"] == "NOEXEC")
            (runner.session_dir(root, noexec) / "adage-home/policy.toml").write_text(runner.POLICY)
            self.assertFalse(runner.verify_grants(root)["passed"])
            (runner.session_dir(root, noexec) / "adage-home/policy.toml").unlink()
            best = next(r for r in m["matrix"] if r["arm"] == "BEST" and r["condition"] == "positive")
            (runner.session_dir(root, best) / "adage-home/approvals.json").unlink()
            self.assertFalse(runner.verify_grants(root)["passed"])


class Freeze(unittest.TestCase):
    def test_best_comes_from_the_dev_report_by_rule(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "report.json"
            report = dict(protocol=dev_arms.PROTOCOL, arms=dict(A2=dict(requests=15.0, cost_usd=0.1), A3=dict(requests=14.0)),
                          verdict=dict(A2=dict(passed=True), A3=dict(passed=True)))
            path.write_text(json.dumps(report))
            d = runner.design(path)
            self.assertEqual((d["arm"], d["card_checks"]), ("A3", True))
            self.assertEqual(d["report_sha256"], runner.pairs.sha(path))
            report["verdict"] = dict(A2=dict(passed=False), A3=dict(passed=False))
            path.write_text(json.dumps(report))
            with self.assertRaises(ValueError):
                runner.design(path)
            path.write_text(json.dumps(dict(report, protocol="other")))
            with self.assertRaises(ValueError):
                runner.design(path)

    def test_binaries_are_bound_by_given_hashes_and_build_commit(self):
        with tempfile.TemporaryDirectory() as tmp:
            bins = Path(tmp)
            (bins / "adage").write_text("a")
            (bins / "adage-exec").write_text("b")
            digests = {n: runner.pairs.sha(bins / n) for n in ("adage", "adage-exec")}
            ok = lambda out="": subprocess.CompletedProcess([], 0, out, "")  # noqa: E731
            no = subprocess.CompletedProcess([], 1, "", "")

            def git(responses):
                def fake(*args):
                    return responses.get(args[0], ok())
                return fake
            commit = "c" * 40
            good = {"rev-parse": ok(commit + "\n")}
            with patch.object(runner, "git", git(good)):
                self.assertEqual(runner.bind_binaries(bins, "abc", digests), (commit, digests))
                with self.assertRaises(ValueError):
                    runner.bind_binaries(bins, "abc", dict(digests, adage="0" * 64))
                with self.assertRaises(ValueError):
                    runner.bind_binaries(bins, "abc", {"adage": digests["adage"]})
            for failure in ({"rev-parse": no}, dict(good, **{"merge-base": no}), dict(good, diff=no),
                            dict(good, status=ok(" M crates/x.rs\n"))):
                with patch.object(runner, "git", git(failure)), self.assertRaises(ValueError):
                    runner.bind_binaries(bins, "abc", digests)

    def test_held_out_controls_must_match_the_frozen_task_manifest(self):
        with tempfile.TemporaryDirectory() as tmp:
            controls = Path(tmp)
            runner.pairs.write(controls / "manifest.json", dict(protocol=heldout_tasks.PROTOCOL))
            runner.pairs.write(controls / "validation.json", dict(complete=True, valid=True))
            for task in heldout_tasks.EVALUATION:
                v = heldout_tasks.variant(task.split("--", 1)[1])
                d = controls / "private/tasks" / task
                d.mkdir(parents=True)
                (d / "prompt.txt").write_text(heldout_tasks.prompt(v))
                (d / "hidden.py").write_text(heldout_tasks.FAMILY["hidden"](v))
            runner.check_heldout_controls(controls)
            (controls / "private/tasks" / heldout_tasks.EVALUATION[0] / "prompt.txt").write_text("changed")
            with self.assertRaises(ValueError):
                runner.check_heldout_controls(controls)


def control_session(root, row, realized=True):
    session = runner.session_dir(root, row)
    session.mkdir(parents=True, exist_ok=True)
    final = "final"
    c = row["condition"]
    if c == "failed":
        events = [dict(kind="receipt", phase="completed", outcome="failed", digest="x")]
    elif c in ("later-edit", "config-change"):
        events = [dict(kind="receipt", phase="completed", outcome="applied", digest="stale")]
    else:
        events = [dict(kind="receipt", phase="completed", outcome="applied", digest=final)]
    if not realized:
        events = []
    runner.pairs.write(session / "summary.json", dict(result_status="success", result_is_error=False, total_cost_usd=0.1))
    runner.pairs.write(session / "collection.json", dict(quality=dict(passed=True), timeline=dict(events=events, final_digest=final),
                                                         replay=dict(necessary_work_skipped=False, repeat_attempts=0)))


class Gate(unittest.TestCase):
    def test_all_five_controls_must_be_realized_before_any_block(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            controls = [r for r in m["matrix"] if r["condition"] != "positive"]
            for row in controls[:4]:
                control_session(root, row)
            self.assertFalse(runner.control_gate(root, m)["passed"])  # one control not yet collected
            control_session(root, controls[4])
            self.assertTrue(runner.control_gate(root, m)["passed"])
            control_session(root, controls[2], realized=False)
            self.assertFalse(runner.control_gate(root, m)["passed"])

    def test_live_refuses_before_any_launch_without_the_person_issued_grants(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            with patch.object(runner.pairs.probe, "execute", side_effect=AssertionError("must not launch")):
                with self.assertRaises(ValueError):
                    runner.live(root)

    def test_a_changed_runner_source_or_manifest_refuses(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            runner.verify(root)
            runner.pairs.write(root / "manifest.json", dict(m, model="other"))
            with self.assertRaises(ValueError):
                runner.verify(root)


if __name__ == "__main__":
    unittest.main()
