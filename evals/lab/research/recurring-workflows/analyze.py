#!/usr/bin/env python3
"""Analysis of the recurring-workflow study (pre-registered in prereg.json). $0, offline.

  OMP_NUM_THREADS=1 nice -n 10 python3 analyze.py [--out analysis.json]

1. live: held-out arms vs control, paired by (variant, repeat): $ / calls / turns ratio - 1 with a
   90% percentile bootstrap clustered by variant; pass rates (Wilson) and paired differences.
2. decomposition: each control/train session's calls, split into the family's repeated procedure
   (an abstract action that occurs in >= half of the family's OTHER sessions) and the novel part;
   and by role (discovery: reads/searches/listings, edit, run, failed); shares of calls and of $
   under the harness's cumulative-context cost model.
3. ceilings: oracle "perfect procedure" (keep only one read of each file finally edited, the
   successful edits, and the final successful command), oracle "hook-executed" (also drop the
   edits of templatable files: files edited in every session whose edit text, variant names
   masked, is >= 0.6 similar across sessions), and a realistic checklist (searches/listings before
   the last card file's first touch, plus failed commands the card says to avoid).
4. learning curve: the realistic removable $ share, card-file recall/precision vs the number of
   prior sessions n = 1..9 (random prior sets of other variants), plus the live n = 0 / n = 4 points.
"""

from __future__ import annotations

import argparse
import difflib
import json
import math
import random
import re
import sys
from pathlib import Path

import numpy as np

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))
from harness import costmodel as CM  # noqa: E402
from harness import traces  # noqa: E402

import families  # noqa: E402
import procedure  # noqa: E402

RUNS = Path("/tmp/adage-research/recurring-workflows/runs")
SEARCH_CMDS = ("find", "grep", "rg", "ls", "tree")  # instrument (as harness.oracle's search class)
EDIT_TOOLS = procedure.EDIT_TOOLS
RNG = 20260929


# ------------------------------------------------------------------------------------ loading

def load(root_names=None) -> list:
    out = []
    for st in sorted(RUNS.glob("*/state.json")):
        if root_names and st.parent.name not in root_names:
            continue
        state = json.loads(st.read_text())
        for rid, r in state["runs"].items():
            m = r.get("metrics") or {}
            sid = m.get("session_id")
            if not sid or m.get("status") in (None, "infra_error", "running"):
                continue
            hits = list((st.parent / "private" / "transcripts").glob(f"*/{sid}.jsonl"))
            if not hits:
                continue
            fam = r["task"].split("--")[0]
            s = traces.session_from_transcript(hits[0], repo=fam, task=r["task"], arm=r["arm"], phase=r["phase"],
                                               passed=bool(m.get("passed")), cost_usd=m.get("cost_usd"),
                                               run=rid, slice=st.parent.name)
            if s is None:
                continue
            s.rep = int(re.search(r"-r(\d+)", rid).group(1)) if re.search(r"-r(\d+)", rid) else 1
            s.metrics = m
            s.score = r.get("score") or {}
            s.root = st.parent.name
            out.append(s)
    return out


# ------------------------------------------------------------------------------------ statistics

def boot_ratio(pairs, level=0.90, n=4000, seed=RNG):
    """pairs: [(cluster, arm_value, control_value)]; ratio of sums - 1, cluster bootstrap."""
    if not pairs:
        return None
    cl = sorted({p[0] for p in pairs})
    by = {c: [(a, b) for k, a, b in pairs if k == c] for c in cl}
    a = sum(p[1] for p in pairs)
    b = sum(p[2] for p in pairs)
    point = a / b - 1 if b else float("nan")
    rng = np.random.default_rng(seed)
    vals = []
    for _ in range(n):
        pick = rng.integers(0, len(cl), len(cl))
        sa = sum(x for i in pick for x, _ in by[cl[i]])
        sb = sum(y for i in pick for _, y in by[cl[i]])
        vals.append(sa / sb - 1 if sb else 0.0)
    lo, hi = (float(x) for x in np.percentile(vals, [(1 - level) / 2 * 100, (1 + level) / 2 * 100]))
    return {"ratio_minus_1": round(point, 4), "ci90": [round(lo, 4), round(hi, 4)], "pairs": len(pairs),
            "clusters": len(cl), "arm_mean": round(a / len(pairs), 4), "control_mean": round(b / len(pairs), 4)}


def boot_mean(values, clusters, level=0.90, n=4000, seed=RNG):
    cl = sorted(set(clusters))
    by = {c: [v for v, k in zip(values, clusters) if k == c] for c in cl}
    rng = np.random.default_rng(seed)
    vals = []
    for _ in range(n):
        pick = rng.integers(0, len(cl), len(cl))
        xs = [v for i in pick for v in by[cl[i]]]
        vals.append(sum(xs) / len(xs))
    lo, hi = (float(x) for x in np.percentile(vals, [(1 - level) / 2 * 100, (1 + level) / 2 * 100]))
    return {"mean": round(float(np.mean(values)), 4), "ci90": [round(lo, 4), round(hi, 4)], "n": len(values),
            "clusters": len(cl)}


def wilson(k, n, z=1.645):
    if n == 0:
        return [0.0, 0.0]
    p = k / n
    d = 1 + z * z / n
    c = (p + z * z / (2 * n)) / d
    h = z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n)) / d
    return [round(c - h, 3), round(c + h, 3)]


# ------------------------------------------------------------------------------------ 1. live

def live(sessions) -> dict:
    ev = [s for s in sessions if s.phase == "eval"]
    arms = sorted({s.arm for s in ev})
    key = {(s.task, s.rep, s.arm): s for s in ev}
    out = {"arms": {}, "contrasts": {}}
    for a in arms:
        xs = [s for s in ev if s.arm == a]
        k = sum(s.passed for s in xs)
        out["arms"][a] = {"sessions": len(xs), "usd": round(float(np.mean([s.cost_usd for s in xs])), 4),
                          "calls": round(float(np.mean([s.n_calls for s in xs])), 2),
                          "turns": round(float(np.mean([s.metrics.get("turns") or 0 for s in xs])), 2),
                          "passed": k, "pass_rate": round(k / len(xs), 3) if xs else None, "pass_wilson90": wilson(k, len(xs))}
        for fam in families.FAMILIES:
            fx = [s for s in xs if s.repo == fam]
            if fx:
                out["arms"][a][fam] = {"sessions": len(fx), "usd": round(float(np.mean([s.cost_usd for s in fx])), 4),
                                       "calls": round(float(np.mean([s.n_calls for s in fx])), 2),
                                       "pass_rate": round(sum(s.passed for s in fx) / len(fx), 3)}
    for a in arms:
        if a == "control":
            continue
        pairs = [(t, key[(t, r, a)], key[(t, r, "control")]) for (t, r, arm) in key if arm == a and (t, r, "control") in key]
        res = {}
        for scope in ["pooled", *families.FAMILIES]:
            ps = [p for p in pairs if scope == "pooled" or p[0].startswith(scope)]
            if not ps:
                continue
            res[scope] = {
                "usd": boot_ratio([(t, x.cost_usd, c.cost_usd) for t, x, c in ps]),
                "calls": boot_ratio([(t, x.n_calls, c.n_calls) for t, x, c in ps]),
                "turns": boot_ratio([(t, x.metrics.get("turns") or 0, c.metrics.get("turns") or 0) for t, x, c in ps]),
                "pass_diff": {"arm": sum(x.passed for _, x, _ in ps), "control": sum(c.passed for _, _, c in ps),
                              "pairs": len(ps),
                              "arm_only": sum(x.passed and not c.passed for _, x, c in ps),
                              "control_only": sum(c.passed and not x.passed for _, x, c in ps)},
            }
        out["contrasts"][f"{a}_vs_control"] = res
    if "agents-md" in arms and "adage" in arms:  # card vs today's adage, same pairing
        ps = [(t, key[(t, r, "agents-md")], key[(t, r, "adage")]) for (t, r, arm) in key
              if arm == "adage" and (t, r, "agents-md") in key]
        out["contrasts"]["card_vs_adage"] = {"pooled": {
            "usd": boot_ratio([(t, x.cost_usd, c.cost_usd) for t, x, c in ps]),
            "calls": boot_ratio([(t, x.n_calls, c.n_calls) for t, x, c in ps])}}
    return out


# ------------------------------------------------------------------------------------ 2-3. decomposition

def rel(p, cwd):
    return procedure.rel(p, cwd)


def action(c, cwd) -> tuple:
    inp = c.input or {}
    if c.tool in ("Read", *EDIT_TOOLS) and inp.get("file_path"):
        return ("edit" if c.tool in EDIT_TOOLS else "read", rel(inp["file_path"], cwd))
    if c.tool == "Bash":
        cmd = (inp.get("command") or "").replace(cwd + "/", "").replace(cwd, ".")
        return ("bash", procedure.norm_cmd(cmd))
    if c.tool in ("Grep", "Glob", "LS"):
        return ("search", c.tool, rel(inp.get("path") or ".", cwd))
    return (c.tool,)


def role(c, cwd) -> str:
    inp = c.input or {}
    if c.is_error:
        return "failed"
    if c.tool in EDIT_TOOLS:
        return "edit"
    if c.tool in ("Grep", "Glob", "LS"):
        return "search"
    if c.tool == "Read":
        return "read"
    if c.tool == "Bash":
        head = procedure.norm_cmd((inp.get("command") or "").replace(cwd, ".")).split()[:1]
        if head and head[0] in SEARCH_CMDS:
            return "search"
        if head and head[0] in ("cat", "head", "tail", "sed", "wc"):
            return "read"
        if head and head[0] == "git":
            return "vcs"
        return "run"
    return "other"


def usd_share(s, idx) -> float:
    """$ saved if the calls `idx` never happened: a direct counterfactual (no double counting for
    large removals, unlike summing the harness's marginal removal): a response whose calls are all
    removed is not made; every other response keeps its output cost and pays for its input in
    proportion to the context left once the removed calls' growth before it is gone."""
    cm = CM.of(s)
    removed = set(idx)
    if not removed or not cm.n:
        return 0.0
    R = s.responses
    gone = [0.0] * cm.n  # growth removed by response j (attributed to its removed calls)
    whole = [False] * cm.n
    for j, r in enumerate(R):
        ks = [k for k in r.calls if k in removed]
        if ks:
            whole[j] = len(ks) == len(r.calls)
            gone[j] = cm.growth[j] * (1.0 if whole[j] else sum(cm.weight[k] for k in ks))
    new = 0.0
    acc = 0.0
    for j, r in enumerate(R):
        rt = cm.rate[j]
        if whole[j]:
            acc += gone[j]
            continue
        ctx = r.cache_read + r.cache_write + r.input
        out = r.output * rt[1]
        inp = cm.cost[j] - out
        new += out + (inp * max(0.0, ctx - acc) / ctx if ctx else inp)
        acc += gone[j]
    return cm.total - new


def final_files(s) -> set:
    return {p for p in (s.score.get("changed_files") or [])}


def mask_names(text: str, fam: str) -> str:
    out = text
    for n in families.names(fam):
        out = re.sub(re.escape(n), "NAME", out, flags=re.I)
        out = re.sub(re.escape(re.sub(r"(?<!^)(?=[A-Z])", "_", n).lower()), "name", out)
    return out


def templatable(views_by_sid, fam, files) -> set:
    """Files edited in every session whose edit text (variant names masked) is similar across sessions."""
    out = set()
    for f in files:
        texts = []
        for v in views_by_sid.values():
            t = "\n".join(e["text"] for e in v["edits"] if e["path"] == f and not e["failed"])
            if t:
                texts.append(mask_names(t, fam))
        if len(texts) < max(2, len(views_by_sid)):
            continue
        sims = [difflib.SequenceMatcher(None, a[:3000], b[:3000]).ratio() for i, a in enumerate(texts) for b in texts[i + 1:]]
        if sims and float(np.median(sims)) >= 0.6:
            out.add(f)
    return out


def decompose(sessions) -> dict:
    base = [s for s in sessions if s.arm in ("train", "control")]
    rows = []
    for fam in families.FAMILIES:
        fs = [s for s in base if s.repo == fam]
        acts = {s.sid: [action(c, s.cwd) for c in s.calls] for s in fs}
        views = {s.sid: procedure.session_view(s.transcript) for s in fs}
        allfiles = {}
        for s in fs:
            for p in {e["path"] for e in views[s.sid]["edits"] if not e["failed"]}:
                allfiles[p] = allfiles.get(p, 0) + 1
        common_files = [p for p, k in allfiles.items() if k == len(fs)]
        templ = templatable(views, fam, common_files) if len(fs) > 2 else set()
        for s in fs:
            others = [o for o in fs if o.task != s.task]
            need = math.ceil(len(others) / 2)
            freq = {}
            for o in others:
                for a in set(acts[o.sid]):
                    freq[a] = freq.get(a, 0) + 1
            proc = [k for k, a in enumerate(acts[s.sid]) if freq.get(a, 0) >= need]
            roles = [role(c, s.cwd) for c in s.calls]
            cm = CM.of(s)
            total = cm.total
            # oracle "perfect procedure": keep one read per edited file, successful edits, final successful command
            edited = {rel(c.input["file_path"], s.cwd) for c in s.calls if c.tool in EDIT_TOOLS and not c.is_error
                      and (c.input or {}).get("file_path")}
            keep, seen_read = set(), set()
            last_ok_bash = max((k for k, c in enumerate(s.calls) if c.tool == "Bash" and not c.is_error
                                and role(c, s.cwd) == "run"), default=None)
            for k, c in enumerate(s.calls):
                inp = c.input or {}
                if c.tool in EDIT_TOOLS and not c.is_error:
                    keep.add(k)
                elif c.tool == "Read" and not c.is_error and rel(inp.get("file_path", ""), s.cwd) in edited \
                        and rel(inp.get("file_path", ""), s.cwd) not in seen_read:
                    keep.add(k)
                    seen_read.add(rel(inp["file_path"], s.cwd))
            if last_ok_bash is not None:
                keep.add(last_ok_bash)
            oracle = [k for k in range(s.n_calls) if k not in keep]
            hook = oracle + [k for k in keep if s.calls[k].tool in EDIT_TOOLS
                             and rel(s.calls[k].input.get("file_path", ""), s.cwd) in templ]
            by_role = {}
            for r in set(roles):
                idx = [k for k, x in enumerate(roles) if x == r]
                by_role[r] = {"calls": len(idx), "usd": usd_share(s, idx)}
            rows.append({"sid": s.sid, "family": fam, "task": s.task, "arm": s.arm, "passed": s.passed,
                         "calls": s.n_calls, "usd": total, "cost_usd": s.cost_usd,
                         "proc_calls": len(proc), "proc_usd": usd_share(s, proc),
                         "novel_calls": s.n_calls - len(proc),
                         "oracle_calls": len(oracle), "oracle_usd": usd_share(s, oracle),
                         "hook_calls": len(hook), "hook_usd": usd_share(s, hook),
                         "by_role": by_role, "templatable_files": sorted(templ), "common_files": sorted(common_files)})
    agg = {}
    for scope in ["pooled", *families.FAMILIES]:
        rs = [r for r in rows if scope == "pooled" or r["family"] == scope]
        if not rs:
            continue
        cl = [r["task"] for r in rs]
        agg[scope] = {
            "sessions": len(rs),
            "calls_per_session": round(float(np.mean([r["calls"] for r in rs])), 2),
            "usd_per_session": round(float(np.mean([r["usd"] for r in rs])), 4),
            "procedural_call_share": boot_mean([r["proc_calls"] / r["calls"] for r in rs], cl),
            "procedural_usd_share": boot_mean([r["proc_usd"] / r["usd"] for r in rs], cl),
            "oracle_perfect_procedure_usd_share": boot_mean([r["oracle_usd"] / r["usd"] for r in rs], cl),
            "oracle_perfect_procedure_call_share": boot_mean([r["oracle_calls"] / r["calls"] for r in rs], cl),
            "oracle_hook_executed_usd_share": boot_mean([r["hook_usd"] / r["usd"] for r in rs], cl),
            "oracle_hook_executed_call_share": boot_mean([r["hook_calls"] / r["calls"] for r in rs], cl),
            "role_call_share": {k: round(sum(r["by_role"].get(k, {}).get("calls", 0) for r in rs) /
                                         sum(r["calls"] for r in rs), 3)
                                for k in ("read", "search", "edit", "run", "failed", "vcs", "other")},
            "role_usd_share": {k: round(sum(r["by_role"].get(k, {}).get("usd", 0) for r in rs) /
                                        sum(r["usd"] for r in rs), 3)
                               for k in ("read", "search", "edit", "run", "failed", "vcs", "other")},
            "templatable_files": sorted({f for r in rs for f in r["templatable_files"]}),
        }
    return {"aggregate": agg, "sessions": rows}


# ------------------------------------------------------------------------------------ 4. curve

def realistic(s, card) -> list:
    """Calls a checklist card makes unnecessary: searches/listings before the last first-touch of a
    card file (discovery of where things go), and failed commands the card lists as avoid."""
    files = {p for p, _, _ in card["files"] if "<new>" not in p}
    dirs = {p.split("/<new>")[0] for p, _, _ in card["files"] if "<new>" in p}
    first = {}
    for k, c in enumerate(s.calls):
        p = rel((c.input or {}).get("file_path", ""), s.cwd)
        if c.tool in ("Read", *EDIT_TOOLS) and (p in files or any(p.startswith(d + "/") for d in dirs)):
            first.setdefault(p, k)
    horizon = max(first.values()) if first else -1
    avoid = {c[0] for c in card["avoid"]}
    out = []
    for k, c in enumerate(s.calls):
        r = role(c, s.cwd)
        if r == "search" and k < horizon:
            out.append(k)
        elif c.is_error and c.tool == "Bash" and action(c, s.cwd)[1] in avoid:
            out.append(k)
    return out


def curve(sessions, max_n=9, draws=8) -> dict:
    base = [s for s in sessions if s.arm in ("train", "control")]
    views = {s.sid: procedure.session_view(s.transcript) for s in base}
    rng = random.Random(RNG)
    res = {}
    for n in range(1, max_n + 1):
        rows = []
        for s in base:
            pool = [o for o in base if o.repo == s.repo and o.task != s.task]
            # one session per other variant (the family's history is one session per variant)
            byv = {}
            for o in pool:
                byv.setdefault(o.task, []).append(o)
            if len(byv) < n:
                continue
            vals, rec, prec = [], [], []
            for _ in range(draws):
                prior = [rng.choice(byv[t]) for t in rng.sample(sorted(byv), n)]
                card = procedure.learn([views[o.sid] for o in prior], {o.sid: o.passed for o in prior})
                idx = realistic(s, card)
                vals.append(usd_share(s, idx) / CM.of(s).total)
                cf = {p for p, _, _ in card["files"] if "<new>" not in p}
                fin = {e["path"] for e in views[s.sid]["edits"] if not e["failed"]}
                rec.append(len(cf & fin) / len(fin) if fin else 0.0)
                prec.append(len(cf & fin) / len(cf) if cf else 0.0)
            rows.append((s.task, float(np.mean(vals)), float(np.mean(rec)), float(np.mean(prec)), s.repo))
        if not rows:
            continue
        cl = [r[0] for r in rows]
        res[n] = {"realistic_usd_share": boot_mean([r[1] for r in rows], cl),
                  "card_file_recall": round(float(np.mean([r[2] for r in rows])), 3),
                  "card_file_precision": round(float(np.mean([r[3] for r in rows])), 3),
                  "by_family": {f: round(float(np.mean([r[1] for r in rows if r[4] == f])), 4)
                                for f in families.FAMILIES if any(r[4] == f for r in rows)}}
    return res


# ------------------------------------------------------------------------------------ uptake

def uptake(sessions) -> dict:
    """Did card sessions go straight to the card's files, and what did adage deliver?"""
    ev = [s for s in sessions if s.phase == "eval"]
    out = {}
    for a in sorted({s.arm for s in ev}):
        xs = [s for s in ev if s.arm == a]
        roles = [[role(c, s.cwd) for c in s.calls] for s in xs]
        first_edit = [next((k for k, r in enumerate(rs) if r == "edit"), len(rs)) for rs in roles]
        out[a] = {"calls_before_first_edit": round(float(np.mean(first_edit)), 2),
                  "search_calls": round(float(np.mean([rs.count("search") for rs in roles])), 2),
                  "read_calls": round(float(np.mean([rs.count("read") for rs in roles])), 2),
                  "edit_calls": round(float(np.mean([rs.count("edit") for rs in roles])), 2),
                  "run_calls": round(float(np.mean([rs.count("run") for rs in roles])), 2),
                  "failed_calls": round(float(np.mean([rs.count("failed") for rs in roles])), 2),
                  "vcs_calls": round(float(np.mean([rs.count("vcs") for rs in roles])), 2),
                  "directives_delivered": round(float(np.mean([len(s.metrics.get("adage") or []) for s in xs])), 2)
                  if a == "adage" else None,
                  "hidden_failed": {}}
        for s in xs:
            for h in (s.score.get("hidden_failed") or []):
                h = h.split("[")[0]
                out[a]["hidden_failed"][h] = out[a]["hidden_failed"].get(h, 0) + 1
            if s.score.get("suite_failed"):
                out[a]["hidden_failed"]["(other suite tests)"] = out[a]["hidden_failed"].get("(other suite tests)", 0) + 1
    return out


def live_curve(sessions) -> dict:
    """Live points of the learning curve on the marshmallow family (the only one with an n = 1 arm):
    per held-out variant, arm mean vs control mean (both repeats); ratio of sums - 1, bootstrap over
    the 6 variants. n = 0 is the control itself."""
    mm = [s for s in sessions if s.repo == "mm-validator" and s.phase == "eval"]
    ctrl = {}
    for s in mm:
        if s.arm == "control":
            ctrl.setdefault(s.task, []).append(s)
    out = {"n0_control": {"usd": round(float(np.mean([x.cost_usd for v in ctrl.values() for x in v])), 4)}}
    for label, pick in (("n1_card", lambda s: s.root.endswith("card1") and s.arm == "agents-md"),
                        ("n4_card", lambda s: not s.root.endswith("card1") and s.arm == "agents-md"),
                        ("n4_adage", lambda s: s.arm == "adage")):
        arm = {}
        for s in mm:
            if pick(s):
                arm.setdefault(s.task, []).append(s)
        tasks = sorted(set(arm) & set(ctrl))
        if not tasks:
            continue
        pairs = [(t, float(np.mean([x.cost_usd for x in arm[t]])), float(np.mean([x.cost_usd for x in ctrl[t]])))
                 for t in tasks]
        callp = [(t, float(np.mean([x.n_calls for x in arm[t]])), float(np.mean([x.n_calls for x in ctrl[t]])))
                 for t in tasks]
        out[label] = {"usd": boot_ratio(pairs), "calls": boot_ratio(callp),
                      "sessions": sum(len(arm[t]) for t in tasks),
                      "passed": sum(x.passed for t in tasks for x in arm[t])}
    return out


def adage_delivered(sessions) -> dict:
    """What the adage arm was told: delivered directives by route (from the runner's spool)."""
    out = {}
    for s in sessions:
        if s.arm != "adage":
            continue
        for d in s.metrics.get("adage") or []:
            if isinstance(d, dict) and d.get("kind") != "directives":
                continue
            r = f"{s.repo}:{d.get('event')}:" + str(d.get("route") or d.get("id") or "?") if isinstance(d, dict) else str(d)[:40]
            out[r] = out.get(r, 0) + 1
    return dict(sorted(out.items(), key=lambda x: -x[1]))


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--out", default=str(HERE / "analysis.json"))
    ap.add_argument("--roots", nargs="*")
    a = ap.parse_args(argv)
    ss = load(a.roots)
    print(f"{len(ss)} sessions")
    main_ss = [x for x in ss if not x.root.endswith("card1")]
    res = {"live": live(main_ss), "uptake": uptake(main_ss), "live_curve": live_curve(ss),
           "adage_delivered": adage_delivered(main_ss), "decomposition": decompose(main_ss)["aggregate"], "curve": curve(main_ss)}
    res["decomposition_sessions"] = decompose(main_ss)["sessions"]
    Path(a.out).write_text(json.dumps(res, indent=1, default=str))
    print(json.dumps({k: v for k, v in res.items() if k != "decomposition_sessions"}, indent=1, default=str)[:12000])
    return 0


if __name__ == "__main__":
    sys.exit(main())
