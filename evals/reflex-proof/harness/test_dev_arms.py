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
    return dict(dev_arm=arm, block=block, condition=condition, requests=requests, cost_usd=0.1, wall_seconds=60,
                tokens=dict(input=1, cache_write=1, cache_read=1, output=1), pytest_requests=1,
                quality=dict(passed=True), replay=extra.pop("replay", {}), stop_gate_outputs=[], order=0, **extra)


class Thresholds(unittest.TestCase):
    def rows(self, a2, a3=None):
        out = []
        for b in range(1, 7):
            out += [row("A0", b, 18), row("A1", b, 18), row("A2", b, a2[b - 1]), row("A3", b, (a3 or a2)[b - 1])]
        return out

    def test_a_real_paired_reduction_passes_and_a_mean_driven_by_one_block_does_not(self):
        v = dev.summarize(self.rows([14] * 6), PROTOCOL)["verdict"]
        self.assertTrue(v["A2"]["passed"], v)
        self.assertEqual(v["A2"]["requests_ratio_to_A0"], round(14 / 18, 4))
        lumpy = dev.summarize(self.rows([2, 16, 18, 18, 18, 18]), PROTOCOL)["verdict"]
        self.assertTrue(lumpy["A2"]["requests_ratio_to_A0"] <= 0.85)
        self.assertFalse(lumpy["A2"]["D1_requests"], "one block cannot carry the result")

    def test_safety_quality_and_repeats_each_fail_an_arm(self):
        rows = self.rows([14] * 6) + [row("A2", None, 20, condition="later-edit", replay=dict(necessary_work_skipped=True))]
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
