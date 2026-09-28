#!/usr/bin/env python3
"""Smoke test of the live runner's dry run. Runs no agent and spends nothing.

  python3 -m unittest evals/live/test_runner.py        (or: python3 evals/live/test_runner.py)
"""

import contextlib
import io
import json
import os
import re
import sys
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import runner  # noqa: E402

CONFIG = str(HERE / "configs" / "acme-billing.json")
HARBOR = str(HERE / "configs" / "harbor-dispatch.json")


def dry_with(*args):
    out = io.StringIO()
    with contextlib.redirect_stdout(out):
        assert runner.main(list(args)) == 0
    return out.getvalue()


def dry(*args):
    return dry_with(CONFIG, *args)


class DryRun(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = Path(self.tmp.name) / "live"

    def tearDown(self):
        self.tmp.cleanup()

    def plan(self, *args):
        return json.loads(dry("--json", "--root", str(self.root), *args))

    def test_the_demo_plan(self):
        p = self.plan()
        self.assertEqual(p["totals"]["sessions"], 54)
        self.assertEqual(p["totals"]["by_arm"], {"adage": 18, "holdout": 18, "train": 18})
        for s in p["sessions"]:
            self.assertEqual(s["command"][:3], ["claude", "-p", s["prompt"]])
            self.assertIn("claude-haiku-4-5-20251001", s["command"])
            self.assertIn(s["task"], s["prompt"])
            self.assertTrue(s["cwd"].startswith(str(self.root.resolve())))
        by_arm = {s["arm"]: s["env"] for s in p["sessions"]}
        self.assertEqual(by_arm["train"], {}, "training sessions run without hooks")
        self.assertEqual(by_arm["holdout"]["ADAGE_ARM"], "holdout")
        self.assertEqual(by_arm["adage"]["ADAGE_ARM"], "treated")
        self.assertIn("condition", p["learn"])

    def test_a_dry_run_writes_nothing(self):
        text = dry("--root", str(self.root))
        self.assertIn("DRY RUN", text)
        self.assertFalse(self.root.exists())

    def test_overrides_and_later_rounds(self):
        p = self.plan("--model", "claude-sonnet-5", "--round", "2", "eval")
        self.assertEqual(p["totals"]["sessions"], 36, "round 2 runs no training sessions")
        self.assertTrue(all(s["run"].startswith("eval2-") for s in p["sessions"]))
        self.assertTrue(all("claude-sonnet-5" in s["command"] for s in p["sessions"]))
        self.assertIsNone(p["learn"])

    def test_execute_is_refused_inside_an_agent_session(self):
        old = os.environ.get("CLAUDECODE")
        os.environ["CLAUDECODE"] = "1"
        try:
            with self.assertRaises(SystemExit):
                runner.main([CONFIG, "--execute", "--root", str(self.root)])
        finally:
            if old is None:
                del os.environ["CLAUDECODE"]
            else:
                os.environ["CLAUDECODE"] = old
        self.assertFalse(self.root.exists())

    def test_the_second_demo_plan(self):
        p = json.loads(dry_with(HARBOR, "--json", "--root", str(self.root)))
        self.assertEqual(p["rounds"], [1, 2])
        self.assertEqual(p["estimate"]["total_runs"], 84)
        self.assertEqual(p["estimate"]["train"]["runs"], 12)
        self.assertEqual(p["estimate"]["eval"]["runs"], 72)
        self.assertEqual(p["totals"]["by_arm"], {"adage": 36, "holdout": 36, "train": 12})
        self.assertEqual(p["totals"]["by_round"], {"1": 48, "2": 36})
        self.assertTrue(all("claude-sonnet-5" in s["command"] for s in p["sessions"]))
        self.assertTrue(all(s["run"].startswith("eval2-") for s in p["sessions"] if s["round"] == 2))
        self.assertFalse(any(s["phase"] == "train" for s in p["sessions"] if s["round"] == 2))
        self.assertTrue(all(s["task"].startswith("INC-") and s["task"] in s["prompt"] for s in p["sessions"]))

    def test_config_flag_and_explicit_dry_run(self):
        text = dry_with("--config", HARBOR, "--dry-run", "--root", str(self.root))
        self.assertIn("DRY RUN", text)
        self.assertIn("total 84 agent runs", text)
        self.assertIn("6 tasks x 3 repeats x 2 arms (holdout, adage) x 2 rounds = 72", text)
        self.assertFalse(self.root.exists())
        with self.assertRaises(SystemExit):
            runner.main(["--config", HARBOR, "--dry-run", "--execute"])

    def test_one_round_of_a_multi_round_config(self):
        p = json.loads(dry_with(HARBOR, "--json", "--root", str(self.root), "--round", "2", "learn", "eval"))
        self.assertEqual(p["totals"]["sessions"], 36)
        self.assertEqual(p["learn_rounds"], [2])

    def test_classification_of_the_second_sandbox(self):
        cfg = runner.load_config(HARBOR, {})
        c = lambda cmd, failed=False: runner.classify(cfg, "Bash", {"command": cmd}, failed)
        self.assertEqual(c("./bin/fetch-incident INC-86"), "lookup")
        self.assertEqual(c("npm test", failed=True), "verify-fail")
        self.assertEqual(c("npm run check"), "verify")
        self.assertEqual(c("npm run codegen"), "regen")
        self.assertEqual(c("node tools/codegen.mjs --verify"), "verify")
        self.assertEqual(c("ls src"), "explore")
        self.assertEqual(runner.classify(cfg, "Edit", {"file_path": "/x/registry/ports.csv"}), "edit")
        # The first demo keeps its own categories.
        acme = runner.load_config(CONFIG, {})
        self.assertEqual(runner.classify(acme, "Bash", {"command": "python3 scripts/wi-lookup W-2042"}), "lookup")
        self.assertEqual(runner.classify(acme, "Bash", {"command": "make gen"}), "regen")

    def test_metrics_follow_the_obligation(self):
        cfg = runner.load_config(HARBOR, {})
        wd = "/w/harbor-dispatch"
        ev = lambda turn, tool, cat, cmd, failed=False: {"t": turn * 2.0, "turn": turn, "tool": tool, "id": str(turn),
                                                         "cat": cat, "cmd": cmd, "failed": failed}
        events = [ev(1, "Bash", "lookup", "./bin/fetch-incident INC-245"), ev(2, "Read", "explore", f"{wd}/registry/ports.csv"),
                  ev(3, "Edit", "edit", f"{wd}/registry/ports.csv"), ev(4, "Bash", "verify-fail", "npm test", True),
                  ev(5, "Bash", "regen", "npm run codegen"), ev(6, "Bash", "verify", "npm run check")]
        spool = [{"kind": "would_have", "route": "lookup-work-item"}, {"kind": "followed", "route": "lookup-work-item"},
                 {"kind": "would_have", "route": "dead-end-x"}]
        m = runner.run_metrics(cfg, events, {"num_turns": 7, "total_cost_usd": 0.1}, {"passed": True, "hidden_passed": True},
                               wd, 12.0, "sid", spool)
        self.assertEqual((m["turns_to_first_edit"], m["dead_end_hits"], m["lookup_turn"]), (2, 1, 1))
        self.assertEqual((m["obligation_applies"], m["obligation_met"]), (True, True))
        self.assertEqual(m["follow"], {"exposed": 2, "followed": 1, "derailed": 0})
        m2 = runner.run_metrics(cfg, events[:4], {}, {"passed": False, "stale_generated": True}, wd, 1.0, "sid", [])
        self.assertEqual((m2["obligation_applies"], m2["obligation_met"]), (True, False))

    def test_regeneration_inside_the_editing_command_counts(self):
        # Found in the Sonnet 5 run: `sed … registry/ports.csv && npm run codegen && npm run check` is one
        # event classified "edit"; the regeneration after the edit must still count.
        cfg = runner.load_config(HARBOR, {})
        wd = "/w/harbor-dispatch"
        ev = lambda turn, cat, cmd: {"t": turn * 2.0, "turn": turn, "tool": "Bash", "id": str(turn), "cat": cat, "cmd": cmd}
        same = [ev(1, "edit", "sed -i '' 's/22.6/24/' registry/ports.csv && npm run codegen && npm run check 2>&1 | tail")]
        self.assertTrue(runner.regen_after_obligation(cfg, same, wd))
        before = [ev(1, "edit", "npm run codegen && sed -i '' 's/22.6/24/' registry/ports.csv")]
        self.assertFalse(runner.regen_after_obligation(cfg, before, wd), "regeneration before the edit does not count")
        reads_after = [ev(1, "edit", "sed -i '' '4a\\ Neo' registry/vessels.csv && cat registry/vessels.csv && npm run codegen; "
                                     "cat registry/ports.csv | grep x")]
        self.assertTrue(runner.regen_after_obligation(cfg, reads_after, wd), "reading the file after codegen is not an edit")
        sed_append = [ev(1, "edit", "sed -i '' '4a\\\n Neo,1,2,3,4' registry/vessels.csv && npm run codegen 2>&1 | tail -5")]
        self.assertTrue(runner.regen_after_obligation(cfg, sed_append, wd), "a newline inside a quoted sed script is not a separator")
        appended = [ev(1, "edit", "echo 'Neo,1,2,3,4' >> registry/vessels.csv && npm run codegen 2>&1 | tail -5")]
        self.assertTrue(runner.regen_after_obligation(cfg, appended, wd))
        verify_only = [ev(1, "edit", "sed -i '' 's/a/b/' registry/ports.csv && node tools/codegen.mjs --verify")]
        self.assertFalse(runner.regen_after_obligation(cfg, verify_only, wd), "a --verify check is not a regeneration")

    def test_report_only_needs_no_execute_and_runs_no_agents(self):
        root = Path(self.tmp.name) / "done"
        root.mkdir()
        (root / "state.json").write_text(json.dumps({"config": "harbor-dispatch", "runs": {}, "rounds": {}}))
        env = {k: v for k, v in os.environ.items()}
        os.environ["CLAUDECODE"] = "1"  # reporting spends nothing, so it is allowed inside a session
        try:
            out = io.StringIO()
            with contextlib.redirect_stdout(out):
                self.assertEqual(runner.main([HARBOR, "report", "--root", str(root)]), 0)
            self.assertTrue((root / "summary.json").exists())
        finally:
            os.environ.clear()
            os.environ.update(env)

    def test_a_bad_config_is_refused(self):
        bad = Path(self.tmp.name) / "bad.json"
        cfg = json.loads(Path(CONFIG).read_text())
        cfg["eval"]["arms"] = ["adage", "sometimes"]
        bad.write_text(json.dumps(cfg))
        with self.assertRaises(SystemExit):
            runner.main([str(bad), "--root", str(self.root)])


def write_config(tmp, **eval_):
    cfg = json.loads(Path(HARBOR).read_text())
    cfg["eval"].update(eval_)
    p = Path(tmp) / "cfg.json"
    p.write_text(json.dumps(cfg))
    return str(p)


class ArmOrder(unittest.TestCase):
    def pairs(self, cfg):
        out = {}
        for run_id, phase, arm, task, _ in runner.plan(cfg, ["eval"], 1):
            out.setdefault(run_id.rsplit("-", 1)[0], []).append(arm)
        return out

    def test_the_order_is_random_per_pair_and_fixed_by_the_seed(self):
        cfg = runner.load_config(HARBOR, {"seed": 11})
        a, b = self.pairs(cfg), self.pairs(runner.load_config(HARBOR, {"seed": 11}))
        self.assertEqual(a, b, "the same seed gives the same order")
        self.assertEqual(len(a), 18)
        for arms in a.values():
            self.assertEqual(sorted(arms), ["adage", "holdout"], "every (task, repeat) runs each arm once")
        firsts = [arms[0] for arms in a.values()]
        self.assertTrue(0 < firsts.count("holdout") < len(firsts), "neither arm always goes first")
        other = self.pairs(runner.load_config(HARBOR, {"seed": 12}))
        self.assertNotEqual(a, other, "another seed gives another order")
        self.assertEqual(runner.arm_order(["holdout", "adage"], 11, 1, "INC-86", 1),
                         runner.arm_order(["holdout", "adage"], 11, 1, "INC-86", 1))

    def test_the_seed_is_in_the_plan(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = json.loads(dry_with(HARBOR, "--json", "--seed", "42", "--root", tmp))
            self.assertEqual(p["seed"], 42)
            again = json.loads(dry_with(HARBOR, "--json", "--seed", "42", "--root", tmp))
            self.assertEqual([s["run"] for s in p["sessions"]], [s["run"] for s in again["sessions"]])
            with self.assertRaises(SystemExit):
                runner.main([HARBOR, "--seed", "x"])


class Status(unittest.TestCase):
    ok_score = {"passed": True}

    def test_classification(self):
        c = runner.classify_status
        self.assertEqual(c({"subtype": "success"}, 5, 6, self.ok_score)[0], "ok")
        self.assertEqual(c({}, 0, 0, self.ok_score)[0], "infra_error", "no result message")
        self.assertEqual(c({"subtype": "success", "is_error": True, "result": "Credit balance is too low"}, 0, 1,
                           self.ok_score)[0], "infra_error")
        self.assertEqual(c({"subtype": "error_during_execution"}, 3, 4, self.ok_score)[0], "infra_error")
        self.assertEqual(c({"subtype": "success"}, 0, 1, {"passed": False}),
                         ("infra_error", "no tool calls in 1 turn(s)"))
        self.assertEqual(c({"subtype": "error_max_turns", "is_error": True}, 30, 31, {"passed": False}, True, 30)[0],
                         "censored")
        self.assertEqual(c({"subtype": "success"}, 5, 6, {}, False)[0], "scorer_error")
        self.assertEqual(c({"subtype": "success"}, 5, 6, {"stale_generated": False})[0], "scorer_error",
                         "a verdict without `passed` is no verdict")
        self.assertEqual(c({"subtype": "error_max_turns"}, 30, 31, {}, False, 30)[0], "scorer_error",
                         "an unscored run cannot be compared, censored or not")

    def test_older_runs_are_classified_from_their_metrics(self):
        s = lambda **m: runner.demo_report.status_of(m, 30)[0]  # noqa: E731
        self.assertEqual(s(tool_calls=0, turns=1, result_subtype="success", passed=False), "infra_error")
        self.assertEqual(s(tool_calls=30, turns=31, result_subtype="error_max_turns", passed=True), "censored")
        self.assertEqual(s(tool_calls=9, turns=10, result_subtype="success", passed=False), "ok")
        self.assertEqual(s(status="scorer_error"), "scorer_error")

    def test_summaries_leave_out_errors_from_both_arms_and_count_them(self):
        def run(task, arm, rep, **m):
            base = {"tool_calls": 8, "turns": 9, "tool_turns": 8, "result_subtype": "success", "passed": True,
                    "turns_to_first_edit": 3, "time_to_first_edit": 5.0, "elapsed": 20.0, "cost_usd": 0.1,
                    "dead_end_hits": 0, "explore_calls_before_edit": 2}
            base.update(m)
            return f"{task}-{rep}-{arm}", {"phase": "eval", "round": 1, "arm": arm, "task": task, "metrics": base}
        runs = dict([run(t, a, r) for t in ("T1", "T2", "T3") for a in ("adage", "holdout") for r in (1, 2)])
        runs.update([run("T1", "holdout", 3, tool_calls=0, turns=1, passed=False),  # infra: not a failure
                     run("T2", "adage", 3, status="scorer_error"),
                     run("T3", "holdout", 3, turns=31, tool_turns=30, result_subtype="error_max_turns",
                         turns_to_first_edit=None, time_to_first_edit=None, passed=False, elapsed=90.0)])
        runs["T3-9-holdout"] = {"phase": "eval", "round": 1, "arm": "holdout", "task": "T3", "status": "skipped"}
        s = runner.summarise({"runs": runs, "max_turns": 30}, 1, n_perm=200, n_boot=200)
        self.assertEqual(s["excluded"], {"adage": 1, "holdout": 1})
        self.assertEqual(s["n_censored"], {"adage": 0, "holdout": 1})
        self.assertEqual(s["status_counts"]["holdout"]["infra_error"], 1)
        self.assertEqual(s["unfinished"], {"adage": 0, "holdout": 1})
        pr = s["metrics"]["pass_rate"]
        self.assertEqual(pr["n"], {"adage": 6, "holdout": 7}, "the infra error is not a failed task")
        self.assertAlmostEqual(pr["holdout"], 6 / 7)
        fe = s["metrics"]["turns_to_first_edit"]
        self.assertEqual(fe["n"]["holdout"], 7, "a censored run without an edit is kept, not dropped")
        self.assertEqual(fe["n_imputed"]["holdout"], 1)
        tt = [r for r in runs.values() if r.get("metrics", {}).get("result_subtype") == "error_max_turns"]
        self.assertEqual(runner.demo_report.METRICS["total_turns"][0](tt[0]["metrics"], "censored", 30), (30, False))
        for row in s["metrics"].values():
            self.assertIn("p_holm", row)


class Statistics(unittest.TestCase):
    # Four tasks, two runs per arm each; adage is 2 better in every task, and the tasks differ a lot.
    TOY = [(t, arm, base + (0 if arm == "adage" else 2) + e)
           for t, base in (("a", 1), ("b", 10), ("c", 50), ("d", 100))
           for arm in ("adage", "holdout") for e in (0.0, 0.5)]

    def test_the_permutation_is_within_tasks(self):
        p = runner.cluster_perm_p(self.TOY, n=2000)
        self.assertLess(p, 0.01, "a consistent effect within every task is found despite big task differences")
        flat = [(t, arm, 5.0) for t, arm, _ in self.TOY]
        self.assertEqual(runner.cluster_perm_p(flat, n=200), 1.0)
        self.assertIsNone(runner.cluster_perm_p([x for x in self.TOY if x[0] == "a"], n=200),
                          "one task is not enough to test anything")
        self.assertEqual(runner.cluster_perm_p(self.TOY, n=500), runner.cluster_perm_p(self.TOY, n=500), "seeded")
        self.assertAlmostEqual(runner.demo_report.task_mean_diff(self.TOY), -2.0)

    def test_the_task_level_test_is_limited_by_the_number_of_tasks(self):
        self.assertAlmostEqual(runner.task_signflip_p(self.TOY), 2 / 16, msg="4 tasks: the smallest p is 2/2^4")
        many = [(f"t{i}", arm, v) for i in range(6) for arm, v in (("adage", 1.0), ("holdout", 2.0))]
        self.assertAlmostEqual(runner.task_signflip_p(many), 2 / 64)

    def test_the_bootstrap_resamples_tasks(self):
        lo, hi = runner.cluster_boot_ci(self.TOY, lambda v: sum(v) / len(v), n=500)
        self.assertLessEqual(lo, -2.0 + 1e-9)
        self.assertGreaterEqual(hi, -2.0 - 1e-9)
        self.assertLess(hi, 0)
        same = [(t, arm, 1.0 if arm == "adage" else 3.0) for t, arm, _ in self.TOY]
        self.assertEqual(runner.cluster_boot_ci(same, runner.demo_report.med, n=100), [-2.0, -2.0])
        self.assertIsNone(runner.cluster_boot_ci(same[:4], runner.demo_report.med), "one task gives no interval")

    def test_holm(self):
        adj = runner.holm({"a": 0.01, "b": 0.04, "c": 0.03, "d": None})
        self.assertEqual(adj["d"], None)
        self.assertAlmostEqual(adj["a"], 0.03)
        self.assertAlmostEqual(adj["c"], 0.06)
        self.assertAlmostEqual(adj["b"], 0.06, msg="adjusted p-values never decrease down the ranking")
        self.assertEqual(runner.holm({"x": 0.9, "y": 0.8}), {"x": 1.0, "y": 1.0})
        self.assertEqual(runner.holm({}), {})


class Isolation(unittest.TestCase):
    BASE = {"HOME": "/Users/op", "CLAUDE_CONFIG_DIR": "/Users/op/.claude", "PATH": "/usr/bin:/Users/op/.local/bin",
            "ANTHROPIC_API_KEY": "sk-test", "CLAUDECODE": "1", "AWS_SECRET_ACCESS_KEY": "nope",
            "ADAGE_HOME": "/Users/op/.adage", "XDG_CONFIG_HOME": "/Users/op/.config", "LANG": "C.UTF-8"}

    def test_the_environment_is_built_from_scratch(self):
        root = Path("/r")
        env = runner.session_env("holdout", root / "runs" / "x" / "home", root / "private" / "adage-home", True, self.BASE)
        self.assertEqual(env["HOME"], "/r/runs/x/home")
        for k in ("CLAUDE_CONFIG_DIR", "XDG_CONFIG_HOME", "XDG_DATA_HOME", "CODEX_HOME", "TMPDIR"):
            self.assertTrue(env[k].startswith("/r/runs/x/home/"), k)
            self.assertFalse(env[k].startswith("/r/private"), k)
        self.assertEqual(env["ANTHROPIC_API_KEY"], "sk-test", "auth is forwarded explicitly")
        self.assertEqual(env["PATH"], self.BASE["PATH"])
        self.assertEqual(env["ADAGE_ARM"], "holdout")
        self.assertEqual(env["ADAGE_HOME"], "/r/private/adage-home")
        for k in ("CLAUDECODE", "AWS_SECRET_ACCESS_KEY"):
            self.assertNotIn(k, env)
        self.assertNotIn("/Users/op", json.dumps({k: v for k, v in env.items() if k != "PATH"}))
        fwd = runner.session_env("train", "/h", "/a", True, self.BASE, ["AWS_SECRET_ACCESS_KEY"])
        self.assertEqual(fwd["AWS_SECRET_ACCESS_KEY"], "nope", "forward_env passes named variables")
        self.assertNotIn("ADAGE_HOME", fwd, "training sessions run without hooks")
        raw = runner.session_env("adage", "/h", "/a", False, self.BASE)
        self.assertEqual(raw["HOME"], "/Users/op", "--no-isolate keeps the operator's environment")
        self.assertEqual(raw["ADAGE_ARM"], "treated")

    def test_auth(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(runner.auth_source({"ANTHROPIC_API_KEY": "k", "HOME": tmp}), ("env", ["ANTHROPIC_API_KEY"]))
            self.assertIsNone(runner.auth_source({"HOME": tmp}))
            (Path(tmp) / ".claude").mkdir()
            (Path(tmp) / ".claude" / ".credentials.json").write_text("{}")
            self.assertEqual(runner.auth_source({"HOME": tmp})[0], "file")
            self.assertEqual(runner.auth_source({"CLAUDE_CONFIG_DIR": tmp + "/none", "HOME": tmp}), None)
            self.assertEqual(runner.auth_source({"HOME": "/nonexistent", "CLAUDE_CODE_USE_BEDROCK": "1"},
                                                ["CLAUDE_CODE_USE_BEDROCK"]), ("forward", ["CLAUDE_CODE_USE_BEDROCK"]))

    def test_the_plan_puts_homes_outside_the_private_tree(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = json.loads(dry_with(CONFIG, "--json", "--root", tmp))
            private = str(Path(tmp).resolve() / "private")
            self.assertTrue(p["isolation"]["enabled"])
            for s in p["sessions"]:
                self.assertTrue(s["home"].startswith(str(Path(tmp).resolve() / "runs")))
                self.assertFalse(s["home"].startswith(private) or s["cwd"].startswith(private))
                if s["env"]:
                    self.assertTrue(s["env"]["ADAGE_HOME"].startswith(private))
            off = json.loads(dry_with(CONFIG, "--json", "--root", tmp, "--no-isolate"))
            self.assertFalse(off["isolation"]["enabled"])
            self.assertIsNone(off["sessions"][0]["home"])


class Rounds(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()

    def tearDown(self):
        self.tmp.cleanup()

    def test_fresh_pools_per_round(self):
        path = write_config(self.tmp.name, tasks_by_round=[["INC-86", "INC-94", "INC-203"], ["INC-219", "INC-245", "INC-268"]])
        p = json.loads(dry_with(path, "--json", "--root", self.tmp.name))
        self.assertEqual(p["leaky_rounds"], [])
        self.assertEqual({s["task"] for s in p["sessions"] if s["round"] == 2 and s["phase"] == "eval"},
                         {"INC-219", "INC-245", "INC-268"})
        self.assertEqual(p["estimate"]["eval"]["runs"], 36)
        text = dry_with(path, "--root", self.tmp.name)
        self.assertNotIn("WARNING", text)
        self.assertIn("eval 3 tasks x 3 repeats x 2 arms (holdout, adage) x 2 rounds = 36", text)

    def test_uneven_pools_as_a_dict(self):
        path = write_config(self.tmp.name, tasks_by_round={"1": ["INC-86", "INC-94"], "2": ["INC-219", "INC-245", "INC-268"]})
        p = json.loads(dry_with(path, "--json", "--root", self.tmp.name))
        self.assertEqual(p["estimate"]["eval"]["tasks"], [2, 3])
        self.assertEqual(p["estimate"]["eval"]["runs"], 30)

    def test_overlapping_pools_are_refused(self):
        path = write_config(self.tmp.name, tasks_by_round=[["INC-86", "INC-94"], ["INC-94", "INC-245"]])
        with self.assertRaises(SystemExit):
            runner.load_config(path, {})
        path = write_config(self.tmp.name, tasks_by_round=[["INC-86"], ["INC-58"]])
        with self.assertRaises(SystemExit, msg="a training task cannot be held out"):
            runner.load_config(path, {})
        path = write_config(self.tmp.name, tasks_by_round=[["INC-86"]])
        with self.assertRaises(SystemExit, msg="two rounds need two pools"):
            runner.load_config(path, {})

    def test_the_old_behaviour_is_marked_leaky(self):
        text = dry_with(HARBOR, "--root", self.tmp.name)
        self.assertIn("WARNING: round(s) 2 learn from earlier sessions of the SAME held-out tasks", text)
        p = json.loads(dry_with(HARBOR, "--json", "--root", self.tmp.name))
        self.assertEqual(p["leaky_rounds"], [2])
        self.assertTrue(runner.summarise({"runs": {}}, 2)["leaky"])
        self.assertFalse(runner.summarise({"runs": {}, "fresh_pools": True, "rounds": {"2": {"leaky": False}}}, 2)["leaky"])

    def test_learning_leaves_out_the_rounds_own_tasks(self):
        def r(phase, task, rnd, arm="train"):
            return {"phase": phase, "task": task, "round": rnd, "arm": arm, "metrics": {"session_id": f"{phase}{task}{rnd}{arm}"}}
        runs = [r("train", "T0", 1), r("eval", "A", 1, "adage"), r("eval", "A", 1, "holdout"), r("eval", "B", 1, "holdout"),
                r("eval", "C", 2, "holdout"), {"phase": "eval", "task": "B", "round": 1, "arm": "adage"}]
        tasks = lambda rs: sorted((x["phase"], x["task"], x["round"]) for x in rs)  # noqa: E731
        self.assertEqual(tasks(runner.learning_runs(runs, 1)), [("train", "T0", 1)])
        self.assertEqual(tasks(runner.learning_runs(runs, 2, {"C", "D"})),
                         [("eval", "A", 1), ("eval", "A", 1), ("eval", "B", 1), ("train", "T0", 1)])
        self.assertEqual(tasks(runner.learning_runs(runs, 3, {"A", "C"})), [("eval", "B", 1), ("train", "T0", 1)],
                         "never a session of a task in this round's pool")
        self.assertEqual(len(runner.learning_runs(runs, 2, None)), 4, "without pools: every earlier session (leaky)")


class Redaction(unittest.TestCase):
    def test_home_is_redacted(self):
        state = {"runs": {"x": {"events": [{"cmd": "ls /Users/op/.claude/projects"}]}}}
        self.assertEqual(runner.demo_report.redact_home(state, "/Users/op")["runs"]["x"]["events"][0]["cmd"],
                         "ls ~/.claude/projects")

    def test_committed_results_name_no_home_directory(self):
        results = HERE.parent.parent / "demo" / "results"
        for p in results.rglob("*"):
            if p.is_file():
                text = p.read_text(errors="replace")
                self.assertNotIn("/Users/", text, str(p))
                self.assertIsNone(re.search(r"/home/(?!user\b)[a-z][\w.-]*/", text), str(p))


if __name__ == "__main__":
    unittest.main()
