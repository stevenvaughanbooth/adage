#!/usr/bin/env python3
"""The held-out decision record, rendered from the registered analysis (`heldout_analysis.analyze`).

The record follows the registered criteria in a fixed order, and every number in it comes from the
report:
1. the outcome;
2. the primary test (the one-sided 95% bootstrap upper bound of BEST/A0 total requests, at most 0.90);
3. quality (BEST passes at least A0's, less one);
4. each applicable G criterion, and each non-applicable one with its registered reason;
5. the sensitivity analyses (without censored blocks; without has-keys);
6. censoring;
7. the request cross-check;
8. the controls;
9. the report-only contrasts;
10. what this result does not establish.

Nothing here decides anything. An incomplete, unverified or protocol-failure outcome is said
plainly, and so is a prominent sensitivity warning.

Usage: python3 heldout_writeup.py <heldout report.json>   (heldout_cohort.py report writes both)
"""
import json
import sys
from pathlib import Path

import heldout_analysis as analysis

OUTCOMES = {
    "pass": "PASS: the primary test, quality and every applicable G criterion hold for this stratum.",
    "fail": "FAIL: at least one registered criterion does not hold.",
    "unverified": "UNVERIFIED: too many blocks have an unverified request count; no verdict.",
    "protocol-failure": "PROTOCOL FAILURE: a control was unrealized or skipped necessary work; no verdict.",
    "incomplete": "INCOMPLETE: not every assigned block was collected; no verdict.",
}


def _yes(v):
    return {True: "yes", False: "no", None: "n/a"}.get(v, str(v))


def _ratio_row(name, c):
    return "| %s | %s | %s | %s | %s | %s | %s | %s |" % (
        name, c.get("ratio"), c.get("lower"), c.get("upper"), c.get("blocks"), c.get("blocks_fewer"),
        c.get("sign_flip_p"), _yes(c.get("passed")))


def render(report):
    r, out = report, []
    best = r.get("best") or {}
    out += ["# Held-out decision record: Marshmallow stratum", "",
            "Protocol `launch-gate-1`, revisions 9 to 11; analysis `%s`. Stratum: %s." % (
                r.get("protocol"), r.get("stratum")),
            "BEST: `%s` (%s), from the development report `%s`." % (
                best.get("arm") or r.get("best_arm"), best.get("label", "?"), best.get("report", "?")),
            "Build commit `%s`; binaries %s." % (r.get("build_commit", "?"), json.dumps(r.get("binary_sha256", {}))),
            "Assignments %s, collected %s." % (r.get("assignments", "?"), r.get("collected", "?")), "",
            "## Outcome", "", OUTCOMES.get(r.get("outcome"), "UNKNOWN outcome: %s" % r.get("outcome")), ""]
    if r.get("valid") is False:
        out += ["**INVALID:** a BEST session ran with a lapsed grant. This report is not evidence.", ""]
    for p in r.get("lapses") or []:
        out.append("- lapsed grant: order %s (%s)" % (p.get("order"), p.get("problem")))
    for p in r.get("problems") or []:
        out.append("- problem: %s" % (p if isinstance(p, str) else json.dumps(p)))
    for w in r.get("warnings") or []:
        out.append("- **warning:** %s" % w)
    if r.get("outcome") == "incomplete":
        out += ["", "No criterion is evaluated until every assigned block is collected."]
        return "\n".join(out) + "\n"
    p, q = r["primary"], r["quality"]
    out += ["", "## Primary (registered)", "",
            "BEST/A0 total model requests over the included blocks. Pass: the one-sided 95%% bootstrap upper "
            "bound is at most %s." % p.get("threshold"), "",
            "| contrast | ratio | lower | upper | blocks | BEST fewer | sign-flip p | passed |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |", _ratio_row("BEST / A0", p),
            "", "Included blocks: %s." % p.get("included_blocks"), "",
            "## Quality (registered)", "",
            "BEST hidden-test passes %s; A0 %s; margin %s; passed: %s." % (
                q["best"], q["a0"], q["margin"], _yes(q["passed"])), "",
            "## G criteria", "", "| criterion | applies | rule | passed | values |", "| --- | --- | --- | --- | --- |"]
    for g, v in sorted(r["go_criteria"].items()):
        rule = v.get("form") if v.get("applies") else v.get("reason")
        values = {k: x for k, x in v.items() if k not in ("applies", "form", "reason", "passed")}
        out.append("| %s | %s | %s | %s | %s |" % (g, _yes(v.get("applies")), rule, _yes(v.get("passed")),
                                                  json.dumps(values) if values else ""))
    s = r["sensitivity"]
    out += ["", "## Sensitivity analyses (registered; reported, not gating)", "",
            "| analysis | ratio | lower | upper | blocks | BEST fewer | sign-flip p | passes the bound |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |",
            _ratio_row("without censored blocks", s["without_censored_blocks"]),
            _ratio_row("without has-keys", s["without_has_keys"]), "",
            "Blocks left out as censored: %s." % s["without_censored_blocks"].get("excluded_blocks"), "",
            "## Censoring", "",
            "A censored session counts as a quality failure. Only BEST is imputed upward, to %d requests." % analysis.CAP_REQUESTS,
            "", "| arm | sessions | censored | observed requests | requests (included) | mean (included) | "
            "hidden-test passes | verification requests | cost USD |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |"]
    for a, x in r["arms"].items():
        out.append("| %s | %s | %s | %s | %s | %s | %s | %s | %s |" % (
            a, x["sessions"], x["censored"], x["observed_requests"], x["requests_included"], x["mean_requests_included"],
            x["hidden_test_passes"], x["verification_requests"], x["cost_usd"]))
    v = r["request_verification"]
    out += ["", "## Request cross-check", "",
            "Blocks excluded for an unverified request count: %s (at most %s allowed)." % (
                v["excluded_blocks"], v["max_excluded"]), "",
            "## Controls (run first; all must be realized and skip no necessary work)", "",
            "| condition | collected | realized | skipped necessary work |", "| --- | --- | --- | --- |"]
    for c in r["controls"]:
        out.append("| %s | %s | %s | %s |" % (c["condition"], _yes(c["collected"]), _yes(c["realized"]),
                                             _yes(c["necessary_work_skipped"])))
    ro = r.get("report_only") or {}
    out += ["", "## Report only", "",
            "| contrast | ratio | lower | upper | blocks | first fewer | sign-flip p | under the bound |",
            "| --- | ---: | ---: | ---: | ---: | ---: | ---: | --- |"]
    for name, key in (("NOEXEC / A0", "noexec_vs_a0"), ("BEST / NOEXEC", "best_vs_noexec")):
        if key in ro:
            out.append(_ratio_row(name, ro[key]))
    out += ["", "## What this does not establish", "",
            "- **No GPU savings.** Model requests are not GPU measurements, and CPU work is reported separately.",
            "- **No other repositories.** This is a single stratum: same repository, same test-file family, and a "
            "guided learned route. G9 (the pipeline-unseen repository) is not run here.",
            "- **No launch readiness.** That needs the cross-repository decision.", ""]
    for n in r.get("notes") or []:
        out.append("- note: %s" % n)
    return "\n".join(out) + "\n"


if __name__ == "__main__":
    path = Path(sys.argv[1])
    print(render(json.loads(path.read_text())), end="")
