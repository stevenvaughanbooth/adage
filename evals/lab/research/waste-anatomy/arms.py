"""Realistic fractions from what already happened: natural experiments in the other arms, and the
localisation split of the orientation segment.

1. `adage` vs `control` (v2, Haiku, same tasks x 3 repeats): what the current system (U11 plus
   whatever was delivered) removed from each segment. Its share of the env-friction ceiling is the
   realistic fraction for that lever class.
2. `agents-md` vs `control` (v1, Haiku and Sonnet, same tasks): does handing the agent repository
   knowledge up front shrink orientation, and at what carry cost? It is the closest natural
   experiment to "pre-loading" / "knowing from past sessions".
3. Orientation split: exploration before the first needed read of a final-diff file (search, the
   part a pointer can remove) vs unneeded exploration after it (understanding / confidence), and
   the $ value of a perfect pointer (paths + line ranges, ~40 tokens per file, no content).
All differences are task-clustered: the per-task mean difference, bootstrapped over tasks (90%).
"""

from __future__ import annotations

import json
import pickle
import sys
from collections import defaultdict
from pathlib import Path

import numpy as np
from scipy.stats import spearmanr

sys.path.insert(0, str(Path(__file__).resolve().parent))
from ceilings import SEGS, lifetime, reprice, boot_mean, boot_ratio  # noqa: E402

CACHE = Path("/tmp/adage-research/waste-anatomy")
HERE = Path(__file__).resolve().parent
RNG = np.random.default_rng(7)


def seg_calls(s, seg):
    return sum(1 for c in s["calls"] if c["seg"] == seg)


def seg_usd(s, seg):
    return sum(lifetime(c) for c in s["calls"] if c["seg"] == seg)


def paired(A, Bs, f):
    """mean over tasks of mean_f(B) - mean_f(A), with a 90% bootstrap over tasks."""
    ta, tb = defaultdict(list), defaultdict(list)
    for s in A:
        ta[s["task"]].append(f(s))
    for s in Bs:
        tb[s["task"]].append(f(s))
    common = sorted(set(ta) & set(tb))
    d = np.array([np.mean(tb[t]) - np.mean(ta[t]) for t in common])
    base = np.array([np.mean(ta[t]) for t in common])
    idx = RNG.integers(0, len(d), (2000, len(d)))
    m = d[idx].mean(1)
    return {"diff": float(d.mean()), "ci90": [float(np.percentile(m, 5)), float(np.percentile(m, 95))],
            "control_mean": float(base.mean()), "tasks": len(common)}


def compare(A, Bs):
    out = {}
    for seg in SEGS[:-1]:
        out[seg] = {"calls": paired(A, Bs, lambda s, seg=seg: seg_calls(s, seg)),
                    "usd": paired(A, Bs, lambda s, seg=seg: seg_usd(s, seg))}
    out["_total"] = {"calls": paired(A, Bs, lambda s: len(s["calls"])), "usd": paired(A, Bs, lambda s: s["cost_actual"]),
                     "pass_rate": paired(A, Bs, lambda s: float(bool(s["passed"]))),
                     "prefix_tokens": paired(A, Bs, lambda s: s["C"][0])}
    return out


def orientation_split(H):
    pre, post, pre_usd, rho_x, rho_y = [], [], [], [], []
    ptr_rows, base_rows = [], []
    for s in H:
        calls = s["calls"]
        diff_files = {c["file"] for c in calls if c["survive"]}
        first = next((c["idx"] for c in calls if c["name"] == "Read" and c["needed"] and c["file"] in diff_files), None)
        if first is None:
            first = next((c["idx"] for c in calls if c["role"] == "edit"), len(calls))
        pre_c = [c for c in calls if c["role"] == "explore" and c["idx"] < first and not c["needed"]]
        post_c = [c for c in calls if c["role"] == "explore" and c["idx"] > first and not c["needed"]]
        pre.append(len(pre_c))
        post.append(len(post_c))
        pre_usd.append(sum(lifetime(c) for c in pre_c))
        rho_x.append(len([c for c in calls if c["role"] == "explore" and c["idx"] < first]))
        rho_y.append(len(post_c))
        nfiles = len({c["file"] for c in calls if c["needed"] and c["name"] == "Read"})
        base_rows.append(reprice(s))
        ptr_rows.append(reprice(s, remove=frozenset(c["idx"] for c in pre_c), prefix_add=40 * max(nfiles, 1)))
    b, p = np.array(base_rows), np.array(ptr_rows)
    r = spearmanr(rho_x, rho_y)
    return {
        "unneeded_explore_before_localisation": boot_mean(pre),
        "unneeded_explore_after_localisation": boot_mean(post),
        "perfect_pointer_calls_saved": boot_mean(b[:, 0] - p[:, 0]),
        "perfect_pointer_usd_saved_share": boot_ratio(b[:, 2] - p[:, 2], b[:, 2]),
        "spearman_calls_to_localise_vs_later_unneeded_exploration": [float(r.statistic), float(r.pvalue)],
        "n": len(H),
    }


def main():
    S = pickle.load((CACHE / "calls.pkl").open("rb"))
    sel = lambda arm, fam, base: [s for s in S if s["arm"] == arm and s["fam"] == fam and s["base"] == base]
    out = {
        "adage_vs_control_haiku_v2": compare(sel("control", "haiku", "adage-rb2"), sel("adage", "haiku", "adage-rb2")),
        "adage_vs_control_sonnet_v2": compare(sel("control", "sonnet", "adage-rb2"), sel("adage", "sonnet", "adage-rb2")),
        "agentsmd_vs_control_haiku_v1": compare(sel("control", "haiku", "adage-repobench"),
                                                sel("agents-md", "haiku", "adage-repobench")),
        "agentsmd_vs_control_sonnet_v1": compare(sel("control", "sonnet", "adage-repobench"),
                                                 sel("agents-md", "sonnet", "adage-repobench")),
        "orientation_split_haiku_v2": orientation_split(sel("control", "haiku", "adage-rb2")),
        "orientation_split_sonnet_v2": orientation_split(sel("control", "sonnet", "adage-rb2")),
    }
    (HERE / "arms.json").write_text(json.dumps(out, indent=1))
    for k, v in out.items():
        print("==", k)
        if k.startswith("orientation"):
            print(json.dumps(v))
            continue
        for seg, x in v.items():
            print(f"  {seg:14s} " + "  ".join(f"{m} {y['diff']:+.3f}[{y['ci90'][0]:+.3f},{y['ci90'][1]:+.3f}] (ctl {y['control_mean']:.3f})"
                                              for m, y in x.items()))


if __name__ == "__main__":
    main()
