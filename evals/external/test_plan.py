import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path

import plan

HERE = Path(__file__).resolve().parent
FIX = HERE / "fixtures"
JOB = FIX / "jobs" / "swebenchpro-eval-ansible__ansible-r1"
TASK = "instance_ansible__ansible-1111111111111111111111111111111111111111-v1"


def run(argv):
    buf = io.StringIO()
    with contextlib.redirect_stdout(buf):
        rc = plan.main(argv)
    return rc, buf.getvalue()


class EmitTest(unittest.TestCase):
    def records(self):
        with tempfile.TemporaryDirectory() as d:
            out = Path(d) / "r.jsonl"
            rc, _ = run(["emit", "--jobs", str(JOB), "--suite", "swebenchpro", "--split", str(FIX / "split.json"),
                         "--out", str(out)])
            self.assertEqual(rc, 0)
            return [json.loads(line) for line in out.read_text().splitlines()]

    def test_one_record_per_trial_not_the_job(self):
        recs = self.records()
        self.assertEqual(len(recs), 4)
        for r in recs:
            for k in ("run_id", "tier", "suite", "task_id", "repo_key", "arm", "repeat", "status", "outcome", "cost",
                      "efficiency", "harness", "model"):
                self.assertIn(k, r)
            self.assertEqual(r["tier"], "l4")
            self.assertEqual(r["task_id"], TASK)
            self.assertEqual(r["repo_key"], "ansible__ansible")

    def test_statuses_outcomes_and_costs(self):
        by = {r["run_id"].rsplit("__", 1)[1]: r for r in self.records()}
        a, b, c, d = by["AaAaAa"], by["BbBbBb"], by["CcCcCc"], by["DdDdDd"]
        self.assertEqual((a["status"], a["outcome"]["passed"], a["arm"]), ("ok", True, "adage"))
        self.assertEqual(a["cost"], {"usd": 0.5, "input_tokens": 120000, "cache_tokens": 100000, "output_tokens": 4000})
        self.assertEqual(a["efficiency"]["turns"], 12, "from the stream-json result event")
        self.assertEqual(a["efficiency"]["agent_seconds"], 600.0)
        self.assertEqual(a["adage"]["spool_records"], 2)
        self.assertEqual(a["adage"]["sessions"], ["sess-a"], "subagent transcripts excluded")
        self.assertEqual(a["harness"]["adage_version"], "adage 0.1.0")
        self.assertEqual(a["model"], "claude-sonnet-4-5")
        self.assertEqual((b["status"], b["outcome"]["passed"], b["arm"]), ("censored", False, "holdout"))
        self.assertEqual((c["status"], c["outcome"]["passed"], c["arm"]), ("infra_error", None, "off"))
        self.assertTrue(d["adage"].get("hooks_inert"), "a hook arm with no spool is flagged")

    def test_repeats_by_start_time(self):
        by = {r["run_id"].rsplit("__", 1)[1]: r for r in self.records()}
        self.assertEqual((by["AaAaAa"]["repeat"], by["DdDdDd"]["repeat"]), (1, 2))
        self.assertEqual(by["BbBbBb"]["repeat"], 1)

    def test_status_rules(self):
        self.assertEqual(plan.status_of({"verifier_result": {"rewards": {"reward": 1}}}, {}), "ok")
        self.assertEqual(plan.status_of({"verifier_result": None}, {}), "scorer_error")
        self.assertEqual(plan.status_of({"exception_info": {"exception_type": "RewardFileNotFoundError"}}, {}),
                         "scorer_error")
        self.assertEqual(plan.status_of({"verifier_result": {"rewards": {"reward": 0}}}, {"subtype": "error_max_turns"}),
                         "censored")
        self.assertEqual(plan.status_of({"exception_info": {"exception_type": "AgentTimeoutError"}}, {}), "infra_error",
                         "a timeout that was never verified has no outcome")

    def test_transcript_turn_fallback(self):
        files = plan.session_files(JOB / "instance_ansible__ansible-111__AaAaAa")
        self.assertEqual(plan.transcript_turns(files), 2)


class HarvestAndChecksTest(unittest.TestCase):
    def test_harvest_takes_only_train_tasks_of_arm_off(self):
        split = json.loads((FIX / "split.json").read_text())
        split["repos"]["ansible/ansible"]["train"] = [TASK]
        split["repos"]["ansible/ansible"]["test"] = []
        with tempfile.TemporaryDirectory() as d:
            sp = Path(d) / "split.json"
            sp.write_text(json.dumps(split))
            rc, out = run(["harvest", "--jobs", str(JOB), "--split", str(sp), "--out", str(Path(d) / "h")])
            self.assertEqual(rc, 0)
            # Every fixture trial is a hook arm or failed infra-wise arm off with no transcript.
            self.assertIn("not arm off", out)
            self.assertFalse((Path(d) / "h" / "ansible__ansible" / "sess-a.jsonl").exists())

    def test_harvest_copies_transcripts_and_outcomes(self):
        split = json.loads((FIX / "split.json").read_text())
        split["repos"]["ansible/ansible"]["train"] = [TASK]
        split["repos"]["ansible/ansible"]["test"] = []
        with tempfile.TemporaryDirectory() as d:
            job = Path(d) / "job"
            import shutil
            shutil.copytree(JOB, job)
            res = job / "instance_ansible__ansible-111__AaAaAa" / "result.json"
            r = json.loads(res.read_text())
            r["config"]["agent"]["kwargs"]["arm"] = "off"
            res.write_text(json.dumps(r))
            sp = Path(d) / "split.json"
            sp.write_text(json.dumps(split))
            rc, out = run(["harvest", "--jobs", str(job), "--split", str(sp), "--out", str(Path(d) / "h")])
            self.assertEqual(rc, 0)
            h = Path(d) / "h" / "ansible__ansible"
            self.assertTrue((h / "sess-a.jsonl").exists())
            self.assertEqual(json.loads((h / "outcomes.json").read_text()), {"sess-a": True})

    def test_check_patches_flags_the_dirty_trial(self):
        rc, out = run(["check-patches", "--jobs", str(JOB)])
        self.assertEqual(rc, 1)
        self.assertIn("BbBbBb", out)
        self.assertNotIn("AaAaAa", out)
        self.assertIn("2 trial worktree(s) checked, 1 with", out)


class PhasesTest(unittest.TestCase):
    def registry(self, d):
        tasks = [{"name": f"instance_o__{r}-{i:040x}-v1", "git_url": "https://example/ds.git", "git_commit_id": "c0",
                  "path": f"datasets/x/instance_o__{r}-{i:040x}-v1"} for r in ("a", "b") for i in range(12)]
        p = Path(d) / "registry.json"
        p.write_text(json.dumps([{"name": "swebenchpro", "version": "1.0", "description": "", "tasks": tasks}]))
        split = {"protocol": "adage-external/1", "train_frac": 0.5, "buffer": 2, "min_test": 3, "excluded": {},
                 "unassigned": [], "repos": {
                     f"o/{r}": {"train": [t["name"] for t in tasks if f"__{r}-" in t["name"]][:6],
                                "buffer": [t["name"] for t in tasks if f"__{r}-" in t["name"]][6:8],
                                "test": [t["name"] for t in tasks if f"__{r}-" in t["name"]][8:]} for r in ("a", "b")}}
        s = Path(d) / "split.json"
        s.write_text(json.dumps(split))
        return p, s

    def test_phases_configs_are_seeded_and_complete(self):
        with tempfile.TemporaryDirectory() as d:
            reg, sp = self.registry(d)
            outs = []
            for _ in range(2):
                cfgs = Path(d) / f"cfg{len(outs)}"
                rc, out = run(["phases", "--registry", str(reg), "--dataset", "swebenchpro@1.0", "--split", str(sp),
                               "--repeats", "2", "--seed", "7", "--write-configs", str(cfgs), "--leave-repo-out",
                               "--adage-bin", "/abs/adage"])
                self.assertEqual(rc, 0)
                outs.append((out.replace(str(cfgs), "CFG"), sorted(f.read_text() for f in cfgs.glob("*.json"))))
            self.assertEqual(outs[0], outs[1], "same seed, same plan")
            out = outs[0][0]
            self.assertIn("COST ESTIMATE", out)
            self.assertIn("NOT measured", out)
            for step in ("1. train", "2. harvest", "3. adage condition", "4. freeze", "5. eval", "6. patch"):
                self.assertIn(step, out)
            cfgs = [json.loads(t) for t in outs[0][1]]
            train = [c for c in cfgs if c["job_name"].endswith("-train")][0]
            self.assertEqual(len(train["tasks"]), 12)
            self.assertEqual({a["kwargs"]["arm"] for a in train["agents"]}, {"off"})
            evals = [c for c in cfgs if "-eval-" in c["job_name"]]
            self.assertEqual(len(evals), 4)  # 2 repos x 2 repeats
            for c in evals:
                labels = sorted(a["kwargs"].get("arm_label") or a["kwargs"]["arm"] for a in c["agents"])
                self.assertEqual(labels, ["adage", "adage-lro", "holdout", "off"])
                self.assertEqual(len(c["tasks"]), 4)
                repo = c["job_name"].split("-eval-")[1].rsplit("-r", 1)[0]
                for a in c["agents"]:
                    kw = a["kwargs"]
                    self.assertTrue(kw["disable_web_search"])
                    if kw["arm"] == "off":
                        self.assertNotIn("adage_home", kw)
                    elif kw.get("arm_label") == "adage-lro":
                        self.assertNotIn(repo, kw["adage_repo_key"])
                    else:
                        self.assertTrue(kw["adage_repo_key"].endswith(repo))
            # No test task is trained on.
            test_names = {t["path"] for c in evals for t in c["tasks"]}
            self.assertFalse(test_names & {t["path"] for t in train["tasks"]})

    def test_pilot_and_estimate_without_split(self):
        with tempfile.TemporaryDirectory() as d:
            reg, sp = self.registry(d)
            rc, out = run(["phases", "--registry", str(reg), "--dataset", "swebenchpro@1.0", "--split",
                           str(Path(d) / "none.json")])
            self.assertEqual(rc, 0)
            self.assertIn("ESTIMATE (not measured)", out)
            rc, out = run(["phases", "--registry", str(reg), "--dataset", "swebenchpro@1.0", "--split", str(sp),
                           "--pilot", "3", "--repeats", "1"])
            self.assertIn("PILOT: 2 repositories, 6 train and 3 test tasks", out)

    def test_power_command(self):
        rc, out = run(["power"])
        self.assertEqual(rc, 0)
        self.assertIn("discordance", out)


if __name__ == "__main__":
    unittest.main()
