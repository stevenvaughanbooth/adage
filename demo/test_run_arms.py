"""Chunk E1, offline: the requests metric and its cross-check, the arm table, plugin staging, the
spool- and stream-derived metrics, and the report's comparison of any two arms (T-E1-01, -02, -04).

  cd demo && python3 -m unittest test_run_arms test_replay_hooks     (or: cargo xtask test python demo)
"""

import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stderr
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import adage_metrics as am  # noqa: E402
import report  # noqa: E402
import run  # noqa: E402

HARNESS = HERE.parent / "evals" / "reflex-proof" / "harness"
TS = "2026-10-03T00:00:00.000Z"


def dispatch(rid, source="sdk"):
    return f"{TS} [DEBUG] [API REQUEST] /v1/messages x-client-request-id={rid} source={source}"


def obs(*request_ids):
    """Stream observations, one per id, shaped as stream-json carries them."""
    return [{"value": {"type": "stream_event", "event": {"type": "message_start", "message": {"id": f"msg_{i}"}},
                       "request_id": rid}} for i, rid in enumerate(request_ids)]


class RequestsTest(unittest.TestCase):
    def test_retry_seen_in_the_stream(self):
        # Three dispatches, one of them a retry after a logged API error; the stream shows three ids.
        log = "\n".join([f"{TS} [DEBUG] something else", dispatch("c-1"), dispatch("c-2"),
                         f"{TS} [ERROR] API error (attempt 1/10): 529 overloaded", dispatch("c-3")])
        r = am.requests(log, obs("req_a", "req_b", "req_c"))
        self.assertEqual(r["requests"], 3)
        self.assertEqual(r["requests"], r["stream_request_ids"])
        self.assertTrue(r["cross_check"])
        self.assertTrue(r["verified"])
        self.assertEqual(r["api_error_entries"], 1)

    def test_retry_explained_by_the_error_entries(self):
        # The failed attempt never reached the stream: the surplus of one is explained by one error.
        log = "\n".join([dispatch("c-1"), f"{TS} [ERROR] API error (attempt 1/10): 529", dispatch("c-2"), dispatch("c-3")])
        r = am.requests(log, obs("req_a", "req_b"))
        self.assertEqual((r["requests"], r["stream_request_ids"]), (3, 2))
        self.assertFalse(r["cross_check"])
        self.assertTrue(r["verified"])

    def test_duplicate_dispatch_ids_are_unverified(self):
        r = am.requests("\n".join([dispatch("c-1"), dispatch("c-1")]), obs("req_a", "req_b"))
        self.assertEqual(r["requests"], 2)
        self.assertTrue(r["cross_check"])
        self.assertFalse(r["verified"])

    def test_unexplained_surplus_and_deficit_are_unverified(self):
        self.assertFalse(am.requests("\n".join([dispatch("c-1"), dispatch("c-2")]), obs("req_a"))["verified"])
        self.assertFalse(am.requests(dispatch("c-1"), obs("req_a", "req_b"))["verified"])

    def test_stream_ids_are_distinct_explicit_fields_only(self):
        o = obs("req_a") + obs("req_a") + [{"value": {"type": "assistant", "message": {"id": "msg_9"}}}]
        o.append({"value": {"nested": [{"requestId": "req_b"}]}})
        r = am.requests("\n".join([dispatch("c-1"), dispatch("c-2")]), o)
        self.assertEqual(r["stream_request_ids"], 2)
        self.assertTrue(r["verified"])

    def test_only_full_dispatch_lines_count(self):
        log = "\n".join([dispatch("c-1"), dispatch("c-2") + " trailing", "[DEBUG] [API REQUEST] /v1/messages x-client-request-id=c-3 source=sdk",
                         dispatch("c-4").replace("/v1/messages", "/v1/messages/count_tokens")])
        self.assertEqual(am.requests(log, obs("r"))["requests"], 1)

    def test_empty(self):
        self.assertEqual(am.requests("", []), dict(requests=0, stream_request_ids=0, api_error_entries=0,
                                                   cross_check=True, verified=True))

    @unittest.skipUnless((HARNESS / "heldout_analysis.py").exists(), "reflex-proof harness not present")
    def test_mirrors_the_harness(self):
        sys.path.insert(0, str(HARNESS))
        try:
            import heldout_analysis as ha
        finally:
            sys.path.remove(str(HARNESS))
        self.assertEqual(am.DEBUG_REQUEST.pattern, ha.DEBUG_REQUEST.pattern)
        self.assertEqual(am.API_ERROR.pattern, ha.API_ERROR.pattern)
        cases = [
            ("\n".join([dispatch("c-1"), f"{TS} [ERROR] API error (attempt 1/10): x", dispatch("c-2")]), obs("a")),
            ("\n".join([dispatch("c-1"), dispatch("c-1")]), obs("a", "b")),
            ("\n".join([dispatch("c-1"), dispatch("c-2")]), obs("a", "a", "b")),
        ]
        sys.path.insert(0, str(HARNESS))  # requests() imports probe lazily
        try:
            for log, o in cases:
                self.assertEqual(am.requests(log, o), ha.requests(log, o))
        finally:
            sys.path.remove(str(HARNESS))


class ArmTableTest(unittest.TestCase):
    BASE = {"PATH": "/usr/bin", "ADAGE_HOME": "/elsewhere", "ADAGE_MOD_OBSERVE": "1", "ADAGE_ARM": "x"}

    def wire(self, arm):
        calls = []

        def settings_for(a):
            calls.append(a)
            return Path(f"/tmp/settings-{a}.json")
        flags, env = run.arm_invocation(arm, self.BASE, settings_for=settings_for, plugin_dir=Path("/p/claude-code"),
                                        adage_home=Path("/h"), debug_file=Path("/r/claude-debug.log"))
        return flags, env, calls

    def test_every_eval_arm_is_declared(self):
        self.assertEqual(set(run.ARMS) - {"train"}, {"a0", "control", "adage", "mod-observe", "mod"})

    def test_a0_has_no_hooks_at_all(self):
        flags, env, calls = self.wire("a0")
        self.assertNotIn("--settings", flags)
        self.assertNotIn("--plugin-dir", flags)
        self.assertEqual(calls, [])
        self.assertEqual([k for k in env if k.startswith("ADAGE_")], [])

    def test_hook_arms(self):
        for arm, adage_arm in (("control", "control"), ("adage", "treated")):
            flags, env, calls = self.wire(arm)
            self.assertEqual(flags[flags.index("--settings") + 1], f"/tmp/settings-{arm}.json")
            self.assertNotIn("--plugin-dir", flags)
            self.assertEqual(calls, [arm])
            self.assertEqual((env["ADAGE_ARM"], env["ADAGE_HOME"], env["ADAGE_AUTO_LEARN"]), (adage_arm, "/h", "0"))
            self.assertNotIn("ADAGE_MOD_OBSERVE", env)

    def test_plugin_arms_never_pass_settings(self):
        for arm, observe in (("mod-observe", True), ("mod", False)):
            flags, env, calls = self.wire(arm)
            self.assertNotIn("--settings", flags)
            self.assertEqual(calls, [], "a plugin arm must not generate hooks-config settings")
            self.assertEqual(flags[flags.index("--plugin-dir") + 1], "/p/claude-code")
            self.assertEqual(flags.count("--plugin-dir"), 1)
            self.assertEqual(env["ADAGE_ARM"], "treated")
            self.assertEqual(env.get("ADAGE_MOD_OBSERVE"), "1" if observe else None)

    def test_plugin_arm_needs_a_plugin(self):
        with self.assertRaises(ValueError):
            run.arm_invocation("mod", {}, settings_for=lambda a: "x")

    def test_every_arm_runs_under_debug(self):
        for arm in run.ARMS:
            flags, _, _ = self.wire(arm)
            self.assertIn("--debug", flags)
            self.assertEqual(flags[flags.index("--debug-file") + 1], "/r/claude-debug.log")

    def test_train_keeps_its_empty_settings(self):
        flags, env, calls = self.wire("train")
        self.assertEqual(calls, ["train"])
        self.assertEqual(env["ADAGE_ARM"], "treated")

    def test_command_line(self):
        a = run.parse_args(["one", "W-2041", "mod", "--debug"])
        self.assertEqual((a.what, a.task, a.arm, a.debug), ("one", "W-2041", "mod", True))
        self.assertEqual(run.parse_args(["eval", "--arms", "a0,mod"]).arms, ["a0", "mod"])
        self.assertEqual(run.parse_args([]).what, "all")
        for bad in (["one", "W-2041", "nope"], ["eval", "--arms", "a0,train"], ["one"]):
            with redirect_stderr(io.StringIO()), self.assertRaises(SystemExit):
                run.parse_args(bad)


class StagePluginTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.fake = self.tmp / "adage"
        self.fake.write_text('#!/bin/sh\n[ "$1" = hook ] && { echo "FAKE $*"; cat >/dev/null; }\nexit 0\n')
        self.fake.chmod(0o755)

    def test_staged_layout_matches_build_sh(self):
        src = self.tmp / "src"
        (src / "scripts").mkdir(parents=True)
        (src / "scripts" / "session-start").write_text("#!/bin/sh\n")
        (src / "dist" / "x86_64-unknown-linux-musl").mkdir(parents=True)  # a stale stage: left behind
        (src / "dist" / "x86_64-unknown-linux-musl" / "adage").write_text("stale")
        dest = run.stage_plugin(src, self.fake, self.tmp / "staged", target="x86_64-unknown-linux-gnu")
        self.assertTrue((dest / "scripts" / "session-start").exists())
        self.assertFalse((dest / "dist" / "x86_64-unknown-linux-musl").exists())
        line = (dest / "dist" / "SHA256SUMS").read_text()
        import hashlib
        self.assertEqual(line, hashlib.sha256(self.fake.read_bytes()).hexdigest() + "  x86_64-unknown-linux-gnu/adage\n")
        self.assertTrue(os.access(dest / "dist" / "x86_64-unknown-linux-gnu" / "adage", os.X_OK))
        self.assertFalse((src / "dist" / "SHA256SUMS").exists(), "the source plugin is never written")

    @unittest.skipUnless((HERE.parent / "plugin" / "claude-code" / "scripts" / "session-start").exists()
                         and shutil.which("sh") and shutil.which("sha256sum"), "plugin shim or sh/sha256sum missing")
    def test_the_plugin_shim_links_the_staged_binary(self):
        dest = run.stage_plugin(HERE.parent / "plugin" / "claude-code", self.fake, self.tmp / "plugin")
        data = self.tmp / "data"
        env = {"PATH": "/usr/bin:/bin", "HOME": str(self.tmp), "ADAGE_HOME": str(self.tmp / "home")}
        r = subprocess.run([str(dest / "scripts" / "session-start"), str(dest), str(data)], input='{"session_id":"s"}',
                           capture_output=True, text=True, env=env, timeout=30)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stdout.strip(), "FAKE hook claude SessionStart", r.stdout + r.stderr)
        self.assertTrue(os.readlink(data / "bin" / "adage").startswith("adage-"))


def write_spool(path, records, corrupt=False):
    data = b"".join(am.frame(r) for r in records)
    if corrupt:
        bad = bytearray(am.frame({"kind": "session_start"}))
        bad[-2] ^= 0xFF  # CRC no longer matches
        data = bytes(bad) + data
    Path(path).write_bytes(data)


SPOOL = [
    {"ts": 1, "event": "SessionStart", "kind": "session_start", "arm": "treated"},
    {"ts": 2, "event": "UserPromptSubmit", "kind": "prompt", "decision": {
        "directives": [{"route": "lookup", "text": "x"}, {"route": "dead", "text": "y"}],
        "would_have": [{"route": "lookup"}], "judgements": [], "observed": []}},
    {"ts": 3, "event": "TurnStep", "kind": "turn_step", "decision": {"predicted": {"route": "gen", "kind": "run"}}},
    {"ts": 4, "event": "TurnStep", "kind": "turn_step", "decision": {"predicted": None, "would_predict": True}},
    {"ts": 5, "event": "TurnStep", "kind": "turn_step", "predict": {"route": "gen"}},
    {"ts": 6, "event": "PreToolUse", "kind": "pretool", "answered": "receipt", "receipt": True},
    {"ts": 7, "event": "PreToolUse", "kind": "pretool", "receipt": True},
    {"ts": 8, "event": "PostToolBatch", "kind": "batch", "decision": {"directives": [], "judgements": [
        {"kind": "substituted", "route": "dead"}, {"kind": "followed", "route": "lookup"}]}},
    {"ts": 9, "event": "PreToolUse", "kind": "pretool", "answered": "substitute", "withheld": True},
    {"ts": 10, "event": "Stop", "kind": "stop_gate", "blocked": True, "decision": "not a dict"},
    "not a record",
]


class SpoolMetricsTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_framed_spool_skips_corrupt_frames(self):
        p = self.tmp / "s.spool"
        write_spool(p, [r for r in SPOOL if isinstance(r, dict)], corrupt=True)
        recs = am.read_spool(p)
        self.assertEqual([r["ts"] for r in recs], list(range(1, 11)))

    def test_json_lines_fallback_and_missing(self):
        p = self.tmp / "s.jsonl"
        p.write_text("\n".join(json.dumps(r) for r in SPOOL[:3]) + "\nnot json\n")
        self.assertEqual(len(am.read_spool(p)), 3)
        self.assertEqual(am.read_spool(self.tmp / "missing.spool"), [])

    def test_counts(self):
        m = am.spool_metrics(SPOOL)
        self.assertEqual(m, {"predicted_steps": 2, "receipts_as_results": 1, "dead_end_substituted": 1,
                             "directives": 2, "session_starts": 1, "spool_records": 10})

    def test_derived_metrics(self):
        events = [{"cat": "verify-fail"}, {"cat": "verify"}, {"cat": "verify-fail"}]
        stream = [
            {"type": "assistant", "parent_tool_use_id": "toolu_task", "message": {"content": [
                {"type": "tool_use", "name": "Read"}, {"type": "tool_use", "name": "Bash"}, {"type": "text"}]}},
            {"type": "assistant", "parent_tool_use_id": None, "message": {"content": [{"type": "tool_use"}]}},
            {"type": "system", "subtype": "hook_response",
             "stdout": '{"additionalContext":"<adage-reflex a>x</adage-reflex>\\n<adage-reflex b>y</adage-reflex>"}'},
            {"type": "system", "subtype": "hook_started", "hook_name": "<adage-reflex never counted"},
        ]
        log = "\n".join([dispatch("c-1"), dispatch("c-2")])
        m = run.derived_metrics("mod", events, stream, obs("a", "b"), log, SPOOL, spool_exists=True)
        self.assertEqual((m["requests"], m["requests_verified"]), (2, True))
        self.assertEqual(m["dead_end_hits"], 2)
        self.assertEqual(m["dead_end"], {"hit": 2, "substituted": 1})
        self.assertEqual((m["predicted_steps"], m["receipts_as_results"], m["directives"]), (2, 1, 2))
        self.assertEqual((m["subagent_tool_calls"], m["directives_in_stream"]), (2, 2))
        self.assertTrue(m["hooks_wired_once"])
        # Without a spool, directives come from the stream.
        self.assertEqual(run.derived_metrics("adage", [], stream, [], "", [], False)["directives"], 2)

    def test_wired_once(self):
        two_starts = [{"kind": "session_start"}, {"kind": "session_start"}]
        self.assertFalse(run.derived_metrics("mod", [], [], [], "", two_starts, True)["hooks_wired_once"])
        self.assertTrue(run.derived_metrics("a0", [], [], [], "", [], False)["hooks_wired_once"])
        self.assertFalse(run.derived_metrics("a0", [], [], [], "", [], True)["hooks_wired_once"])
        self.assertIsNone(run.derived_metrics("train", [], [], [], "", [], False)["hooks_wired_once"])


class ReportTest(unittest.TestCase):
    def runs(self):
        out = []
        for t, task in enumerate(("W-2041", "W-2042", "W-2043", "W-2044")):
            for rep in range(3):
                for arm, req in (("a0", 20 + t + rep), ("mod", 8 + t + rep), ("adage", 15 + t)):
                    out.append({"arm": arm, "task": task, "metrics": {
                        "status": "ok", "requests": req, "requests_verified": True, "cost_usd": req / 100,
                        "passed": rep != 1 or arm != "mod", "hidden_passed": True, "stale_generated": False,
                        "tool_calls": 5, "turns": 6, "predicted_steps": 2 if arm == "mod" else 0}})
        return out

    def test_compare_mod_against_a0(self):
        cmp = report.compare(self.runs(), a="mod", b="a0", n_perm=2000, n_boot=500)
        for name in ("requests", "cost_usd", "pass_rate", "hidden_pass_rate", "stale_generated_rate"):
            row = cmp["metrics"][name]
            self.assertIn("p_value", row, name)
            self.assertIn("ci95", row, name)
            self.assertEqual(row["n_tasks"], 4, name)
        r = cmp["metrics"]["requests"]
        self.assertAlmostEqual(r["diff"], -12.0)
        self.assertLess(r["ci95"][1], 0)
        self.assertLess(r["p_value"], 0.01)
        self.assertEqual(r["n"], {"a0": 12, "adage": 12, "mod": 12})
        self.assertIsNotNone(r["p_holm"])
        self.assertIsNone(cmp["metrics"]["predicted_steps"]["p_holm"], "mechanism counts stay out of the Holm family")
        self.assertIn("requests", report.table(cmp, a="mod", b="a0"))

    def test_unverified_requests_are_missing(self):
        runs = self.runs()
        runs[0]["metrics"]["requests_verified"] = False
        row = report.compare(runs, a="mod", b="a0", n_perm=200, n_boot=100)["metrics"]["requests"]
        self.assertEqual(row["n_missing"][runs[0]["arm"]], 1)

    def test_old_runs_without_e1_keys(self):
        runs = [{"arm": a, "task": "W-1", "metrics": {"status": "ok", "passed": True, "tool_calls": 1}} for a in ("adage", "control")]
        row = report.compare(runs, n_perm=10, n_boot=10)["metrics"]["requests"]
        self.assertEqual((row["adage"], row["control"]), (None, None))


if __name__ == "__main__":
    unittest.main()
