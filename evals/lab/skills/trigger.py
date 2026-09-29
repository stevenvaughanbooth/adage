#!/usr/bin/env python3
"""The compiled skill's trigger, measured offline: does a prompt call for this skill?

  python3 trigger.py [--json]

Three triggers, cheapest first, on real prompts (no model calls):

- span: the prompt holds a value of the skill's prompt-bound slot type (a work-item id); Adage's
  `prompt` trigger with `span: ticket`.
- lexical: cosine similarity of TF-IDF vectors (words and character 4-grams) between the prompt
  and the skill's profile: its description, plus the prompts of the exploration sessions that
  aligned to it. A stand-in for Adage's local static embedding (potion-base-8M, `intent` triggers):
  the same shape (a centroid and a threshold), with no model file.
- repo scope: the skill applies to the repository it was compiled for (pack applicability `repo`).

Positives are the held-out prompts of the skill's own repository; negatives are the prompts of the
other repositories' tasks (RepoBench: attrs, marshmallow, urfave-cli, vue-core; the acme prompts).
The threshold is chosen on the exploration prompts only (the lowest exploration similarity).
"""

from __future__ import annotations

import json
import math
import re
import sys
from collections import Counter
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def feats(text: str) -> Counter:
    t = text.lower()
    words = re.findall(r"[a-z_][a-z0-9_]+", t)
    grams = [t[i:i + 4] for i in range(len(t) - 3) if not t[i:i + 4].isspace()]
    return Counter(words + grams)


def tfidf(docs: list[Counter]):
    df = Counter()
    for d in docs:
        df.update(set(d))
    n = len(docs)
    return lambda c: {k: v * math.log((1 + n) / (1 + df[k])) for k, v in c.items()}


def cos(a: dict, b: dict) -> float:
    num = sum(v * b.get(k, 0.0) for k, v in a.items())
    da = math.sqrt(sum(v * v for v in a.values()))
    db = math.sqrt(sum(v * v for v in b.values()))
    return num / (da * db) if da and db else 0.0


def auc(pos: list[float], neg: list[float]) -> float:
    wins = sum((p > n) + 0.5 * (p == n) for p in pos for n in neg)
    return wins / (len(pos) * len(neg)) if pos and neg else float("nan")


def repobench_prompts(repo: str) -> list[str]:
    d = json.loads((REPO / f"evals/repobench/tasks/{repo}.json").read_text())
    return [t["prompt"] for t in d["tasks"] if t.get("prompt")]


def acme_prompts(ids):
    tpl = ["Pick up {id} and get it done.", "Please work on {id}.", "Can you take care of {id}?"]
    return [tpl[i % 3].format(id=x) for i, x in enumerate(ids)]


def evaluate(name, desc, explore, positives, negatives, span=None):
    corpus = [feats(x) for x in [desc, *explore, *positives, *negatives]]
    w = tfidf(corpus)
    prof = Counter()
    for x in [desc, *explore]:
        prof.update(feats(x))
    centroid = w(prof)
    score = lambda x: cos(w(feats(x)), centroid)
    # Leave-one-out on the exploration prompts: each scored against a centroid built without it.
    loo = []
    for i, x in enumerate(explore):
        pr = Counter()
        for y in [desc, *explore[:i], *explore[i + 1:]]:
            pr.update(feats(y))
        loo.append(cos(w(feats(x)), w(pr)))
    thr = min(loo) * 0.9 if loo else 0.1
    ps, ns = [score(x) for x in positives], [score(x) for x in negatives]
    res = {"skill": name, "positives": len(ps), "negatives": len(ns), "threshold": round(thr, 3),
           "lexical_auc": round(auc(ps, ns), 3),
           "lexical_recall": round(sum(p >= thr for p in ps) / len(ps), 3),
           "lexical_false_positive_rate": round(sum(n >= thr for n in ns) / len(ns), 3)}
    if span:
        res["span_recall"] = round(sum(bool(re.search(span, x)) for x in positives) / len(positives), 3)
        res["span_false_positive_rate"] = round(sum(bool(re.search(span, x)) for x in negatives) / len(negatives), 3)
    return res


def main(argv):
    others = {r: repobench_prompts(r) for r in ("attrs", "marshmallow", "urfave-cli", "vue-core")}
    click_cfg = json.loads((HERE / "configs/click.json").read_text())
    click_all = click_cfg["task_prompts"]
    click_explore = [click_all[t] for t in click_cfg["train"]["tasks"]]
    click_held = [p for t, p in click_all.items() if t not in click_cfg["train"]["tasks"]]
    acme_explore = acme_prompts([f"W-104{i}" for i in range(1, 7)])
    acme_held = acme_prompts([f"W-204{i}" for i in range(1, 7)])
    neg_click = [p for v in others.values() for p in v] + acme_held
    neg_acme = [p for v in others.values() for p in v] + click_held
    desc = lambda p: re.search(r"description: (.*)", (HERE / p).read_text()).group(1)
    out = [
        evaluate("acme-work-item", desc("skills/acme-work-item/SKILL.md"), acme_explore, acme_held, neg_acme,
                 span=r"\b[A-Z]{1,5}-\d{3,9}\b"),
        evaluate("click-bugfix", desc("skills/click-bugfix/SKILL.md"), click_explore, click_held, neg_click),
        evaluate("click-bugfix (description only)", desc("skills/click-bugfix/SKILL.md"), [], click_held, neg_click),
    ]
    if "--json" in argv:
        print(json.dumps(out, indent=1))
    else:
        for r in out:
            print(json.dumps(r))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
