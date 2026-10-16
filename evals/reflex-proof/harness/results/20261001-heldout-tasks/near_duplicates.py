"""Offline: predicate and signature similarity of each authored task to every earlier mm-curve task.

A screening aid for the near-duplicate grouping, calibrated on the registered near-duplicate pairs
(odd/multiple-of, no-adjacent-duplicates/unique, max-utf8-bytes/max-words). Text similarity is not
the grouping rule: concept and interface are judged too (see README.md). Prints JSON.
"""
import difflib
import json
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
import curve_tasks  # noqa: E402
import heldout_tasks  # noqa: E402
import split_audit  # noqa: E402

EARLIER = {v["id"]: v for v in curve_tasks.VARIANTS}
SPLITS = split_audit.splits(json.loads(split_audit.PROTOCOL.read_text()), "marshmallow")


def sim(a, b):
    return round(difflib.SequenceMatcher(None, a, b).ratio(), 2)


def interface(v):
    return v["sig"].split("(", 1)[1]


registered = {"%s/%s" % pair: sim(EARLIER[pair[0]]["check"], EARLIER[pair[1]]["check"])
              for pair in (("odd", "multiple-of"), ("no-adjacent-duplicates", "unique"), ("max-utf8-bytes", "max-words"))}
authored = {}
for v in heldout_tasks.VARIANTS:
    ranked = sorted(((sim(v["check"], e["check"]), vid) for vid, e in EARLIER.items()), reverse=True)[:3]
    authored[v["id"]] = [dict(task=vid, split=SPLITS[vid][0], group=SPLITS[vid][1], check_similarity=s,
                              same_interface=interface(v) == interface(EARLIER[vid])) for s, vid in ranked]
print(json.dumps(dict(registered_near_duplicate_pairs=registered, nearest_earlier_tasks=authored), indent=2))
