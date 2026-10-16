#!/usr/bin/env python3
"""Driver for bench_cost.py. For each model:

  - COLD trials: fresh subprocess, --n-latency 1 --n-determinism 0, so the
    parent's subprocess wall time is (as closely as a Python harness can
    give us) "process start -> first embedding ready" per the SPIKE's cold
    load metric. Repeated N_TRIALS times per thread setting to see the
    disk-cache-cold-vs-warm effect (first run vs later runs).
  - WARM trial: one longer-lived process per (model, threads) that embeds
    n-latency prompts to get steady-state p50/p95 latency, RSS, and
    same-config determinism.
  - CROSS-THREAD determinism: embeddings emitted from the threads=1 warm
    run vs the threads=default warm run, diffed.

Records system load (`uptime`) since other spikes run concurrently on this
machine -- these numbers are indicative, not clean-room.

Usage:
    .venv/bin/python src/s2/run_cost.py
"""
from __future__ import annotations

import json
import os
import subprocess
import time
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
RESULTS_DIR = ROOT / "results"
PY = str(ROOT / ".venv" / "bin" / "python")
SCRIPT = str(ROOT / "src" / "s2" / "bench_cost.py")

MODELS = ["potion-8m", "bge-fp32", "bge-int8", "minilm-int8"]
N_COLD_TRIALS = 5
N_LATENCY = 300


def run(cmd) -> tuple[dict, float]:
    t0 = time.perf_counter()
    out = subprocess.run(cmd, capture_output=True, text=True, cwd=str(ROOT), check=True)
    wall = time.perf_counter() - t0
    return json.loads(out.stdout.strip().splitlines()[-1]), wall


def get_load_note() -> str:
    try:
        up = subprocess.run(["uptime"], capture_output=True, text=True).stdout.strip()
    except Exception:
        up = "unavailable"
    return f"uptime: {up} | logical_cpus={os.cpu_count()}"


def main() -> None:
    RESULTS_DIR.mkdir(exist_ok=True)
    load_note_start = get_load_note()
    print(f"[system load at run start] {load_note_start}")

    all_results = {}
    for model in MODELS:
        print(f"=== {model} ===")

        # -- cold trials, threads=1 (matches the architecture's intra-op cap) --
        cold_trials = []
        for i in range(N_COLD_TRIALS):
            cmd = [PY, SCRIPT, "--model", model, "--threads", "1",
                   "--n-latency", "1", "--n-determinism", "0"]
            r, wall = run(cmd)
            r["subprocess_wall_ms"] = wall * 1000
            cold_trials.append(r)
            print(f"  [cold #{i}] subprocess_wall={wall*1000:.1f}ms "
                  f"(in-process cold_load={r['cold_load_ms']:.1f}ms)")

        # -- warm trial, threads=1: latency distribution, memory, determinism --
        emit1 = RESULTS_DIR / f".tmp_{model}_t1.npy"
        cmd = [PY, SCRIPT, "--model", model, "--threads", "1",
               "--n-latency", str(N_LATENCY), "--n-determinism", "50",
               "--emit-embeddings", str(emit1)]
        warm_t1, _ = run(cmd)
        print(f"  [warm threads=1] p50={warm_t1['latency_p50_ms']:.3f}ms "
              f"p95={warm_t1['latency_p95_ms']:.3f}ms "
              f"rss_after_load={warm_t1['rss_after_load_mb']:.1f}MB "
              f"determinism={warm_t1['determinism_max_abs_diff_same_config']:.2e}")

        # -- warm trial, default threads --
        emit_def = RESULTS_DIR / f".tmp_{model}_tdef.npy"
        cmd = [PY, SCRIPT, "--model", model,
               "--n-latency", str(N_LATENCY), "--n-determinism", "50",
               "--emit-embeddings", str(emit_def)]
        warm_def, _ = run(cmd)
        print(f"  [warm threads=default] p50={warm_def['latency_p50_ms']:.3f}ms "
              f"p95={warm_def['latency_p95_ms']:.3f}ms "
              f"rss_after_load={warm_def['rss_after_load_mb']:.1f}MB "
              f"determinism={warm_def['determinism_max_abs_diff_same_config']:.2e}")

        cross_thread_max_diff = None
        if emit1.exists() and emit_def.exists():
            e1, e2 = np.load(emit1), np.load(emit_def)
            cross_thread_max_diff = float(np.max(np.abs(e1 - e2)))
            emit1.unlink()
            emit_def.unlink()

        all_results[model] = {
            "cold_trials_threads1": cold_trials,
            "warm_threads1": warm_t1,
            "warm_threads_default": warm_def,
            "cross_thread_determinism_max_abs_diff": cross_thread_max_diff,
        }

    load_note_end = get_load_note()
    payload = {
        "system_load_note_start": load_note_start,
        "system_load_note_end": load_note_end,
        "n_cold_trials": N_COLD_TRIALS,
        "n_latency_warm": N_LATENCY,
        "results": all_results,
    }
    (RESULTS_DIR / "cost.json").write_text(json.dumps(payload, indent=2))

    lines = [
        f"System load (start): {load_note_start}",
        f"System load (end): {load_note_end}",
        "",
        "| Model | Cold start, 1st run (ms) | Cold start, median of runs 2-5 (ms) | "
        "Embed p50 @1 thread (ms) | Embed p95 @1 thread (ms) | RSS after load (MB) | "
        "Determinism (same config) | Determinism (1 vs default threads) |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for model, mr in all_results.items():
        cold = mr["cold_trials_threads1"]
        cold_first = cold[0]["subprocess_wall_ms"]
        cold_rest = [c["subprocess_wall_ms"] for c in cold[1:]]
        cold_median_rest = float(np.median(cold_rest)) if cold_rest else float("nan")
        w1 = mr["warm_threads1"]
        det_cross = mr["cross_thread_determinism_max_abs_diff"]
        det_cross_str = f"{det_cross:.2e}" if det_cross is not None else "n/a"
        lines.append(
            f"| {model} | {cold_first:.1f} | {cold_median_rest:.1f} | "
            f"{w1['latency_p50_ms']:.3f} | {w1['latency_p95_ms']:.3f} | "
            f"{w1['rss_after_load_mb']:.1f} | "
            f"{w1['determinism_max_abs_diff_same_config']:.2e} | {det_cross_str} |"
        )
    (RESULTS_DIR / "cost.md").write_text("\n".join(lines) + "\n")
    print("\nwrote results/cost.json and results/cost.md")


if __name__ == "__main__":
    main()
