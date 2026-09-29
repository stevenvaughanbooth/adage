#!/usr/bin/env python3
"""Hindsight shortcuts: the protocol.

  OMP_NUM_THREADS=1 nice -n 10 python3 experiment.py [--quick]

1. Label every session in hindsight (`hindsight.label`): achieving calls, waste, shortcut instances
   with their earliest point t0 and removable set D.
2. Per repository, memory = the chronological split's training sessions (7-10). Shortcut instances of
   memory form a library of classes (`shortcuts.Library`); memory decision points are labelled with
   the classes that would have saved calls there.
3. Held-out = every Haiku 4.5 control-arm session (RepoBench v1 + v2; hooks on, directives withheld),
   plus the Sonnet 5 control slices as a supplement. Policies deliver shortcuts; each delivery is
   scored against the held-out session's own hindsight labels (the measurement instrument):
   useful if a matching instance has t* >= i and D ∩ [i, t*) non-empty; calls saved = |∪ D ∩ window|;
   $ and tokens saved under the cumulative-context cost model (`costs.saved`), net of the delivered
   text's own cost.
4. Policies: the learned kNN matcher (θ picked by leave-one-task-out CV on memory, max net calls at
   precision >= 0.6), the frequency prior (deliver the classes most often useful at session start),
   the perfect-matcher ceiling (every held-out instance whose class is in the library, delivered at its
   t0) and the absolute hindsight oracle (every instance, library or not).
5. Learning curve over memory size (2, 5, 10 training sessions; "all" adds the adage and agents-md arms
   of other tasks), leave-one-repo-out (memory = the other four repositories), 90% bootstrap CIs over
   tasks, Wilson intervals for precision.
"""

from __future__ import annotations

import argparse
import json
import math
import random
import time
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

import costs
from hindsight import Idf, label, learn_writers
from load import load, price
from matcher import Embedder, Situations, points, scores
from shortcuts import Library, Vocab, instances

HERE = Path(__file__).resolve().parent
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]
THETAS = [round(x, 2) for x in np.arange(0.10, 0.95, 0.05)]
LAMBDAS = (0.0, 1.0, 2.0)  # assumed cost of a misfired shortcut, in calls (0 = harmless; 2 = the harness default)
MAX_PRIOR = 3


# ---------------------------------------------------------------- memory


class Memory:
    def __init__(self, sessions, labs, idf, emb, writers, tools):
        self.sessions = sessions
        self.by_sid = {s.sid: s for s in sessions}
        every = [x for s in sessions for x in instances(s, labs[s.sid])]
        self.insts = [x for x in every if x.D]  # the library: shortcuts that saved something
        self.vocab = Vocab(sessions)
        self.lib = Library(self.insts, idf, self.vocab)
        self.sits = Situations(sessions, emb, writers, tools)
        by = defaultdict(list)
        for x in self.insts:
            by[x.sid].append(x)
        self.by_inst = defaultdict(list)  # for scoring a memory session in CV: every instance
        for x in every:
            self.by_inst[x.sid].append(x)
        self.labels = []
        for sid, task, i in self.sits.rows:
            lab = {}
            for x in by[sid]:
                if x.anchor >= i:
                    v = sum(1 for d in x.D if i <= d < x.anchor)
                    if v:
                        c = self.lib.cls(x)
                        lab[c] = max(lab.get(c, 0), v)
            self.labels.append(lab)
        self.task_of = {s.sid: s.task for s in sessions}

    def support(self, cid, task):
        return self.lib.support(cid, exclude_task=task)

    def prior(self, task) -> dict:
        """Share of memory sessions (other tasks) for which a class was useful from the start."""
        n, cnt = 0, Counter()
        for r, (sid, t, i) in enumerate(self.sits.rows):
            if t == task or i != 0:
                continue
            n += 1
            for c in self.labels[r]:
                cnt[c] += 1
        return {c: v / n for c, v in cnt.items()} if n else {}


# ---------------------------------------------------------------- scoring one session


def score_session(s, deliveries, mem: Memory, hinst) -> dict:
    """deliveries: [(i, cid)]. Scored against the session's own hindsight instances `hinst`."""
    p = price(s.model)
    scost = costs.session_cost(s)
    per_call = scost / max(len(s.calls), 1)
    saved, issued, useful, redundant, wrong, cost_usd = set(), 0, 0, 0, 0, 0.0
    log = []
    kinds = Counter()
    for i, cid in deliveries:
        issued += 1
        rep = mem.lib.classes[cid].rep
        kinds[rep.fam] += 1
        text = rep.text
        api_now = s.calls[i].batch if i < len(s.calls) else len(s.usage)
        tok = (len(text) + 200) / 4.0
        cost_usd += tok * (p[2] + p[3] * max(len(s.usage) - api_now - 1, 0)) / 1e6
        m = [h for h in hinst if h.anchor >= i and mem.lib.matches(cid, h)]
        if not m:
            wrong += 1
            log.append([i, rep.kind, text[:120], "wrong", []])
            continue
        win = set()
        for h in m:
            win |= {d for d in h.D if i <= d < h.anchor}
        if win:
            useful += 1
            kinds["useful_" + rep.fam] += 1
            saved |= win
            log.append([i, rep.kind, text[:120], "useful", sorted(win), [(h.kind, h.anchor, s.calls[h.anchor].inp_text[:100]) for h in m][:2]])
        else:
            redundant += 1
            log.append([i, rep.kind, text[:120], "redundant", []])
    sv = costs.saved(s, saved)
    return {"sid": s.sid, "task": s.task, "root": s.root, "arm": s.arm, "calls": len(s.calls), "cost": scost,
            "issued": issued, "useful": useful, "redundant": redundant, "wrong": wrong,
            "calls_saved": len(saved), "strict_saved": sum(1 for k in saved if s.calls[k].is_error),
            "cost_calls": cost_usd / per_call if per_call else 0.0,
            "net_calls": len(saved) - (cost_usd / per_call if per_call else 0.0),
            "net_calls_l1": len(saved) - (cost_usd / per_call if per_call else 0.0) - wrong,
            "net_calls_l2": len(saved) - (cost_usd / per_call if per_call else 0.0) - 2 * wrong,
            "usd_saved": sv["usd"] - cost_usd, "tokens_saved": sv["tokens"], "kinds": dict(kinds), "log": log}


def knn_deliveries(sc_list, pts, mem: Memory, task, theta) -> list:
    out, done = [], set()
    for i, sc in zip(pts, sc_list):
        cands = [(v, c) for c, v in sc.items() if v >= theta and c not in done and mem.support(c, task) >= 2]
        if cands:
            v, c = max(cands)
            done.add(c)
            out.append((i, c))
    return out


def prior_deliveries(mem: Memory, task, theta) -> list:
    pr = mem.prior(task)
    c = sorted(((v, c) for c, v in pr.items() if v >= theta and mem.support(c, task) >= 2), reverse=True)
    return [(0, cid) for _, cid in c[:MAX_PRIOR]]


def ceiling(s, mem: Memory, hinst, task) -> dict:
    cls = [c.cid for c in mem.lib.classes if mem.support(c.cid, task) >= 2]
    saved = set()
    for h in hinst:
        if any(mem.lib.matches(c, h) for c in cls):
            saved |= set(h.D)
    sv = costs.saved(s, saved)
    return {"calls_saved": len(saved), "usd_saved": sv["usd"], "tokens_saved": sv["tokens"],
            "calls": len(s.calls), "cost": costs.session_cost(s), "task": s.task, "root": s.root, "arm": s.arm}


def oracle(s, hinst) -> dict:
    saved = set()
    for h in hinst:
        saved |= set(h.D)
    sv = costs.saved(s, saved)
    return {"calls_saved": len(saved), "usd_saved": sv["usd"], "tokens_saved": sv["tokens"],
            "calls": len(s.calls), "cost": costs.session_cost(s), "task": s.task, "root": s.root, "arm": s.arm}


# ---------------------------------------------------------------- statistics


def boot(rows, key, draws=2000, seed=7, ratio_to=None):
    """Mean of rows[key] per session (or Σkey/Σratio_to), 90% bootstrap over task clusters."""
    if not rows:
        return {"mean": None, "lo": None, "hi": None, "n": 0}
    by = defaultdict(list)
    for r in rows:
        by[r["task"]].append(r)
    tasks = list(by)
    rng = np.random.default_rng(seed)

    def stat(rs):
        if ratio_to:
            d = sum(r[ratio_to] for r in rs)
            return sum(r[key] for r in rs) / d if d else 0.0
        return float(np.mean([r[key] for r in rs]))

    point = stat(rows)
    bs = []
    for _ in range(draws):
        pick = rng.integers(0, len(tasks), len(tasks))
        rs = [r for j in pick for r in by[tasks[j]]]
        bs.append(stat(rs))
    lo, hi = np.percentile(bs, [5, 95])
    return {"mean": round(point, 4), "lo": round(float(lo), 4), "hi": round(float(hi), 4), "n": len(rows),
            "tasks": len(tasks)}


def wilson(k, n, z=1.645):
    if n == 0:
        return None
    p = k / n
    d = 1 + z * z / n
    c = p + z * z / (2 * n)
    r = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))
    return {"p": round(p, 3), "lo": round((c - r) / d, 3), "hi": round((c + r) / d, 3), "k": k, "n": n}


def summarise(rows) -> dict:
    if not rows:
        return {}
    out = {"sessions": len(rows)}
    for k in ("net_calls", "net_calls_l1", "net_calls_l2", "calls_saved", "strict_saved", "usd_saved", "tokens_saved", "issued"):
        if k in rows[0]:
            out[k] = boot(rows, k)
    out["pct_calls"] = boot(rows, "calls_saved", ratio_to="calls")
    out["pct_cost"] = boot(rows, "usd_saved", ratio_to="cost")
    if "issued" in rows[0]:
        n = sum(r["issued"] for r in rows)
        u = sum(r["useful"] for r in rows)
        out["precision"] = wilson(u, n)
        out["redundant"] = sum(r["redundant"] for r in rows)
        out["wrong"] = sum(r["wrong"] for r in rows)
        out["per_session"] = round(n / len(rows), 3)
        kinds = Counter()
        for r in rows:
            kinds.update(r["kinds"])
        out["kinds"] = dict(kinds)
    return out


# ---------------------------------------------------------------- one memory setting


class Ctx:
    def __init__(self, quick=False):
        t = time.time()
        S = load()
        self.all = S
        H = [s for s in S if "sonnet" not in s.root]
        self.train = [s for s in H if s.phase == "train"]
        self.idf = Idf(self.train)  # the instrument's token weights (unsupervised)
        self.writers = learn_writers(self.train)
        self.tools = sorted({c.tool for s in self.train for c in s.calls}) + ["<start>"]
        self.emb = Embedder("potion-8M")
        self.labs = {s.sid: label(s, self.idf, self.writers) for s in S}
        self.hinst = {s.sid: instances(s, self.labs[s.sid]) for s in S}
        self.held = [s for s in S if s.phase == "eval" and s.arm in ("control", "adage")]
        self.held_sits = Situations(self.held, self.emb, self.writers, self.tools)
        self.emb.save()
        print(f"setup {time.time() - t:.0f}s: {len(S)} sessions, writers={sorted(self.writers)}", flush=True)

    def memory(self, sessions, labs=None, idf=None):
        return Memory(sessions, labs or self.labs, idf or self.idf, self.emb, self.writers, self.tools)


def tune(mem: Memory) -> dict:
    """Leave-one-task-out on memory: θ for kNN and prior maximising mean(net calls - λ·misfires) for
    each assumed misfire cost λ; abstain (None) when no θ has a positive objective."""
    msc = {}
    for s in mem.sessions:
        msc[s.sid] = scores(mem.sits, s.sid, mem.sits, mem.labels, s.task)
    curves = {}
    for pol in ("knn", "prior"):
        curve = []
        for th in THETAS:
            rows = []
            for s in mem.sessions:
                if pol == "knn":
                    dl = knn_deliveries(msc[s.sid], points(s), mem, s.task, th)
                else:
                    dl = prior_deliveries(mem, s.task, th)
                rows.append(score_session(s, dl, mem, mem.by_inst[s.sid]))
            curve.append((th, float(np.mean([r["net_calls"] for r in rows])),
                          float(np.mean([r["wrong"] for r in rows]))))
        curves[pol] = curve
    best = {}
    for pol, curve in curves.items():
        for lam in LAMBDAS:
            obj = [(net - lam * wr, th) for th, net, wr in curve]
            o, th = max(obj, key=lambda x: (x[0], x[1]))
            best[f"{pol}|{lam}"] = th if o > 0 else None
    return best


def run_setting(ctx: Ctx, mem_for, held, thetas=None, name="") -> dict:
    """mem_for(task) -> Memory (cached by identity). Returns per-policy session rows."""
    rows = defaultdict(list)
    cache = {}
    tuned = {}
    for s in held:
        mem = mem_for(s)
        key = id(mem)
        if key not in tuned:
            tuned[key] = thetas or tune(mem)
            cache[key] = mem
        th = tuned[key]
        hinst = ctx.hinst[s.sid]
        sc = scores(ctx.held_sits, s.sid, mem.sits, mem.labels, s.task)
        pts = points(s)
        for lam in LAMBDAS:
            tk, tp = th.get(f"knn|{lam}"), th.get(f"prior|{lam}")
            rows[f"knn|{lam}"].append(score_session(
                s, knn_deliveries(sc, pts, mem, s.task, tk) if tk is not None else [], mem, hinst))
            rows[f"prior|{lam}"].append(score_session(
                s, prior_deliveries(mem, s.task, tp) if tp is not None else [], mem, hinst))
        # fixed-θ variants, to show the precision/savings trade-off without tuning noise
        for fth in (0.3, 0.5, 0.7):
            rows[f"knn@{fth}"].append(score_session(s, knn_deliveries(sc, pts, mem, s.task, fth), mem, hinst))
        rows["ceiling"].append(ceiling(s, mem, hinst, s.task))
        rows["oracle"].append(oracle(s, hinst))
    ths = [{k: v for k, v in t.items() if not k.endswith("_curve")} for t in tuned.values()]
    return {"rows": rows, "thetas": ths}


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--quick", action="store_true")
    ap.add_argument("--out", default=str(HERE / "results.json"))
    args = ap.parse_args()
    ctx = Ctx(args.quick)
    haiku_held = [s for s in ctx.held if "sonnet" not in s.root and s.arm == "control"]
    sonnet_held = [s for s in ctx.held if "sonnet" in s.root and s.arm == "control"]
    # the adage arm received U11 live: what shortcuts save there is what is left after today's system
    adage_held = [s for s in ctx.held if "sonnet" not in s.root and s.arm == "adage"]
    out = {}

    # ---- main: memory = the repository's training split
    mems = {r: ctx.memory([s for s in ctx.train if s.repo == r]) for r in REPOS}
    res = run_setting(ctx, lambda s: mems[s.repo], haiku_held + sonnet_held + adage_held)
    out["main_thetas"] = res["thetas"]
    groups = {
        "haiku_all": lambda r: "sonnet" not in r["root"] and r["arm"] == "control",
        "haiku_v2": lambda r: r["root"].startswith("v2/") and "sonnet" not in r["root"] and r["arm"] == "control",
        "sonnet": lambda r: "sonnet" in r["root"],
        "adage_arm_v2": lambda r: r["arm"] == "adage" and r["root"].startswith("v2/"),
    }
    for r in REPOS:
        groups[f"repo:{r}"] = (lambda rr: (lambda x: "sonnet" not in x["root"] and x["arm"] == "control"
                                             and x["root"].split("/")[1] == rr))(r)
    out["main"] = {g: {pol: summarise([x for x in rows if f(x)]) for pol, rows in res["rows"].items()}
                   for g, f in groups.items()}
    out["main_rows"] = {pol: rows for pol, rows in res["rows"].items()}
    print(json.dumps({g: {p: (v.get("net_calls") or v.get("calls_saved"), v.get("pct_cost"), v.get("precision"))
                          for p, v in d.items()} for g, d in out["main"].items() if g in ("haiku_all", "haiku_v2")},
                     indent=1), flush=True)

    # ---- leave-one-repo-out: memory = the other four repositories' training sessions
    loro = {}
    for r in REPOS:
        other = [s for s in ctx.train if s.repo != r]
        idf_o = Idf(other)
        labs_o = {s.sid: label(s, idf_o, ctx.writers) for s in other}
        mem = ctx.memory(other, labs=labs_o, idf=idf_o)
        rr = run_setting(ctx, lambda s: mem, [s for s in haiku_held if s.repo == r])
        loro[r] = {pol: summarise(rows) for pol, rows in rr["rows"].items()}
        loro[r]["thetas"] = rr["thetas"]
        print("loro", r, loro[r]["knn|0.0"].get("net_calls"), loro[r]["ceiling"].get("calls_saved"), flush=True)
    out["loro"] = loro

    # ---- learning curve: 2, 5, 10 training sessions; "all" adds the adage / agents-md arms
    curve = defaultdict(dict)
    seeds = [0] if args.quick else [0, 1, 2, 3, 4]
    for n in (2, 5):
        per = defaultdict(list)
        for seed in seeds:
            rng = random.Random(seed)
            mem_n = {}
            for r in REPOS:
                tr = [s for s in ctx.train if s.repo == r]
                mem_n[r] = ctx.memory(rng.sample(tr, min(n, len(tr))))
            rr = run_setting(ctx, lambda s: mem_n[s.repo], haiku_held, thetas=None)
            for pol, rows in rr["rows"].items():
                per[pol].append(rows)
        curve[str(n)] = {pol: _avg_seeds(runs) for pol, runs in per.items()}
        print("curve", n, curve[str(n)]["knn|0.0"].get("net_calls"), curve[str(n)]["ceiling"].get("calls_saved"), flush=True)
    curve["10"] = {pol: summarise([x for x in rows if "sonnet" not in x["root"] and x["arm"] == "control"])
                   for pol, rows in res["rows"].items()}
    extra = [s for s in ctx.all if "sonnet" not in s.root and s.phase == "eval" and s.arm in ("adage", "agents-md")]
    mem_all = {}

    def mem_all_for(s):
        k = (s.repo, s.task)
        if k not in mem_all:
            pool = [x for x in ctx.train if x.repo == s.repo] + \
                   [x for x in extra if x.repo == s.repo and x.task != s.task]
            mem_all[k] = ctx.memory(pool)
        return mem_all[k]

    rr = run_setting(ctx, mem_all_for, haiku_held)
    curve["all"] = {pol: summarise(rows) for pol, rows in rr["rows"].items()}
    curve["all_memory_sessions"] = {f"{k[0]}/{k[1]}": len(m.sessions) for k, m in mem_all.items()}
    print("curve all", curve["all"]["knn|0.0"].get("net_calls"), curve["all"]["ceiling"].get("calls_saved"), flush=True)
    out["curve"] = curve
    ctx.emb.save()
    Path(args.out).write_text(json.dumps(out, indent=1, default=str))
    print("wrote", args.out)


def _avg_seeds(runs):
    """Pool the seeds' session rows (each seed's rows are a full pass over the held-out set), with the
    task-clustered bootstrap over the pooled rows; point estimate = mean over seeds."""
    rows = [x for run in runs for x in run]
    return summarise(rows)


if __name__ == "__main__":
    main()
