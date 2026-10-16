"""Quality metrics: family top-1 (LOO nearest-centroid), verb-contrast
accuracy, open-set precision@90recall with per-family thresholds, and
HDBSCAN clustering agreement (ARI / purity / noise fraction).
"""
from __future__ import annotations

from collections import defaultdict

import numpy as np
from sklearn.cluster import HDBSCAN
from sklearn.metrics import adjusted_rand_score


def unit_norm(x: np.ndarray) -> np.ndarray:
    n = np.linalg.norm(x, axis=-1, keepdims=True)
    return x / np.clip(n, 1e-9, None)


def family_top1_loo(embs: np.ndarray, labels: list[str]) -> tuple[float, dict]:
    """Leave-one-out nearest-centroid top-1 accuracy: for each row, build
    each family's centroid excluding that row (if it belongs to that
    family), then predict the family whose centroid is most similar.
    """
    embs = unit_norm(embs)
    labels_arr = np.asarray(labels)
    families = sorted(set(labels))
    fam_idx = {f: np.where(labels_arr == f)[0] for f in families}
    fam_sum = {f: embs[idx].sum(axis=0) for f, idx in fam_idx.items()}
    fam_count = {f: len(idx) for f, idx in fam_idx.items()}

    correct = 0
    per_family_correct = defaultdict(int)
    per_family_total = defaultdict(int)
    for i, true_fam in enumerate(labels):
        best_f, best_s = None, -1e9
        for f in families:
            n = fam_count[f] - (1 if f == true_fam else 0)
            if n <= 0:
                continue
            centroid = (fam_sum[f] - (embs[i] if f == true_fam else 0)) / n
            centroid = centroid / max(np.linalg.norm(centroid), 1e-9)
            s = float(np.dot(embs[i], centroid))
            if s > best_s:
                best_s, best_f = s, f
        per_family_total[true_fam] += 1
        if best_f == true_fam:
            correct += 1
            per_family_correct[true_fam] += 1

    per_family = {
        f: per_family_correct[f] / per_family_total[f] for f in families
    }
    return correct / len(labels), per_family


def centroids_from(embs: np.ndarray, labels: list[str]) -> dict[str, np.ndarray]:
    embs = unit_norm(embs)
    labels_arr = np.asarray(labels)
    out = {}
    for f in sorted(set(labels)):
        idx = np.where(labels_arr == f)[0]
        c = embs[idx].mean(axis=0)
        out[f] = c / max(np.linalg.norm(c), 1e-9)
    return out


def verb_contrast_accuracy(
    hard_embs: np.ndarray,
    hard_families: list[str],
    contrast_groups: list[str],
    centroids: dict[str, np.ndarray],
) -> tuple[float, dict]:
    """For each hard-subset row, restrict the choice to the two families in
    its own contrast group and predict the more similar centroid.
    """
    hard_embs = unit_norm(hard_embs)
    correct = 0
    per_group_correct = defaultdict(int)
    per_group_total = defaultdict(int)
    for i in range(len(hard_families)):
        group = contrast_groups[i]
        fa, fb = group.split(":")[1].split("-vs-")
        sa = float(np.dot(hard_embs[i], centroids[fa]))
        sb = float(np.dot(hard_embs[i], centroids[fb]))
        pred = fa if sa >= sb else fb
        true_fam = hard_families[i]
        per_group_total[group] += 1
        if pred == true_fam:
            correct += 1
            per_group_correct[group] += 1
    per_group = {
        g: per_group_correct[g] / per_group_total[g] for g in per_group_total
    }
    return correct / len(hard_families), per_group


def multiclass_top1(
    embs: np.ndarray, true_labels: list[str], centroids: dict[str, np.ndarray]
) -> tuple[float, dict]:
    """Full multiclass nearest-centroid accuracy against ALL family
    centroids (not just the two in a contrast group) -- the realistic
    routing scenario from Design Spec §8.2.1, where a prompt is compared
    against every family with an active route, not a pre-narrowed pair.
    """
    embs = unit_norm(embs)
    families = list(centroids.keys())
    cmat = np.stack([centroids[f] for f in families])  # [F, D]
    sims = embs @ cmat.T  # [N, F]
    pred_idx = np.argmax(sims, axis=1)
    preds = [families[i] for i in pred_idx]
    correct = sum(1 for p, t in zip(preds, true_labels) if p == t)
    per_label_correct = defaultdict(int)
    per_label_total = defaultdict(int)
    for p, t in zip(preds, true_labels):
        per_label_total[t] += 1
        if p == t:
            per_label_correct[t] += 1
    per_label = {t: per_label_correct[t] / per_label_total[t] for t in per_label_total}
    return correct / len(true_labels), per_label


def open_set_precision_at_recall(
    pos_embs: np.ndarray,
    pos_families: list[str],
    noise_embs: np.ndarray,
    centroids: dict[str, np.ndarray],
    target_recall: float = 0.90,
) -> tuple[float, float, dict]:
    """Per-family calibrated thresholds: for each family, find the score
    threshold (similarity to that family's own centroid) that keeps at
    least `target_recall` of the family's own rows, then measure precision
    of that threshold against ALL noise rows scored against the same
    centroid. Aggregate as micro precision/recall across families.
    """
    pos_embs = unit_norm(pos_embs)
    noise_embs = unit_norm(noise_embs)
    pos_families_arr = np.asarray(pos_families)

    total_tp = total_fp = total_pos = 0
    per_family = {}
    for f, centroid in centroids.items():
        idx = np.where(pos_families_arr == f)[0]
        if len(idx) == 0:
            continue
        pos_scores = pos_embs[idx] @ centroid
        noise_scores = noise_embs @ centroid
        sorted_scores = np.sort(pos_scores)[::-1]
        keep = max(1, int(np.ceil(target_recall * len(sorted_scores))))
        threshold = sorted_scores[keep - 1]

        tp = int(np.sum(pos_scores >= threshold))
        fp = int(np.sum(noise_scores >= threshold))
        recall = tp / len(idx)
        precision = tp / (tp + fp) if (tp + fp) > 0 else 1.0
        per_family[f] = {
            "threshold": float(threshold),
            "recall": recall,
            "precision": precision,
            "tp": tp,
            "fp": fp,
            "n_pos": len(idx),
        }
        total_tp += tp
        total_fp += fp
        total_pos += len(idx)

    micro_precision = total_tp / (total_tp + total_fp) if (total_tp + total_fp) > 0 else 1.0
    micro_recall = total_tp / total_pos if total_pos > 0 else 0.0
    return micro_precision, micro_recall, per_family


def clustering_metrics(
    embs: np.ndarray, true_labels: list[str], min_cluster_size: int = 5
) -> dict:
    embs = unit_norm(embs)
    clusterer = HDBSCAN(min_cluster_size=min_cluster_size, metric="euclidean")
    pred = clusterer.fit_predict(embs)

    true_arr = np.asarray(true_labels)
    ari = adjusted_rand_score(true_arr, pred)

    n = len(pred)
    noise_fraction = float(np.sum(pred == -1)) / n

    # Purity: for each predicted cluster (including -1, treated as its own
    # bucket of singleton "clusters" conceptually, but scored the same way),
    # take the majority true label share, weighted by cluster size.
    purity_num = 0
    for c in set(pred.tolist()):
        idx = np.where(pred == c)[0]
        labels_here = true_arr[idx]
        _, counts = np.unique(labels_here, return_counts=True)
        purity_num += counts.max()
    purity = purity_num / n

    n_clusters = len(set(pred.tolist()) - {-1})
    return {
        "ari": float(ari),
        "purity": float(purity),
        "noise_fraction": noise_fraction,
        "n_predicted_clusters": n_clusters,
        "n_true_labels": len(set(true_labels)),
    }
