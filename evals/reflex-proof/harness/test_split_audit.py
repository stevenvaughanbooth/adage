"""Training-isolation audit: splits, foreign sessions, corpus, fitted state and markers; offline."""
import copy
import json
from pathlib import Path
import tempfile
import unittest

import split_audit as audit

PROTOCOL = json.loads(audit.PROTOCOL.read_text())


def session(root, name, sid, kind, task, hooks=False):
    d = root / "authority" / name
    d.mkdir(parents=True)
    (d / "transcript-provenance.json").write_text(json.dumps(dict(session_id=sid)))
    (d / "manifest.json").write_text(json.dumps(dict(kind=kind, task=task, training_hooks_enabled=hooks)))
    return d


def learning(root, inputs, corpus=None, why=None, weights=1):
    run = root / "run"
    home = run / "adage-home"
    (home / "cache/sessions").mkdir(parents=True)
    (run / "history").mkdir()
    rows = []
    for sid, task in inputs:
        copy_ = run / "history" / (sid + ".jsonl")
        copy_.write_text(json.dumps({"sessionId": sid, "text": "edit tests"}) + "\n")
        rows.append(dict(session_id=sid, task=task, kind="organic-training", synthetic=False, hooks_enabled=False,
                         copy=str(copy_), sha256=audit.sha(copy_)))
        (home / "cache/sessions" / (sid + ".json")).write_text(json.dumps(dict(id=sid)))
    entries = {r["copy"]: {} for r in rows} if corpus is None else corpus
    (home / "cache/sessions/manifest.json").write_text(json.dumps(dict(entries=entries)))
    track = home / "tracks/repo/learned"
    track.mkdir(parents=True)
    (track / ".why.json").write_text(json.dumps({"run-1": dict(examples=why or [rows[0]["session_id"]])}))
    (home / "tracks/repo/.follow-weights.json").write_text(json.dumps(dict(sessions=weights)))
    (home / "index.json").write_text(json.dumps(dict(routes=[dict(route=dict(id="run-1", evidence=dict(support=1)))])))
    (run / "attempt.json").write_text(json.dumps(dict(inputs=rows)))
    return run


class Splits(unittest.TestCase):
    def test_registered_splits_are_disjoint_and_near_duplicates_share_a_split(self):
        self.assertTrue(audit.validate_splits(PROTOCOL))
        mm = audit.splits(PROTOCOL, "marshmallow")
        self.assertEqual(mm["odd"][0], mm["multiple-of"][0])
        self.assertEqual(mm["no-adjacent-duplicates"][0], mm["unique"][0])
        self.assertEqual(mm["max-utf8-bytes"][0], mm["max-words"][0])
        self.assertEqual(mm["finite"][0], "development")
        self.assertTrue(all(mm[t][0] == "evaluation" for t in ("has-keys", "iso-date", "json-object", "lat-long")))
        self.assertEqual(mm["max-sum"], mm["max-words"])  # r9: withdrawn near-duplicate of size-limit
        click = audit.splits(PROTOCOL, "click")
        self.assertFalse([t for t, (s, _) in click.items() if s == "evaluation" and t in
                          ("hex-color", "port", "duration", "ip-address", "semver", "percentage", "byte-size",
                           "slug", "date", "host-port", "time", "log-level")])

    def test_overlapping_declarations_are_refused(self):
        bad = copy.deepcopy(PROTOCOL)
        bad["splits"]["marshmallow"]["splits"]["evaluation"].append("string-affix")
        with self.assertRaises(ValueError):
            audit.validate_splits(bad)
        bad = copy.deepcopy(PROTOCOL)
        bad["splits"]["marshmallow"]["groups"]["mapping-keys"].append("mm-curve--odd")
        with self.assertRaises(ValueError):
            audit.validate_splits(bad)


class Isolation(unittest.TestCase):
    def check(self, mutate=None, inputs=(("s1", "mm-curve--unique"), ("s2", "mm-curve--ascii")), **kw):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            for sid, task in inputs:
                session(root, "train-" + sid, sid, "organic-training", task)
            session(root, "heldout", "e1", None, "mm-curve--finite")
            run = learning(root, inputs, **kw)
            if mutate:
                mutate(root, run)
            return audit.audit(run, "marshmallow", [root / "authority"])

    def test_clean_training_run_passes_and_records_contributors(self):
        r = self.check()
        self.assertTrue(r["passed"], r["problems"])
        self.assertEqual(r["contributing_sessions"], ["s1", "s2"])
        self.assertEqual(r["forbidden_sessions"], 1)

    def test_every_leak_path_fails_closed(self):
        self.assertFalse(self.check(inputs=(("s1", "mm-curve--finite"),))["passed"])  # development task
        self.assertFalse(self.check(inputs=(("s1", "mm-curve--odd"), ("e1", "mm-curve--unique")))["passed"])  # foreign id
        self.assertFalse(self.check(corpus={"/elsewhere.jsonl": {}})["passed"])
        self.assertFalse(self.check(why=["e1"])["passed"])
        self.assertFalse(self.check(weights=9)["passed"])
        def spools(root, run):
            (run / "adage-home/sessions").mkdir()
        self.assertFalse(self.check(spools)["passed"])
        def marker(root, run):
            (run / "adage-home/tracks/repo/learned/.why.json").write_text(json.dumps({"x": dict(examples=[], note="IsoDate")}))
        self.assertFalse(self.check(marker)["passed"])
        def guided_hooks(root, run):
            (root / "authority/train-s1/manifest.json").write_text(json.dumps(
                dict(kind="organic-training", task="mm-curve--unique", training_hooks_enabled=True)))
        self.assertFalse(self.check(guided_hooks)["passed"])


if __name__ == "__main__":
    unittest.main()
