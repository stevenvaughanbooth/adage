#!/usr/bin/env python3
"""Quality benchmark: for every model x normalisation condition, compute
family top-1 (LOO), verb-contrast accuracy, open-set precision@90recall,
and HDBSCAN clustering agreement. Writes results/quality.json and a
markdown summary table to results/quality.md.

Usage:
    PYTHONPATH=src .venv/bin/python src/s2/bench_quality.py
"""
from __future__ import annotations

import json
import sys
from pathlib import Path

import numpy as np

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "src"))

from s2.models import ALL_MODEL_KEYS, make_model  # noqa: E402
from s2.normalize import normalize, raw_head  # noqa: E402
from s2 import metrics as M  # noqa: E402

DATA_PATH = ROOT / "data" / "prompts.jsonl"
RESULTS_DIR = ROOT / "results"


def load_rows() -> list[dict]:
    rows = []
    with open(DATA_PATH) as f:
        for line in f:
            rows.append(json.loads(line))
    return rows


def apply_condition(rows: list[dict], condition: str) -> list[str]:
    if condition == "raw":
        return [raw_head(r["text"]) for r in rows]
    if condition == "normalized":
        return [normalize(r["text"]).text for r in rows]
    raise ValueError(condition)


def run_one(model_key: str, condition: str, rows: list[dict]) -> dict:
    fam_rows = [r for r in rows if not r["is_noise"] and not r["is_hard"]]
    hard_rows = [r for r in rows if r["is_hard"]]
    noise_rows = [r for r in rows if r["is_noise"]]

    fam_texts = apply_condition(fam_rows, condition)
    hard_texts = apply_condition(hard_rows, condition)
    noise_texts = apply_condition(noise_rows, condition)

    model = make_model(model_key)
    model.load()

    fam_embs = model.encode(fam_texts)
    hard_embs = model.encode(hard_texts)
    noise_embs = model.encode(noise_texts)

    fam_labels = [r["family"] for r in fam_rows]
    hard_labels = [r["family"] for r in hard_rows]
    hard_groups = [r["contrast_group"] for r in hard_rows]

    top1, per_family_top1 = M.family_top1_loo(fam_embs, fam_labels)

    centroids = M.centroids_from(fam_embs, fam_labels)
    verb_acc, per_group_acc = M.verb_contrast_accuracy(
        hard_embs, hard_labels, hard_groups, centroids
    )
    hard_top1_full, _ = M.multiclass_top1(hard_embs, hard_labels, centroids)

    # open-set: positives = family rows + hard rows (both "belong to a
    # family"), negatives = noise.
    pos_embs = np.concatenate([fam_embs, hard_embs], axis=0)
    pos_families = fam_labels + hard_labels
    precision, recall, per_family_openset = M.open_set_precision_at_recall(
        pos_embs, pos_families, noise_embs, centroids, target_recall=0.90
    )

    all_embs = np.concatenate([fam_embs, hard_embs, noise_embs], axis=0)
    all_labels = fam_labels + hard_labels + ["__noise__"] * len(noise_rows)
    clustering = M.clustering_metrics(all_embs, all_labels)

    return {
        "model": model_key,
        "model_label": model.name,
        "dim": int(fam_embs.shape[1]),
        "condition": condition,
        "family_top1_loo": top1,
        "per_family_top1": per_family_top1,
        "verb_contrast_accuracy": verb_acc,
        "per_group_verb_contrast": per_group_acc,
        "hard_subset_top1_full_multiclass": hard_top1_full,
        "open_set_precision_at_90recall": precision,
        "open_set_recall_achieved": recall,
        "per_family_open_set": per_family_openset,
        "clustering": clustering,
        "n_family_rows": len(fam_rows),
        "n_hard_rows": len(hard_rows),
        "n_noise_rows": len(noise_rows),
    }


def main() -> None:
    RESULTS_DIR.mkdir(exist_ok=True)
    rows = load_rows()
    results = []
    for model_key in ALL_MODEL_KEYS:
        for condition in ["raw", "normalized"]:
            print(f"=== {model_key} / {condition} ===", flush=True)
            r = run_one(model_key, condition, rows)
            print(
                f"  top1={r['family_top1_loo']:.3f}  "
                f"verb_contrast(pairwise)={r['verb_contrast_accuracy']:.3f}  "
                f"verb_contrast(multiclass)={r['hard_subset_top1_full_multiclass']:.3f}  "
                f"open_set_p@90r={r['open_set_precision_at_90recall']:.3f}  "
                f"ari={r['clustering']['ari']:.3f}  "
                f"purity={r['clustering']['purity']:.3f}  "
                f"noise_frac={r['clustering']['noise_fraction']:.3f}"
            )
            results.append(r)

    (RESULTS_DIR / "quality.json").write_text(json.dumps(results, indent=2))

    lines = [
        "| Model | Norm | dim | Family top-1 (LOO) | Verb-contrast (pairwise) | Verb-contrast (multiclass) | Open-set P@90R | ARI | Purity | Noise frac |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in results:
        c = r["clustering"]
        lines.append(
            f"| {r['model_label']} | {r['condition']} | {r['dim']} | "
            f"{r['family_top1_loo']:.3f} | {r['verb_contrast_accuracy']:.3f} | "
            f"{r['hard_subset_top1_full_multiclass']:.3f} | "
            f"{r['open_set_precision_at_90recall']:.3f} | {c['ari']:.3f} | "
            f"{c['purity']:.3f} | {c['noise_fraction']:.3f} |"
        )
    (RESULTS_DIR / "quality.md").write_text("\n".join(lines) + "\n")
    print("\nwrote results/quality.json and results/quality.md")


if __name__ == "__main__":
    main()
