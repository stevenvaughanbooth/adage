#!/usr/bin/env python3
"""The Click held-out decision record, rendered from the registered r13 analysis (`click_analysis.analyze`).

Fixed order; every number comes from the report:
1. the two outcomes (advisory mode, execution mode), each its own claim;
2. advisory co-primary: NOEXEC/A0 bound, quality, its no-execution conditions;
3. execution co-primary: BEST/A0 bound, quality, each G criterion (or why it did not run);
4. the sensitivity analyses of each co-primary;
5. per-task tables;
6. censoring and the request cross-check;
7. the controls;
8. report-only contrasts;
9. what the result does not establish.

Nothing here decides anything. Usage: python3 click_writeup.py <click-heldout report.json>
"""
import json
import sys
from pathlib import Path

OUTCOMES = {
    "pass": "PASS: its bound, quality and every registered condition hold.",
    "fail": "FAIL: at least one registered criterion does not hold.",
    "unverified": "UNVERIFIED: too many blocks have an unverified request count; no verdict.",
    "protocol-failure": "PROTOCOL FAILURE: no verdict for this mode.",
    "incomplete": "INCOMPLETE: not every assigned block was collected; no verdict.",
    "not-run": "NOT RUN, as registered.",
}


def _yes(v):
    return {True: "yes", False: "no", None: "n/a"}.get(v, str(v))


def _row(name, c):
    c = c or {}
    return "| %s | %s | %s | %s | %s | %s | %s | %s |" % (
        name, c.get("ratio"), c.get("lower"), c.get("upper"), c.get("blocks"), c.get("blocks_fewer"),
        c.get("sign_flip_p"), _yes(c.get("passed")))


HEAD = ["| contrast | ratio | lower | upper | blocks | first fewer | sign-flip p | upper <= 0.90 |",
        "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |"]


def _contrast(out, name, c, adage):
    p, q = c["primary"], c["quality"]
    out += HEAD + [_row(name, p), "", "Included blocks: %s; excluded for an unverified count: %s." % (
        p.get("included_blocks"), c.get("excluded")), "",
        "Quality: %s hidden-test passes %s; A0 %s; margin %s; holds: %s." % (adage, q["adage"], q["a0"], q["margin"],
                                                                             _yes(q["passed"])), "",
        "Sensitivity (reported, not gating):", ""] + HEAD + [
        _row("without censored blocks", c["sensitivity"]["without_censored_blocks"]),
        _row("without key-value", c["sensitivity"]["without_key_value"]), ""]


def render(r):
    out = ["# Held-out decision record: Click stratum", "",
           "Protocol `launch-gate-1`, revision %s; analysis `%s`. Stratum: %s." % (r.get("revision"), r.get("protocol"),
                                                                                 r.get("stratum")),
           "Mode: **%s** (%s)." % (r.get("mode"), r.get("mode_reason")),
           "Learned home: %s. Route: `%s`; command `%s`." % (r.get("learned_provenance"), r.get("route"), r.get("command")),
           "Build commit `%s`; binaries %s." % (r.get("build_commit"), json.dumps(r.get("binary_sha256", {}))),
           "Assignments %s, collected %s." % (r.get("assignments"), r.get("collected")), "",
           "## Outcomes (two separate claims; never combined)", "",
           "- **Advisory mode (NOEXEC vs A0):** " + OUTCOMES.get(r["advisory"]["outcome"], r["advisory"]["outcome"]),
           "- **Execution mode (BEST vs A0):** " + OUTCOMES.get(r["execution"]["outcome"], r["execution"]["outcome"]), ""]
    if r.get("best_dropped"):
        out += ["BEST was dropped at order %s: %s" % (r["best_dropped"].get("order"), r["best_dropped"].get("reason")), ""]
    for p in r.get("problems") or []:
        out.append("- problem: %s" % p)
    for w in r.get("warnings") or []:
        out.append("- **warning:** %s" % w)
    for p in (r["execution"].get("problems") or []):
        out.append("- execution: %s" % p)
    a = r["advisory"]
    out += ["", "## Advisory co-primary (registered, gated)", ""]
    if "primary" in a:
        _contrast(out, "NOEXEC / A0", a, "NOEXEC")
    s = a.get("safety") or {}
    out += ["No execution in A0 or NOEXEC: %s (receipts or executions %s; execution contracts %s; unknown costs %s)." % (
        _yes(s.get("passed")), s.get("executions_or_receipts"), s.get("execution_contracts"), s.get("unknown_costs")), ""]
    e = r["execution"]
    out += ["## Execution co-primary (registered, gated)", ""]
    if "primary" in e:
        _contrast(out, "BEST / A0", e, "BEST")
        out += ["G criteria:", "", "| criterion | applies | rule | passed | values |", "| --- | --- | --- | --- | --- |"]
        for g, v in sorted(e["go_criteria"].items()):
            rule = v.get("form") if v.get("applies") else v.get("reason")
            values = {k: x for k, x in v.items() if k not in ("applies", "form", "reason", "passed")}
            out.append("| %s | %s | %s | %s | %s |" % (g, _yes(v.get("applies")), rule, _yes(v.get("passed")),
                                                      json.dumps(values) if values else ""))
        out.append("")
    else:
        out += [e.get("reason") or "; ".join(e.get("problems") or []) or OUTCOMES.get(e["outcome"], ""), ""]
    if r.get("per_task"):
        out += ["## Per task", ""]
        for task, row in r["per_task"].items():
            out += ["### " + task, "", "| arm | blocks | hidden passes | censored | requests | mean | cost USD | verification requests |",
                    "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
            for arm in ("A0", "NOEXEC", "BEST"):
                if arm in row:
                    x = row[arm]
                    out.append("| %s | %s | %s | %s | %s | %s | %s | %s |" % (
                        arm, x["blocks"], x["hidden_test_passes"], x["censored"], x["observed_requests"],
                        x["mean_requests"], x["cost_usd"], x["verification_requests"]))
            ratios = ["%s %s (%s blocks fewer)" % (k, v["ratio"], v["blocks_fewer"]) for k, v in row.items() if "/" in k]
            out += ["", "Ratios within the task (report only): " + "; ".join(ratios), ""]
    if r.get("arms"):
        out += ["## Arms, censoring and costs", "",
                "A censored session counts as a quality failure; only the Adage arm under test is imputed upward.", "",
                "| arm | sessions | censored | observed requests | hidden passes | verification requests | cost USD |",
                "| --- | ---: | ---: | ---: | ---: | ---: | ---: |"]
        for arm, x in r["arms"].items():
            out.append("| %s | %s | %s | %s | %s | %s | %s |" % (arm, x["sessions"], x["censored"], x["observed_requests"],
                                                             x["hidden_test_passes"], x["verification_requests"], x["cost_usd"]))
        out.append("")
    unverified = [v for v in r.get("request_verification") or [] if not v.get("verified")]
    out += ["## Request cross-check", "", "Sessions with an unverified request count: %s." % (
        [(v["order"], v["arm"]) for v in unverified] or "none"), ""]
    controls = e.get("controls") or []
    if controls:
        out += ["## Controls (run first; all must be realized and skip no necessary work)", "",
                "| condition | collected | realized | skipped necessary work |", "| --- | --- | --- | --- |"]
        for c in controls:
            out.append("| %s | %s | %s | %s |" % (c["condition"], _yes(c["collected"]), _yes(c["realized"]),
                                                 _yes(c["necessary_work_skipped"])))
        out.append("")
    ro = (e.get("report_only") or {}).get("best_vs_noexec")
    if ro:
        out += ["## Report only", ""] + HEAD + [_row("BEST / NOEXEC", ro), ""]
    out += ["## What this does not establish", "",
            "- **No GPU savings.** Model requests are not GPU measurements; CPU work is reported separately.",
            "- **No other repositories.** One stratum: same repository, same test-file family. G9 (the pipeline-unseen "
            "repository) is not run here.",
            "- **No combined claim.** Advisory and execution mode are separate claims; neither result speaks for the other.",
            "- **No launch readiness by itself.** That needs the cross-repository decision.", ""]
    for n in r.get("notes") or []:
        out.append("- note: %s" % n)
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    print(render(json.loads(Path(sys.argv[1]).read_text())), end="")
