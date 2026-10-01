#!/usr/bin/env python3
"""Registered GO/NO-GO scoring for launch-gate-1 held-out rows. Offline; thresholds from the JSON.

A row is one collected held-out session: repo, task, arm (agent|noexec|automatic), repetition,
quality_passed, cost_usd, model_requests, executions (journal receipts), undeclared_writes,
fresh_receipt (a successful receipt whose digest equals the final source digest), stale_receipts
(successful receipts on a superseded digest) and `replay` (receipt_replay.replay output).
Missing or unknown values fail their criterion; nothing is imputed.
"""
import argparse
import json
from pathlib import Path
import shlex

import receipt_replay as rr

PROTOCOL = Path(__file__).with_name("launch_gate_protocol.json")


def equivalent(segment, command, root=""):
    """The registered equivalence: same interpreter, non-output flags and targets."""
    a, b = rr.pytest_call(shlex.split(segment), root), rr.pytest_call(shlex.split(command), root)
    return a is not None and a == b


def ran_equivalent(row, command):
    return any(q.get("segment") and equivalent(q["segment"], command) for q in row["replay"].get("requests", []))


def score(rows, reflexes, protocol=None):
    protocol = protocol or json.loads(PROTOCOL.read_text())
    t = protocol["go_thresholds"]
    learned = sorted(r for r, v in reflexes.items() if v.get("certified"))
    arm = lambda name, repos=None: [r for r in rows if r["arm"] == name and (repos is None or r["repo"] in repos)]
    auto, noexec = arm("automatic", learned), arm("noexec", learned)
    fired = [r for r in auto if r.get("executions")]
    succeeded = [r for r in fired if r["replay"].get("successful_receipts")]
    fresh = [r for r in auto if r.get("fresh_receipt")]
    eliminated = [r for r in auto if r["replay"].get("confirmed_elimination")]
    per_repo = {repo: sum(r["repo"] == repo for r in eliminated) for repo in learned}
    noexec_equivalent = [r for r in noexec if ran_equivalent(r, reflexes[r["repo"]]["command"])]
    repeating = [r for r in fresh if r["replay"].get("redundant_repeats")]
    failures = {name: sum(r.get("quality_passed") is not True for r in arm(name)) for name in ("agent", "noexec", "automatic")}
    executions = sum(r.get("executions") or 0 for r in auto)
    stale = sum(r.get("stale_receipts") or 0 for r in auto)
    total = lambda rs, key: sum(r[key] for r in rs) if all(isinstance(r.get(key), (int, float)) for r in rs) else None
    frac = lambda a, b: len(a) / len(b) if b else 0.0
    requests = (total(auto, "model_requests"), total(noexec, "model_requests"))
    costs = (total(auto, "cost_usd"), total(noexec, "cost_usd"))
    ne_frac, rep_frac = frac(noexec_equivalent, noexec), frac(repeating, fresh)
    criteria = dict(
        G1=dict(value=len(learned), passed=len(learned) >= t["G1_learned_reflex_repositories_min"]),
        G2=dict(fired=frac(fired, auto), succeeded=frac(succeeded, fired),
                passed=bool(auto) and frac(fired, auto) >= t["G2_context_fired_fraction_of_automatic_sessions_min"]
                and frac(succeeded, fired) >= t["G2_successful_receipt_fraction_of_fired_min"]),
        G3=dict(total=len(eliminated), per_repository=per_repo, rate=frac(eliminated, fresh),
                passed=len(eliminated) >= t["G3_confirmed_eliminations_total_min"]
                and sum(n >= 2 for n in per_repo.values()) >= t["G3_repositories_with_at_least_2_eliminations_min"]
                and frac(eliminated, fresh) >= t["G3_elimination_rate_among_fresh_receipt_sessions_min"]),
        G4=dict(noexec_equivalent_fraction=ne_frac, automatic_repeat_fraction=rep_frac,
                passed=bool(noexec) and bool(fresh) and ne_frac >= t["G4_noexec_equivalent_command_session_fraction_min"]
                and rep_frac <= t["G4_automatic_repeat_session_fraction_max_ratio_to_noexec"] * ne_frac),
        G5=dict(failures=failures, passed=failures["automatic"] <= min(failures["agent"], failures["noexec"])),
        G6=dict(necessary_work_skipped=sum(bool(r["replay"].get("necessary_work_skipped")) for r in rows),
                non_automatic_executions=sum(r.get("executions") or 0 for r in rows if r["arm"] != "automatic"),
                undeclared_writes=sum(bool(r.get("undeclared_writes")) for r in rows),
                unknown_costs=sum(not isinstance(r.get("cost_usd"), (int, float)) for r in rows)),
        G7=dict(unnecessary=stale, executions=executions,
                passed=executions > 0 and stale / executions <= t["G7_unnecessary_execution_fraction_max"]),
        G8=dict(model_requests=requests, cost_usd=costs,
                passed=None not in requests + costs and requests[0] <= t["G8_automatic_model_requests_max_ratio_to_noexec"] * requests[1]
                and costs[0] <= t["G8_automatic_cost_max_ratio_to_noexec"] * costs[1]),
        G9=dict(repository=t["G9_pipeline_unseen_repository"], eliminations=per_repo.get(t["G9_pipeline_unseen_repository"], 0),
                passed=per_repo.get(t["G9_pipeline_unseen_repository"], 0) >= t["G9_pipeline_unseen_confirmed_eliminations_min"]))
    g6 = criteria["G6"]
    g6["passed"] = not any(g6[k] for k in ("necessary_work_skipped", "non_automatic_executions", "undeclared_writes", "unknown_costs"))
    strata = {}
    for repo, spec in protocol["repositories"].items():
        group = strata.setdefault(spec.get("stratum", "unassigned"), dict(repositories=[], eliminations=0, automatic=0))
        group["repositories"].append(repo)
        group["eliminations"] += per_repo.get(repo, 0)
        group["automatic"] += sum(r["repo"] == repo for r in auto)
    return dict(go=all(c["passed"] for c in criteria.values()), criteria=criteria, sessions=len(rows), strata=strata,
                learned_repositories=learned, provenance={r: reflexes[r].get("provenance") for r in reflexes},
                note="Model requests and cost are API measurements, not GPU utilization or energy.")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("rows", type=Path, help="JSON list of collected rows")
    parser.add_argument("reflexes", type=Path, help="JSON map repo -> {certified, provenance, command}")
    args = parser.parse_args()
    print(json.dumps(score(json.loads(args.rows.read_text()), json.loads(args.reflexes.read_text())), indent=2))
