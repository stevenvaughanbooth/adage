#!/usr/bin/env python3
"""Synthetic held-out reports for the site's tests: NOT study results.

Each fixture is the registered analysis (`heldout_analysis.analyze`, the code `heldout_cohort.py
report` runs) applied to made-up session rows, wrapped the way `heldout_cohort.report` wraps it, so
the site's converter and page are tested against the report's real shape:

- pass.json: Adage needs far fewer requests and loses no quality (outcome "pass", GO);
- fail.json: a small, uncertain difference and one more hidden-test failure (outcome "fail");
- unverified.json: three blocks with unexplained request counts (outcome "unverified", no verdict).

Usage, from the repository root: python3 web/site/e2e/fixtures/heldout/make.py
"""
import json
import random
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
sys.path.insert(0, str(ROOT / "evals/reflex-proof/harness"))
import heldout_analysis as analysis  # noqa: E402

TASKS = ["mm-curve--has-keys", "mm-curve--iso-date", "mm-curve--json-object", "mm-curve--lat-long"]
ROUTE = "github-com-marshmallow-code-marshmallow/learned/run-175d85"


def receipt(digest, outcome="applied"):
    return dict(kind="receipt", origin="context", phase="completed", outcome=outcome, digest=digest)


def control(order, condition):
    final = "f%d" % order
    events = {"failed": [receipt("d0", "failed")], "later-edit": [receipt("d0")], "config-change": [receipt("d0")]}
    return dict(order=order, arm="BEST", condition=condition, block=None, task="mm-curve--finite", repetition=None,
                status="collected", result_status="success", result_is_error=False, quality_passed=True, cost_usd=0.08,
                requests=12, verified=True, receipts=[], replay=dict(necessary_work_skipped=False, checked=True),
                timeline=dict(final_digest=final, events=events.get(condition, [receipt(final)])))


def rows(means, failures=(), unverified=(), seed=7):
    rng = random.Random(seed)
    out = [control(i, c) for i, c in enumerate(analysis.CONTROLS)]
    for m in analysis.matrix(TASKS):
        if m["condition"] != "positive":
            continue
        arm, block = m["arm"], m["block"]
        final = "b%d-%s" % (block, arm)
        mean, spread, cost = means[arm]
        events = [receipt(final)] if arm == "BEST" else []
        out.append(dict(m, status="collected", result_status="success", result_is_error=False,
                        quality_passed=(block, arm) not in failures, requests=max(4, round(rng.gauss(mean, spread))),
                        verified=(block, arm) not in unverified, cost_usd=round(rng.gauss(cost, cost / 10), 6),
                        receipts=[], replay={}, timeline=dict(final_digest=final, events=events),
                        verification_requests=0 if arm == "BEST" else 2))
    return out


def report(rs):
    result = analysis.analyze(rs, "A4", draws=4000, route=ROUTE)
    return dict(result, best=dict(arm="A4", label="verification ownership plus the orientation card"),
                build_commit="0000000000000000000000000000000000000000",
                binary_sha256={"adage": "synthetic", "adage-exec": "synthetic"}, assignments=len(rs),
                collected=sum(r.get("status") == "collected" for r in rs), valid=True, lapses=[])


CASES = {
    "pass": dict(means={"A0": (18, 3, 0.115), "NOEXEC": (16.5, 3, 0.105), "BEST": (11.5, 2.5, 0.083)}),
    "fail": dict(means={"A0": (17, 3, 0.112), "NOEXEC": (16.5, 3, 0.108), "BEST": (16, 3, 0.106)},
                 failures={(3, "BEST"), (11, "BEST")}),
    "unverified": dict(means={"A0": (18, 3, 0.115), "NOEXEC": (16.5, 3, 0.105), "BEST": (11.5, 2.5, 0.083)},
                       unverified={(2, "A0"), (9, "BEST"), (14, "NOEXEC")}),
}

if __name__ == "__main__":
    here = Path(__file__).resolve().parent
    for name, case in CASES.items():
        value = report(rows(**case))
        (here / (name + ".json")).write_text(json.dumps(value, indent=2) + "\n")
        print(name, value["outcome"], value.get("primary", {}).get("ratio"), value.get("primary", {}).get("upper"))
