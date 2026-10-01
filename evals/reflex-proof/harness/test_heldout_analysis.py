"""Revision 9 held-out analysis: arm selection, matrix, budget, classifier, request counts, censoring, verdict."""
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


def dev_report(a2=(True, 20.0, 0.10), a3=(True, 18.0, 0.12)):
    arms = {"A0": dict(requests=25.0), "A1": dict(requests=10.0, cost_usd=0.01)}
    verdict = {}
    for name, (passed, requests, cost) in (("A2", a2), ("A3", a3)):
        arms[name] = dict(requests=requests, cost_usd=cost)
        verdict[name] = dict(passed=passed)
    return dict(arms=arms, verdict=verdict)


def receipt(digest, outcome="applied"):
    return dict(kind="receipt", phase="completed", outcome=outcome, digest=digest)


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
                replay=dict(necessary_work_skipped=skipped, repeat_attempts=0))


def session(arm, requests, passed=True, status="success", cost=0.12, cross_check=True):
    return dict(arm=arm, condition="positive", status="collected", requests=requests, cross_check=cross_check,
                result_status=status, result_is_error=status != "success", quality_passed=passed, cost_usd=cost,
                verification_requests=1)


def rows(best=lambda a0: round(a0 * 0.6), noexec=lambda a0: a0, a0_requests=(20, 30, 25, 35, 28), edit=None):
    out = [dict(control(c), order=i) for i, c in enumerate(analysis.CONTROLS)]
    for r in analysis.matrix(TASKS):
        if r["condition"] != "positive":
            continue
        a0 = a0_requests[(r["block"] - 1) % len(a0_requests)]
        value = {"A0": a0, "NOEXEC": noexec(a0), "BEST": best(a0)}[r["arm"]]
        out.append(dict(r, **session(r["arm"], value)))
    if edit:
        edit(out)
    return out


class Selection(unittest.TestCase):
    def test_lowest_mean_requests_among_arms_passing_d1_to_d4(self):
        self.assertEqual(analysis.select_best(dev_report()), "A3")
        self.assertEqual(analysis.select_best(dev_report(a3=(False, 10.0, 0.01))), "A2")
        self.assertEqual(analysis.select_best(dev_report(a2=(True, 18.0, 0.10))), "A2")  # tie: lower cost
        self.assertIsNone(analysis.select_best(dev_report(a2=(False, 1, 0), a3=(False, 1, 0))))  # nothing runs
        self.assertNotEqual(analysis.select_best(dev_report()), "A1")


class Matrix(unittest.TestCase):
    def test_controls_first_then_paired_blocks_in_a_recorded_random_order(self):
        m = analysis.matrix(TASKS)
        self.assertEqual(m, analysis.matrix(TASKS))
        self.assertEqual([r["order"] for r in m], list(range(len(m))))
        controls, positives = m[:5], m[5:]
        self.assertEqual([r["condition"] for r in controls], list(analysis.CONTROLS))
        self.assertTrue(all(r["arm"] == "BEST" and r["task"] == "mm-curve--finite" for r in controls))
        self.assertEqual(len(positives), len(TASKS) * 5 * 3)
        blocks = {}
        for r in positives:
            blocks.setdefault(r["block"], []).append(r)
        self.assertEqual(len(blocks), 20)
        for rs in blocks.values():
            self.assertEqual(sorted(r["arm"] for r in rs), sorted(analysis.ARMS))
            self.assertEqual(len({(r["task"], r["repetition"]) for r in rs}), 1)
        self.assertGreater(len({tuple(r["arm"] for r in rs) for rs in blocks.values()}), 1)
        reps = [r["repetition"] for r in positives]
        self.assertEqual(reps, sorted(reps))
        self.assertTrue(all(sum(r["task"] == t for r in positives) == 15 for t in TASKS))
        with self.assertRaises(ValueError):
            analysis.matrix(TASKS[:3])

    def test_worst_case_fits_the_held_out_reserve(self):
        b = analysis.budget(len(analysis.matrix(TASKS)))
        self.assertEqual(b["sessions"], 65)
        self.assertEqual(b["worst_case_usd"], 21.45)
        self.assertTrue(b["fits"])
        self.assertEqual(b["worst_case_at_r8_cap_usd"], 97.5)
        self.assertGreaterEqual(b["max_sessions_at_cap"], 65)


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


class Sessions(unittest.TestCase):
    def test_debug_dispatches_must_match_stream_request_ids_one_to_one(self):
        debug = "\n".join("2026-10-01T00:00:0%dZ [DEBUG] [API REQUEST] /v1/messages x-client-request-id=c%d source=sdk" % (i, i)
                          for i in range(3)) + "\n2026-10-01T00:00:09Z [DEBUG] other line"
        stream = [dict(value=dict(type="assistant", request_id="req_%d" % i)) for i in range(3)]
        self.assertEqual(analysis.requests(debug, stream), dict(requests=3, stream_request_ids=3, cross_check=True))
        self.assertFalse(analysis.requests(debug, stream[:2])["cross_check"])

    def test_censored_sessions_are_failures_at_the_cap_request_count(self):
        self.assertEqual(analysis.effective(session("BEST", 12, status="error_max_turns")),
                         dict(requests=60, passed=False, censored=True))
        self.assertEqual(analysis.effective(session("BEST", 75, status="error_max_budget_usd"))["requests"], 75)
        self.assertEqual(analysis.effective(dict(session("BEST", 12), result_status=None))["censored"], True)
        self.assertEqual(analysis.effective(session("BEST", 12)), dict(requests=12, passed=True, censored=False))

    def test_session_row_reads_the_collected_files(self):
        with tempfile.TemporaryDirectory() as tmp:
            s = Path(tmp)
            (s / "claude-debug.log").write_text(
                "t [DEBUG] [API REQUEST] /v1/messages x-client-request-id=a source=sdk\n"
                "t [DEBUG] [API REQUEST] /v1/messages x-client-request-id=b source=sdk\n")
            (s / "stream-observations.jsonl").write_text("\n".join(json.dumps(dict(value=dict(request_id=r))) for r in ("r1", "r2")))
            tool = lambda c: dict(type="tool_use", name="Bash", input=dict(command=c))  # noqa: E731
            (s / "stdout.jsonl").write_text("\n".join(json.dumps(x) for x in (
                dict(type="assistant", message=dict(id="m1", content=[tool(".venv/bin/python -m pytest -q")])),
                dict(type="assistant", message=dict(id="m2", content=[tool("ls"), tool("python -c 'print(1)'")])))))
            (s / "summary.json").write_text(json.dumps(dict(result_status="success", result_is_error=False, num_turns=2,
                                                            total_cost_usd=0.05)))
            (s / "collection.json").write_text(json.dumps(dict(quality=dict(passed=True))))
            row = analysis.session_row(s, dict(arm="A0", condition="positive", block=1, task=TASKS[0]))
            self.assertEqual((row["requests"], row["stream_request_ids"], row["cross_check"]), (2, 2, True))
            self.assertEqual(row["verification_requests"], 2)
            self.assertTrue(row["quality_passed"] and row["status"] == "collected")
            self.assertFalse(analysis.censored(row))


class Verdict(unittest.TestCase):
    def test_a_clear_reduction_with_equal_quality_passes(self):
        result = analysis.analyze(rows(), "A3", draws=DRAWS)
        self.assertTrue(result["passed"], result["problems"])
        self.assertLessEqual(result["primary"]["upper"], 0.90)
        self.assertLessEqual(result["primary"]["lower"], result["primary"]["ratio"])
        self.assertLessEqual(result["primary"]["ratio"], result["primary"]["upper"])
        self.assertEqual(result["arms"]["BEST"]["sessions"], 20)
        self.assertEqual(result["stratum"], analysis.STRATUM)

    def test_no_reduction_or_a_marginal_one_fails(self):
        self.assertFalse(analysis.analyze(rows(best=lambda a0: a0), "A2", draws=DRAWS)["passed"])
        uneven = {20: 10, 30: 34, 25: 19, 35: 34, 28: 24}  # 121/138 = 0.877, very uneven across blocks
        noisy = analysis.analyze(rows(best=lambda a0: uneven[a0]), "A2", draws=DRAWS)
        self.assertLess(noisy["primary"]["ratio"], 0.90)
        self.assertFalse(noisy["primary"]["passed"])  # a point estimate under 0.90 is not enough

    def test_quality_non_inferiority_allows_one_fewer_pass_only(self):
        def fail(n):
            def edit(out):
                for r in [r for r in out if r["arm"] == "BEST" and r["condition"] == "positive"][:n]:
                    r["quality_passed"] = False
            return edit
        self.assertTrue(analysis.analyze(rows(edit=fail(1)), "A3", draws=DRAWS)["quality"]["passed"])
        result = analysis.analyze(rows(edit=fail(2)), "A3", draws=DRAWS)
        self.assertFalse(result["quality"]["passed"] or result["passed"])

    def test_censored_best_sessions_count_at_the_cap_and_as_failures(self):
        def edit(out):
            for r in [r for r in out if r["arm"] == "BEST" and r["condition"] == "positive"][:2]:
                r["result_status"], r["result_is_error"] = "error_max_turns", True
        result = analysis.analyze(rows(edit=edit), "A3", draws=DRAWS)
        self.assertEqual(result["arms"]["BEST"]["censored"], 2)
        self.assertEqual(result["arms"]["BEST"]["hidden_test_passes"], 18)
        self.assertFalse(result["passed"])

    def test_unrealized_or_unsafe_controls_are_protocol_failures(self):
        for condition in analysis.CONTROLS:
            def unrealized(out, c=condition):
                i = next(i for i, r in enumerate(out) if r["condition"] == c)
                out[i] = dict(control(c, realized=False), order=i)
            result = analysis.analyze(rows(edit=unrealized), "A3", draws=DRAWS)
            self.assertFalse(result["passed"], condition)
            self.assertTrue(any("unrealized" in p for p in result["problems"]), condition)
        def unsafe(out):
            out[0] = dict(control("failed", skipped=True), order=0)
        self.assertFalse(analysis.analyze(rows(edit=unsafe), "A3", draws=DRAWS)["passed"])
        def missing(out):
            del out[4]
        self.assertFalse(analysis.analyze(rows(edit=missing), "A3", draws=DRAWS)["passed"])

    def test_incomplete_blocks_unverified_counts_and_unknown_costs_fail(self):
        def drop(out):
            next(r for r in out if r["arm"] == "A0" and r["condition"] == "positive")["status"] = "uncollected"
        result = analysis.analyze(rows(edit=drop), "A3", draws=DRAWS)
        self.assertFalse(result["passed"])
        self.assertTrue(any("incomplete" in p for p in result["problems"]))
        def mismatch(out):
            next(r for r in out if r["arm"] == "BEST" and r["condition"] == "positive")["cross_check"] = False
        self.assertFalse(analysis.analyze(rows(edit=mismatch), "A3", draws=DRAWS)["passed"])
        def cost(out):
            next(r for r in out if r["arm"] == "NOEXEC")["cost_usd"] = None
        self.assertFalse(analysis.analyze(rows(edit=cost), "A3", draws=DRAWS)["passed"])

    def test_bootstrap_is_deterministic_and_stratified(self):
        by_task = {"a": [(6, 10), (5, 10)], "b": [(9, 10), (7, 10)]}
        first = analysis.bootstrap_bounds(by_task, draws=DRAWS)
        self.assertEqual(first, analysis.bootstrap_bounds(by_task, draws=DRAWS))
        self.assertLessEqual(first["lower"], 27 / 40)
        self.assertGreaterEqual(first["upper"], 27 / 40)
        self.assertLessEqual(first["upper"], 0.8)  # never mixes a task's blocks into another task


class Registration(unittest.TestCase):
    def test_constants_match_the_protocol_revision_9_draft(self):
        r9 = PROTOCOL["heldout_analysis"]
        self.assertEqual(r9["script"], "evals/reflex-proof/harness/heldout_analysis.py")
        self.assertEqual(r9["arms"], list(analysis.ARMS))
        self.assertEqual(r9["tasks"], TASKS)
        self.assertEqual(r9["repetitions"], analysis.REPETITIONS)
        self.assertEqual(r9["primary_test"]["ratio_upper_bound_max"], analysis.RATIO_MAX)
        self.assertEqual(r9["primary_test"]["confidence"], analysis.CONFIDENCE)
        self.assertEqual(r9["primary_test"]["bootstrap_draws"], analysis.BOOTSTRAP)
        self.assertEqual(r9["primary_test"]["seed"], analysis.SEED)
        self.assertEqual(r9["quality_margin"], analysis.QUALITY_MARGIN)
        self.assertEqual(r9["censoring"]["imputed_requests"], analysis.CAP_REQUESTS)
        self.assertEqual(r9["budget"]["per_session_cap_usd"], analysis.SESSION_CAP_USD)
        self.assertEqual(r9["budget"]["ceiling_usd"], analysis.CEILING_USD)
        self.assertEqual(r9["budget"]["sessions"], len(analysis.matrix(TASKS)))
        self.assertEqual(r9["stratum"], analysis.STRATUM)
        self.assertEqual(r9["controls"]["conditions"], list(analysis.CONTROLS))
        self.assertEqual(PROTOCOL["ledger"]["phase_ceilings_usd"]["heldout"], analysis.CEILING_USD)


if __name__ == "__main__":
    unittest.main()
