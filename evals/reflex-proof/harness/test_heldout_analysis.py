"""Revision 9 held-out analysis: selection, matrix, budget, classifier, request verification, censoring,
sensitivity analyses, G1-G9 and the verdict."""
import json
from pathlib import Path
import tempfile
import unittest

import heldout_analysis as analysis
import heldout_tasks
import split_audit

PROTOCOL = json.loads(split_audit.PROTOCOL.read_text())
TASKS = heldout_tasks.EVALUATION
DRAWS = 2000
ROUTE = "github-com-marshmallow-code-marshmallow/learned/run-175d85"


def dev_report(a2=(True, 20.0, 0.10), a3=(True, 18.0, 0.12)):
    arms = {"A0": dict(requests=25.0), "A1": dict(requests=10.0, cost_usd=0.01)}
    verdict = {}
    for name, (passed, requests, cost) in (("A2", a2), ("A3", a3)):
        arms[name] = dict(requests=requests, cost_usd=cost)
        verdict[name] = dict(passed=passed)
    return dict(arms=arms, verdict=verdict)


def receipt(digest, outcome="applied"):
    return dict(kind="receipt", phase="completed", outcome=outcome, digest=digest)


def journal(changed=(".pytest_cache/v/cache/nodeids",), route=ROUTE, session="s"):
    return dict(route=route, session=session, phase="completed",
                outcome=dict(outcome="applied", changed=[dict(change="created", path=p) for p in changed]))


def control(condition, realized=True, skipped=False):
    final = "final"
    if condition == "failed":
        events = [receipt("x", outcome="failed")] if realized else []
    elif condition in ("later-edit", "config-change"):
        events = [receipt("stale" if realized else final)]
    else:
        events = [receipt(final)] if realized else []
    return dict(condition=condition, arm="BEST", status="collected", cost_usd=0.1, block=None,
                timeline=dict(events=events, final_digest=final),
                replay=dict(necessary_work_skipped=skipped, repeat_attempts=0), receipts=[journal()], session_id="s")


def session(arm, requests, passed=True, status="success", cost=0.12, verified=True):
    row = dict(arm=arm, condition="positive", status="collected", requests=requests, cross_check=verified,
               verified=verified, stream_request_ids=requests, api_error_entries=0, result_status=status,
               result_is_error=status != "success", quality_passed=passed, cost_usd=cost, verification_requests=1,
               session_id="s", receipts=[], timeline=dict(events=[], final_digest="final"), replay={})
    if arm == "BEST":
        row.update(receipts=[journal()], timeline=dict(events=[receipt("final")], final_digest="final"))
    return row


def rows(best=lambda a0, task: round(a0 * 0.6), noexec=lambda a0, task: a0, a0_requests=(20, 30, 25, 35, 28), edit=None):
    out = [dict(control(c), order=i) for i, c in enumerate(analysis.CONTROLS)]
    for r in analysis.matrix(TASKS):
        if r["condition"] != "positive":
            continue
        a0 = a0_requests[(r["block"] - 1) % len(a0_requests)]
        value = {"A0": a0, "NOEXEC": noexec(a0, r["task"]), "BEST": best(a0, r["task"])}[r["arm"]]
        out.append(dict(r, **session(r["arm"], value)))
    if edit:
        edit(out)
    return out


def positives(out, arm):
    return [r for r in out if r["arm"] == arm and r["condition"] == "positive"]


def run(out, **kw):
    return analysis.analyze(out, "A3", draws=DRAWS, route=ROUTE, **kw)


class Selection(unittest.TestCase):
    def test_lowest_mean_requests_among_arms_passing_d1_to_d4(self):
        self.assertEqual(analysis.select_best(dev_report()), "A3")
        self.assertEqual(analysis.select_best(dev_report(a3=(False, 10.0, 0.01))), "A2")
        self.assertEqual(analysis.select_best(dev_report(a2=(True, 18.0, 0.10))), "A2")  # tie: lower cost
        self.assertIsNone(analysis.select_best(dev_report(a2=(False, 1, 0), a3=(False, 1, 0))))  # nothing runs
        self.assertNotEqual(analysis.select_best(dev_report()), "A1")


class Mod(unittest.TestCase):
    """T-E3-01's script half: the analysis takes MOD as BEST by name."""

    def test_a_reports_own_candidates_select_mod(self):
        report = dict(candidates=["MOD"], arms=dict(A4=dict(requests=12.0), MOD=dict(requests=13.0)),
                      verdict=dict(A4=dict(passed=True), MOD=dict(passed=True)))
        self.assertEqual(analysis.select_best(report), "MOD", "A4 has no controls in the MOD design")
        self.assertIsNone(analysis.select_best(dict(report, verdict=dict(MOD=dict(passed=False)))))
        self.assertEqual(analysis.select_best(dev_report()), "A3", "dev-arms-1 reports keep their candidates")

    def test_rows_naming_best_by_its_development_arm_are_best(self):
        named = [dict(r, arm="MOD") if r["arm"] == "BEST" else r for r in rows()]
        mod = analysis.analyze(named, "MOD", draws=DRAWS, route=ROUTE)
        best = analysis.analyze(rows(), "MOD", draws=DRAWS, route=ROUTE)
        self.assertEqual(mod["outcome"], "pass", mod["problems"])
        self.assertEqual((mod["primary"], mod["best_arm"]), (best["primary"], "MOD"))


class Matrix(unittest.TestCase):
    def test_controls_first_then_paired_blocks_in_a_recorded_random_order(self):
        m = analysis.matrix(TASKS)
        self.assertEqual(m, analysis.matrix(TASKS))
        self.assertEqual([r["order"] for r in m], list(range(len(m))))
        controls, blocks_rows = m[:5], m[5:]
        self.assertEqual([r["condition"] for r in controls], list(analysis.CONTROLS))
        self.assertTrue(all(r["arm"] == "BEST" and r["task"] == "mm-curve--finite" for r in controls))
        self.assertEqual(len(blocks_rows), len(TASKS) * 5 * 3)
        blocks = {}
        for r in blocks_rows:
            blocks.setdefault(r["block"], []).append(r)
        self.assertEqual(len(blocks), 20)
        for rs in blocks.values():
            self.assertEqual(sorted(r["arm"] for r in rs), sorted(analysis.ARMS))
            self.assertEqual(len({(r["task"], r["repetition"]) for r in rs}), 1)
        self.assertGreater(len({tuple(r["arm"] for r in rs) for rs in blocks.values()}), 1)
        reps = [r["repetition"] for r in blocks_rows]
        self.assertEqual(reps, sorted(reps))
        self.assertTrue(all(sum(r["task"] == t for r in blocks_rows) == 15 for t in TASKS))
        with self.assertRaises(ValueError):
            analysis.matrix(TASKS[:3])

    def test_worst_case_equals_the_held_out_reserve(self):
        b = analysis.budget(len(analysis.matrix(TASKS)))
        self.assertEqual((b["sessions"], b["per_session_cap_usd"], b["ceiling_usd"]), (65, 0.40, 26.0))
        self.assertEqual(b["worst_case_usd"], 26.0)
        self.assertTrue(b["fits"])
        self.assertEqual(b["max_sessions_at_cap"], 65)
        self.assertFalse(analysis.budget(66)["fits"])
        self.assertEqual(b["worst_case_at_r8_cap_usd"], 97.5)


class Classifier(unittest.TestCase):
    def test_verification_forms(self):
        cases = {
            ".venv/bin/python -m pytest tests/test_validate.py -q": ["pytest"],
            "pytest -x": ["pytest"],
            ".venv/bin/pytest tests -k finite": ["pytest"],
            "uv run pytest -q": ["pytest"],
            "uv run --with pytest python -m pytest tests": ["pytest"],
            "cd /repo && source .venv/bin/activate && py.test": ["pytest"],
            "timeout 60 python3 -m pytest 2>&1 | tail -5": ["pytest"],
            "PYTHONPATH=src python -c 'from marshmallow import validate; print(validate.Finite()(1))'": ["python-c"],
            "python3 -c \"import json; print(json.loads('{}'))\"": ["python-c"],
            "python3 <<'EOF'\nfrom marshmallow import validate\nprint(validate.Finite()(1))\nEOF": ["inline-python"],
            "cat check.py | .venv/bin/python": ["inline-python"],
            "tox -e py312": ["tox"],
            "python -m unittest discover": ["unittest"],
            "python /tmp/test_finite_manual.py": ["test-script"],
            "python -W ignore -m pytest": ["pytest"],
            "python -m pytest -q && python -c 'print(1)'": ["pytest", "python-c"],
        }
        for command, kinds in cases.items():
            self.assertEqual(analysis.verification_kinds(command), kinds, command)

    def test_non_verification_commands(self):
        for command in ("grep -rn pytest .", "pip install pytest", "python -m pip install -e .", "ls tests",
                        "cat .pytest_cache/README.md", "git diff", "python setup.py --version", "echo 'pytest; tox'",
                        "sed -n 1,20p tests/test_validate.py", "python3 -m py_compile src/marshmallow/validate.py"):
            self.assertEqual(analysis.verification_kinds(command), [], command)

    def test_unbalanced_quotes_fall_back_to_a_plain_tokenizer(self):
        self.assertEqual(analysis.verification_kinds("python3 <<EOF\nprint(\"it's\nEOF"), ["inline-python"])


def debug(n, errors=0):
    lines = ["2026-10-01T00:00:%02dZ [DEBUG] [API REQUEST] /v1/messages x-client-request-id=c%d source=sdk" % (i, i)
             for i in range(n)]
    lines += ["2026-10-01T00:01:%02dZ [ERROR] API error (attempt 1/11): undefined Connection error." % i for i in range(errors)]
    return "\n".join(lines + ["2026-10-01T00:02:00Z [DEBUG] other line"])


def stream(n):
    return [dict(value=dict(type="assistant", request_id="req_%d" % i)) for i in range(n)]


class Requests(unittest.TestCase):
    def test_debug_dispatches_count_retries_and_the_cross_check_is_reported(self):
        self.assertEqual(analysis.requests(debug(3), stream(3)),
                         dict(requests=3, stream_request_ids=3, api_error_entries=0, cross_check=True, verified=True))
        explained = analysis.requests(debug(62, errors=2), stream(60))  # the archived v5 hex-color retry case
        self.assertEqual((explained["requests"], explained["cross_check"], explained["verified"]), (62, False, True))

    def test_unexplained_mismatches_are_unverified(self):
        self.assertFalse(analysis.requests(debug(5, errors=1), stream(3))["verified"])  # surplus 2 > 1 error
        self.assertFalse(analysis.requests(debug(3, errors=2), stream(4))["verified"])  # fewer dispatches than ids
        self.assertFalse(analysis.requests(debug(3), stream(2))["verified"])

    def test_censoring_is_conservative_against_adage(self):
        for status in ("error_max_turns", "error_max_budget_usd", None):
            row = dict(session("A0", 12), result_status=status, result_is_error=True)
            self.assertEqual(analysis.effective(row, "BEST"), dict(requests=60, passed=False, censored=True))
            self.assertEqual(analysis.effective(row, "A0"), dict(requests=12, passed=False, censored=True))
            self.assertEqual(analysis.effective(row, "NOEXEC"), dict(requests=12, passed=False, censored=True))
        self.assertEqual(analysis.effective(dict(session("BEST", 75), result_status="error_max_turns"), "BEST")["requests"], 75)
        self.assertEqual(analysis.effective(session("BEST", 12), "BEST"), dict(requests=12, passed=True, censored=False))

    def test_session_row_reads_the_collected_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = Path(tmp)
            (s / "claude-debug.log").write_text(debug(2) + "\n")
            (s / "stream-observations.jsonl").write_text("\n".join(json.dumps(x) for x in stream(2)))
            tool = lambda c: dict(type="tool_use", name="Bash", input=dict(command=c))  # noqa: E731
            (s / "stdout.jsonl").write_text("\n".join(json.dumps(x) for x in (
                dict(type="assistant", message=dict(id="m1", content=[tool(".venv/bin/python -m pytest -q")])),
                dict(type="assistant", message=dict(id="m2", content=[tool("ls"), tool("python -c 'print(1)'")])))))
            (s / "summary.json").write_text(json.dumps(dict(result_status="success", result_is_error=False, num_turns=2,
                                                            total_cost_usd=0.05)))
            assignment = dict(arm="A0", condition="positive", block=1, task=TASKS[0])
            self.assertEqual(analysis.session_row(s, assignment)["status"], "uncollected")  # collection pending
            (s / "collection.json").write_text(json.dumps(dict(quality=dict(passed=True), provenance=dict(session_id="x"))))
            row = analysis.session_row(s, assignment)
            self.assertEqual((row["requests"], row["stream_request_ids"], row["verified"]), (2, 2, True))
            self.assertEqual(row["verification_requests"], 2)
            self.assertTrue(row["quality_passed"] and row["status"] == "collected")
            self.assertEqual(row["session_id"], "x")
            self.assertFalse(analysis.censored(row))


class Verdict(unittest.TestCase):
    def test_a_clear_reduction_with_equal_quality_passes(self):
        result = run(rows())
        self.assertEqual(result["outcome"], "pass", (result["problems"], result["go_criteria"]))
        self.assertLessEqual(result["primary"]["upper"], 0.90)
        self.assertLessEqual(result["primary"]["lower"], result["primary"]["ratio"])
        self.assertLessEqual(result["primary"]["ratio"], result["primary"]["upper"])
        self.assertEqual(result["primary"]["included_blocks"], 20)
        self.assertEqual(result["stratum"], analysis.STRATUM)
        self.assertEqual(result["warnings"], [])

    def test_no_reduction_or_a_marginal_one_fails(self):
        self.assertEqual(run(rows(best=lambda a0, t: a0))["outcome"], "fail")
        uneven = {20: 10, 30: 34, 25: 19, 35: 34, 28: 24}  # 121/138 = 0.877, very uneven across blocks
        noisy = run(rows(best=lambda a0, t: uneven[a0]))
        self.assertLess(noisy["primary"]["ratio"], 0.90)
        self.assertFalse(noisy["primary"]["passed"])  # a point estimate under 0.90 is not enough
        self.assertEqual(noisy["outcome"], "fail")

    def test_quality_margin_and_the_stricter_g5(self):
        def fail(n):
            def edit(out):
                for r in positives(out, "BEST")[:n]:
                    r["quality_passed"] = False
            return edit
        one = run(rows(edit=fail(1)))
        self.assertTrue(one["quality"]["passed"])  # within the r9 margin of one
        self.assertFalse(one["go_criteria"]["G5"]["passed"])  # but G5 applies and is stricter
        self.assertEqual(one["outcome"], "fail")
        self.assertFalse(run(rows(edit=fail(2)))["quality"]["passed"])

    def test_censored_best_counts_at_the_cap_and_censored_a0_at_its_observed_count(self):
        def best_censored(out):
            for r in positives(out, "BEST")[:2]:
                r["result_status"], r["result_is_error"] = "error_max_turns", True
        result = run(rows(edit=best_censored))
        self.assertEqual(result["arms"]["BEST"]["censored"], 2)
        self.assertEqual(result["arms"]["BEST"]["hidden_test_passes"], 18)
        self.assertGreater(result["arms"]["BEST"]["requests_included"], result["arms"]["BEST"]["observed_requests"])

        def a0_censored(out):
            for r in positives(out, "A0")[:2]:
                r["result_status"], r["result_is_error"] = "error_max_budget_usd", True
        result = run(rows(edit=a0_censored))
        self.assertEqual(result["arms"]["A0"]["requests_included"], result["arms"]["A0"]["observed_requests"])
        self.assertEqual(result["arms"]["A0"]["hidden_test_passes"], 18)
        sensitivity = result["sensitivity"]["without_censored_blocks"]
        self.assertEqual(sensitivity["blocks"], 18)
        self.assertEqual(len(sensitivity["excluded_blocks"]), 2)

    def test_unverified_blocks_leave_the_ratio_and_more_than_two_make_the_result_unverified(self):
        def unverify(n):
            def edit(out):
                for r in positives(out, "NOEXEC")[:n]:
                    r["verified"] = False
            return edit
        two = run(rows(edit=unverify(2)))
        self.assertEqual(two["primary"]["included_blocks"], 18)
        self.assertEqual(len(two["request_verification"]["excluded_blocks"]), 2)
        self.assertEqual(two["outcome"], "pass")
        three = run(rows(edit=unverify(3)))
        self.assertEqual(three["outcome"], "unverified")
        self.assertFalse(three["passed"])
        retried = rows()
        retried[10].update(requests=retried[10]["requests"] + 2, stream_request_ids=retried[10]["requests"],
                           api_error_entries=2, cross_check=False, verified=True)
        self.assertEqual(run(retried)["primary"]["included_blocks"], 20)

    def test_has_keys_sensitivity_is_reported_and_flagged_prominently(self):
        carried = run(rows(best=lambda a0, t: round(a0 * 0.2) if t == analysis.SENSITIVITY_TASK else round(a0 * 0.97)))
        self.assertTrue(carried["primary"]["passed"])
        self.assertFalse(carried["sensitivity"]["without_has_keys"]["passed"])
        self.assertEqual(carried["sensitivity"]["without_has_keys"]["blocks"], 15)
        self.assertTrue(carried["warnings"] and carried["warnings"][0].startswith("PROMINENT"))
        self.assertEqual(carried["outcome"], "pass")  # GO stays on the primary

    def test_unrealized_or_unsafe_controls_are_protocol_failures(self):
        for condition in analysis.CONTROLS:
            def unrealized(out, c=condition):
                i = next(i for i, r in enumerate(out) if r["condition"] == c)
                out[i] = dict(control(c, realized=False), order=i)
            result = run(rows(edit=unrealized))
            self.assertEqual(result["outcome"], "protocol-failure", condition)
        def unsafe(out):
            out[0] = dict(control("failed", skipped=True), order=0)
        self.assertEqual(run(rows(edit=unsafe))["outcome"], "protocol-failure")
        def missing(out):
            del out[4]
        self.assertEqual(run(rows(edit=missing))["outcome"], "protocol-failure")

    def test_incomplete_blocks_fail(self):
        def drop(out):
            positives(out, "A0")[0]["status"] = "uncollected"
        result = run(rows(edit=drop))
        self.assertEqual(result["outcome"], "incomplete")
        self.assertFalse(result["passed"])

    def test_bootstrap_is_deterministic_and_stratified(self):
        by_task = {"a": [(6, 10), (5, 10)], "b": [(9, 10), (7, 10)]}
        first = analysis.bootstrap_bounds(by_task, draws=DRAWS)
        self.assertEqual(first, analysis.bootstrap_bounds(by_task, draws=DRAWS))
        self.assertLessEqual(first["lower"], 27 / 40)
        self.assertGreaterEqual(first["upper"], 27 / 40)
        self.assertLessEqual(first["upper"], 0.8)  # never mixes a task's blocks into another task


class GoCriteria(unittest.TestCase):
    def test_every_criterion_is_marked_and_not_applicable_ones_carry_a_reason(self):
        result = run(rows())["go_criteria"]
        self.assertEqual(sorted(result), ["G%d" % i for i in range(1, 10)])
        for g, v in result.items():
            if v["applies"]:
                self.assertTrue(v["form"] and v["passed"] is True, g)
            else:
                self.assertTrue(v["reason"], g)
                self.assertIsNone(v["passed"])
        self.assertEqual({g for g, v in result.items() if not v["applies"]}, {"G3", "G4", "G9"})

    def test_each_applicable_criterion_can_fail_the_study(self):
        def no_firing(out):
            for r in positives(out, "BEST"):
                r["timeline"] = dict(events=[], final_digest="final")
        def noexec_executes(out):
            positives(out, "NOEXEC")[0]["receipts"] = [journal()]
        def foreign(out):
            positives(out, "BEST")[0]["receipts"] = [journal(session="other")]
        def undeclared(out):
            positives(out, "BEST")[0]["receipts"] = [journal(changed=("src/marshmallow/validate.py",))]
        def unknown_cost(out):
            positives(out, "NOEXEC")[0]["cost_usd"] = None
        def skipped(out):
            positives(out, "BEST")[0]["replay"] = dict(necessary_work_skipped=True)
        def stale(out):
            for r in positives(out, "BEST")[:6]:
                r["timeline"] = dict(events=[receipt("old")], final_digest="final")
        def more_than_noexec(out):
            for r in positives(out, "NOEXEC"):
                r["requests"] = 1
        def only_the_gate_ran(out):
            # A passing gate run at Stop is not the learned context firing (revision 11).
            for r in positives(out, "BEST"):
                r["timeline"] = dict(events=[dict(receipt("final"), origin="stop-gate", at="Stop")], final_digest="final")
        cases = [(no_firing, "G2"), (noexec_executes, "G6"), (foreign, "G6"), (undeclared, "G6"),
                 (unknown_cost, "G6"), (skipped, "G6"), (stale, "G7"), (more_than_noexec, "G8"),
                 (only_the_gate_ran, "G2")]
        for edit, g in cases:
            result = run(rows(edit=edit))
            self.assertFalse(result["go_criteria"][g]["passed"], (edit.__name__, g))
            self.assertEqual(result["outcome"], "fail", edit.__name__)


class Registration(unittest.TestCase):
    def test_constants_match_the_protocol_revision_9_draft(self):
        r9 = PROTOCOL["heldout_analysis"]
        self.assertEqual(r9["script"], "evals/reflex-proof/harness/heldout_analysis.py")
        self.assertEqual(r9["runner"], "evals/reflex-proof/harness/heldout_cohort.py")
        self.assertEqual(r9["arms"], list(analysis.ARMS))
        self.assertEqual(r9["tasks"], TASKS)
        self.assertEqual(r9["repetitions"], analysis.REPETITIONS)
        self.assertEqual(r9["primary_test"]["ratio_upper_bound_max"], analysis.RATIO_MAX)
        self.assertEqual(r9["primary_test"]["confidence"], analysis.CONFIDENCE)
        self.assertEqual(r9["primary_test"]["bootstrap_draws"], analysis.BOOTSTRAP)
        self.assertEqual(r9["primary_test"]["seed"], analysis.SEED)
        self.assertEqual(r9["quality_margin"], analysis.QUALITY_MARGIN)
        self.assertEqual(r9["censoring"]["imputed_requests_best"], analysis.CAP_REQUESTS)
        self.assertEqual(r9["request_verification"]["max_excluded_blocks"], analysis.MAX_EXCLUDED_BLOCKS)
        self.assertEqual(r9["sensitivity"]["without_task"], analysis.SENSITIVITY_TASK)
        self.assertEqual(r9["budget"]["per_session_cap_usd"], analysis.SESSION_CAP_USD)
        self.assertEqual(r9["budget"]["ceiling_usd"], analysis.CEILING_USD)
        self.assertEqual(r9["budget"]["sessions"], len(analysis.matrix(TASKS)))
        self.assertEqual(r9["budget"]["worst_case_usd"], analysis.budget(len(analysis.matrix(TASKS)))["worst_case_usd"])
        self.assertEqual(r9["stratum"], analysis.STRATUM)
        self.assertEqual(r9["controls"]["conditions"], list(analysis.CONTROLS))
        self.assertEqual({g: v["applies"] for g, v in r9["go_criteria"].items()},
                         {g: v["applies"] for g, v in analysis.GO_CRITERIA.items()})
        ledger = PROTOCOL["ledger"]
        self.assertEqual(ledger["phase_ceilings_usd"]["heldout"], analysis.CEILING_USD)
        self.assertEqual(sum(ledger["phase_ceilings_usd"].values()), ledger["protocol_ceiling_usd"])


if __name__ == "__main__":
    unittest.main()
