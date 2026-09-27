"""Paired analysis for the external-benchmark protocol (stdlib only).

Unit of analysis: the task. Each (task, arm) is summarised by its mean over repeats, then arms are
compared within task (paired). Tasks are nested in repositories, so intervals come from a
two-stage cluster bootstrap (resample repositories, then tasks within each drawn repository),
and p-values from a sign-flip permutation test on the per-task differences (exact under the null
of exchangeable arms within a task).

Effects:
  delta_resolve   mean over tasks of (resolve rate treated - resolve rate control), in points
  cost_ratio      exp(mean over tasks of log(cost treated / cost control)): a geometric-mean ratio
  turns_ratio     the same for turns

Records are the JSONL rows plan.py `emit` writes (fields: task_id, repo_key, arm, status,
outcome.passed, cost.usd, efficiency.turns). Only status "ok" and "censored" rows count: an
infra_error or scorer_error says nothing about the arm, and is reported, not imputed.
"""

from __future__ import annotations

import math
import random
import statistics
from collections import defaultdict
from typing import Callable, Iterable, Sequence

ANALYSED = ("ok", "censored")


def _get(rec: dict, path: str):
    cur = rec
    for k in path.split("."):
        if not isinstance(cur, dict):
            return None
        cur = cur.get(k)
    return cur


def metric_value(rec: dict, metric: str):
    """resolve -> 0/1, cost -> USD, turns -> count; None when missing."""
    if metric == "resolve":
        v = _get(rec, "outcome.passed")
        return None if v is None else float(bool(v))
    if metric == "cost":
        return _get(rec, "cost.usd")
    if metric == "turns":
        return _get(rec, "efficiency.turns")
    return _get(rec, metric)


def per_task_means(records: Iterable[dict], metric: str) -> dict[tuple[str, str], dict[str, float]]:
    """{(repo, task): {arm: mean over analysed repeats}}."""
    acc: dict[tuple[str, str], dict[str, list[float]]] = defaultdict(lambda: defaultdict(list))
    for r in records:
        if r.get("status") not in ANALYSED:
            continue
        v = metric_value(r, metric)
        if v is None:
            continue
        acc[(r.get("repo_key") or "", r["task_id"])][r["arm"]].append(float(v))
    return {k: {a: statistics.fmean(v) for a, v in arms.items()} for k, arms in acc.items()}


def paired(means: dict[tuple[str, str], dict[str, float]], treated: str, control: str,
           transform: Callable[[float, float], float] | None = None) -> dict[str, list[float]]:
    """{repo: [per-task difference]} for tasks with both arms. `transform(t, c)` defaults to t - c;
    tasks where it is undefined (returns None) are dropped."""
    f = transform or (lambda t, c: t - c)
    out: dict[str, list[float]] = defaultdict(list)
    for (repo, _task), arms in sorted(means.items()):
        if treated in arms and control in arms:
            d = f(arms[treated], arms[control])
            if d is not None:
                out[repo].append(d)
    return dict(out)


def log_ratio(t: float, c: float, floor: float = 1e-9):
    if t is None or c is None or t < 0 or c < 0:
        return None
    return math.log(max(t, floor)) - math.log(max(c, floor))


def _flat(clusters: dict[str, list[float]]) -> list[float]:
    return [x for repo in sorted(clusters) for x in clusters[repo]]


def permutation_test(diffs: Sequence[float], n: int = 10000, seed: int = 0) -> float | None:
    """Two-sided paired sign-flip test of mean(diffs) == 0. Exact enumeration up to 2^16 pairs'
    worth of sign patterns, Monte Carlo (with the +1 correction) beyond."""
    d = [x for x in diffs if x is not None]
    if not d:
        return None
    obs = abs(sum(d)) - 1e-12
    nz = [x for x in d if x != 0]
    if not nz:
        return 1.0
    if len(nz) <= 16:
        hits = 0
        total = 1 << len(nz)
        for mask in range(total):
            s = sum(x if (mask >> i) & 1 else -x for i, x in enumerate(nz))
            hits += abs(s) >= obs
        return hits / total
    rng = random.Random(seed)
    hits = 0
    for _ in range(n):
        s = sum(x if rng.random() < 0.5 else -x for x in nz)
        hits += abs(s) >= obs
    return (hits + 1) / (n + 1)


def cluster_bootstrap(clusters: dict[str, list[float]], stat: Callable[[list[float]], float] = statistics.fmean,
                      b: int = 5000, seed: int = 0, alpha: float = 0.05) -> tuple[float, float] | None:
    """Percentile CI from a two-stage bootstrap: repositories with replacement, then tasks with
    replacement within each drawn repository."""
    repos = sorted(r for r, v in clusters.items() if v)
    if not repos:
        return None
    rng = random.Random(seed)
    stats = []
    for _ in range(b):
        sample = []
        for _ in repos:
            v = clusters[repos[rng.randrange(len(repos))]]
            sample.extend(v[rng.randrange(len(v))] for _ in v)
        stats.append(stat(sample))
    stats.sort()
    lo = stats[max(0, int(math.floor(alpha / 2 * b)))]
    hi = stats[min(b - 1, int(math.ceil((1 - alpha / 2) * b)) - 1)]
    return lo, hi


def holm(pvalues: dict[str, float | None]) -> dict[str, float | None]:
    """Holm-Bonferroni adjusted p-values (monotone), None passed through."""
    items = sorted(((p, k) for k, p in pvalues.items() if p is not None))
    m = len(items)
    out: dict[str, float | None] = {k: None for k, p in pvalues.items() if p is None}
    running = 0.0
    for i, (p, k) in enumerate(items):
        running = max(running, min(1.0, (m - i) * p))
        out[k] = running
    return out


def effect(records: list[dict], treated: str, control: str, metric: str, b: int = 5000, seed: int = 0) -> dict:
    """One comparison: point estimate, cluster-bootstrap CI, permutation p, and counts."""
    means = per_task_means(records, metric)
    if metric == "resolve":
        clusters = paired(means, treated, control)
        point_f, back = statistics.fmean, (lambda x: x)
    else:
        clusters = paired(means, treated, control, log_ratio)
        point_f, back = statistics.fmean, math.exp
    flat = _flat(clusters)
    if not flat:
        return {"metric": metric, "treated": treated, "control": control, "pairs": 0}
    ci = cluster_bootstrap(clusters, point_f, b=b, seed=seed)
    name = "delta_resolve" if metric == "resolve" else f"{metric}_ratio"
    return {
        "metric": metric, "effect": name, "treated": treated, "control": control,
        "pairs": len(flat), "repos": len(clusters),
        "estimate": back(point_f(flat)),
        "ci95": [back(ci[0]), back(ci[1])] if ci else None,
        "p": permutation_test(flat, seed=seed),
    }


def analyse(records: list[dict], comparisons: Sequence[tuple[str, str]] = (("adage", "holdout"), ("adage", "off"),
                                                                        ("holdout", "off")),
            metrics: Sequence[str] = ("resolve", "cost", "turns"), b: int = 5000, seed: int = 0) -> dict:
    """Every (comparison x metric) effect, with Holm-adjusted p-values across the whole family."""
    rows = [effect(records, t, c, m, b=b, seed=seed) for t, c in comparisons for m in metrics]
    adj = holm({f"{r['treated']}-{r['control']}:{r['metric']}": r.get("p") for r in rows})
    for r in rows:
        r["p_holm"] = adj.get(f"{r['treated']}-{r['control']}:{r['metric']}")
    status = defaultdict(lambda: defaultdict(int))
    for r in records:
        status[r.get("arm")][r.get("status")] += 1
    return {"effects": rows, "status_by_arm": {a: dict(s) for a, s in status.items()}}


# ------------------------------------------------------------------------------------ power

def _z(p: float) -> float:
    return statistics.NormalDist().inv_cdf(p)


def pairs_for_binary(discordance: float, delta: float, alpha: float = 0.05, power: float = 0.8) -> int:
    """Pairs needed to detect a paired difference in resolve rate `delta` (e.g. 0.05 = 5 points)
    when a fraction `discordance` of pairs disagree (one arm resolves, the other does not):
    McNemar's test, Connor (1987) normal approximation."""
    if not 0 < abs(delta) < discordance <= 1:
        raise ValueError("need 0 < |delta| < discordance <= 1")
    za, zb = _z(1 - alpha / 2), _z(power)
    n = (za * math.sqrt(discordance) + zb * math.sqrt(discordance - delta ** 2)) ** 2 / delta ** 2
    return math.ceil(n)


def pairs_for_continuous(sd_diff: float, delta: float, alpha: float = 0.05, power: float = 0.8) -> int:
    """Pairs needed for a paired mean difference `delta` with per-pair SD `sd_diff` (e.g. of
    log cost ratios: delta = log(0.9) for a 10% saving)."""
    if sd_diff <= 0 or delta == 0:
        raise ValueError("need sd_diff > 0 and delta != 0")
    za, zb = _z(1 - alpha / 2), _z(power)
    return math.ceil(((za + zb) * sd_diff / abs(delta)) ** 2)


def design_effect(tasks_per_repo: float, icc: float) -> float:
    """Variance inflation for tasks clustered in repositories: 1 + (m - 1) * ICC."""
    return 1 + (tasks_per_repo - 1) * icc


def power_table(discordances=(0.1, 0.2, 0.3), deltas=(0.03, 0.05, 0.1), alpha=0.05, power=0.8) -> list[dict]:
    rows = []
    for d in discordances:
        for e in deltas:
            try:
                rows.append({"discordance": d, "delta": e, "pairs": pairs_for_binary(d, e, alpha, power)})
            except ValueError:
                rows.append({"discordance": d, "delta": e, "pairs": None})
    return rows
