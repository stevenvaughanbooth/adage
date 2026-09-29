"""Assemble result.json from results/*.json (python3 make_result.py)."""

from __future__ import annotations

import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
R = HERE / "results"


def j(name):
    return json.loads((R / name).read_text())


def hs(rep):
    s = rep["summary"]
    return {"calls_saved_per_task": round(s["calls_saved_per_task"], 3), "calls_ci90": [round(x, 3) for x in s["calls_saved_ci"]],
            "usd_saved_per_task": round(s["usd_saved_per_task"], 4), "usd_ci90": [round(x, 4) for x in s["usd_saved_ci"]],
            "usd_share": round(s["usd_saved_share"], 4), "usd_share_ci90": [round(x, 4) for x in s["usd_saved_share_ci"]],
            "sessions": s["sessions"], "tasks": s["tasks"], "test_set_id": s["test_set_id"],
            "directives": s["directives"], "useful_rate": s["useful_rate"], "useful_lb90": s["useful_lb90"],
            "harmful": s["harmful"], "s_hat_rust": s["s_hat_rust"], "s_hat_rust_ci95": s["s_hat_rust_ci95"]}


def main():
    e, e2, h, x, sh = j("eval.json"), j("eval2.json"), j("harness.json"), j("extras.json"), j("ship.json")
    best_split = hs(h["learned post-hoc split min_rate=0.5"])
    res = {
        "slug": "learned-outcomes",
        "headline": ("Success/failure CAN be learned from tool outputs with no hand-written strings (Drain templates "
                     "taught by the exit status: failure AUC 0.96 held-out, 0.95 leave-one-repo-out, vs 0.92 for the "
                     "hand-coded signatures), but it does not turn into savings: reflexes mined with it save "
                     "0.0-0.4 calls/task (<=0.9% of $) against U11's 0.9-1.6, and adding them to U11 lowers U11's "
                     "value; TD/MC value over trace states is at chance."),
        "metric_name": ("primary: $ and calls saved per task (common harness, cumulative-context cost model, 90% "
                        "task-clustered CI); instrument: failure AUC on 277 hand-labelled outputs"),
        "held_out": {
            "detector_failure_auc": {"value": e["failure_auc"]["drain→exit"]["auc"],
                                     "ci90": e["failure_auc"]["drain→exit"]["ci90"],
                                     "n_labels": e["n_labels"], "n_sessions": e["n_label_sessions"],
                                     "hand_coded_baseline": e["failure_auc"]["B1 hand-coded"],
                                     "is_error_baseline": e["failure_auc"]["B0 is_error"]},
            "detector_on_failures_the_exit_status_hides_auc": {
                "learned_drain_exit": e["failure_auc_is_error_false"]["drain→exit"],
                "learned_potion_exit_clean_teacher": e2["failure_auc_is_error_false"]["potion→exit (clean teacher)"],
                "hand_coded": e["failure_auc_is_error_false"]["B1 hand-coded"],
                "n_failures": e["n_failures_not_flagged"]},
            "success_auc": {"learned": e["success_auc"]["learned recover×(1−fail)"],
                            "hand_coded": e["success_auc"]["B1 hand-coded (executed>ok>failed)"]},
            "macro_f1_3class": e["macro_f1_3class"],
            "td_lambda_advantage_failure_auc": e["failure_auc"]["td-advantage"],
            "mc_advantage_failure_auc": e["failure_auc"]["mc-advantage"],
            "retry_self_supervision_failure_auc": e["failure_auc"]["retry-excess"],
            "downstream_learned_reflexes_split_post_hoc": best_split,
            "downstream_learned_reflexes_split_predictive": hs(h["learned predictive split min_rate=0.5"]),
        },
        "leave_one_repo_out": {
            "detector_failure_auc_pooled": e["loro_pooled_failure_auc"]["drain→exit"],
            "detector_failure_auc_pooled_hand_coded": e["loro_pooled_failure_auc"]["B1 hand-coded"],
            "detector_by_repo": {r: {"learned": v["drain→exit"], "hand_coded": v["B1 hand-coded"]}
                                 for r, v in e["loro_by_repo"].items()},
            "downstream_post_hoc_min_rate_0.7": hs(h["learned post-hoc loro min_rate=0.7"]),
            "downstream_post_hoc_min_rate_0.5": hs(h["learned post-hoc loro min_rate=0.5"]),
        },
        "vs_baseline": {
            "U11 post-hoc (harness, same test set)": hs(h["U11 post-hoc"]),
            "U11 predictive (split)": hs(h["U11 predictive split"]),
            "U11 predictive + learned post-hoc (split)": hs(x["U11 predictive + learned post-hoc split"]),
            "reading": ("The learned reflexes are worth less than U11 in every protocol and subtract from it when "
                        "added (1.10 vs 1.63 calls/task): their extra directives misfire (useful 0.57-0.77 vs U11 "
                        "0.95) and the dead ends they catch are mostly U11's."),
        },
        "hard_coded_parts": [
            "Training teacher: the harness's is_error flag (exit status) - data, not a rule.",
            "Piped-command test for the clean teacher: one shell-grammar regex (an unquoted '|' that is not '||').",
            "Tool name used as an opaque categorical feature (no taxonomy).",
            "Constants: k=3 retry window, cos>=0.9 'same action', cos>=0.8 'same intent', 15-call horizon, "
            "64 k-means clusters, reflex support>=2 sessions, recover rate>=0.5/0.7 (both reported), theta=0.5.",
            "Measurement only (never read by the learner): the hand labels (277, labelled by this agent), and the "
            "replay instrument (instrument.py = port of savings_replay.rs ENV_SIG/EXECUTED/ENV_PROBE/invocation; "
            "harness.features) for Sat/W and the hand-coded baseline.",
        ],
        "ceiling": {
            "oracle_delete_every_hand_coded_failed_call": x["oracle: hand-coded failed (is_error or env signature)"],
            "oracle_delete_every_learned_failure": x["oracle: learned failure (p_fail > 0.5, or is_error)"],
            "oracle_delete_learned_dead_end_windows (loose: includes productive edits between a failing and a "
            "passing run)": x["oracle: learned dead-end windows (failure → same-intent success)"],
            "reading": "~10-13% of $ for failed calls; the 19% window figure is a loose upper bound. Below the 15-20% goal.",
        },
        "ship": {"template_clusters": sh["template_clusters"], "dictionary_bytes": sh["template_dictionary_bytes_pickled"],
                 "classifier_weights_tiny": sh["tiny_classifier_weights"],
                 "latency_ms_tiny": sh["latency_ms_tiny"], "latency_ms_with_potion": sh["latency_ms_full"],
                 "note": sh["note"]},
        "build_recommendation": (
            "Do not build learned dead-end reflexes (negative: <=0.4 calls/task, subtracts from U11). Build the "
            "learned outcome detector only as a replacement for the hand-written signature lists where Adage labels "
            "outcomes (adage-learn outcome.rs env_signature/executed_tests, the replay's ENV_SIG): a Drain template "
            "dictionary (~14k templates, ~1.3 MB) + a logistic regression over hashed templates, taught by exit "
            "status on non-piped calls, ~12 ms/output in Python. Expected value: better labels (AUC 0.96 vs 0.92; "
            "0.85 vs 0.57 on piped failures) for any learner, $0 direct saving; its value is only what a downstream "
            "lever built on it earns. Ship the pre-emptive U11 rewrite first (+1.6 calls, +3.5% of $)."),
        "so_what": ("If shipped as reflexes: about +0.2 calls/task [-0.07, +0.42], +0.2% of $ [-0.3, +0.8] on Haiku "
                    "held-out (split), <0.5% of $ in any protocol; confidence it survives a live A/B as a positive "
                    "effect: low. As a labeller it is a better instrument, not a lever; ceiling for any "
                    "dead-end lever is ~10-13% of $."),
    }
    (HERE / "result.json").write_text(json.dumps(res, indent=1, ensure_ascii=False))
    print(json.dumps(res, indent=1, ensure_ascii=False)[:3000])


if __name__ == "__main__":
    main()
