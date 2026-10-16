"""Development arms: matrix, grants, hook arms, the registered thresholds, and production-shaped replay."""
import json
import os
from pathlib import Path
import shlex
import tempfile
import unittest
from unittest.mock import patch

import dev_arms as dev
import receipt_replay as rr

PROTOCOL = json.loads(dev.canary.GATE.read_text())


def frozen(root):
    rev7 = {"adage": str(root / "bin/rev7/adage"), "adage-exec": str(root / "bin/rev7/adage-exec")}
    current = {"adage": str(root / "bin/current/adage"), "adage-exec": str(root / "bin/current/adage-exec")}
    m = dict(protocol=dev.PROTOCOL, source_sha256=dev.sources(), frozen_inputs={}, matrix=dev.matrix(),
             route=dev.canary.ROUTE, command=dev.canary.COMMAND, caches=list(dev.canary.CACHES),
             binaries={"rev7": rev7, "current": current})
    dev.pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(dev.pairs.sha(root / "manifest.json") + "\n")
    return m


class Matrix(unittest.TestCase):
    def test_six_paired_blocks_with_rotating_arm_order_then_controls_in_the_lever_arms(self):
        rows = dev.matrix()
        positives = [r for r in rows if r["condition"] == "positive"]
        self.assertEqual(len(positives), 24)
        for block in range(1, 7):
            arms = [r["arm"] for r in positives if r["block"] == block]
            self.assertEqual(sorted(arms), ["A0", "A1", "A2", "A3"])
        firsts = [next(r["arm"] for r in positives if r["block"] == b) for b in range(1, 7)]
        self.assertEqual(firsts, ["A0", "A1", "A2", "A3", "A0", "A1"], "no arm always goes first")
        controls = [r for r in rows if r["condition"] != "positive"]
        self.assertEqual(sorted({(r["arm"], r["condition"]) for r in controls}),
                         sorted((a, c) for a in ("A2", "A3") for c in dev.CONTROLS))
        self.assertEqual({r["task"] for r in rows}, {"mm-curve--finite"}, "development split only")
        self.assertEqual([r["order"] for r in rows], list(range(len(rows))))

    def test_arms_differ_only_as_registered(self):
        self.assertEqual(dev.ARMS["A0"]["hooks"], False)
        self.assertEqual((dev.ARMS["A1"]["binaries"], dev.ARMS["A2"]["binaries"], dev.ARMS["A3"]["binaries"]),
                         ("rev7", "current", "current"))
        self.assertEqual((dev.ARMS["A2"]["card_checks"], dev.ARMS["A3"]["card_checks"]), (False, True))
        self.assertIn(("PostToolBatch", None), dev.EVENTS)
        self.assertIn(("PreToolUse", "Bash"), dev.EVENTS)
        self.assertNotIn("PostToolUse", [e for e, _ in dev.EVENTS], "production registration, not per tool")


class Grants(unittest.TestCase):
    def test_script_grants_only_hook_arms_each_with_its_own_binary(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            text = Path(dev.approval_script(root)["path"]).read_text()
            self.assertEqual(text.count("review delegate"), 6 * 3 + 10)
            self.assertEqual(text.count("bin/rev7/adage review delegate"), 6)
            self.assertNotIn("-A0-", text)
            self.assertIn('auto_approve = "off"', text)
            self.assertNotIn("approvals.json >", text)

    def test_grant_check_requires_none_in_the_no_hook_arm(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            for row in m["matrix"]:
                home = dev.session_dir(root, row) / "adage-home"
                home.mkdir(parents=True)
                if dev.ARMS[row["arm"]]["hooks"]:
                    (home / "policy.toml").write_text(dev.POLICY)
                    dev.pairs.write(home / "approvals.json", dict(approvals=[dict(
                        route=dev.canary.ROUTE, by="launch-evaluation-human", auto=False,
                        key=dict(level="e2", effect="verify", argv_templates=[shlex.split(dev.canary.COMMAND)]),
                        terms=dict(cache=list(dev.canary.CACHES), timeout_ms=30000))]))
            self.assertTrue(dev.verify_grants(root)["passed"])
            a0 = next(r for r in m["matrix"] if r["arm"] == "A0")
            dev.pairs.write(dev.session_dir(root, a0) / "adage-home/approvals.json", dict(approvals=[]))
            self.assertFalse(dev.verify_grants(root)["passed"])


class Hook(unittest.TestCase):
    def test_no_hook_arm_refuses_and_lever2_arm_turns_card_checks_off(self):
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "manifest.json"
            path.write_text(json.dumps(dict(arm="agent")))
            with self.assertRaises(ValueError):
                dev.hook(path, {})
            path.write_text(json.dumps(dict(arm="automatic", card_checks=False)))
            with patch.object(dev.pairs, "hook", return_value="out") as h, patch.dict(os.environ, {}, clear=False):
                self.assertEqual(dev.hook(path, {}), "out")
                self.assertEqual(os.environ.get("ADAGE_CARD_CHECKS"), "0")
                h.assert_called_once()
                # A recorder process never carries one arm's knob into another's hook.
                path.write_text(json.dumps(dict(arm="automatic", card_checks=True)))
                dev.hook(path, {})
                self.assertNotIn("ADAGE_CARD_CHECKS", os.environ)


def row(arm, block, requests, condition="positive", **extra):
    """A collected row whose integrity facts hold: in a hook arm the grant bound (contract), the learned
    verification ran (journal receipt, spool outcome) and its condition occurred; counts are verified."""
    hooked = dev.ARMS[arm]["hooks"]
    receipt = dict(kind="receipt", phase="completed", outcome="failed" if condition == "failed" else "applied", digest="a")
    base = dict(dev_arm=arm, block=block, condition=condition, requests=requests, cost_usd=0.1, wall_seconds=60,
                tokens=dict(input=1, cache_write=1, cache_read=1, output=1), pytest_requests=1,
                quality=dict(passed=True), replay={"receipts": 1}, stop_gate_outputs=[], order=0,
                observed_dispatches=[dict(source="sdk")] * requests, censored=[], contract_emitted=hooked,
                exec_decisions=[dict(outcome="applied", withheld=None)] if hooked else [],
                all_journal_receipts=[dict(id="op")] if hooked else [],
                timeline=dict(events=[receipt] if hooked else [],
                              final_digest="b" if condition in ("later-edit", "config-change") else "a"))
    base.update(extra)
    return base


class Thresholds(unittest.TestCase):
    def rows(self, a2, a3=None):
        out = []
        for b in range(1, 7):
            out += [row("A0", b, 18), row("A1", b, 18), row("A2", b, a2[b - 1]), row("A3", b, (a3 or a2)[b - 1])]
        out += [row(arm, None, 15, condition=c) for arm in dev.CONTROL_ARMS for c in dev.CONTROLS]
        for order, r in enumerate(out):
            r["order"] = order
        return out

    def test_a_real_paired_reduction_passes_and_a_mean_driven_by_one_block_does_not(self):
        v = dev.summarize(self.rows([14] * 6), PROTOCOL)["verdict"]
        self.assertTrue(v["A2"]["passed"], v)
        self.assertEqual(v["A2"]["requests_ratio_to_A0"], round(14 / 18, 4))
        lumpy = dev.summarize(self.rows([2, 16, 18, 18, 18, 18]), PROTOCOL)["verdict"]
        self.assertTrue(lumpy["A2"]["requests_ratio_to_A0"] <= 0.85)
        self.assertFalse(lumpy["A2"]["D1_requests"], "one block cannot carry the result")

    def test_safety_quality_and_repeats_each_fail_an_arm(self):
        rows = self.rows([14] * 6)
        next(r for r in rows if r["dev_arm"] == "A2" and r["condition"] == "later-edit")["replay"] = dict(necessary_work_skipped=True)
        self.assertFalse(dev.summarize(rows, PROTOCOL)["verdict"]["A2"]["D3_safety"])
        rows = self.rows([14] * 6)
        for r in rows[:12]:
            if r["dev_arm"] == "A3":
                r["quality"] = dict(passed=False)
        self.assertFalse(dev.summarize(rows, PROTOCOL)["verdict"]["A3"]["D2_quality"])
        rows = self.rows([14] * 6)
        for r in rows:
            if r["dev_arm"] == "A2":
                r["replay"] = dict(repeat_attempts=1)
        self.assertFalse(dev.summarize(rows, PROTOCOL)["verdict"]["A2"]["D4_repeat_sessions"])


class Integrity(unittest.TestCase):
    """dev-v8b averaged 20 lapsed-grant sessions into its arms; a report must now refuse that loudly."""

    def rows(self):
        return Thresholds().rows([14] * 6)

    def problems_after(self, change):
        rows = self.rows()
        target = next(r for r in rows if r["dev_arm"] == "A3" and r["condition"] == "positive")
        change(target)
        out = dev.summarize(rows, PROTOCOL)
        self.assertFalse(out["valid"])
        self.assertFalse(out["verdict"]["A2"]["passed"], "an invalid study passes no arm, not just the affected one")
        return [p["problem"] for p in out["problems"] if p["order"] == target["order"]]

    def test_a_clean_study_is_valid(self):
        out = dev.summarize(self.rows(), PROTOCOL)
        self.assertEqual(out["problems"], [])
        self.assertTrue(out["valid"] and out["verdict"]["A2"]["passed"] and out["verdict"]["A2"]["D3_tested"])

    def test_no_contract_a_withheld_approval_or_no_receipt_each_invalidate_the_study(self):
        self.assertIn("no execution contract at SessionStart: the grant did not bind",
                      self.problems_after(lambda r: r.update(contract_emitted=False)))
        withheld = self.problems_after(lambda r: r.update(exec_decisions=[dict(
            withheld="approval", reason="exec-changed: the approval for this route no longer matches (route_exec_digest)")]))
        self.assertTrue(any("withheld (approval: exec-changed" in p for p in withheld), withheld)
        self.assertIn("no receipt: the learned verification never ran",
                      self.problems_after(lambda r: r.update(all_journal_receipts=[])))
        self.assertIn("learned-verification decisions were not recorded",
                      self.problems_after(lambda r: r.update(exec_decisions=None)))

    def test_a_duplicate_suppression_is_not_a_lapse(self):
        rows = self.rows()
        rows[3]["exec_decisions"].append(dict(withheld="duplicate", reason="already ran on these inputs"))
        self.assertTrue(dev.summarize(rows, PROTOCOL)["valid"])

    def test_unverified_or_truncated_counts_invalidate_the_study(self):
        self.assertIn("model requests differ from the debug log's dispatches",
                      self.problems_after(lambda r: r.update(observed_dispatches=[{}])))
        self.assertTrue(any(p.startswith("censored (turn cap)") for p in
                            self.problems_after(lambda r: r.update(censored=["turn cap"]))))

    def test_the_no_hook_arm_needs_no_contract(self):
        rows = self.rows()
        self.assertFalse(rows[0]["contract_emitted"])
        self.assertTrue(dev.summarize(rows, PROTOCOL)["valid"])

    def test_an_unrealized_control_leaves_d3_untested(self):
        rows = self.rows()
        later = next(r for r in rows if r["dev_arm"] == "A2" and r["condition"] == "later-edit")
        later["timeline"]["final_digest"] = "a"  # the receipt still covers the final sources: nothing was stale
        out = dev.summarize(rows, PROTOCOL)
        self.assertFalse(out["verdict"]["A2"]["D3_tested"])
        self.assertFalse(out["verdict"]["A2"]["D3_safety"], "no skipped work in an untested control is not safety")
        self.assertIn("control not realized: its condition never occurred, so it tested nothing",
                      [p["problem"] for p in out["problems"] if p["order"] == later["order"]])

    def test_dev_v8b_would_have_failed_loudly_at_exactly_the_lapsed_sessions(self):
        base = Path(__file__).parent / "results/20261001-dev-arms/dev-v8b"
        facts = {s["order"]: s for s in json.loads((base / "posthoc-audit.json").read_text())["sessions"]}
        rows = []
        for p in sorted((base / "collections").glob("*.json")):
            r = json.loads(p.read_text())
            s = facts[r["order"]]  # the facts dev-arms-1 never recorded, from the post hoc audit
            r.update(exec_decisions=s["learned_verification"], censored=s["censored"],
                     observed_dispatches=[{}] * sum(s["debug_dispatches"].values()))
            rows.append(r)
        out = dev.summarize(rows, PROTOCOL)
        self.assertFalse(out["valid"])
        self.assertFalse(any(v["passed"] for v in out["verdict"].values()))
        self.assertFalse(out["verdict"]["A2"]["D3_tested"] or out["verdict"]["A3"]["D3_tested"])
        hooked = {r["order"] for r in rows if dev.ARMS[r["dev_arm"]]["hooks"]}
        self.assertEqual({p["order"] for p in out["problems"]}, {o for o in hooked if o >= 9},
                         "sessions 01-06 and 08 ran as designed; every hook-arm session from 09 did not")


class ProductionShapedReplay(unittest.TestCase):
    def test_batch_registration_timeline_and_stop_gate_receipts(self):
        def hookrow(event, payload, before="a", after="a", journal=(), output=""):
            return json.dumps(dict(event=event, payload=dict(payload, cwd="/r"), before=dict(digest=before),
                                   after=dict(digest=after), journal_new=list(journal), output=output))
        receipt = dict(id="op", route="r", phase="completed", outcome=dict(outcome="applied", changed=[]))
        ctx = json.dumps({"hookSpecificOutput": {"additionalContext": "Adage ran `.venv/bin/python -m pytest tests/test_validate.py -q` after your edits: passed"}})
        lines = [
            hookrow("PostToolBatch", dict(tool_calls=[dict(tool_name="Edit", tool_use_id="e1", tool_input=dict(file_path="/r/CHANGELOG.rst"))]),
                    journal=[receipt], output=ctx),
            hookrow("PreToolUse", dict(tool_name="Bash", tool_use_id="b1", tool_input=dict(command="python -m pytest tests/test_validate.py::t"))),
            hookrow("PostToolUseFailure", dict(tool_name="Bash", tool_use_id="b1")),
            hookrow("PostToolBatch", dict(tool_calls=[dict(tool_name="Bash", tool_use_id="b1", tool_input={})])),
            hookrow("Stop", dict(stop_hook_active=False)),
        ]
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "hooks.jsonl"
            path.write_text("\n".join(lines) + "\n")
            tl = rr.extract(path)
        kinds = [(e["kind"], e.get("name"), e.get("completed")) for e in tl["events"]]
        self.assertEqual(kinds, [("tool", "Edit", True), ("receipt", None, None), ("tool", "Bash", False)])
        r = rr.replay(tl)
        self.assertEqual((r["successful_receipts"], r["repeat_attempts"], r["repeat_attempts_not_completed"]), (1, 1, 1))
        self.assertEqual(rr.stop_gate_replay(tl)["gate_runs"], False)

    def test_offline_stop_gate_replay_on_the_archived_canaries(self):
        root = Path(__file__).parent / "results/20260930-launch-canary"
        runs = {}
        for version in ("canary-v5", "canary-v7"):
            for p in sorted((root / version / "timelines").glob("*.json")):
                condition = p.stem.split("-", 1)[1].rsplit("-", 1)[0]
                g = rr.stop_gate_replay(json.loads(p.read_text()))
                runs.setdefault((version, condition), []).append(g["gate_runs"])
        # No positive in canary-v7 would have cost a gate run; stale and failed controls would.
        self.assertEqual(runs[("canary-v7", "positive")], [False] * 6)
        self.assertEqual(runs[("canary-v7", "config-change")], [True])
        self.assertEqual(runs[("canary-v7", "failed")], [True])
        self.assertEqual(runs[("canary-v5", "positive")], [True, True], "both v5 positives failed their receipt")
        self.assertEqual(sum(sum(v) for v in runs.values()), 9)


if __name__ == "__main__":
    unittest.main()
