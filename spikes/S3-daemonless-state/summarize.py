#!/usr/bin/env python3
"""Summarise results/bench-*.json into the markdown tables used in FINDINGS.md."""
import glob, json, os

here = os.path.dirname(os.path.abspath(__file__))
rows = {}
for p in sorted(glob.glob(os.path.join(here, "results", "bench-*.json"))):
    with open(p) as f:
        r = json.load(f)
    rows[r["label"]] = r


def g(d, *ks, default="–"):
    for k in ks:
        if not isinstance(d, dict) or k not in d:
            return default
        d = d[k]
    return d


def table(labels, title):
    out = [f"### {title}", "",
           "| run | sessions | decide n | decide p50 / p95 / p99 (µs) | state access p95 | lock wait p95 | "
           "observe p50 / p95 (µs) | spawn-incl. decide p95 | reconstruct | load (1m before→after) |",
           "|---|---:|---:|---|---:|---:|---|---:|---|---|"]
    for l in labels:
        r = rows.get(l)
        if not r:
            continue
        d, o = r["ops"]["decide:ALL"], r["ops"]["observe:ALL"]
        out.append(
            f"| {l} | {r['sessions']} | {d['n']} | {d['total_us']['p50']} / {d['total_us']['p95']} / {d['total_us']['p99']} | "
            f"{d['access_us']['p95']} | {d['lock_us']['p95']} | {o['total_us']['p50']} / {o['total_us']['p95']} | "
            f"{g(d, 'wall_us', 'p95')} | {r['reconstruct_ok']} | "
            f"{r['loadavg_before'].split()[0]}→{r['loadavg_after'].split()[0]} |")
    return "\n".join(out) + "\n"


def per_kind(label):
    r = rows.get(label)
    if not r:
        return ""
    out = [f"### Per decision point — {label}", "",
           "| op | n | p50 | p95 | p99 | max | records folded p95 |", "|---|---:|---:|---:|---:|---:|---:|"]
    for k, v in r["ops"].items():
        if k.endswith("ALL"):
            continue
        out.append(f"| {k} | {v['n']} | {v['total_us']['p50']} | {v['total_us']['p95']} | {v['total_us']['p99']} | "
                   f"{v['total_us']['max']} | {v['folded']['p95']} |")
    return "\n".join(out) + "\n"


print("## Latency (µs; 8 parallel PostToolUse per batch; batch decision overlaps them)\n")
N = (1, 4, 16)
for regime in ("paced", "stress"):
    print(table([f"file-slots-{n}-{regime}" for n in N] + [f"file-rename-{n}-{regime}" for n in N],
                f"Daemonless, in-process driver — {regime} (state access = lock + load + store)"))
    print(table([f"proc-slots-{n}-{regime}" for n in N] + [f"proc-rename-{n}-{regime}" for n in N],
                f"Daemonless, one process per hook — {regime} (in-child timings; spawn-incl. = parent wall clock)"))
    print(table([f"daemon-{n}-{regime}" for n in N],
                f"Minimal daemon — {regime} (decide = client connect + round trip; state access = server-side handling)"))
print(table([f"file-slots-{n}-paced-{d}" for d in ("fsync", "full") for n in (1, 16)] + ["file-slots-16-paced-sequential"],
            "Durability and control runs (paced)"))
for l in ("file-slots-16-paced", "proc-slots-16-paced", "daemon-16-paced", "file-slots-16-stress"):
    print(per_kind(l))
d = rows.get("disk", {}).get("disk")
if d:
    print("### Disk (16 sessions × 32 turns)\n")
    print("```json\n" + json.dumps(d, indent=1) + "\n```")
