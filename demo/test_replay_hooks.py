"""Chunk E1, T-E1-03: demo/replay_hooks.py. The compare mode over the fixture recordings always
runs; the replay through a real `adage` binary runs when ADAGE_BIN names one, and is skipped
otherwise.

  ADAGE_BIN=target/release/adage python3 -m unittest test_replay_hooks     (from demo/)
"""

import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from contextlib import redirect_stdout
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import adage_metrics as am  # noqa: E402
import replay_hooks as rh  # noqa: E402

FIX = HERE / "fixtures"
SIM_PROJECTS = HERE.parent / "sim" / "fixtures" / "claude" / "projects"
BIN = os.environ.get("ADAGE_BIN")


class CompareTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)

    def test_fixture_recordings_are_identical(self):
        a, b = rh.load_jsonl(FIX / "recording-adage.jsonl"), rh.load_jsonl(FIX / "recording-mod-observe.jsonl")
        self.assertEqual(len(a), len(rh.load_jsonl(FIX / "hook-stream.jsonl")))
        self.assertEqual(rh.compare(a, b), [])
        with redirect_stdout(io.StringIO()) as out:
            code = rh.main([str(FIX / "recording-adage.jsonl"), "--compare", str(FIX / "recording-mod-observe.jsonl")])
        self.assertEqual((code, out.getvalue().strip()), (0, "identical"))

    def test_one_byte_of_stdout_differs(self):
        b = rh.load_jsonl(FIX / "recording-mod-observe.jsonl")
        i = next(n for n, r in enumerate(b) if "<adage-reflex" in r["stdout"])
        b[i]["stdout"] = b[i]["stdout"].replace("W-2042", "W-2043", 1)
        other = self.tmp / "b.jsonl"
        rh.write_recording(b, other)
        diff = rh.compare(rh.load_jsonl(FIX / "recording-adage.jsonl"), b)
        self.assertTrue(any(line.startswith("+  stdout|") and "W-2043" in line for line in diff), diff)
        with redirect_stdout(io.StringIO()):
            self.assertEqual(rh.main([str(FIX / "recording-adage.jsonl"), "--compare", str(other)]), 1)

    def test_a_spool_record_differs(self):
        b = rh.load_jsonl(FIX / "recording-mod-observe.jsonl")
        b[0]["spool"][0]["arm"] = "control"
        self.assertTrue(rh.compare(rh.load_jsonl(FIX / "recording-adage.jsonl"), b))

    def test_normalise(self):
        home, cwd = "/tmp/x/home", "/tmp/x/cwd"
        v = {"ts": 5, "duration_us": 3, "repo": "/tmp/x/cwd/sub", "p": ["/tmp/x/home/sessions"], "n": {"ts": 1, "k": 2}}
        self.assertEqual(rh.normalise(v, home, cwd), {"repo": "$CWD/sub", "p": ["$ADAGE_HOME/sessions"], "n": {"k": 2}})

    def test_hook_env_is_explicit(self):
        env = rh.hook_env("/h", "treated", True, {"X": "1"}, base={"PATH": "/bin", "ADAGE_ARM": "control", "ADAGE_MOD_OBSERVE": "0"})
        self.assertEqual(env, {"PATH": "/bin", "ADAGE_HOME": "/h", "ADAGE_ARM": "treated", "ADAGE_AUTO_LEARN": "0",
                               "ADAGE_MOD_OBSERVE": "1", "X": "1"})
        self.assertNotIn("ADAGE_MOD_OBSERVE", rh.hook_env("/h", base={"ADAGE_MOD_OBSERVE": "1"}))


@unittest.skipUnless(BIN and Path(BIN).exists(), "set ADAGE_BIN to a built adage binary to replay through it")
class ReplayTest(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.tmp)
        self.cwd = self.tmp / "cwd"
        self.cwd.mkdir()
        self.stream = rh.load_jsonl(FIX / "hook-stream.jsonl")

    def test_replay_records_every_event(self):
        rec = rh.replay(self.stream, BIN, self.tmp / "home", self.cwd)
        self.assertEqual([r["event"] for r in rec], [s["event"] for s in self.stream])
        self.assertTrue(all(r["exit"] == 0 for r in rec), [(r["event"], r["exit"]) for r in rec])
        # Nothing in a recording names the machine's temporary paths.
        self.assertNotIn(str(self.tmp), json.dumps(rec))

    def test_a_seeded_homes_spools_are_not_recorded(self):
        home = self.tmp / "home"
        (home / "sessions").mkdir(parents=True)
        (home / "sessions" / "earlier.spool").write_bytes(am.frame({"kind": "session_start", "earlier": True}))
        rec = rh.replay(self.stream, BIN, home, self.cwd)
        self.assertEqual([r["event"] for r in rec], [s["event"] for s in self.stream])
        self.assertNotIn('"earlier"', json.dumps(rec))

    def test_observe_mode_changes_nothing(self):
        base = rh.replay(self.stream, BIN, self.tmp / "home-a", self.cwd)
        obs = rh.replay(self.stream, BIN, self.tmp / "home-b", self.cwd, observe=True)
        self.assertEqual(rh.compare(base, obs), [])

    @unittest.skipUnless((SIM_PROJECTS).is_dir(), "sim/fixtures/claude/projects not present")
    def test_learned_home_observe_mode_changes_nothing(self):
        # A home learned offline from the simulator's Claude history, and a checkout named like the
        # learned repository: the replay now delivers directives and writes spool records.
        home, user = self.tmp / "learned", self.tmp / "user"
        user.mkdir()
        repo = self.tmp / "acme-billing"
        (repo / ".git").mkdir(parents=True)
        env = dict(os.environ, ADAGE_HOME=str(home), HOME=str(user), ADAGE_AUTO_LEARN="0")
        r = subprocess.run([BIN, "condition", "--demo", "--json", "--claude-projects", str(SIM_PROJECTS)],
                           capture_output=True, text=True, env=env, cwd=self.tmp, timeout=300)
        self.assertEqual(r.returncode, 0, r.stderr)
        recs = []
        for name, observe in (("a", False), ("b", True)):
            h = self.tmp / f"home-{name}"
            shutil.copytree(home, h)
            recs.append(rh.replay(self.stream, BIN, h, repo, observe=observe, extra_env={"HOME": str(user)}))
        base, obs = recs
        self.assertTrue(any("<adage-reflex" in r["stdout"] for r in base), "the learned home delivers a directive")
        spool = [s for r in base for s in r["spool"]]
        m = am.spool_metrics(spool)
        self.assertEqual(m["session_starts"], 1)
        self.assertGreaterEqual(m["directives"], 1)
        self.assertEqual(rh.compare(base, obs), [])

    def test_identity_command(self):
        with redirect_stdout(io.StringIO()) as out:
            code = rh.main([str(FIX / "hook-stream.jsonl"), "--identity", "--adage", BIN, "--keep", str(self.tmp / "keep")])
        self.assertEqual(code, 0, out.getvalue())
        self.assertTrue((self.tmp / "keep" / "mod-observe.jsonl").exists())

    def test_identity_command_takes_a_relative_binary(self):
        # The documented command line names target/release/adage relative to the checkout, and
        # every hook runs in the replay's own directory.
        rel = os.path.relpath(BIN)
        with redirect_stdout(io.StringIO()) as out:
            code = rh.main([str(FIX / "hook-stream.jsonl"), "--identity", "--adage", rel])
        self.assertEqual(code, 0, out.getvalue())


if __name__ == "__main__":
    unittest.main()
