#!/usr/bin/env python3
"""Pre-registered held-out analysis for the Click stratum, launch-gate-1 revision 13. Offline and pure.

The r9 rules (`heldout_analysis`), applied to Click, with two co-primaries this time:

- **Execution mode (BEST vs A0).** Unchanged from r9: the one-sided 95% upper bound of sum(BEST)/sum(A0)
  model requests over the included blocks (percentile bootstrap over blocks, stratified by task, 20,000
  draws, seed 20261001) is at most 0.90; BEST's hidden-test passes are at least A0's minus one; every
  applicable G criterion holds (G1, G2, G5, G6, G7, G8; G3, G4 and G9 are not applicable, with the r9
  reasons); all five controls are realized and skip no necessary verification. It runs only when the
  learning step selected a route (`mode == "execution"`).
- **Advisory mode (NOEXEC vs A0), gated this time.** Its own bound: the one-sided 95% upper bound of
  sum(NOEXEC)/sum(A0) at most 0.90 (same bootstrap), NOEXEC's hidden-test passes at least A0's minus
  one, and no execution: zero receipts or executions in NOEXEC and A0 sessions, zero execution
  contracts in them, and zero unknown costs. It needs no route and no control, so it is evaluated in
  both modes.

Each co-primary is a separate claim about a separate mode. The report never combines them ("either
passes"): the advisory claim rests only on the advisory outcome, the execution claim only on the
execution outcome. Each is tested at one-sided 95% because each supports its own launch statement.

Shared rules, as r9:
- Primary metric: model requests per session, the debug log's `[API REQUEST] /v1/messages` lines
  (retries count), cross-checked per session against the stream's request ids; an unexplained mismatch
  removes the session's block from that contrast (any arm of the contrast); more than 2 excluded
  blocks make that contrast `unverified`.
- Censoring, conservative against Adage: a censored session is a quality failure in every arm. For
  requests, the Adage arm under test counts max(observed, 60) (BEST in the execution contrast, NOEXEC
  in the advisory contrast); A0 counts its observed requests. In G8 (BEST against NOEXEC), only BEST is
  imputed, as r9.
- Sensitivity (reported, never gating): each co-primary without blocks that contain a censored session,
  and without the key-value blocks (its nearest training neighbour is host-port). If a co-primary passes
  and a sensitivity analysis fails, the report says so prominently.
- Per-task tables for every arm and both contrasts. Report only: BEST/NOEXEC, sign-flip p-values,
  verification requests, tokens, cost, wall time and CPU work. Model requests are never GPU savings.
- If BEST is dropped during the study (a control unrealized or unsafe, or a lapsed grant), the
  execution outcome is `protocol-failure`, no further BEST session is dispatched, and the advisory
  co-primary is still evaluated on A0 and NOEXEC.
"""
from __future__ import annotations

import argparse
import fnmatch
import json
import random
from pathlib import Path

import heldout_analysis as r9

PROTOCOL = "click-heldout-analysis-1"
REVISION = 13
STRATUM = "same repository (Click), same test-file family, guided-learned route if one certified"
MODES = ("execution", "advisory")
ARMS = {"execution": ("A0", "NOEXEC", "BEST"), "advisory": ("A0", "NOEXEC")}
CONTROLS = r9.CONTROLS
SENSITIVITY_TASK = "click-type--key-value"
REPETITIONS = r9.REPETITIONS
MAX_TURNS = r9.MAX_TURNS
CAP_REQUESTS = r9.CAP_REQUESTS
SEED = r9.SEED
BOOTSTRAP = r9.BOOTSTRAP
RATIO_MAX = r9.RATIO_MAX
QUALITY_MARGIN = r9.QUALITY_MARGIN
MAX_EXCLUDED_BLOCKS = r9.MAX_EXCLUDED_BLOCKS
SESSION_CAP_USD = 0.60
NATIVE_CAP_USD = 0.50
TRAINING_SESSIONS, TRAINING_CAP_USD = 10, 0.60
LEDGER_FLOOR_USD = 3.0
# Click's pytest writes bytecode beside every collected test package (tests/, tests/test_types/, ...).
CACHES = (".pytest_cache/**", "tests/__pycache__/**", "tests/**/__pycache__/**", "src/**/__pycache__/**",
          ".pytest_cache/.gitignore")
# Expected cost per session, for planning only (never a rule): Click's five v5 Haiku sessions averaged
# $0.452 (max $0.528) before the realism line and the project environment; Marshmallow's no-Adage arm
# fell from v5's $0.304 to $0.201 at held-out (x0.66), and NOEXEC and BEST cost x0.67 of A0 there.
EXPECTED_USD = dict(A0=0.30, NOEXEC=0.20, BEST=0.20, control=0.22, training=0.35, native=0.03)
PESSIMISTIC_USD = 0.452
GO_CRITERIA = dict(r9.GO_CRITERIA)
GO_CRITERIA["G1"] = dict(applies=True, form="single repository: the frozen learned home holds the selected certified "
                         "verification route (checked at freeze); the two-repository clause belongs to the "
                         "cross-repository decision")
GO_CRITERIA["G9"] = dict(applies=False, reason="the pipeline-unseen repository (more-itertools) is not run in this "
                         "stratum; G9 stays required for any claim that covers it")


def matrix(tasks, mode, seed=SEED, repetitions=REPETITIONS, control_task="click-type--log-level"):
    """Execution mode: the five BEST controls first, then repetition-major paired blocks of A0, NOEXEC and
    BEST in a seeded random order. Advisory mode: the blocks only, with A0 and NOEXEC."""
    if mode not in MODES:
        raise ValueError("mode is execution or advisory")
    if len(tasks) < 4 or len(set(tasks)) != len(tasks):
        raise ValueError("at least four distinct held-out tasks")
    rng = random.Random(seed)
    rows = [dict(order=i, arm="BEST", condition=c, block=None, task=control_task, repetition=None)
            for i, c in enumerate(CONTROLS)] if mode == "execution" else []
    block = 0
    for repetition in range(1, repetitions + 1):
        for task in tasks:
            block += 1
            arms = list(ARMS[mode])
            rng.shuffle(arms)
            for arm in arms:
                rows.append(dict(order=len(rows), arm=arm, condition="positive", block=block, task=task,
                                 repetition=repetition))
    return rows


def budget(remaining_usd, cap=SESSION_CAP_USD, training=TRAINING_SESSIONS, training_cap=TRAINING_CAP_USD,
           native=NATIVE_CAP_USD, floor=LEDGER_FLOOR_USD, tasks=4, repetitions=REPETITIONS):
    """Worst and expected cost of the whole chain (native, training, held-out) per mode, against the
    ledger's remaining balance less the floor. Every session is reserved at its cap before dispatch."""
    available = round(remaining_usd - floor, 7)
    blocks = tasks * repetitions
    sessions = {"execution": blocks * 3 + len(CONTROLS), "advisory": blocks * 2}
    out = dict(remaining_usd=round(remaining_usd, 7), floor_usd=floor, available_usd=available,
               caps=dict(native=native, training=training_cap, heldout=cap), modes={})
    chain_before = round(native + training * training_cap, 4)
    for mode in MODES:
        heldout = round(sessions[mode] * cap, 4)
        expected = (blocks * (EXPECTED_USD["A0"] + EXPECTED_USD["NOEXEC"])
                    + (blocks * EXPECTED_USD["BEST"] + len(CONTROLS) * EXPECTED_USD["control"] if mode == "execution" else 0))
        expected_chain = EXPECTED_USD["native"] + training * EXPECTED_USD["training"] + expected
        pessimistic = (sessions[mode] + training) * PESSIMISTIC_USD + EXPECTED_USD["native"]
        worst = round(chain_before + heldout, 4)
        out["modes"][mode] = dict(heldout_sessions=sessions[mode], heldout_worst_usd=heldout,
                                  chain_worst_usd=worst, fits=worst <= available + 1e-9,
                                  shortfall_usd=round(max(0.0, worst - available), 7),
                                  ceiling_needed_usd=round(60 + max(0.0, worst - available), 7),
                                  heldout_expected_usd=round(expected, 2), chain_expected_usd=round(expected_chain, 2),
                                  chain_expected_at_v5_mean_usd=round(pessimistic, 2),
                                  max_cap_that_fits_usd=round(max(0.0, available - chain_before) / sessions[mode], 4))
    out["training_and_native_worst_usd"] = chain_before
    return out


# ------------------------------------------------------------------------------------- analysis

def _effective(row, arm, imputed):
    """r9's censoring rule with the imputed arm named: only the Adage arm under test goes up."""
    if r9.censored(row):
        observed = row.get("requests") or 0
        return dict(requests=max(observed, CAP_REQUESTS) if arm == imputed else observed, passed=False, censored=True)
    return dict(requests=row["requests"], passed=row.get("quality_passed") is True, censored=False)


def _blocks(rows, arms):
    blocks = {}
    for r in rows:
        if r["condition"] == "positive" and r["arm"] in arms:
            blocks.setdefault(r["block"], dict(task=r["task"]))[r["arm"]] = r
    return blocks


def _receipts(row):
    return r9._receipts(row)


def _declared(path):
    return any(fnmatch.fnmatch(path, glob) for glob in CACHES)


def contrast(blocks, x, y, imputed, exclusion_arms=None, seed=SEED, draws=BOOTSTRAP):
    """One co-primary: the bootstrap bound, quality, censoring and the registered sensitivity analyses.
    A block leaves the ratio when a session of any of `exclusion_arms` (default: the two compared arms)
    has an unverified request count (r9: the whole paired block leaves, symmetrically)."""
    eff = {n: {a: _effective(b[a], a, imputed) for a in (x, y)} for n, b in blocks.items()}
    excluded = sorted(n for n, b in blocks.items() if not all(b[a].get("verified") for a in (exclusion_arms or (x, y))))
    included = [n for n in sorted(blocks) if n not in excluded]
    pick = lambda numbers: {n: dict(task=blocks[n]["task"], **eff[n]) for n in numbers}  # noqa: E731
    primary = r9.compare(pick(included), x, y, seed, draws)
    no_censored = [n for n in included if not any(eff[n][a]["censored"] for a in (x, y))]
    no_task = [n for n in included if blocks[n]["task"] != SENSITIVITY_TASK]
    passes = {a: sum(eff[n][a]["passed"] for n in blocks) for a in (x, y)}
    return dict(contrast="%s/%s" % (x, y), imputed_arm=imputed, eff=eff, excluded=excluded, included=included,
                primary=dict(primary, threshold=RATIO_MAX, included_blocks=len(included)),
                quality=dict(adage=passes[x], a0=passes[y], margin=QUALITY_MARGIN,
                             passed=passes[x] >= passes[y] - QUALITY_MARGIN),
                sensitivity=dict(without_censored_blocks=dict(r9.compare(pick(no_censored), x, y, seed, draws),
                                                              excluded_blocks=[n for n in included if n not in no_censored]),
                                 without_key_value=r9.compare(pick(no_task), x, y, seed, draws)))


def session_row(session, assignment):
    """r9's analysis row, plus whether the session's SessionStart emitted an execution contract."""
    row = r9.session_row(session, assignment)
    path = Path(session) / "collection.json"
    row["contract_emitted"] = json.loads(path.read_text()).get("contract_emitted") if path.exists() else None
    return row


def warnings_of(name, c):
    out = []
    if c["primary"]["passed"] and not c["sensitivity"]["without_key_value"]["passed"]:
        out.append("PROMINENT (%s): the co-primary passes, but the sensitivity analysis without the key-value blocks "
                   "fails (upper bound %s > %.2f)." % (name, c["sensitivity"]["without_key_value"]["upper"], RATIO_MAX))
    if c["primary"]["passed"] and not c["sensitivity"]["without_censored_blocks"]["passed"]:
        out.append("%s: the co-primary passes, but the sensitivity analysis without censored blocks fails." % name)
    return out


def go_criteria(rows, blocks, included, eff, route):
    """G1-G9 for the execution co-primary (r9's forms; this repository's declared caches)."""
    best = [r for r in rows if r["arm"] == "BEST" and r["condition"] == "positive"]
    hooks_without_grant = [r for r in rows if r["arm"] in ("A0", "NOEXEC") and (r.get("receipts") or _receipts(r))]
    fired = [r for r in best if _receipts(r)]
    succeeded = [r for r in fired if any(r9._ok(e) for e in _receipts(r))]
    foreign = [r.get("order") for r in rows if r["arm"] == "BEST" for g in r.get("receipts") or []
               if (route and g.get("route") != route) or (r.get("session_id") and g.get("session") not in (None, r["session_id"]))]
    undeclared = sorted({c.get("path") for r in rows for g in r.get("receipts") or []
                         for c in (g.get("outcome") or {}).get("changed", []) if not _declared(c.get("path") or "")})
    skipped = [r.get("order") for r in rows if (r.get("replay") or {}).get("necessary_work_skipped")]
    unknown = [r.get("order") for r in rows if r.get("cost_usd") is None]
    executions = [(e, r) for r in best for e in _receipts(r) if r9._ok(e)]
    unnecessary = [e for e, r in executions if e.get("digest") != (r.get("timeline") or {}).get("final_digest")]
    failures = {a: sum(not eff[n][a]["passed"] for n in blocks) for a in ("A0", "NOEXEC", "BEST")}
    req = {a: sum(eff[n][a]["requests"] for n in included) for a in ("NOEXEC", "BEST")}
    cost = {a: sum(blocks[n][a].get("cost_usd") or 0 for n in included) for a in ("NOEXEC", "BEST")}
    values = {
        "G1": dict(passed=True if route else None, note="checked at freeze (route present, isolation audit passed)"),
        "G2": dict(fired=len(fired), sessions=len(best), successful=len(succeeded),
                   passed=bool(best) and len(fired) >= r9.G2_FIRED_MIN * len(best)
                   and len(succeeded) >= r9.G2_SUCCESS_MIN * len(fired)),
        "G5": dict(failures=failures, passed=failures["BEST"] <= min(failures["A0"], failures["NOEXEC"])),
        "G6": dict(necessary_work_skipped=skipped, executions_without_grant=[r.get("order") for r in hooks_without_grant],
                   foreign_or_unapproved_receipts=foreign, undeclared_writes=undeclared, unknown_costs=unknown,
                   passed=not (skipped or hooks_without_grant or foreign or undeclared or unknown)),
        "G7": dict(executions=len(executions), unnecessary=len(unnecessary),
                   passed=len(unnecessary) <= r9.UNNECESSARY_MAX * len(executions)),
        "G8": dict(requests=req, cost_usd={a: round(v, 6) for a, v in cost.items()},
                   passed=req["BEST"] <= req["NOEXEC"] and cost["BEST"] <= r9.G8_COST_MAX_RATIO * cost["NOEXEC"] + 1e-9),
    }
    return {g: dict(spec, **values.get(g, dict(passed=None))) for g, spec in GO_CRITERIA.items()}


def advisory_safety(rows):
    """The advisory co-primary's own conditions: no execution and known costs in A0 and NOEXEC."""
    plain = [r for r in rows if r["arm"] in ("A0", "NOEXEC") and r["condition"] == "positive"]
    executed = [r.get("order") for r in plain if r.get("receipts") or _receipts(r)]
    contract = [r.get("order") for r in plain if r.get("contract_emitted")]
    unknown = [r.get("order") for r in plain if r.get("cost_usd") is None]
    return dict(executions_or_receipts=executed, execution_contracts=contract, unknown_costs=unknown,
                passed=not (executed or contract or unknown))


def per_task(blocks, arms, effs):
    """Per task and arm: blocks, hidden-test passes, censored sessions, observed requests, cost and
    verification requests; and each contrast's ratio within the task (report only)."""
    out = {}
    for task in sorted({b["task"] for b in blocks.values()}):
        numbers = [n for n, b in blocks.items() if b["task"] == task]
        row = {}
        for a in arms:
            sessions = [blocks[n][a] for n in numbers]
            row[a] = dict(blocks=len(numbers), hidden_test_passes=sum(s.get("quality_passed") is True and not r9.censored(s)
                                                                      for s in sessions),
                          censored=sum(r9.censored(s) for s in sessions),
                          observed_requests=sum(s.get("requests") or 0 for s in sessions),
                          mean_requests=round(sum(s.get("requests") or 0 for s in sessions) / len(sessions), 2),
                          cost_usd=round(sum(s.get("cost_usd") or 0 for s in sessions), 6),
                          verification_requests=sum(s.get("verification_requests") or 0 for s in sessions))
        for name, (x, y, eff) in effs.items():
            num = sum(eff[n][x]["requests"] for n in numbers if n in eff)
            den = sum(eff[n][y]["requests"] for n in numbers if n in eff)
            row[name] = dict(ratio=round(num / den, 4) if den else None,
                             blocks_fewer=sum(eff[n][x]["requests"] < eff[n][y]["requests"] for n in numbers if n in eff))
        out[task] = row
    return out


def outcome_of(c, complete, extra_ok=True, protocol_failure=False):
    if not complete:
        return "incomplete"
    if protocol_failure:
        return "protocol-failure"
    if len(c["excluded"]) > MAX_EXCLUDED_BLOCKS:
        return "unverified"
    return "pass" if c["primary"]["passed"] and c["quality"]["passed"] and extra_ok else "fail"


def _public(c):
    return {k: v for k, v in c.items() if k not in ("eff", "included")}


def analyze(rows, mode, route=None, best_dropped=None, seed=SEED, draws=BOOTSTRAP):
    """The registered verdicts of both co-primaries. Every assigned row counts; nothing is rerun."""
    if mode not in MODES:
        raise ValueError("mode is execution or advisory")
    out = dict(protocol=PROTOCOL, revision=REVISION, stratum=STRATUM, mode=mode, route=route, problems=[], warnings=[],
               best_dropped=best_dropped,
               notes=["Each co-primary is its own claim: advisory mode (NOEXEC vs A0) and execution mode (BEST vs A0). "
                      "No claim is made from either one passing.",
                      "Model requests are never presented as GPU savings; CPU work is reported separately.",
                      "Censoring: every censored session is a quality failure; only the Adage arm under test is imputed "
                      "upward (to %d requests)." % CAP_REQUESTS])
    plain = _blocks(rows, ("A0", "NOEXEC"))
    tasks = sorted({b["task"] for b in plain.values()})
    complete_plain = (not [n for n, b in plain.items() if any(a not in b or b[a].get("status") != "collected" for a in ("A0", "NOEXEC"))]
                      and len(tasks) >= 4 and all(sum(b["task"] == t for b in plain.values()) >= REPETITIONS for t in tasks))
    if not complete_plain:
        out["problems"].append("incomplete A0/NOEXEC blocks")
    # Advisory co-primary (gated in both modes).
    adv = contrast(plain, "NOEXEC", "A0", imputed="NOEXEC", seed=seed, draws=draws) if complete_plain else None
    safety = advisory_safety(rows)
    out["advisory"] = (dict(_public(adv), safety=safety, outcome=outcome_of(adv, True, safety["passed"]))
                       if adv else dict(outcome="incomplete", safety=safety))
    if adv:
        out["warnings"] += warnings_of("advisory", adv)
        if len(adv["excluded"]) > MAX_EXCLUDED_BLOCKS:
            out["problems"].append("advisory: %d of %d blocks unverified (more than %d)" % (
                len(adv["excluded"]), len(plain), MAX_EXCLUDED_BLOCKS))
    effs = {"NOEXEC/A0": ("NOEXEC", "A0", adv["eff"])} if adv else {}
    arms = ("A0", "NOEXEC")
    # Execution co-primary.
    if mode == "advisory":
        out["execution"] = dict(outcome="not-run", reason="no certified route fit the registered rule; BEST and its "
                                                          "controls were skipped, as pre-registered")
    else:
        arms = ("A0", "NOEXEC", "BEST")
        controls = r9.control_rows([r for r in rows if r["condition"] != "positive"])
        failures = []
        for c in controls:
            if not c["realized"]:
                failures.append("control %s unrealized (protocol failure)" % c["condition"])
            if c["necessary_work_skipped"]:
                failures.append("control %s skipped necessary verification (protocol failure)" % c["condition"])
        if sorted(c["condition"] for c in controls) != sorted(CONTROLS):
            failures.append("the five registered controls are not all assigned (protocol failure)")
        if best_dropped:
            failures.append("BEST dropped at order %s: %s (protocol failure)" % (best_dropped.get("order"), best_dropped.get("reason")))
        full = _blocks(rows, arms)
        complete = complete_plain and not [n for n, b in full.items() if "BEST" not in b or b["BEST"].get("status") != "collected"]
        if best_dropped:
            out["execution"] = dict(outcome="protocol-failure", problems=failures, controls=controls)
        elif not complete:
            # As r9: no criterion is evaluated until every assigned session is collected.
            out["execution"] = dict(outcome="incomplete", problems=["incomplete BEST blocks"] + failures, controls=controls)
        elif failures:
            out["execution"] = dict(outcome="protocol-failure", problems=failures, controls=controls)
        else:
            ex = contrast(full, "BEST", "A0", imputed="BEST", exclusion_arms=arms, seed=seed, draws=draws)
            eff = {n: {a: _effective(b[a], a, "BEST") for a in arms} for n, b in full.items()}
            pick = lambda numbers: {n: dict(task=full[n]["task"], **eff[n]) for n in numbers}  # noqa: E731
            criteria = go_criteria(rows, full, ex["included"], eff, route)
            applicable_ok = all(v["passed"] is not False for v in criteria.values() if v["applies"])
            out["execution"] = dict(_public(ex), go_criteria=criteria, controls=controls,
                                    outcome=outcome_of(ex, True, applicable_ok),
                                    report_only=dict(best_vs_noexec=r9.compare(pick(ex["included"]), "BEST", "NOEXEC", seed, draws)))
            out["warnings"] += warnings_of("execution", ex)
            effs["BEST/A0"] = ("BEST", "A0", eff)
            effs["BEST/NOEXEC"] = ("BEST", "NOEXEC", eff)
    blocks = _blocks(rows, arms)
    if blocks:
        out["per_task"] = per_task({n: b for n, b in blocks.items() if all(a in b for a in arms)}, arms, effs)
        out["arms"] = {a: dict(sessions=sum(a in b for b in blocks.values()),
                               observed_requests=sum((b[a].get("requests") or 0) for b in blocks.values() if a in b),
                               hidden_test_passes=sum(b[a].get("quality_passed") is True and not r9.censored(b[a])
                                                      for b in blocks.values() if a in b),
                               censored=sum(r9.censored(b[a]) for b in blocks.values() if a in b),
                               verification_requests=sum((b[a].get("verification_requests") or 0) for b in blocks.values() if a in b),
                               cost_usd=round(sum((b[a].get("cost_usd") or 0) for b in blocks.values() if a in b), 6))
                       for a in arms}
    out["request_verification"] = [dict(order=b[a].get("order"), block=n, arm=a, requests=b[a].get("requests"),
                                        stream_request_ids=b[a].get("stream_request_ids"),
                                        api_error_entries=b[a].get("api_error_entries"), verified=b[a].get("verified"))
                                   for n, b in sorted(blocks.items()) for a in arms if a in b]
    out["outcomes"] = dict(advisory=out["advisory"]["outcome"], execution=out["execution"]["outcome"])
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("matrix", "budget", "analyze"))
    parser.add_argument("path", type=Path, nargs="?")
    parser.add_argument("--mode", choices=MODES, default="execution")
    parser.add_argument("--remaining", type=float)
    args = parser.parse_args()
    if args.command == "matrix":
        import click_tasks
        value = matrix(click_tasks.EVALUATION, args.mode)
    elif args.command == "budget":
        value = budget(args.remaining)
    else:
        data = json.loads(args.path.read_text())
        value = analyze(data["rows"], data.get("mode", args.mode), data.get("route"), data.get("best_dropped"))
    print(json.dumps(value, indent=2, default=str))


if __name__ == "__main__":
    main()
