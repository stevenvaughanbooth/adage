"""Receipt-consumption replay: archived transfer-v3 duplicates and negative controls; offline."""
import json
from pathlib import Path
import unittest

import receipt_replay as rr

ARCHIVE = Path(__file__).parent / "results/20260930-receipt-replay/timelines"
CMD = ".venv/bin/python -m pytest tests/test_validate.py -q"


def tool(command=None, digest="a", name="Bash", path=None):
    return dict(kind="tool", id="t", name=name, command=command, path=path, digest=digest)


def receipt(digest="a", phase="completed", outcome="applied", command=CMD):
    return dict(kind="receipt", command=command, phase=phase, outcome=outcome, digest=digest)


def timeline(*events, final="a"):
    return dict(root="/repo", events=list(events), final_digest=final)


def gate(digest="a", outcome="applied", command=CMD):
    return dict(receipt(digest, outcome=outcome, command=command), origin="stop-gate", at="Stop")


def ctx(digest="a", outcome="applied"):
    return dict(receipt(digest, outcome=outcome), origin="context", at="PostToolBatch")


class GateRuns(unittest.TestCase):
    """Version 3: a finish-line gate run never decides whether verification was necessary; a gate
    pass on the final sources meets the need (except a requested rerun or a wider scope)."""

    def test_failed_context_run_then_a_gate_pass_on_the_final_sources_meets_the_need(self):
        r = rr.replay(timeline(ctx("a", "failed"), tool(path="src/v.py", name="Edit", digest="a"), gate("b"), final="b"))
        self.assertTrue(r["verification_necessary"])
        self.assertFalse(r["necessary_work_skipped"])
        self.assertEqual(r["gate_runs"], [dict(operation=None, outcome="applied", digest="b", covers_final=True)])
        self.assertFalse(r["confirmed_elimination"], "the gate's run is not the context's elimination")

    def test_a_failing_gate_run_or_one_on_stale_sources_does_not_meet_it(self):
        failing = rr.replay(timeline(ctx("a", "failed"), gate("b", "failed"), final="b"))
        self.assertTrue(failing["necessary_work_skipped"])
        stale = rr.replay(timeline(ctx("a"), gate("b"), final="c"))
        self.assertTrue(stale["verification_necessary"] and stale["necessary_work_skipped"])
        self.assertEqual(stale["gate_runs"][0]["covers_final"], False)

    def test_a_passing_context_run_on_the_final_sources_needs_nothing_more(self):
        r = rr.replay(timeline(ctx("a"), final="a"))
        self.assertFalse(r["verification_necessary"])
        self.assertTrue(r["confirmed_elimination"])
        self.assertEqual(r["gate_runs"], [])

    def test_a_requested_rerun_or_wider_scope_is_not_met_by_the_gate(self):
        rerun = rr.replay(timeline(ctx("a"), gate("a"), final="a"), rerun_requested=True)
        self.assertTrue(rerun["necessary_work_skipped"], "the user asked the agent to run it")
        wider = rr.replay(timeline(ctx("a"), gate("a"), final="a"), broader_required=[[], ["tests"]])
        self.assertTrue(wider["necessary_work_skipped"], "the gate ran the narrow command")
        whole = rr.replay(timeline(ctx("a"), gate("a", command=".venv/bin/python -m pytest tests -q"), final="a"),
                          broader_required=[[], ["tests"]])
        self.assertFalse(whole["necessary_work_skipped"], "a gate run of the wider scope would meet it")

    def test_an_older_timeline_infers_the_gate_from_its_stop_event(self):
        old = dict(receipt("b"), at="Stop")  # journaled before origins were recorded
        self.assertEqual(rr.origin(old), "stop-gate")
        self.assertEqual(rr.origin(dict(receipt("a"), at="PostToolBatch")), "context")
        self.assertEqual(rr.origin(receipt("a")), "context", "canary timelines before the gate existed")
        r = rr.replay(timeline(dict(receipt("a", outcome="failed"), at="PostToolBatch"), old, final="b"))
        self.assertTrue(r["verification_necessary"] and not r["necessary_work_skipped"])


class ArchivedTransfer(unittest.TestCase):
    """The frozen study's 0/4 result is reproduced from native hook records, not reinterpreted."""

    def test_every_automatic_session_repeated_the_completed_verification(self):
        expected = {"odd": (2, 4), "finite": (1, 2), "no-adjacent-duplicates": (2, 1), "max-utf8-bytes": (1, 2)}
        for task, (redundant, subsets) in expected.items():
            auto = rr.replay(json.loads((ARCHIVE / f"mm-curve--{task}--automatic.json").read_text()))
            learned = rr.replay(json.loads((ARCHIVE / f"mm-curve--{task}--learned.json").read_text()))
            self.assertEqual((auto["successful_receipts"], auto["redundant_repeats"], auto["covered_subsets"]),
                             (1, redundant, subsets), task)
            self.assertFalse(auto["confirmed_elimination"])
            self.assertFalse(auto["verification_necessary"])  # Source never changed after the receipt.
            # The first post-receipt test request used the bare interpreter in every session.
            self.assertEqual([r["classification"] for r in auto["requests"] if "segment" in r][0], "different_environment")
            self.assertEqual(learned["receipts"], 0)
            self.assertTrue(learned["final_source_verified"])
            exact = [r for r in auto["requests"] if r["classification"] == "exact_repeat"]
            self.assertTrue(exact and all(r["segment"] == CMD for r in exact))


class CanaryV5(unittest.TestCase):
    """Development canary (launch-gate-v4/canary-v5): the two mechanisms behind zero eliminations."""
    ROOT = Path(__file__).parent / "results/20260930-launch-canary/canary-v5/timelines"

    def load(self, name):
        return json.loads((self.ROOT / (name + ".json")).read_text())

    def test_positive_sessions_got_failing_receipts_from_their_own_first_draft_tests(self):
        for name in ("00-positive-1", "06-positive-2", "09-config-change-2"):
            tl = self.load(name)
            receipt = next(i for i, e in enumerate(tl["events"]) if e["kind"] == "receipt")
            before = [e["path"] for e in tl["events"][:receipt] if e.get("name") == "Edit"]
            # The context fired at the natural end of the work: source, tests, then the changelog.
            self.assertEqual((before[0], before[-1]), ("src/marshmallow/validate.py", "CHANGELOG.rst"), name)
            self.assertIn("tests/test_validate.py", before)
            self.assertEqual(tl["events"][receipt]["outcome"], "failed")
            # The agent then fixed its own tests: the failure was real, and the work necessary.
            after = [e.get("path") for e in tl["events"][receipt:] if e.get("name") == "Edit"]
            self.assertIn("tests/test_validate.py", after)
            r = rr.replay(tl)
            self.assertEqual((r["successful_receipts"], r["necessary_work_skipped"]), (0, False))

    def test_interpreter_hint_turned_the_first_post_receipt_attempt_into_a_rerun(self):
        hinted = 0
        for path in sorted(self.ROOT.glob("*.json")):
            tl = json.loads(path.read_text())
            receipt = next(i for i, e in enumerate(tl["events"]) if e["kind"] == "receipt")
            tests = [e for e in tl["events"][receipt + 1:] if e.get("name") == "Bash" and "pytest" in (e.get("command") or "")]
            if tests[0]["command"].startswith("python -m pytest"):
                hinted += 1
                self.assertTrue(tests[0]["adage_pre_context"], path.stem)
                self.assertTrue(tests[1]["command"].startswith(".venv/bin/python -m pytest"), path.stem)
        self.assertEqual(hinted, 10)
        later, broader = rr.replay(self.load("08-later-edit-2")), rr.replay(self.load("04-broader-1"))
        self.assertEqual([r["classification"] for r in later["requests"] if "segment" in r][:2], ["different_environment", "covered_subset"])
        self.assertEqual((later["successful_receipts"], later["redundant_repeats"], later["covered_subsets"]), (1, 1, 4))
        self.assertEqual([r["classification"] for r in broader["requests"] if "segment" in r][:2], ["different_environment", "equivalent_repeat"])
        self.assertFalse(later["confirmed_elimination"] or broader["confirmed_elimination"])


class CanaryV7(unittest.TestCase):
    """Development canary-v7, re-scored post hoc: every positive requested covered verification."""
    ROOT = Path(__file__).parent / "results/20260930-launch-canary/canary-v7/timelines"

    def test_no_positive_is_an_elimination_once_failed_attempts_count(self):
        positives = sorted(self.ROOT.glob("*-positive-*.json"))
        self.assertEqual(len(positives), 6)
        for path in positives:
            r = rr.replay(json.loads(path.read_text()))
            self.assertEqual(r["successful_receipts"], 1, path.stem)
            self.assertFalse(r["confirmed_elimination"], path.stem)
            first = next(q for q in r["requests"] if q.get("covered"))
            # The first covered request was the bare interpreter: it never launched, yet it was made,
            # and Adage's receipt note arrived with it rather than before it.
            self.assertEqual((first["classification"], first["completed"], first["receipt_note"]),
                             ("different_environment", False, True), path.stem)
        attempts = [rr.replay(json.loads(p.read_text()))["repeat_attempts"] for p in positives]
        self.assertEqual(sum(attempts), 10)
        self.assertEqual(sum(a > 1 for a in attempts), 2, "two agents went on to run the .venv form")


class Equivalence(unittest.TestCase):
    def kind(self, command, digest="a"):
        rows = rr.replay(timeline(receipt(), tool(command, digest)))["requests"]
        return rows[0]["classification"] if rows else None

    def test_predeclared_equivalence_is_narrow(self):
        self.assertEqual(self.kind(CMD), "exact_repeat")
        self.assertEqual(self.kind(".venv/bin/python -m pytest tests/test_validate.py --tb=short -p no:cacheprovider"),
                         "equivalent_repeat")
        self.assertEqual(self.kind("/repo/.venv/bin/python -m pytest /repo/tests/test_validate.py -xvs 2>&1 | tail -20"),
                         "equivalent_repeat")
        self.assertEqual(self.kind("python -m pytest tests/test_validate.py -q"), "different_environment")
        self.assertEqual(self.kind(".venv/bin/python -m pytest tests/test_validate.py::test_odd -v"), "covered_subset")
        self.assertEqual(self.kind(".venv/bin/python -m pytest tests/test_validate.py -k odd"), "different_scope")
        self.assertEqual(self.kind(".venv/bin/python -m pytest tests/"), "different_scope")
        self.assertEqual(self.kind(".venv/bin/python -m pytest"), "different_scope")
        self.assertIsNone(self.kind("git status"))

    def test_unparsed_verification_blocks_elimination_but_other_shell_does_not(self):
        heredoc = rr.replay(timeline(receipt(), tool(".venv/bin/python << 'EOF'\nprint(1)\nEOF")))
        self.assertTrue(heredoc["confirmed_elimination"])
        hidden = rr.replay(timeline(receipt(), tool("bash -c \"$(echo pytest)\" > out")))
        self.assertEqual(hidden["opaque"], 1)
        self.assertFalse(hidden["confirmed_elimination"])


class RepeatAttempts(unittest.TestCase):
    """Scorer version 2: a covered request is a repeat attempt even if it never ran."""

    def test_a_failed_bare_interpreter_request_is_a_repeat_attempt_not_an_elimination(self):
        bare = dict(tool("python -m pytest tests/test_validate.py::test_finite_accepts_finite_numbers -v"), completed=False)
        r = rr.replay(timeline(receipt(), bare))
        self.assertEqual(r["requests"][0]["classification"], "different_environment")
        self.assertEqual((r["repeat_attempts"], r["repeat_attempts_not_completed"]), (1, 1))
        self.assertFalse(r["confirmed_elimination"])
        for command in (".venv/bin/python -m pytest tests/test_validate.py::test_x", ".venv/bin/pytest tests/test_validate.py -q"):
            self.assertEqual(rr.replay(timeline(receipt(), tool(command)))["repeat_attempts"], 1, command)
        for command in (".venv/bin/python -m pytest tests/", ".venv/bin/python -m pytest tests/test_validate.py -k x",
                        "git diff"):
            self.assertEqual(rr.replay(timeline(receipt(), tool(command)))["repeat_attempts"], 0, command)
        self.assertTrue(rr.replay(timeline(receipt(), tool("git diff")))["confirmed_elimination"])

    def test_requests_that_never_completed_do_not_satisfy_necessary_verification(self):
        failed = dict(tool(CMD, digest="b"), completed=False)
        self.assertTrue(rr.replay(timeline(receipt("a"), failed, final="b"))["necessary_work_skipped"])
        self.assertFalse(rr.replay(timeline(receipt("a"), tool(CMD, digest="b"), final="b"))["necessary_work_skipped"])


class NegativeControls(unittest.TestCase):
    """Failed, stale, broader and explicitly requested verification must remain necessary."""

    def test_fresh_receipt_without_repeat_is_an_elimination(self):
        r = rr.replay(timeline(tool(name="Edit", path="CHANGELOG.rst"), receipt(), tool("git diff")))
        self.assertTrue(r["confirmed_elimination"])
        self.assertFalse(r["necessary_work_skipped"])

    def test_failed_receipt_requires_agent_verification(self):
        skipped = rr.replay(timeline(receipt(phase="completed", outcome="refused")))
        self.assertTrue(skipped["verification_necessary"] and skipped["necessary_work_skipped"])
        self.assertFalse(skipped["confirmed_elimination"])
        ran = rr.replay(timeline(receipt(outcome="refused"), tool(CMD)))
        self.assertEqual(ran["requests"][0]["classification"], "after_failed_receipt")
        self.assertFalse(ran["necessary_work_skipped"])
        self.assertEqual(ran["redundant_repeats"], 0)

    def test_later_relevant_edit_makes_the_receipt_stale(self):
        skipped = rr.replay(timeline(receipt("a"), tool(name="Edit", path="src/x.py", digest="a"), final="b"))
        self.assertTrue(skipped["necessary_work_skipped"])
        ran = rr.replay(timeline(receipt("a"), tool(CMD, digest="b"), final="b"))
        self.assertEqual(ran["requests"][0]["classification"], "after_relevant_change")
        self.assertFalse(ran["necessary_work_skipped"] or ran["confirmed_elimination"])
        # A verification run before the final edit does not cover the final source.
        early = rr.replay(timeline(receipt("a"), tool(CMD, digest="b"), final="c"))
        self.assertTrue(early["necessary_work_skipped"])

    def test_broader_scope_request_needs_the_broader_run(self):
        narrow = rr.replay(timeline(receipt(), tool(CMD)), broader_required=[(), ("tests",)])
        self.assertTrue(narrow["necessary_work_skipped"])
        broad = rr.replay(timeline(receipt(), tool(".venv/bin/python -m pytest tests -q")), broader_required=[(), ("tests",)])
        self.assertEqual(broad["requests"][0]["classification"], "different_scope")
        self.assertFalse(broad["necessary_work_skipped"] or broad["confirmed_elimination"])

    def test_explicit_rerun_is_necessary_not_redundant(self):
        skipped = rr.replay(timeline(receipt()), rerun_requested=True)
        self.assertTrue(skipped["necessary_work_skipped"])
        ran = rr.replay(timeline(receipt(), tool(CMD)), rerun_requested=True)
        self.assertEqual((ran["requests"][0]["classification"], ran["redundant_repeats"]), ("requested_rerun", 0))
        self.assertFalse(ran["necessary_work_skipped"] or ran["confirmed_elimination"])


if __name__ == "__main__":
    unittest.main()
