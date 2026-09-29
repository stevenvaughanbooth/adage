"""Leakage checks.

  assert_clean(train, test)        raises LeakageError if a training session is a held-out session
                                   (same id), on a held-out task, or a content copy of one
  scan_memory(texts, test, train)  distinctive held-out text (8-word shingles of held-out task
                                   prompts that no training session contains) found in a learner's
                                   memory: pass whatever strings your policy stores
  audit(corpus)                    the corpus-level report: split disjointness, sessions outside
                                   their split, v1/v2 copies, discarded reruns, content copies
"""

from __future__ import annotations

import hashlib
import json
import re
from collections import Counter, defaultdict

from .traces import SPLITS, Corpus


class LeakageError(AssertionError):
    pass


def fingerprint(s) -> str:
    """Content hash of a session's tool-call sequence (paths relative to its cwd): copies of one
    session under different ids or roots hash the same."""
    h = hashlib.sha1()
    cwd = s.cwd.rstrip("/")
    for c in s.calls[:60]:
        h.update(c.tool.encode())
        h.update(json.dumps(c.input, sort_keys=True).replace(cwd, ".").encode())
    return h.hexdigest()


def assert_clean(train, test) -> None:
    ids = {s.sid for s in test}
    bad = [s.sid for s in train if s.sid in ids]
    if bad:
        raise LeakageError(f"{len(bad)} held-out sessions in training: {bad[:3]}")
    tasks = {s.task for s in test}
    bad = sorted({s.task for s in train if s.task in tasks})
    if bad:
        raise LeakageError(f"training sessions on held-out tasks: {bad[:5]}")
    fps = {fingerprint(s) for s in test if s.calls}
    bad = [s.run for s in train if s.calls and fingerprint(s) in fps]
    if bad:
        raise LeakageError(f"{len(bad)} training sessions are content copies of held-out ones: {bad[:3]}")


WORD = re.compile(r"[A-Za-z0-9_]+")


def _shingles(text: str, k: int = 8) -> set:
    w = [x.lower() for x in WORD.findall(text or "")]
    return {" ".join(w[i:i + k]) for i in range(max(0, len(w) - k + 1))}


def scan_memory(texts, test, train, k: int = 8) -> list[dict]:
    """Held-out task statements leaking into a learner's memory. Returns one hit per (task, text)."""
    seen = set()
    for s in train:
        seen |= _shingles(s.prompt, k)
    distinct = {}
    for s in test:
        if s.task not in distinct:
            distinct[s.task] = _shingles(s.prompt, k) - seen
    hits = []
    for i, t in enumerate(texts):
        sh = _shingles(t, k)
        for task, d in distinct.items():
            n = len(sh & d)
            if n:
                hits.append({"text_index": i, "task": task, "shingles": n})
    return hits


def audit(corpus: Corpus) -> dict:
    out = {"sessions": len(corpus.sessions)}
    # Split files: train / buffer / test disjoint per repository.
    splits = {}
    for p in sorted(SPLITS.glob("*.json")):
        for repo, sp in json.loads(p.read_text())["repos"].items():
            tr, bu, te = set(sp.get("train", [])), set(sp.get("buffer", [])), set(sp.get("test", []))
            splits[repo] = {"train": len(tr), "buffer": len(bu), "test": len(te),
                            "overlap": sorted((tr & te) | (tr & bu) | (bu & te))}
    out["splits"] = splits
    # Sessions outside their phase's split.
    out["train_phase_not_on_train_tasks"] = sorted({s.run for s in corpus.sessions if s.phase == "train" and s.split != "train"})
    out["eval_phase_not_on_test_tasks"] = sorted({s.run for s in corpus.sessions if s.phase == "eval" and s.split != "test"})
    # v1/v2 copies.
    d = corpus.duplicates
    out["duplicate_copies_dropped"] = len(d)
    out["duplicate_outcome_disagreements"] = [x for x in d if not x["outcome_agrees"] or not x["same_task"]]
    out["duplicates_by_pair"] = dict(Counter(f"{x['kept'].split(':')[0]}->{x['dropped'].split(':')[0]}" for x in d))
    out["excluded_transcripts"] = dict(Counter(e["why"] for e in corpus.excluded))
    # Content copies that survived id de-duplication.
    fp = defaultdict(list)
    for s in corpus.sessions:
        if s.calls:
            fp[fingerprint(s)].append(s.run)
    out["content_copies_with_distinct_ids"] = [v for v in fp.values() if len(v) > 1]
    # Held-out control sessions whose task also has training sessions (should be none).
    held = corpus.held_out(arm=None)
    train_tasks = {s.task for s in corpus.train()}
    out["held_out_tasks_in_training"] = sorted({s.task for s in held if s.task in train_tasks})
    # Held-out prompts verbatim in training prompts.
    tp = {s.prompt.strip() for s in corpus.train()}
    out["held_out_prompts_in_training"] = sorted({s.task for s in held if s.prompt.strip() in tp})
    out["clean"] = not (any(v["overlap"] for v in splits.values()) or out["train_phase_not_on_train_tasks"]
                        or out["eval_phase_not_on_test_tasks"] or out["duplicate_outcome_disagreements"]
                        or out["content_copies_with_distinct_ids"] or out["held_out_tasks_in_training"]
                        or out["held_out_prompts_in_training"])
    return out
