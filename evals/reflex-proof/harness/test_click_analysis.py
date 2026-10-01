"""Click r13 analysis: the matrix, the budget, both gated co-primaries, censoring, BEST drop and the record."""
import json
import random
import unittest

import click_analysis as a
import click_tasks
import click_writeup
import heldout_analysis as r9
import split_audit

PROTOCOL = json.loads(split_audit.PROTOCOL.read_text())
SPEC = PROTOCOL["click_heldout_analysis"]


def context(outcome, digest):
    return dict(kind="receipt", origin="context", phase="completed", outcome=outcome, digest=digest)


def rows(mode, means=None, seed=1, **overrides):
    means = means or dict(A0=30, NOEXEC=24, BEST=20)
    rng = random.Random(seed)
    out = a.matrix(click_tasks.EVALUATION, mode)
    for r in out:
        r.update(status="collected", result_status="success", result_is_error=False, verified=True, cost_usd=0.2,
                 quality_passed=True, verification_requests=1, receipts=[], timeline={}, replay={},
                 contract_emitted=r["arm"] == "BEST", requests=means[r["arm"]] + rng.randint(-3, 3))
        if r["condition"] != "positive":
            final = "other" if r["condition"] in ("later-edit", "config-change") else "d"
            r.update(timeline=dict(events=[context("failed" if r["condition"] == "failed" else "applied", "d")],
                                   final_digest=final), replay=dict(ok=True))
        elif r["arm"] == "BEST":
            r.update(timeline=dict(events=[context("applied", "f")], final_digest="f"))
    for (order, key), value in overrides.items():
        out[order][key] = value
    return out


def find(rs, arm, block):
    return next(i for i, r in enumerate(rs) if r["arm"] == arm and r["block"] == block)


class Registration(unittest.TestCase):
    def test_protocol_and_module_agree(self):
        self.assertEqual(SPEC["revision"], a.REVISION)
        self.assertEqual(SPEC["co_primaries"]["execution"]["upper_bound_max"], a.RATIO_MAX)
        self.assertEqual(SPEC["co_primaries"]["advisory"]["upper_bound_max"], a.RATIO_MAX)
        self.assertEqual(SPEC["co_primaries"]["max_excluded_blocks"], a.MAX_EXCLUDED_BLOCKS)
        self.assertEqual(SPEC["sensitivity"]["without_task"], a.SENSITIVITY_TASK)
        self.assertEqual(SPEC["caches"], list(a.CACHES))
        self.assertEqual(SPEC["budget"]["per_session_cap_usd"], dict(native=a.NATIVE_CAP_USD, training=a.TRAINING_CAP_USD,
                                                                     heldout=a.SESSION_CAP_USD))
        self.assertEqual(SPEC["repetitions"], a.REPETITIONS)
        self.assertEqual((a.SEED, a.BOOTSTRAP, a.QUALITY_MARGIN, a.CAP_REQUESTS), (20261001, 20000, 1, 60))
        self.assertEqual(SPEC["tasks"], click_tasks.EVALUATION)
        self.assertIn(a.SENSITIVITY_TASK, click_tasks.EVALUATION)


class Matrix(unittest.TestCase):
    def test_execution_mode_runs_controls_first_then_three_arm_blocks(self):
        m = a.matrix(click_tasks.EVALUATION, "execution")
        self.assertEqual(len(m), 65)
        self.assertEqual([r["condition"] for r in m[:5]], list(a.CONTROLS))
        self.assertTrue(all(r["arm"] == "BEST" and r["task"] == click_tasks.CONTROL_TASK for r in m[:5]))
        blocks = {}
        for r in m[5:]:
            blocks.setdefault(r["block"], []).append(r)
        self.assertEqual(len(blocks), 20)
        for n, b in blocks.items():
            self.assertEqual(sorted(r["arm"] for r in b), ["A0", "BEST", "NOEXEC"])
            self.assertEqual(len({r["task"] for r in b}), 1)
        self.assertEqual([r["order"] for r in m], list(range(65)))
        self.assertEqual(m, a.matrix(click_tasks.EVALUATION, "execution"))

    def test_advisory_mode_runs_two_arm_blocks_and_no_controls(self):
        m = a.matrix(click_tasks.EVALUATION, "advisory")
        self.assertEqual(len(m), 40)
        self.assertTrue(all(r["condition"] == "positive" and r["arm"] in ("A0", "NOEXEC") for r in m))
        with self.assertRaises(ValueError):
            a.matrix(click_tasks.EVALUATION, "other")


class Budget(unittest.TestCase):
    def test_registration_budget_does_not_fit_and_names_the_shortfall(self):
        b = a.budget(20.1084449)
        ex, adv = b["modes"]["execution"], b["modes"]["advisory"]
        self.assertEqual((ex["heldout_sessions"], adv["heldout_sessions"]), (65, 40))
        self.assertEqual((ex["chain_worst_usd"], adv["chain_worst_usd"]), (45.5, 30.5))
        self.assertFalse(ex["fits"] or adv["fits"])
        self.assertAlmostEqual(ex["shortfall_usd"], SPEC["budget"]["shortfall_usd"]["execution_chain"])
        self.assertAlmostEqual(adv["shortfall_usd"], SPEC["budget"]["shortfall_usd"]["advisory_chain"])
        self.assertAlmostEqual(ex["ceiling_needed_usd"], SPEC["budget"]["ceiling_needed_usd"]["execution_chain"])
        self.assertEqual(ex["chain_expected_usd"], SPEC["budget"]["expected_usd"]["chain_execution"])
        self.assertEqual(adv["chain_expected_usd"], SPEC["budget"]["expected_usd"]["chain_advisory"])

    def test_enough_allocation_fits(self):
        self.assertTrue(a.budget(48.5)["modes"]["execution"]["fits"])
        self.assertFalse(a.budget(48.4)["modes"]["execution"]["fits"])


class Analysis(unittest.TestCase):
    def test_both_co_primaries_pass_on_clear_data(self):
        res = a.analyze(rows("execution"), "execution", route="t/learned/run-x")
        self.assertEqual(res["outcomes"], dict(advisory="pass", execution="pass"))
        self.assertLess(res["execution"]["primary"]["upper"], 0.9)
        self.assertEqual(set(res["per_task"]), set(click_tasks.EVALUATION))
        self.assertEqual(set(res["per_task"][a.SENSITIVITY_TASK]), {"A0", "NOEXEC", "BEST", "NOEXEC/A0", "BEST/A0", "BEST/NOEXEC"})
        self.assertIn("best_vs_noexec", res["execution"]["report_only"])

    def test_advisory_mode_evaluates_advisory_only(self):
        res = a.analyze(rows("advisory"), "advisory")
        self.assertEqual(res["outcomes"], dict(advisory="pass", execution="not-run"))
        self.assertNotIn("BEST", res["arms"])

    def test_advisory_fails_on_its_own_bound_or_quality(self):
        res = a.analyze(rows("advisory", means=dict(A0=30, NOEXEC=29)), "advisory")
        self.assertEqual(res["outcomes"]["advisory"], "fail")
        rs = rows("advisory")
        for block in (1, 2):
            rs[find(rs, "NOEXEC", block)]["quality_passed"] = False
        self.assertFalse(a.analyze(rs, "advisory")["advisory"]["quality"]["passed"])

    def test_advisory_requires_no_execution_in_plain_arms(self):
        rs = rows("advisory")
        rs[find(rs, "NOEXEC", 3)]["receipts"] = [dict(route="x")]
        res = a.analyze(rs, "advisory")
        self.assertEqual(res["outcomes"]["advisory"], "fail")
        self.assertFalse(res["advisory"]["safety"]["passed"])
        rs = rows("advisory")
        rs[find(rs, "A0", 3)]["cost_usd"] = None
        self.assertEqual(a.analyze(rs, "advisory")["outcomes"]["advisory"], "fail")

    def test_censoring_imputes_only_the_arm_under_test(self):
        rs = rows("execution")
        i = find(rs, "NOEXEC", 4)
        rs[i].update(result_status="error_max_turns", requests=10)
        res = a.analyze(rs, "execution", route="r")
        block4 = [p for p in res["request_verification"] if p["block"] == 4]
        self.assertTrue(block4)
        self.assertEqual(res["advisory"]["quality"]["adage"], 19)
        self.assertEqual(res["execution"]["go_criteria"]["G5"]["failures"]["NOEXEC"], 1)
        self.assertEqual(res["advisory"]["sensitivity"]["without_censored_blocks"]["excluded_blocks"], [4])
        eff = a.contrast(a._blocks(rs, ("A0", "NOEXEC")), "NOEXEC", "A0", imputed="NOEXEC")["eff"]
        self.assertEqual(eff[4]["NOEXEC"]["requests"], 60)
        eff = a.contrast(a._blocks(rs, ("A0", "NOEXEC", "BEST")), "BEST", "A0", imputed="BEST")["eff"]
        self.assertEqual(eff[4]["A0"]["requests"], rs[find(rs, "A0", 4)]["requests"])

    def test_unverified_blocks_beyond_two_make_a_contrast_unverified(self):
        rs = rows("execution")
        for block in (1, 2, 3):
            rs[find(rs, "BEST", block)]["verified"] = False
        res = a.analyze(rs, "execution", route="r")
        self.assertEqual(res["outcomes"], dict(advisory="pass", execution="unverified"))
        for block in (1, 2, 3):
            rs[find(rs, "A0", block)]["verified"] = False
        self.assertEqual(a.analyze(rs, "execution", route="r")["outcomes"]["advisory"], "unverified")

    def test_a_dropped_best_fails_execution_but_advisory_is_still_evaluated(self):
        rs = rows("execution")
        rs[1].update(timeline=dict(events=[], final_digest="d"))  # later-edit unrealized
        for r in rs:
            if r["arm"] == "BEST" and r["condition"] == "positive":
                r["status"] = "uncollected"
        res = a.analyze(rs, "execution", route="r", best_dropped=dict(order=5, reason="a control was unrealized"))
        self.assertEqual(res["outcomes"], dict(advisory="pass", execution="protocol-failure"))
        self.assertTrue(any("unrealized" in p for p in res["execution"]["problems"]))

    def test_an_incomplete_study_has_no_verdict(self):
        rs = rows("execution")
        rs[find(rs, "A0", 20)]["status"] = "uncollected"
        res = a.analyze(rs, "execution", route="r")
        self.assertEqual(res["outcomes"], dict(advisory="incomplete", execution="incomplete"))
        rs = [dict(r, status="uncollected") for r in rows("execution")]
        self.assertEqual(a.analyze(rs, "execution", route="r")["outcomes"], dict(advisory="incomplete", execution="incomplete"))

    def test_a_failing_sensitivity_is_reported_prominently(self):
        rs = rows("advisory", means=dict(A0=30, NOEXEC=22))
        for r in rs:
            if r["task"] == a.SENSITIVITY_TASK:
                r["requests"] = 30 if r["arm"] == "A0" else 10
            elif r["arm"] == "NOEXEC":
                r["requests"] = 30
        res = a.analyze(rs, "advisory")
        if res["advisory"]["primary"]["passed"]:
            self.assertTrue(any(w.startswith("PROMINENT (advisory)") for w in res["warnings"]))
        self.assertFalse(res["advisory"]["sensitivity"]["without_key_value"]["passed"])

    def test_the_record_renders_for_every_outcome(self):
        cases = [a.analyze(rows("execution"), "execution", route="r"), a.analyze(rows("advisory"), "advisory"),
                 a.analyze([dict(r, status="uncollected") for r in rows("execution")], "execution", route="r")]
        for res in cases:
            text = click_writeup.render(dict(res, assignments=65, collected=0))
            self.assertIn("## Outcomes (two separate claims; never combined)", text)
            self.assertIn("No GPU savings", text)
            self.assertIn("Advisory mode (NOEXEC vs A0)", text)

    def test_r9_rules_are_reused_unchanged(self):
        self.assertIs(a.r9.compare, r9.compare)
        self.assertEqual(a.GO_CRITERIA["G3"], r9.GO_CRITERIA["G3"])
        self.assertTrue(all(a.GO_CRITERIA[g]["applies"] for g in ("G1", "G2", "G5", "G6", "G7", "G8")))
        self.assertFalse(any(a.GO_CRITERIA[g]["applies"] for g in ("G3", "G4", "G9")))


if __name__ == "__main__":
    unittest.main()
