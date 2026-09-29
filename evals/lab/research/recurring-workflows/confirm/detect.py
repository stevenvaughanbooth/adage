#!/usr/bin/env python3
"""Automatic family detection, offline ($0): does a new prompt belong to a known recurring family?

  OMP_NUM_THREADS=1 nice -n 10 python3 detect.py [--out detection.json]

Score of a prompt for family F (same repository only): the max cosine similarity between the
prompt's embedding and the prompts of F's prior sessions (potion-base-8M, model2vec: mean of token
vectors, L2-normalised; potion-code-16m-v2 as a secondary). The card fires when the score is at
least tau. tau is chosen on the two TRAINING families (click-type, mm-validator) against their
repositories' negatives (every RepoBench prompt of click and marshmallow + hand-written hard
negatives), then evaluated on the held-out Go family (go-flag) against all 48 urfave-cli RepoBench
prompts + hard negatives, as pre-registered in prereg.json.

Secondary signal: a prompt in the grey zone [tau - 0.10, tau) fires later, once the agent edits one
of the family's card files. Its false fires are measured on the RepoBench transcripts of the three
repositories (harness corpus, every arm), which are not family tasks.
"""

from __future__ import annotations

import argparse
import json
import math
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parents[1]))  # evals/lab/research (harness)
import card_hook  # noqa: E402  (the runtime's embedder and matcher, so offline == live)
import families  # noqa: E402

RB_TASKS = HERE.parents[3] / "repobench" / "tasks"
MODELS = {"potion-8M": Path("/tmp/adage-lab-models/yarflam-potion-base-8m-1.0.4/package/models"),
          "potion-code-16M": Path("/tmp/adage-lab-models/potion-code-16m-v2/package/assets")}
REPO_OF = {"click-type": "click", "mm-validator": "marshmallow", "go-flag": "urfave-cli"}
TRAIN_FAMS = ["click-type", "mm-validator"]
EVAL_FAM = "go-flag"
GREY = 0.10


def wilson(k, n, z=1.645):
    if n == 0:
        return [0.0, 1.0]
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return [round(max(0.0, c - h), 3), round(min(1.0, c + h), 3)]


def prompts_of(fam):
    f = families.FAMILIES[fam]
    return [f["prompt"](families.parse_task(t)[1]) for t in families.order(fam)]


def negatives(repo, extra):
    rb = [t["prompt"] for t in json.loads((RB_TASKS / f"{repo}.json").read_text())["tasks"] if t.get("prompt")]
    return rb, extra["hard_negatives"][repo]


def scores(emb, library, texts):
    if not texts:
        return []
    L = emb.encode(library)
    X = emb.encode(texts)
    return (X @ L.T).max(axis=1).tolist()


def auc(pos, neg):
    """P(a positive outscores a negative): threshold-free separability."""
    if not pos or not neg:
        return None
    return round(sum((p > n) + 0.5 * (p == n) for p in pos for n in neg) / (len(pos) * len(neg)), 3)


def choose_tau(pos, neg):
    mx, mn = max(neg), min(pos)
    return ((mx + mn) / 2 if mn > mx else mx + 0.01), mx, mn


def evaluate(emb, extra, n_lib=families.TRAIN_N):
    out = {"train": {}}
    pos_all, neg_all = [], []
    for fam in TRAIN_FAMS:
        ps = prompts_of(fam)
        lib, held = ps[:n_lib], ps[families.TRAIN_N:]
        rb, hard = negatives(REPO_OF[fam], extra)
        sp = scores(emb, lib, held + extra["paraphrases"][fam])
        sn = scores(emb, lib, rb + hard)
        pos_all += sp
        neg_all += sn
        out["train"][fam] = {"pos_min": round(min(sp), 4), "pos_mean": round(float(np.mean(sp)), 4),
                             "neg_max": round(max(sn), 4), "neg_mean": round(float(np.mean(sn)), 4),
                             "hard_neg_max": round(max(sn[len(rb):]), 4)}
    tau, mx, mn = choose_tau(pos_all, neg_all)
    out["tau"], out["tau_low"] = round(tau, 4), round(tau - GREY, 4)
    out["train_separable"] = mn > mx
    # held-out family
    ps = prompts_of(EVAL_FAM)
    lib, held = ps[:n_lib], ps[families.TRAIN_N:]
    rb, hard = negatives(REPO_OF[EVAL_FAM], extra)
    para = extra["paraphrases"][EVAL_FAM]
    s_t, s_p = scores(emb, lib, held), scores(emb, lib, para)
    s_rb, s_h = scores(emb, lib, rb), scores(emb, lib, hard)
    tp_t, tp_p = sum(s >= tau for s in s_t), sum(s >= tau for s in s_p)
    fp_rb, fp_h = sum(s >= tau for s in s_rb), sum(s >= tau for s in s_h)
    tp, fp = tp_t + tp_p, fp_rb + fp_h
    grey_neg = sum(tau - GREY <= s < tau for s in s_rb + s_h)
    out["eval"] = {
        "family": EVAL_FAM, "library_prompts": n_lib,
        "recall_templated": [tp_t, len(s_t)], "recall_paraphrased": [tp_p, len(s_p)],
        "recall": round(tp / (len(s_t) + len(s_p)), 3), "recall_wilson90": wilson(tp, len(s_t) + len(s_p)),
        "false_fires_repobench": [fp_rb, len(s_rb)], "false_fires_hard": [fp_h, len(s_h)],
        "false_fire_rate": round(fp / (len(s_rb) + len(s_h)), 4), "false_fire_wilson90": wilson(fp, len(s_rb) + len(s_h)),
        "precision": round(tp / (tp + fp), 3) if tp + fp else None,
        "negatives_in_grey_zone": grey_neg,
        "auc_templated": auc(s_t, s_rb + s_h), "auc_paraphrased": auc(s_p, s_rb + s_h),
        "scores": {"templated": [round(x, 3) for x in s_t], "paraphrased": [round(x, 3) for x in s_p],
                   "repobench_max": round(max(s_rb), 3), "hard": [round(x, 3) for x in s_h]},
    }
    return out


def file_signal(emb, tau, cards):
    """Grey-zone file signal on non-family sessions: RepoBench transcripts of the three repositories
    (every arm). A false fire = prompt score in [tau-0.10, tau) AND the session edits a card file."""
    from harness import traces
    corpus = traces.load()
    out = {}
    for fam, repo in REPO_OF.items():
        card = cards.get(fam)
        if not card:
            continue
        lib = prompts_of(fam)[: families.TRAIN_N]
        ss = [s for s in corpus.sessions if s.repo == repo]
        sc = scores(emb, lib, [s.prompt for s in ss])
        grey = fired = touched = 0
        for s, x in zip(ss, sc):
            edits = [card_hook.rel((c.input or {}).get("file_path", ""), s.cwd) for c in s.calls
                     if c.tool in ("Edit", "Write", "MultiEdit") and not c.is_error]
            hit = any(card_hook.file_matches(p, card["files"], card["patterns"]) for p in edits)
            touched += hit
            if tau - GREY <= x < tau:
                grey += 1
                fired += hit
        out[fam] = {"sessions": len(ss), "prompt_fires": sum(x >= tau for x in sc), "in_grey_zone": grey,
                    "file_signal_fires": fired, "sessions_editing_a_card_file": touched,
                    "false_fire_wilson90": wilson(sum(x >= tau for x in sc) + fired, len(ss))}
    return out


# ---------------------------------------------------------------- post-hoc variant (labelled as such)
SPLITS = HERE.parents[3] / "repobench" / "splits"


class Weighted:
    """POST-HOC (chosen after the pre-registered detector missed every paraphrase): per repository,
    token weights learned from the traces' prompts, w_t = max(0, log-odds that a prompt containing
    token t is a family prompt rather than one of the repository's other prior prompts) (document
    frequencies, add-0.5 smoothing); a prompt's vector is the w-weighted mean of its token vectors;
    the score is the cosine to the family centroid. No token list is written by hand."""

    def __init__(self, emb, fam_prompts, other_prompts, alpha=0.5):
        self.emb = emb
        tok = emb.tok
        def docs(ps):
            return [set(e.ids) for e in tok.encode_batch(ps, add_special_tokens=False)]
        F, O = docs(fam_prompts), docs(other_prompts)
        vocab = set().union(*F) if F else set()
        self.w = {}
        for t in vocab:
            cf = sum(t in d for d in F)
            co = sum(t in d for d in O)
            lo = math.log((cf + alpha) / (len(F) + 2 * alpha)) - math.log((co + alpha) / (len(O) + 2 * alpha))
            if lo > 0:
                self.w[t] = lo
        self.c = self.vec(fam_prompts).mean(0)
        self.c /= max(np.linalg.norm(self.c), 1e-9)

    def vec(self, texts):
        out = np.zeros((len(texts), self.emb.E.shape[1]), np.float32)
        for i, e in enumerate(self.emb.tok.encode_batch(list(texts), add_special_tokens=False)):
            ids = [j for j in e.ids if j in self.w and j < len(self.emb.E)]
            if ids:
                ws = np.array([self.w[j] for j in ids], np.float32)
                out[i] = (self.emb.E[ids] * ws[:, None]).sum(0) / ws.sum()
        n = np.linalg.norm(out, axis=1, keepdims=True)
        return out / np.maximum(n, 1e-9)

    def score(self, texts):
        return (self.vec(texts) @ self.c).tolist() if texts else []


def split_prompts(repo):
    """The repository's other prior sessions: RepoBench training-split prompts (the library);
    everything else of the repository is a negative to evaluate on."""
    sp = json.loads((SPLITS / f"{repo}.json").read_text())["repos"][repo]
    tasks = {t["id"]: t.get("prompt") for t in json.loads((RB_TASKS / f"{repo}.json").read_text())["tasks"]}
    train = [tasks[t] for t in sp["train"] if tasks.get(t)]
    rest = [p for t, p in tasks.items() if p and t not in sp["train"]]
    return train, rest


def evaluate_weighted(emb, extra, n_lib=families.TRAIN_N):
    out = {"method": "POST-HOC contrastive token weighting (see Weighted)", "train": {}}
    pos_all, neg_all = [], []
    for fam in TRAIN_FAMS:
        ps = prompts_of(fam)
        other, rest = split_prompts(REPO_OF[fam])
        m = Weighted(emb, ps[:n_lib], other)
        sp = m.score(ps[families.TRAIN_N:] + extra["paraphrases"][fam])
        sn = m.score(rest + extra["hard_negatives"][REPO_OF[fam]])
        pos_all += sp
        neg_all += sn
        out["train"][fam] = {"pos_min": round(min(sp), 4), "neg_max": round(max(sn), 4)}
    tau, mx, mn = choose_tau(pos_all, neg_all)
    out["tau"], out["train_separable"] = round(tau, 4), mn > mx
    ps = prompts_of(EVAL_FAM)
    other, rest = split_prompts(REPO_OF[EVAL_FAM])
    m = Weighted(emb, ps[:n_lib], other)
    s_t, s_p = m.score(ps[families.TRAIN_N:]), m.score(extra["paraphrases"][EVAL_FAM])
    s_n, s_h = m.score(rest), m.score(extra["hard_negatives"][REPO_OF[EVAL_FAM]])
    tp = sum(s >= tau for s in s_t + s_p)
    fp = sum(s >= tau for s in s_n + s_h)
    # per-family threshold from the repository's own history: max score of its other prior prompts + 0.02
    ts = max(m.score(other)) + 0.02
    out["self_calibrated"] = {"tau": round(ts, 4),
                              "recall_templated": [sum(s >= ts for s in s_t), len(s_t)],
                              "recall_paraphrased": [sum(s >= ts for s in s_p), len(s_p)],
                              "false_fires": [sum(s >= ts for s in s_n + s_h), len(s_n) + len(s_h)],
                              "false_fire_wilson90": wilson(sum(s >= ts for s in s_n + s_h), len(s_n) + len(s_h))}
    out["eval"] = {"library_prompts": n_lib, "other_prior_prompts": len(other),
                   "recall_templated": [sum(s >= tau for s in s_t), len(s_t)],
                   "recall_paraphrased": [sum(s >= tau for s in s_p), len(s_p)],
                   "recall_wilson90": wilson(tp, len(s_t) + len(s_p)),
                   "false_fires_repobench": [sum(s >= tau for s in s_n), len(s_n)],
                   "false_fires_hard": [sum(s >= tau for s in s_h), len(s_h)],
                   "false_fire_wilson90": wilson(fp, len(s_n) + len(s_h)),
                   "precision": round(tp / (tp + fp), 3) if tp + fp else None,
                   "auc_templated": auc(s_t, s_n + s_h), "auc_paraphrased": auc(s_p, s_n + s_h),
                   "scores": {"templated": [round(x, 3) for x in s_t], "paraphrased": [round(x, 3) for x in s_p],
                              "neg_max": round(max(s_n), 3), "hard": [round(x, 3) for x in s_h]}}
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(HERE / "detection.json"))
    ap.add_argument("--cards", default="/tmp/adage-research/recurring-workflows/confirm/hookenv/index.json")
    a = ap.parse_args(argv)
    extra = json.loads((HERE / "prompts_extra.json").read_text())
    res = {}
    for name, d in MODELS.items():
        emb = card_hook.Static(d)
        res[name] = {"n4": evaluate(emb, extra), "n1": evaluate(emb, extra, n_lib=1),
                     "posthoc_weighted_n4": evaluate_weighted(emb, extra),
                     "posthoc_weighted_n1": evaluate_weighted(emb, extra, n_lib=1)}
    emb = card_hook.Static(MODELS["potion-8M"])
    idx = Path(a.cards)
    if idx.exists():
        cards = {f["name"]: f for r in json.loads(idx.read_text())["repos"].values() for f in r["families"]}
        res["file_signal_potion-8M"] = file_signal(emb, res["potion-8M"]["n4"]["tau"], cards)
    Path(a.out).write_text(json.dumps(res, indent=1))
    for name in MODELS:
        e = res[name]["n4"]
        print(name, "tau", e["tau"], "separable", e["train_separable"], json.dumps(e["train"]))
        print("  eval n4", {k: v for k, v in e["eval"].items() if k != "scores"})
        print("  eval n1", {k: v for k, v in res[name]["n1"]["eval"].items() if k != "scores"}, "tau", res[name]["n1"]["tau"])
        for k in ("posthoc_weighted_n4", "posthoc_weighted_n1"):
            w = res[name][k]
            print("  ", k, "tau", w["tau"], w["train"], {x: y for x, y in w["eval"].items()})
    if "file_signal_potion-8M" in res:
        print("file signal", json.dumps(res["file_signal_potion-8M"]))
    return 0


if __name__ == "__main__":
    sys.exit(main())
