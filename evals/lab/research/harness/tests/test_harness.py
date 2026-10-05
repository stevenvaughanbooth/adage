"""Unit tests for the harness.   cd evals/lab/research && python3 -m unittest discover -s harness/tests -t .

Synthetic Claude Code transcripts with known S (the Rust replay's own test cases, ported), the
cost model on hand-computable usage, the loader's discarded-run exclusion on a synthetic runner
root, leakage checks, statistics, and (when the RepoBench archives and target/debug/adage exist)
parity with the Rust replay on real sessions.
"""

from __future__ import annotations

import json
import shutil
import tempfile
import unittest
from pathlib import Path

from harness import costmodel as CM
from harness import features as F
from harness import stats as ST
from harness import traces as T
from harness.leakage import LeakageError, assert_clean, scan_memory
from harness.replay import Directive, score, score_session

NO_PYTEST = "/usr/bin/python: No module named pytest"
RAN = "============ test session starts ============\ncollected 3 items\n\ntests/test_x.py ...\n\n3 passed in 0.10s"


def transcript(calls, sid="s", usage=None, prompt="Fix the bug", model="claude-haiku-4-5-20251001"):
    """One tool call per model turn: (tool, input, output, is_error)."""
    lines = [json.dumps({"type": "user", "sessionId": sid, "cwd": "/w/r", "message": {"content": prompt}})]
    for i, (tool, inp, out, err) in enumerate(calls):
        u = usage[i] if usage else {"input_tokens": 1, "output_tokens": 10, "cache_read_input_tokens": 1000 + 100 * i,
                                    "cache_creation_input_tokens": 100}
        lines.append(json.dumps({"type": "assistant", "sessionId": sid, "cwd": "/w/r", "message": {
            "id": f"m{i}", "model": model, "usage": u,
            "content": [{"type": "tool_use", "id": f"c{i}", "name": tool, "input": inp}]}}))
        lines.append(json.dumps({"type": "user", "sessionId": sid, "cwd": "/w/r", "message": {"content": [
            {"type": "tool_result", "tool_use_id": f"c{i}", "content": out, "is_error": err}]}}))
    return "\n".join(lines) + "\n"


def bash(cmd, out, err):
    return ("Bash", {"command": cmd}, out, err)


def dead_end_calls(piped=False):
    first = "python -m pytest tests/test_x.py 2>&1 | head -50" if piped else "python -m pytest tests/test_x.py"
    return [
        ("Read", {"file_path": "/w/r/src/app.py"}, "1\tcode", False),
        bash(first, NO_PYTEST, not piped),
        bash("ls -la", "total 8\n.venv\nsrc\ntests", False),
        bash("pip install pytest", "Successfully installed pytest-8.0", False),
        ("Read", {"file_path": "/w/r/tests/test_x.py"}, "1\tdef test_x(): pass", False),
        bash("python -m pytest tests/test_x.py", RAN, False),
    ]


class Tmp(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def session(self, calls, name="s", passed=True, task="t1", **kw):
        p = self.tmp / f"{name}.jsonl"
        p.write_text(transcript(calls, sid=name, **kw))
        return T.session_from_transcript(p, task=task, passed=passed, cost_usd=0.1, run=name)


def venv_on_failure(sit):
    """U11-shaped: after a failure with an environment signature, run pytest in the .venv."""
    if sit.event == "PostToolUse" and sit.failed and "No module named" in sit.call.output:
        return Directive("run", command=".venv/bin/python -m pytest", route="u11")
    return None


def venv_pre(sit):
    """Pre-emptive with hindsight (Rust --mode pre-emptive)."""
    if sit.event == "PreToolUse" and sit.hindsight and sit.hindsight["will_fail"] and "No module named" in sit.hindsight["output"]:
        if "u11" not in sit.state:
            sit.state["u11"] = 1
            return Directive("run", command=".venv/bin/python -m pytest", route="u11")
    return None


class Features(unittest.TestCase):
    def test_invocations_match_the_rust_cases(self):
        cases = [
            ("python -m pytest tests/test_x.py -xvs", ("system", "pytest")),
            ("python3 -m pytest tests/ 2>&1 | head -100", ("system", "pytest")),
            (".venv/bin/python -m pytest tests/test_x.py::t", ("venv", "pytest")),
            ("/tmp/w/repo/.venv/bin/pytest -q", ("venv", "pytest")),
            ("source .venv/bin/activate && python -m pytest tests/test_x.py", ("venv", "pytest")),
            ("cd /w/r && . .venv/bin/activate && pytest -x", ("venv", "pytest")),
            ("uv run pytest tests", ("venv", "pytest")),
            ("uv run --frozen python -m pytest", ("venv", "pytest")),
            ("PYTHONPATH=src python -m pytest tests", ("system", "pytest")),
            ("pytest tests/test_x.py", ("system", "pytest")),
            ("tox -e py", ("tox", "tox")),
            ("python -m unittest discover", ("system", "unittest")),
            ("pnpm vitest run packages/runtime-core/__tests__/x.spec.ts", ("node", "vitest")),
            ("npx jest foo", ("node", "jest")),
            ("npm test", ("node", "pkg-test")),
            ("node_modules/.bin/vitest run --project unit x", ("node", "vitest")),
            ("go test ./... -run TestX", ("go", "go-test")),
            ("cargo test -p adage-eval", ("cargo", "cargo-test")),
            ("python repro.py", None),
            ('python -c "import click"', None),
            ("grep -rn pytest pyproject.toml", None),
            ("echo 'python -m pytest && x'", None),
        ]
        for cmd, want in cases:
            got = F.invocation(cmd)
            self.assertEqual((got[0].interp, got[0].runner) if got else None, want, cmd)
        _, args = F.invocation("python -m pytest tests/test_x.py::test_a -xvs")
        self.assertEqual(F.targets(args, "/w/r"), ("tests/test_x.py",))
        self.assertEqual(F.go_packages(["-run", "TestX", "./..."], "/w/r"), ("pkg:...",))

    def test_features_of_calls(self):
        c = lambda cmd, out, err=False: F.act("Bash", {"command": cmd}, out, err, "/w/r")  # noqa: E731
        a = c("python -m pytest /w/r/tests/test_x.py 2>&1 | head", NO_PYTEST)
        self.assertTrue(a.env_sig and a.failed and a.env_friction and not a.executed)
        self.assertEqual(a.targets, ("tests/test_x.py",))
        self.assertTrue(c(".venv/bin/python -m pytest tests", RAN).executed)
        self.assertFalse(c(".venv/bin/python -m pytest tests", "collected 0 items\n\nno tests ran in 0.01s").executed)
        for probe in ["ls -la | head -20", "pip install -e .", "which python", "ls .venv/bin", "cat pyproject.toml",
                      "python --version"]:
            self.assertTrue(c(probe, "").env_probe, probe)
        for not_probe in ["ls src/click", "git status", "grep -n def src/app.py"]:
            self.assertFalse(c(not_probe, "").env_probe, not_probe)
        self.assertTrue(c("find tests -name '*.py' | xargs grep -l Choice", "").discovery)


class Scoring(Tmp):
    def test_post_hoc_saves_the_friction_between_the_dead_end_and_the_working_run(self):
        s = self.session(dead_end_calls())
        r = score_session(venv_on_failure, s)
        self.assertEqual(len(r.scored), 1)
        x = r.scored[0]
        self.assertEqual((x.t, x.pre, x.t_star, x.event), (1, False, 5, "PostToolUseFailure"))
        self.assertTrue(x.useful and not x.harmful and not x.superseded)
        self.assertEqual(x.saved, 2.0)  # ls -la, pip install (the Read is not friction)
        self.assertEqual(x.saved_idx, (2, 3))
        self.assertTrue(r.dead_end)
        strict = score_session(venv_on_failure, s, sat="strict")
        self.assertFalse(strict.scored[0].useful)
        self.assertAlmostEqual(strict.net_raw, -strict.cost)  # nothing saved; the directive's carry remains

    def test_pre_emptive_saves_the_intercepted_call_and_sees_piped_dead_ends(self):
        s = self.session(dead_end_calls())
        r = score_session(venv_pre, s, hindsight=True)
        self.assertEqual([(x.t, x.pre, x.saved) for x in r.scored], [(1, True, 3.0)])
        piped = self.session(dead_end_calls(piped=True), name="p")
        self.assertEqual(score_session(venv_on_failure, piped).scored, [])  # exit 0: no failure hook
        self.assertEqual(score_session(venv_pre, piped, hindsight=True).saved, 3.0)
        # Without hindsight the PreToolUse policy sees no future: nothing fires.
        self.assertEqual(score_session(venv_pre, piped).scored, [])

    def test_pre_tool_use_hides_the_output(self):
        seen = []

        def spy(sit):
            if sit.event == "PreToolUse":
                seen.append((sit.call.output, sit.hindsight, len(sit.history)))
            return None
        s = self.session(dead_end_calls())
        score_session(spy, s)
        self.assertTrue(all(o == "" and h is None for o, h, _ in seen))
        self.assertEqual([n for _, _, n in seen], list(range(6)))

    def test_a_different_runner_that_ran_makes_it_harmful_only_in_a_passed_session(self):
        calls = [bash("python -m pytest tests", NO_PYTEST, True),
                 bash("tox -e py", "py: commands[0]> pytest\n3 passed in 0.2s\n  py: OK", False)]
        r = score_session(venv_on_failure, self.session(calls, name="h", passed=True))
        self.assertTrue(r.scored[0].harmful and not r.scored[0].useful)
        self.assertTrue(r.harm_session)
        self.assertAlmostEqual(r.net_raw, -r.cost)
        # followed and never satisfied: a misfire, charged misfire_calls at p_follow
        self.assertAlmostEqual(r.net_calls, -2.0 - r.cost)
        r2 = score_session(venv_on_failure, self.session(calls, name="h2", passed=False))
        self.assertFalse(r2.scored[0].harmful)

    def test_a_directive_inside_an_earlier_window_is_superseded(self):
        calls = [bash("python -m pytest", NO_PYTEST, True), bash("pip list", "No module named foo", False),
                 bash("python -m pytest", NO_PYTEST, True), bash("python -m pytest", RAN, False)]

        def every_failure(sit):
            if sit.event == "PostToolUse" and sit.failed:
                return Directive("run", command=".venv/bin/python -m pytest", route=f"r{sit.t}")
            return None
        r = score_session(every_failure, self.session(calls))
        a, b = r.scored
        self.assertEqual((a.t_star, a.saved), (3, 2.0))
        self.assertTrue(b.superseded and b.saved == 0 and b.cost == 0 and b.usd_cost == 0)
        self.assertEqual((r.directives, r.superseded), (1, 1))

    def test_test_files_class(self):
        calls = [("Edit", {"file_path": "/w/r/src/app.py", "old_string": "a", "new_string": "b"}, "ok", False),
                 ("Grep", {"pattern": "def test_", "path": "/w/r/tests"}, "tests/test_app.py", False),
                 bash("find tests -name 'test_*.py'", "tests/test_app.py", False),
                 ("Read", {"file_path": "/w/r/tests/test_app.py"}, "1\tdef test_a(): pass", False),
                 bash(".venv/bin/python -m pytest tests/test_app.py::test_a -q", RAN, False)]

        def prime(sit):
            if sit.event == "PostToolBatch" and sit.t == 0:
                return Directive("test_files", files=("tests/test_app.py",), route="cochange")
            return None
        r = score_session(prime, self.session(calls))
        self.assertEqual((r.scored[0].t, r.scored[0].t_star, r.saved), (0, 4, 2.0))

    def test_advice_is_costed_never_credited(self):
        s = self.session(dead_end_calls())
        r = score_session(lambda sit: Directive("advice", text="Use the venv.") if sit.event == "SessionStart" else None, s)
        self.assertEqual(r.saved, 0.0)
        self.assertLess(r.net_usd, 0.0)
        self.assertFalse(r.scored[0].misfire)

    def test_p_follow_scales_savings(self):
        s = self.session(dead_end_calls())
        full = score_session(venv_on_failure, s, p_follow=1.0)
        half = score_session(venv_on_failure, s, p_follow=0.5)
        self.assertAlmostEqual(half.net_calls, 0.5 * full.saved - full.cost)
        self.assertLess(half.net_usd, full.net_usd)

    def test_report_headline_and_flags(self):
        ss = [self.session(dead_end_calls(), name=f"s{i}", task=f"t{i % 2}") for i in range(4)]
        rep = score(venv_pre, ss, hindsight=True)
        h = rep.headline()
        self.assertIn("$ saved/task", h)
        self.assertIn("HINDSIGHT", h)
        s = rep.summary()
        self.assertEqual((s["sessions"], s["tasks"], s["directives"]), (4, 2, 4))
        cost = sum(r.cost for r in rep.rows) / 4
        self.assertAlmostEqual(s["calls_saved_per_task"], 3.0 - cost)
        self.assertGreater(s["usd_saved_per_task"], 0.0)


class CostModel(Tmp):
    def test_session_total_and_removal(self):
        # Three turns, one call each; hand-priced at Haiku rates ($/Mtok: in 1, out 5, write 1.25, read 0.1).
        usage = [
            {"input_tokens": 10, "output_tokens": 100, "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 500},
            {"input_tokens": 10, "output_tokens": 100, "cache_read_input_tokens": 1500, "cache_creation_input_tokens": 300},
            {"input_tokens": 10, "output_tokens": 100, "cache_read_input_tokens": 1800, "cache_creation_input_tokens": 200},
        ]
        calls = [bash("a", "x", False), bash("b", "y", False), bash("c", "z", False)]
        s = self.session(calls, usage=usage)
        cm = CM.of(s)
        want = sum(u["input_tokens"] * 1 + u["output_tokens"] * 5 + u["cache_creation_input_tokens"] * 1.25
                   + u["cache_read_input_tokens"] * 0.1 for u in usage) / 1e6
        self.assertAlmostEqual(cm.total, want)
        # Removing call 0 (its whole response): g_0 = ctx_1 − ctx_0 = 1810 − 1510 = 300 tokens, written
        # once by response 1 (1.25) and read by 1 later response (0.1); plus response 0's own read,
        # input and output.
        usd, tok = cm.removal([0])
        own = (1000 * 0.1 + 10 * 1 + 100 * 5) / 1e6
        self.assertAlmostEqual(usd, 300 * (1.25 + 0.1 * 1) / 1e6 + own)
        self.assertAlmostEqual(tok, 300 * 2 + 1510)
        # A later call is carried by fewer responses: cheaper to remove than an earlier one of equal size.
        self.assertGreater(cm.removal([0])[0], cm.removal([2])[0])
        # Injecting 100 tokens before any call: written once, read by the 2 later responses.
        self.assertAlmostEqual(cm.injection(100, -1)[0], 100 * (1.25 + 0.1 * 2) / 1e6)
        self.assertEqual(cm.injection(100, 2), (0.0, 0.0))

    def test_one_hour_cache_writes_are_billed_at_twice_input(self):
        usage = [{"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 1000,
                  "cache_creation": {"ephemeral_1h_input_tokens": 1000, "ephemeral_5m_input_tokens": 0}}]
        s = self.session([bash("a", "x", False)], usage=usage)
        self.assertAlmostEqual(CM.of(s).total, 1000 * 2.0 / 1e6)

    def test_rates_by_longest_prefix(self):
        self.assertEqual(CM.rates("claude-haiku-4-5-20251001"), (1.0, 5.0, 1.25, 0.1))
        self.assertEqual(CM.rates("claude-sonnet-5")[3], 0.2)


class Loader(Tmp):
    def make_root(self, with_rerun=True):
        root = self.tmp / "runs" / "click"
        run = "eval-click-aaaaaaaa-r1-control"
        d = root / "private" / "transcripts" / f"-tmp-x-runs-click-runs-{run}-repo"
        d.mkdir(parents=True)
        (d / "old.jsonl").write_text(transcript([bash("python -m pytest", NO_PYTEST, True)], sid="old"))
        if with_rerun:
            (d / "new.jsonl").write_text(transcript([bash(".venv/bin/python -m pytest", RAN, False)], sid="new"))
        state = {"runs": {run: {"status": "passed", "phase": "eval", "arm": "control", "task": "click-aaaaaaaa",
                                "metrics": {"session_id": "new", "hidden_passed": True, "cost_usd": 0.1}}}}
        (root / "state.json").write_text(json.dumps(state))
        return self.tmp / "runs"

    def test_a_discarded_run_is_excluded_not_given_the_reruns_outcome(self):
        base = self.make_root()
        c = T.load(bases=[("v2", base)], include_dx=False, cache=False)
        self.assertEqual([s.sid for s in c.sessions], ["new"])
        self.assertTrue(c.sessions[0].passed)
        self.assertTrue(any(e.get("path", "").endswith("old.jsonl") for e in c.excluded))

    def test_a_missing_rerun_never_falls_back_to_the_discarded_transcript(self):
        base = self.make_root(with_rerun=False)
        c = T.load(bases=[("v2", base)], include_dx=False, cache=False)
        self.assertEqual(c.sessions, [])

    def test_copies_are_deduplicated_by_session_id(self):
        base = self.make_root()
        c = T.load(bases=[("v2", base), ("v1", base)], include_dx=False, cache=False)
        self.assertEqual(len(c.sessions), 1)
        self.assertEqual(len(c.duplicates), 1)
        self.assertTrue(c.duplicates[0]["outcome_agrees"])

    def test_turns_group_by_message_id_and_text_rows_do_not_duplicate_responses(self):
        lines = [
            {"type": "user", "sessionId": "s", "cwd": "/w/r", "message": {"content": "go"}},
            {"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {"id": "m1", "model": "claude-haiku-4-5",
             "usage": {"output_tokens": 5}, "content": [{"type": "thinking", "thinking": ""}]}},
            {"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {"id": "m1", "model": "claude-haiku-4-5",
             "usage": {"output_tokens": 5}, "content": [{"type": "text", "text": "hi"}]}},
            {"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {"id": "m1", "model": "claude-haiku-4-5",
             "usage": {"output_tokens": 5}, "content": [{"type": "tool_use", "id": "a", "name": "Bash", "input": {"command": "ls"}}]}},
            {"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {"id": "m1", "model": "claude-haiku-4-5",
             "usage": {"output_tokens": 5}, "content": [{"type": "tool_use", "id": "b", "name": "Read", "input": {"file_path": "/w/r/x"}}]}},
            {"type": "user", "sessionId": "s", "cwd": "/w/r", "message": {"content": [{"type": "tool_result", "tool_use_id": "a", "content": "x"}]}},
            {"type": "assistant", "sessionId": "s", "cwd": "/w/r", "message": {"id": "m2", "model": "claude-haiku-4-5",
             "usage": {"output_tokens": 7}, "content": [{"type": "text", "text": "done"}]}},
        ]
        p = self.tmp / "g.jsonl"
        p.write_text("\n".join(json.dumps(x) for x in lines))
        s = T.session_from_transcript(p)
        self.assertEqual([v for k, v in s.items if k == "turn"], [[0, 1]])
        self.assertEqual([(r.output, r.calls) for r in s.responses], [(5, (0, 1)), (7, ())])


class Leakage(Tmp):
    def test_assert_clean(self):
        a = self.session(dead_end_calls(), name="a", task="t1")
        b = self.session(dead_end_calls(piped=True), name="b", task="t2")
        assert_clean([a], [b])
        with self.assertRaises(LeakageError):
            assert_clean([a], [a])
        c = self.session(dead_end_calls(), name="c", task="t3")  # a content copy of a under another id and task
        with self.assertRaises(LeakageError):
            assert_clean([c], [a])
        d = self.session(dead_end_calls(piped=True), name="d", task="t2")
        with self.assertRaises(LeakageError):
            assert_clean([d], [b])

    def test_scan_memory_finds_held_out_task_text(self):
        test = self.session(dead_end_calls(), name="x", prompt="Shell completion breaks for groups nested more than one level deep inside other groups")
        train = self.session(dead_end_calls(piped=True), name="y", prompt="Fix the option parser")
        hits = scan_memory(["when completion breaks for groups nested more than one level deep inside, run tests"], [test], [train])
        self.assertEqual(len(hits), 1)
        self.assertEqual(scan_memory(["run the tests with the venv"], [test], [train]), [])


class Stats(unittest.TestCase):
    def test_clustered_constant_and_wilson(self):
        m, ci = ST.clustered([2.0] * 6, ["a", "a", "b", "b", "c", "c"])
        self.assertEqual((m, ci), (2.0, [2.0, 2.0]))
        p, lo, hi = ST.wilson(35, 35, 0.90)
        self.assertAlmostEqual(lo, 0.9284, places=3)  # the baseline table's 0.93
        r, ci = ST.clustered_ratio([1, 1], [10, 10], ["a", "b"])
        self.assertAlmostEqual(r, 0.1)

    def test_splitmix_is_deterministic(self):
        a, b = ST.SplitMix64(), ST.SplitMix64()
        self.assertEqual([a.next() for _ in range(3)], [b.next() for _ in range(3)])


class RealData(unittest.TestCase):
    """Parity with the Rust replay's published numbers (evals/replay/README.md), when the archives exist."""

    @classmethod
    def setUpClass(cls):
        if not (T.V2 / "click" / "state.json").exists():
            raise unittest.SkipTest("RepoBench v2 archive not on disk")
        cls.c = T.load()

    def test_u11_reproduces_the_baseline_table(self):
        from harness.policies.u11 import U11
        hv2 = [s for s in self.c.held_out(model="haiku") if s.source == "v2"]
        self.assertEqual(len(hv2), 82)
        post = score(U11("post-hoc"), hv2).summary()
        pre = score(U11("hindsight"), hv2, hindsight=True).summary()
        self.assertAlmostEqual(post["calls_saved_per_task"], 1.00, places=2)
        self.assertEqual((post["directives"], pre["directives"]), (35, 58))
        self.assertAlmostEqual(pre["s_hat_rust"], 2.105, places=2)
        click = score(U11("post-hoc"), [s for s in hv2 if s.slice == "click"]).summary()
        self.assertAlmostEqual(click["s_hat_rust"], 0.4904, places=4)
        self.assertEqual([round(x, 4) for x in click["s_hat_rust_ci95"]], [0.2046, 0.7806])

    def test_no_discarded_session_is_loaded(self):
        for s in self.c.sessions:
            state = json.loads((Path(s.transcript).parents[3] / "state.json").read_text())
            sid = ((state["runs"].get(s.run) or {}).get("metrics") or {}).get("session_id")
            if sid:
                self.assertEqual(sid, s.sid, s.run)


if __name__ == "__main__":
    unittest.main()
