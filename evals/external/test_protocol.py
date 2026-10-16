import json
import random
import tempfile
import unittest
from pathlib import Path

import protocol as p

HERE = Path(__file__).resolve().parent
FIX = HERE / "fixtures"


def tasks(repo, n, day0=1):
    return [p.TaskMeta(task_id=f"{repo.split('/')[1]}-{i}", repo=repo, base_commit=f"{i:040x}") for i in range(n)]


def dates(offsets):
    """date_of(repo, sha) from {sha-int: day}; unknown shas get the sha's int as the day."""
    def f(repo, sha):
        d = offsets.get(int(sha, 16), int(sha, 16))
        return f"2024-01-{1 + d // 24:02d}T{d % 24:02d}:00:00+00:00"
    return f


class SplitTest(unittest.TestCase):
    def test_chronological_with_buffer(self):
        ts = tasks("o/a", 20)
        s = p.chronological_split(ts, dates({}), train_frac=0.5, buffer=3)
        r = s["repos"]["o/a"]
        self.assertEqual(r["train"], [f"a-{i}" for i in range(10)])
        self.assertEqual(r["buffer"], ["a-10", "a-11", "a-12"])
        self.assertEqual(r["test"], [f"a-{i}" for i in range(13, 20)])
        self.assertLess(r["cutoff"], r["first_test"])

    def test_order_follows_dates_not_input(self):
        ts = tasks("o/a", 12)
        # Reverse the chronology: task 11 is the oldest.
        off = {i: 100 - i for i in range(12)}
        s = p.chronological_split(ts, dates(off), train_frac=0.5, buffer=2, min_test=2)
        self.assertEqual(s["repos"]["o/a"]["train"], [f"a-{i}" for i in range(11, 5, -1)])

    def test_deterministic_under_shuffle_and_ties(self):
        ts = tasks("o/a", 15) + tasks("o/b", 15)
        same_day = dates({i: 5 for i in range(15)})  # all tied: base commit then task id decide
        base = p.chronological_split(ts, same_day, buffer=2)
        for seed in range(5):
            sh = list(ts)
            random.Random(seed).shuffle(sh)
            again = p.chronological_split(sh, same_day, buffer=2)
            self.assertEqual(base["repos"], again["repos"])
            self.assertEqual(p.split_digest(base), p.split_digest(again))

    def test_timezones_compare_in_utc(self):
        ts = tasks("o/a", 2)
        d = {0: "2024-01-01T10:00:00+02:00", 1: "2024-01-01T09:30:00+00:00"}  # 08:00Z vs 09:30Z
        s = p.chronological_split(ts, lambda r, sha: d[int(sha, 16)], train_frac=0.5, buffer=0, min_test=1)
        self.assertEqual(s["repos"]["o/a"]["train"], ["a-0"])

    def test_small_repos_excluded_and_unassigned_listed(self):
        ts = tasks("o/a", 6) + [p.TaskMeta("x", None, None)]
        s = p.chronological_split(ts, dates({}), buffer=3, min_test=5)
        self.assertIn("o/a", s["excluded"])
        self.assertEqual(s["unassigned"], ["x"])

    def test_digest_changes_with_the_split(self):
        ts = tasks("o/a", 20)
        a = p.chronological_split(ts, dates({}), buffer=3)
        b = p.chronological_split(ts, dates({}), buffer=2)
        self.assertNotEqual(p.split_digest(a), p.split_digest(b))

    def test_leave_repo_out_is_a_derangement(self):
        m = p.leave_repo_out(["c", "a", "b", "d"], seed=1)
        self.assertEqual(sorted(m), ["a", "b", "c", "d"])
        self.assertTrue(all(k != v for k, v in m.items()))
        self.assertEqual(sorted(m.values()), ["a", "b", "c", "d"])

    def test_commit_dates_cache(self):
        calls = []

        class R:
            def __init__(self, out, rc=0):
                self.stdout, self.returncode = out, rc

        def run(cmd, **kw):
            calls.append(cmd)
            return R("2024-05-01T12:00:00+00:00\n")

        with tempfile.TemporaryDirectory() as d:
            (Path(d) / "clones" / "o__a.git").mkdir(parents=True)
            cd = p.CommitDates(Path(d) / "clones", Path(d) / "dates.json", run=run)
            self.assertEqual(cd("o/a", "abc"), "2024-05-01T12:00:00+00:00")
            self.assertEqual(calls[-1][-4:], ["show", "-s", "--format=%cI", "abc"])
            n = len(calls)
            again = p.CommitDates(Path(d) / "clones", Path(d) / "dates.json", run=run)
            self.assertEqual(again("o/a", "abc"), "2024-05-01T12:00:00+00:00")
            self.assertEqual(len(calls), n, "second lookup served from the JSON cache")


class MetaTest(unittest.TestCase):
    def test_load_task_meta_from_fixture(self):
        d = next((FIX / "tasks").iterdir())
        m = p.load_task_meta(d)
        self.assertEqual(m.repo, "ansible/ansible")
        self.assertEqual(m.gold_files, ["lib/ansible/modules/secret_mod.py"])
        self.assertEqual(m.test_files, ["test/units/modules/test_secret_mod.py"])
        self.assertEqual(m.test_ids, ["test/units/modules/test_secret_mod.py::test_rotates"])

    def test_upper_case_keys_and_lists(self):
        m = p.meta_from_record("t", {"repo": "o/a", "base_commit": "c", "FAIL_TO_PASS": ["t1"], "PASS_TO_PASS": "[\"t2\"]"})
        self.assertEqual(m.test_ids, ["t1", "t2"])
        # swebenchpro's tests/config.json stores Python-literal lists (checked at harbor-datasets c8e8f3f).
        m = p.meta_from_record("t", {"fail_to_pass": "['test/a.py::T::test_x']", "pass_to_pass": "[]"})
        self.assertEqual(m.test_ids, ["test/a.py::T::test_x"])

    def test_patch_files_renames_and_new_files(self):
        diff = ("diff --git a/x/old.py b/x/new.py\nrename from x/old.py\nrename to x/new.py\n"
                "diff --git a/y.py b/y.py\nnew file mode 100644\n--- /dev/null\n+++ b/y.py\n")
        self.assertEqual(p.patch_files(diff), ["x/old.py", "x/new.py", "y.py"])

    def test_repo_from_task_name(self):
        self.assertEqual(p.repo_from_task_name("instance_future-architect__vuls-" + "a" * 40 + "-v1"),
                         "future-architect/vuls")
        self.assertEqual(p.repo_from_task_name("scikit-learn__scikit-learn-10297"), "scikit-learn/scikit-learn")
        self.assertEqual(p.repo_from_task_name("oauthlib__oauthlib.1fd52536.combine_file__x"), "oauthlib/oauthlib")
        self.assertIsNone(p.repo_from_task_name("adaptive-rejection-sampler"))


class LeakageTest(unittest.TestCase):
    def setUp(self):
        self.test_task = p.TaskMeta("t9", "o/a", "c", gold_files=["pkg/secret_mod.py", "pkg/common.py"],
                                    test_files=["tests/test_secret_mod.py"],
                                    test_ids=["tests/test_secret_mod.py::test_rotates"])
        self.train = ['{"command": "pytest tests/test_common.py && cat pkg/common.py"}']

    def home(self, d, route_text):
        route = {"repo": "adage-eval:x/o__a", "track": "t", "version": "1",
                 "route": {"id": "r1", "summary": route_text, "trigger": {"on": "session_start"},
                           "step": {"do": "run", "command": route_text}}}
        Path(d, "index.json").write_text(json.dumps({"generation": "g", "routes": [route]}))
        (Path(d) / "tracks" / "t" / "routes").mkdir(parents=True)
        Path(d, "tracks", "t", "routes", "r1.yaml").write_text(f"summary: {route_text}\n")

    def test_flags_test_only_paths_and_ids(self):
        with tempfile.TemporaryDirectory() as d:
            self.home(d, "pytest tests/test_secret_mod.py::test_rotates")
            rep = p.check_leakage(p.artifact_texts(d), [self.test_task], self.train)
        self.assertFalse(rep["ok"])
        kinds = {(f["kind"], f["where"].split(":")[0]) for f in rep["findings"]}
        self.assertIn(("test_id", "index.json"), kinds)
        self.assertIn(("test_file", "tracks/t/routes/r1.yaml"), kinds)

    def test_items_seen_in_training_are_allowed(self):
        with tempfile.TemporaryDirectory() as d:
            self.home(d, "cat pkg/common.py before editing")
            rep = p.check_leakage(p.artifact_texts(d), [self.test_task], self.train)
        self.assertTrue(rep["ok"], rep)
        items = {i["item"] for i in p.leakage_items([self.test_task], self.train)}
        self.assertNotIn("pkg/common.py", items)
        self.assertIn("pkg/secret_mod.py", items)

    def test_whole_token_matching(self):
        self.assertFalse(p._mentions("pkg/secret_mod.pyc", "pkg/secret_mod.py"))
        self.assertFalse(p._mentions("mypkg/secret_mod.py", "pkg/secret_mod.py"))
        self.assertTrue(p._mentions("run `pkg/secret_mod.py`", "pkg/secret_mod.py"))
        self.assertTrue(p._mentions('"src\\/pkg\\/secret_mod.py"', "pkg/secret_mod.py"))


class PatchTest(unittest.TestCase):
    def test_clean_and_dirty(self):
        clean = "diff --git a/src/x.py b/src/x.py\n--- a/src/x.py\n+++ b/src/x.py\n"
        self.assertTrue(p.check_patch_clean(clean, " M src/x.py\n")["ok"])
        for bad in (".claude/settings.local.json", ".adage/disabled", "sub/.codex/hooks.json",
                    ".opencode/plugins/adage.js"):
            diff = f"diff --git a/{bad} b/{bad}\nnew file mode 100644\n--- /dev/null\n+++ b/{bad}\n"
            rep = p.check_patch_clean(diff)
            self.assertEqual(rep["violations"], [bad])
        self.assertEqual(p.check_patch_clean(None, "?? .adage/x\nR  a.py -> .claude/a.py\n")["violations"],
                         [".adage/x", ".claude/a.py"])
        self.assertEqual(p.check_patch_clean(None, None, ".codex/hooks.json\n")["violations"], [".codex/hooks.json"])
        # Lookalikes are fine.
        self.assertTrue(p.check_patch_clean(None, "?? docs/claude/notes.md\n?? my.adage/x\n")["ok"])


if __name__ == "__main__":
    unittest.main()
