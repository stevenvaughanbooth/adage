"""Registered launch-gate thresholds applied to synthetic rows; no paid evidence."""
import copy
import json
import unittest

import launch_gate_score as gs

CMD = ".venv/bin/python -m pytest tests/test_x.py -q"
REPOS = ("marshmallow", "click", "more-itertools")
REFLEXES = {r: dict(certified=True, provenance="explicitly guided training", command=CMD) for r in REPOS}


def row(repo, arm, rep=1, task="t", **changes):
    replay = dict(requests=[], successful_receipts=0, redundant_repeats=0, repeat_attempts=0, confirmed_elimination=False,
                  necessary_work_skipped=False)
    r = dict(repo=repo, task=task, arm=arm, repetition=rep, quality_passed=True, cost_usd=0.2, model_requests=30,
             executions=0, undeclared_writes=False, fresh_receipt=False, stale_receipts=0, replay=replay)
    if arm == "noexec":
        replay["requests"] = [dict(segment=CMD.replace("-q", "--tb=short"), classification="before_receipt")]
    if arm == "automatic":
        r.update(executions=1, fresh_receipt=True, model_requests=26, cost_usd=0.18)
        replay.update(successful_receipts=1, confirmed_elimination=True)
    for k, v in changes.items():
        (replay if k in replay else r)[k] = v
    return r


def cohort():
    return [row(repo, arm, rep, task) for repo in REPOS for task in ("t1", "t2", "t3", "t4")
            for arm in ("agent", "noexec", "automatic") for rep in (1, 2)]


class Thresholds(unittest.TestCase):
    def test_a_clean_cohort_is_go(self):
        result = gs.score(cohort(), REFLEXES)
        self.assertTrue(result["go"], json.dumps(result["criteria"]))
        self.assertEqual(result["criteria"]["G3"]["total"], 24)
        self.assertEqual(sorted(len(v["repositories"]) for v in result["strata"].values()), [1, 2])

    def test_each_registered_failure_mode_is_no_go(self):
        def no_go(mutate, criterion):
            rows = cohort()
            mutate(rows)
            result = gs.score(rows, REFLEXES)
            self.assertFalse(result["criteria"][criterion]["passed"], criterion)
            self.assertFalse(result["go"])
        autos = lambda rows: [r for r in rows if r["arm"] == "automatic"]
        def repeats(rows):
            for r in autos(rows):
                r["replay"].update(repeat_attempts=1, confirmed_elimination=False)
        no_go(repeats, "G3")
        no_go(repeats, "G4")
        no_go(lambda rows: autos(rows)[0].update(quality_passed=False), "G5")
        no_go(lambda rows: autos(rows)[0]["replay"].update(necessary_work_skipped=True), "G6")
        no_go(lambda rows: rows[0].update(executions=1), "G6")
        no_go(lambda rows: rows[0].update(cost_usd=None), "G6")
        no_go(lambda rows: [r.update(stale_receipts=1) for r in autos(rows)[:7]], "G7")
        no_go(lambda rows: [r.update(model_requests=40) for r in autos(rows)], "G8")
        no_go(lambda rows: [r.update(executions=0) for r in autos(rows)[:13]], "G2")
        def habitless(rows):
            for r in rows:
                if r["arm"] == "noexec":
                    r["replay"]["requests"] = [dict(segment=CMD.replace("tests/test_x.py", "tests/test_x.py::one"))]
        no_go(habitless, "G4")
        def unseen_repeats(rows):
            for r in autos(rows):
                if r["repo"] == "more-itertools":
                    r["replay"].update(repeat_attempts=1, confirmed_elimination=False)
        no_go(unseen_repeats, "G9")
        reflexes = copy.deepcopy(REFLEXES)
        reflexes["click"]["certified"] = reflexes["more-itertools"]["certified"] = False
        self.assertFalse(gs.score(cohort(), reflexes)["criteria"]["G1"]["passed"])

    def test_equivalence_matches_the_registered_classifier(self):
        self.assertTrue(gs.equivalent(CMD + " --tb=short -p no:cacheprovider", CMD))
        self.assertFalse(gs.equivalent("python -m pytest tests/test_x.py -q", CMD))
        self.assertFalse(gs.equivalent(CMD + " -k one", CMD))

    def test_thresholds_come_from_the_registered_protocol(self):
        protocol = json.loads(gs.PROTOCOL.read_text())
        self.assertEqual(protocol["protocol"], "launch-gate-1")
        self.assertEqual(protocol["go_thresholds"]["G3_confirmed_eliminations_total_min"], 8)


if __name__ == "__main__":
    unittest.main()
