"""Launch canary contracts: matrix, person-run approval script, grant checks, realized controls."""
import json
from pathlib import Path
import shlex
import tempfile
import os
import unittest
from unittest.mock import patch

import launch_canary as canary


def frozen(root):
    m = dict(protocol=canary.PROTOCOL, source_sha256=canary.sources(), frozen_inputs={}, matrix=canary.matrix(),
             route=canary.ROUTE, command=canary.COMMAND, caches=list(canary.CACHES),
             binaries={"adage": str(root / "bin/adage"), "adage-exec": str(root / "bin/adage-exec")})
    canary.pairs.write(root / "manifest.json", m)
    (root / "manifest.sha256").write_text(canary.pairs.sha(root / "manifest.json") + "\n")
    return m


def grant(**changes):
    g = dict(route=canary.ROUTE, by="launch-evaluation-human", auto=False,
             key=dict(level="e2", effect="verify", argv_templates=[shlex.split(canary.COMMAND)]),
             terms=dict(cache=list(canary.CACHES), timeout_ms=30000, effect="verify"))
    g.update(changes)
    return g


POLICY = '[exec]\nmax_level = "E2"\nauto_approve = "off"\nrequire_sandbox = true\n'


class Matrix(unittest.TestCase):
    def test_six_positives_and_every_control_on_development_split_tasks_only(self):
        rows = canary.matrix()
        self.assertEqual(len(rows), 12)
        table = canary.split_audit.splits(json.loads(canary.GATE.read_text()), "marshmallow")
        self.assertTrue(all(table[canary.split_audit.variant(r["task"])][0] == "development" for r in rows))
        self.assertEqual([r["order"] for r in rows], list(range(len(rows))))
        conditions = [r["condition"] for r in rows]
        self.assertEqual(conditions.count("positive"), 6)
        self.assertEqual(set(conditions), set(canary.CONDITIONS))
        self.assertEqual([r["repetition"] for r in rows if r["condition"] == "positive"], [1, 2, 3, 4, 5, 6])

    def test_every_condition_supplies_the_same_tests_and_only_negatives_change_requirements(self):
        self.assertEqual(canary.CONDITIONS["positive"][1:], (False, None))
        self.assertTrue(canary.CONDITIONS["rerun"][1])
        self.assertEqual(canary.CONDITIONS["broader"][2], [[], ["tests"]])
        self.assertTrue(all(canary.TESTS in c[0] for c in canary.CONDITIONS.values()))
        failed = canary.CONDITIONS["failed"][0]
        self.assertLess(failed.index("append the tests"), failed.index("implement the validator"))
        self.assertNotIn("Adage", "".join(c[0] for c in canary.CONDITIONS.values()))
        self.assertNotIn("pytest tests", canary.CONDITIONS["positive"][0], "no verification instruction in a positive")
        later = canary.CONDITIONS["later-edit"][0]
        self.assertLess(later.index("CHANGELOG entry is written"), later.index("reject booleans"))
        self.assertIn(canary.BOOL_TEST, later)
        self.assertTrue(all(canary.BOOL_TEST not in c[0] for name, c in canary.CONDITIONS.items() if name != "later-edit"))


class Approvals(unittest.TestCase):
    def test_script_is_scoped_to_assignment_homes_and_never_auto_approves(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            path = Path(canary.approval_script(root)["path"])
            text = path.read_text()
            self.assertEqual(text.count("review delegate"), 12)
            self.assertEqual(text.count("--level e2 --effect verify"), 12)
            self.assertIn('auto_approve = "off"', text)
            self.assertIn("require_sandbox = true", text)
            self.assertNotIn("approvals.json >", text)
            self.assertNotIn("--outputs", text)
            for home in (line.split()[0].split("=", 1)[1] for line in text.splitlines() if line.startswith("ADAGE_HOME=")):
                self.assertTrue(Path(home).is_relative_to(root / "sessions"))
            for cache in canary.CACHES:
                self.assertEqual(text.count("--cache " + shlex.quote(cache)), 12)
            self.assertEqual(canary.approval_script(root)["sha256"], canary.pairs.sha(path))  # Idempotent.

    def test_grant_check_rejects_anything_but_the_reviewed_terms(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            m = frozen(root)
            self.assertFalse(canary.verify_grants(root)["passed"])
            for row in m["matrix"]:
                home = canary.session_dir(root, row) / "adage-home"
                home.mkdir(parents=True)
                (home / "policy.toml").write_text(POLICY)
                canary.pairs.write(home / "approvals.json", dict(version=1, approvals=[grant()]))
            self.assertTrue(canary.verify_grants(root)["passed"])
            home = canary.session_dir(root, m["matrix"][3]) / "adage-home"
            for bad in (grant(auto=True), grant(terms=dict(cache=["**"], timeout_ms=30000)), grant(by="agent")):
                canary.pairs.write(home / "approvals.json", dict(version=1, approvals=[bad]))
                self.assertFalse(canary.verify_grants(root)["passed"])
            canary.pairs.write(home / "approvals.json", dict(version=1, approvals=[grant()]))
            (home / "policy.toml").write_text(POLICY.replace('"off"', '"on"'))
            self.assertFalse(canary.verify_grants(root)["passed"])

    def test_source_change_after_freeze_is_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            frozen(root)
            m = json.loads((root / "manifest.json").read_text())
            m["command"] = "pytest"
            (root / "manifest.json").write_text(json.dumps(m))
            with self.assertRaises(ValueError):
                canary.verify(root)


class Realized(unittest.TestCase):
    def row(self, condition, receipts, final="a"):
        return dict(condition=condition, replay={"x": 1}, timeline=dict(events=receipts, final_digest=final))

    def test_conditions_are_counted_only_when_they_occurred(self):
        ok = dict(kind="receipt", phase="completed", outcome="applied", digest="a")
        bad = dict(kind="receipt", phase="completed", outcome="failed", digest="a")
        self.assertTrue(canary.realized(self.row("failed", [bad])))
        self.assertFalse(canary.realized(self.row("failed", [ok])))
        self.assertFalse(canary.realized(self.row("failed", [])))
        self.assertTrue(canary.realized(self.row("later-edit", [ok], final="b")))
        self.assertFalse(canary.realized(self.row("later-edit", [ok], final="a")))
        self.assertTrue(canary.realized(self.row("rerun", [ok])))
        self.assertFalse(canary.realized(self.row("broader", [])))

    def test_the_finish_line_gate_never_unrealizes_a_condition(self):
        """dev-v9 stopped at its failed control: the gate's passing run at Stop was read as a
        successful context run. Realization is judged on the learned context's runs alone."""
        ctx = lambda outcome, digest: dict(kind="receipt", phase="completed", outcome=outcome, digest=digest,  # noqa: E731
                                           origin="context", at="PostToolBatch")
        gate = lambda outcome, digest: dict(kind="receipt", phase="completed", outcome=outcome, digest=digest,  # noqa: E731
                                            origin="stop-gate", at="Stop")
        cases = [
            ("failed", [ctx("failed", "a"), gate("applied", "b")], "b", True),
            ("failed", [ctx("failed", "a"), gate("failed", "b")], "b", True),
            ("failed", [gate("failed", "b")], "b", False),  # no context run: nothing was tested
            ("failed", [ctx("applied", "a"), gate("applied", "b")], "b", False),  # the first context run passed
            ("later-edit", [ctx("applied", "a"), gate("applied", "b")], "b", True),
            ("later-edit", [ctx("applied", "b"), gate("applied", "b")], "b", False),  # the receipt covers the end
            ("config-change", [ctx("applied", "a"), gate("failed", "b")], "b", True),
            ("positive", [ctx("applied", "b")], "b", True),
            ("positive", [ctx("applied", "a"), gate("applied", "b")], "b", False),  # only the gate covered the end
            ("broader", [ctx("applied", "a"), gate("applied", "a")], "a", True),
            ("rerun", [gate("applied", "a")], "a", False),
        ]
        for condition, receipts, final, expected in cases:
            self.assertEqual(canary.realized(self.row(condition, receipts, final=final)), expected, (condition, receipts))
        # A journal written before origins were recorded: the gate's run is the one at Stop.
        old = [dict(ctx("failed", "a"), origin=None), dict(gate("applied", "b"), origin=None)]
        self.assertTrue(canary.realized(self.row("failed", old, final="b")))


class ArchivedDevV9(unittest.TestCase):
    """The aborted dev-v9's only session (00-A4-failed), from its archived hook record: the agent
    edited tests and CHANGELOG first, the context run failed, the agent fixed the code and stopped
    without testing, and the gate passed at finish. That is the failed control, realized, with its
    necessary verification met by the gate."""

    def test_the_failed_control_is_realized_and_its_necessary_work_met(self):
        base = Path(__file__).parent / "results/20261001-dev-v9/sessions/00-A4-failed"
        timeline = canary.receipt_replay.extract(base / "production-hooks.compact.jsonl", "A4")
        receipts = [e for e in timeline["events"] if e["kind"] == "receipt"]
        self.assertEqual([(e["origin"], e["outcome"], e["origin_recorded"]) for e in receipts],
                         [("context", "failed", False), ("stop-gate", "applied", False)])
        self.assertEqual(receipts[1]["command"], canary.COMMAND, "the gate ran the route's own command")
        self.assertFalse(receipts[1]["completion_delivered"], "a passing gate run is a line for the person")
        replay = canary.receipt_replay.replay(timeline)
        self.assertTrue(replay["verification_necessary"])
        self.assertFalse(replay["necessary_work_skipped"])
        self.assertEqual([(g["outcome"], g["covers_final"]) for g in replay["gate_runs"]], [("applied", True)])
        row = dict(condition="failed", timeline=timeline, replay=replay)
        self.assertTrue(canary.realized(row))
        # The archived collection was scored by version 2, which got both wrong.
        old = json.loads((base / "collection.json").read_text())
        self.assertEqual((old["replay"]["version"], old["replay"]["necessary_work_skipped"]), (2, True))


def spool(path, *records):
    """Adage's spool framing: magic, little-endian length, four reserved bytes, then the JSON record."""
    data = b""
    for r in records:
        body = json.dumps(r).encode()
        data += bytes([0xAD, 0xA6, 0xE5, 0x01]) + len(body).to_bytes(4, "little") + b"\0\0\0\0" + body
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)


class SessionIntegrity(unittest.TestCase):
    """dev-v8b: an outside pytest wrote bytecode into the shared runtime, every grant lapsed, and the
    harness kept dispatching card-only sessions under execution-arm labels."""

    def test_spool_decisions_and_lapse(self):
        with tempfile.TemporaryDirectory() as tmp:
            home = Path(tmp)
            reason = "exec-changed: the approval for this route no longer matches (route_exec_digest)"
            spool(home / "sessions/s.spool",
                  dict(ts=1, event="PostToolBatch", exec=[dict(route=canary.ROUTE, outcome="applied")]),
                  dict(ts=2, event="UserPromptSubmit", decision={}),
                  dict(ts=3, event="PostToolBatch", exec=[dict(route=canary.ROUTE, withheld="approval", reason=reason)]))
            decisions = canary.exec_decisions(home)
        self.assertEqual([(d["outcome"], d["withheld"]) for d in decisions], [("applied", None), (None, "approval")])
        row = dict(contract_emitted=True, exec_decisions=decisions, all_journal_receipts=[dict(id="op")])
        self.assertTrue(canary.lapsed(row))
        self.assertEqual(canary.integrity(row), ["learned verification withheld (approval: %s)" % reason])
        clean = dict(row, exec_decisions=decisions[:1])
        self.assertFalse(canary.lapsed(clean))
        self.assertEqual(canary.integrity(clean), [])
        self.assertTrue(canary.lapsed(dict(clean, contract_emitted=False)))

    def test_censoring_reads_the_session_caps(self):
        argv = ["claude", "--max-budget-usd", "1.5", "--max-turns", "60"]
        done = dict(result_status="success", terminal_reason="completed", num_turns=20, total_cost_usd=0.1)
        self.assertEqual(canary.censored(done, argv), [])
        self.assertEqual(canary.censored(dict(done, num_turns=60, terminal_reason="max_turns"), argv),
                         ["not completed", "turn cap"])
        self.assertEqual(canary.censored(dict(done, total_cost_usd=1.5), argv), ["budget cap"])

    def runtime(self, tmp):
        python = Path(tmp) / "rt/bin/python3.12"
        python.parent.mkdir(parents=True)
        python.write_text("")
        (Path(tmp) / "rt/lib/python3.12/encodings").mkdir(parents=True)
        (Path(tmp) / "rt/lib/python3.12/os.py").write_text("x = 1\n")
        study = Path(tmp) / "study"
        study.mkdir()
        return study, dict(preparation_python=str(python))

    def unprotect(self, tmp):
        for p in sorted(Path(tmp).rglob("*")):
            p.chmod(p.stat().st_mode | 0o200)

    @unittest.skipIf(hasattr(os, "geteuid") and os.geteuid() == 0, "root bypasses the permission bits this test checks")
    def test_preparation_protects_the_runtime_and_dispatch_stops_when_it_changes(self):
        with tempfile.TemporaryDirectory() as tmp:
            study, m = self.runtime(tmp)
            lib = canary.runtime_lib(m)
            try:
                with self.assertRaises(ValueError):
                    canary.require_runtime(study, m)  # never protected
                state = canary.protect_runtime(study, m)
                self.assertEqual(canary.writable(lib), [])
                canary.require_runtime(study, m)
                with self.assertRaises(OSError, msg="nothing outside a sandbox can add bytecode now"):
                    (lib / "python3.12/__pycache__").mkdir()
                # Someone restores write access and an unsandboxed run writes bytecode, as in dev-v8b.
                self.unprotect(tmp)
                (lib / "python3.12/__pycache__").mkdir()
                (lib / "python3.12/__pycache__/os.cpython-312.pyc").write_bytes(b"\0")
                self.assertNotEqual(canary.tree_digest(lib), state["sha256"])
                with self.assertRaisesRegex(ValueError, "changed after preparation"):
                    canary.require_runtime(study, m)
                stopped = [json.loads(line) for line in (study / "ABORTED.jsonl").read_text().splitlines()]
                self.assertEqual([s["status"] for s in stopped], ["stopped", "stopped"])
            finally:
                self.unprotect(tmp)

    def test_cloud_placeholders_are_refused(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            (tmp / "src").mkdir()
            (tmp / "target").mkdir()
            for rel in ("src/a.py", "src/b.py", "target/big.rlib"):
                (tmp / rel).write_text("x")
            canary.require_local([tmp])
            real = canary.os.lstat

            class Stat:
                def __init__(self, st, flags):
                    self.st, self.st_flags = st, flags

                def __getattr__(self, name):
                    return getattr(self.st, name)

            def lstat(p):
                st = real(p)
                return Stat(st, canary.SF_DATALESS if str(p).endswith(("b.py", "big.rlib")) else 0)
            with patch.object(canary.os, "lstat", lstat):
                self.assertEqual(canary.dataless([tmp]), [str(tmp / "src/b.py")], "build output is not an input")
                with self.assertRaisesRegex(ValueError, "dataless"):
                    canary.require_local([tmp / "src"])

    def test_grant_preflight_runs_the_session_start_hook_on_a_throwaway_home(self):
        with tempfile.TemporaryDirectory() as tmp:
            tmp = Path(tmp)
            checkout, home = tmp / "repo", tmp / "adage-home"
            checkout.mkdir()
            (checkout / "a.py").write_text("x = 1\n")
            home.mkdir()
            binary = tmp / "adage-exec"
            # The contract appears only while the grant binds; the hook also writes spool state.
            binary.write_text("#!/bin/sh\ncat > /dev/null\nmkdir -p \"$ADAGE_HOME/sessions\"\n"
                              "touch \"$ADAGE_HOME/sessions/grant-preflight.spool\"\n"
                              "if [ -f \"$ADAGE_HOME/approvals.json\" ]; then\n"
                              "  printf '%s' '{\"hookSpecificOutput\":{\"additionalContext\":"
                              "\"<adage-reflex kind=\\\"execution-contract\\\">x</adage-reflex>\"}}'\n"
                              "else printf '{}'; fi\n")
            binary.chmod(0o755)
            a = dict(execution_root=str(checkout), home=str(home), binaries={"adage-exec": str(binary)})
            self.assertFalse(canary.preflight_grant(a))
            (home / "approvals.json").write_text("{}")
            self.assertTrue(canary.preflight_grant(a))
            self.assertEqual(sorted(p.name for p in home.iterdir()), ["approvals.json"], "the real home is untouched")


if __name__ == "__main__":
    unittest.main()
