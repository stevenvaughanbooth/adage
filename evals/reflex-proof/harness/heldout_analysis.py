#!/usr/bin/env python3
"""Pre-registered held-out analysis for launch-gate-1 revision 9 (draft). Offline and pure: no dispatch.

Stratum: same repository, same test file family, guided-learned route (run-175d85).

- Arms: A0 (no Adage hooks), NOEXEC (Adage hooks, BEST's binaries and card setting, no execution
  grant) and BEST: the development design chosen by `select_best` from the development canary-v8
  report (the arm passing D1-D4 with the lowest mean requests), built from the final pre-freeze
  commit, which includes generic correctness fixes made before any held-out run. If no arm passes,
  nothing here runs.
- Matrix: four held-out tasks x five repetitions = 20 paired blocks; each block runs the three arms
  on one task and repetition in a seeded random order recorded in the matrix. The five safety
  controls run first, in BEST, on the development task with the validated canary suffixes.
- Primary metric: model requests per session, the count of `[API REQUEST]` dispatch lines in the
  debug log (retries are real, billed requests and count). Cross-check per session against the
  stream's distinct request ids: a mismatch fully explained by logged `[ERROR] API error (attempt
  ...)` entries is verified; an unexplained mismatch makes the session unverified, and its whole
  paired block leaves the ratio (all arms alike). More than 2 of 20 blocks excluded: unverified.
- Primary test (the headline GO criterion for this stratum): the one-sided 95% upper bound of the
  paired ratio sum(BEST)/sum(A0) over included blocks (percentile bootstrap over blocks, stratified
  by task) is at most 0.90, and BEST's hidden-test passes are at least A0's minus one (all blocks).
- Censoring, conservative against Adage: a session without a successful result is a quality failure
  in every arm. For requests, a censored BEST session counts max(observed, 60); a censored A0 or
  NOEXEC session counts its observed requests. Sensitivity: the same test without any block that
  contains a censored session. Sensitivity: the same test without the has-keys blocks; if the
  primary passes and that fails, the report says so prominently. GO uses the primary.
- G1-G9: each is marked applies or not applicable with a reason (`GO_CRITERIA`); every applicable
  criterion must also pass. Controls must be realized (launch_canary.realized) and skip no
  necessary work; an unrealized control is a protocol failure, not a pass.
- Verification requests: Bash requests running pytest in any form (pytest, py.test, python -m
  pytest, uv run pytest), tox or nox, python -m unittest, inline Python checks (python -c, python
  reading stdin or a heredoc) or a test script. Reported, never gated.
"""
from __future__ import annotations

import argparse
import fnmatch
import json
import math
import random
import re
import shlex
from pathlib import Path

PROTOCOL = "heldout-analysis-1"
STRATUM = "same repository, same test file family, guided-learned route (run-175d85)"
ARMS = ("A0", "NOEXEC", "BEST")
CANDIDATES = ("A2", "A3")
CONTROLS = ("failed", "later-edit", "config-change", "broader", "rerun")
CONTROL_TASK = "mm-curve--finite"
SENSITIVITY_TASK = "mm-curve--has-keys"
REPETITIONS = 5
MAX_TURNS = 60
CAP_REQUESTS = MAX_TURNS
SESSION_CAP_USD = 0.40
CEILING_USD = 26.0
RATIO_MAX = 0.90
CONFIDENCE = 0.95
BOOTSTRAP = 20000
SEED = 20261001
QUALITY_MARGIN = 1
MAX_EXCLUDED_BLOCKS = 2
UNNECESSARY_MAX = 0.25
G2_FIRED_MIN, G2_SUCCESS_MIN = 0.5, 0.75
G8_COST_MAX_RATIO = 1.05
CACHES = (".pytest_cache/**", "tests/__pycache__/**", "src/**/__pycache__/**", ".pytest_cache/.gitignore")
DEBUG_REQUEST = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")
API_ERROR = re.compile(r"^\S+ \[ERROR\] API error \(attempt \d+/\d+\)")
GO_CRITERIA = {
    "G1": dict(applies=True, form="single repository: the frozen learned home holds the certified verification route "
               "run-175d85 (checked at freeze); the two-repository clause belongs to the cross-repository decision"),
    "G2": dict(applies=True, form="BEST positive sessions: a learned verification ran in >= 50%, and >= 75% of those "
               "had a successful receipt"),
    "G3": dict(applies=False, reason="confirmed eliminations were the r1-r7 primary; r8/r9 replaced it with model "
               "requests per session, which this stratum gates, and its two-repository clauses cannot apply to one "
               "repository; eliminations are reported"),
    "G4": dict(applies=False, reason="a repeat-elimination mechanism criterion superseded by the r9 primary; repeats "
               "were gated in development (D4); NOEXEC equivalent-command and BEST repeat fractions are reported"),
    "G5": dict(applies=True, form="BEST hidden-test failures <= min(A0 failures, NOEXEC failures) over all blocks; "
               "stricter than the r9 margin of one, so it governs quality"),
    "G6": dict(applies=True, form="zero skipped necessary verification (controls and positives), zero executions "
               "without a grant or in NOEXEC/A0, zero foreign or unapproved receipts, zero undeclared writes, zero "
               "unknown costs"),
    "G7": dict(applies=True, form="at most 25% of BEST CPU executions in positive sessions unnecessary (receipt "
               "stale before the session ended); controls are stale by construction and excluded"),
    "G8": dict(applies=True, form="BEST total requests <= NOEXEC total and BEST cost <= 105% of NOEXEC, over "
               "included blocks with the censoring rule"),
    "G9": dict(applies=False, reason="the pipeline-unseen repository (more-itertools) is not run in this stratum; G9 "
               "stays required for any claim that covers it"),
}


def select_best(dev_report, candidates=None):
    """The development arm passing D1-D4 with the lowest mean requests (ties: lower mean cost, then
    the name). None means the held-out comparison is not run. `candidates`: the design's arms that
    can be BEST (dev-arms-1: A2, A3; dev-arms-2: A4, the arm with controls; dev-arms-3: MOD); by
    default the report's own `candidates`, else dev-arms-1's."""
    if candidates is None:
        candidates = tuple(dev_report.get("candidates") or CANDIDATES)
    verdict, arms = dev_report.get("verdict", {}), dev_report.get("arms", {})
    passing = [a for a in candidates if verdict.get(a, {}).get("passed") is True
               and isinstance(arms.get(a, {}).get("requests"), (int, float))]
    if not passing:
        return None
    cost = lambda a: arms[a].get("cost_usd") if isinstance(arms[a].get("cost_usd"), (int, float)) else math.inf  # noqa: E731
    return min(passing, key=lambda a: (arms[a]["requests"], cost(a), a))


def matrix(tasks, seed=SEED, repetitions=REPETITIONS, control_task=CONTROL_TASK):
    """Controls first (BEST, development task), then repetition-major paired blocks with a seeded
    random arm order per block. The order is part of the frozen matrix."""
    if len(tasks) < 4 or len(set(tasks)) != len(tasks):
        raise ValueError("at least four distinct held-out tasks")
    rng = random.Random(seed)
    rows = [dict(order=i, arm="BEST", condition=c, block=None, task=control_task, repetition=None)
            for i, c in enumerate(CONTROLS)]
    block = 0
    for repetition in range(1, repetitions + 1):
        for task in tasks:
            block += 1
            arms = list(ARMS)
            rng.shuffle(arms)
            for arm in arms:
                rows.append(dict(order=len(rows), arm=arm, condition="positive", block=block, task=task,
                                 repetition=repetition))
    return rows


def budget(sessions, cap=SESSION_CAP_USD, ceiling=CEILING_USD, typical=(0.11, 0.15), historical_mean=0.1905):
    """Every session is reserved at its cap before dispatch, so the cap bounds the worst case."""
    worst = round(sessions * cap, 4)
    return dict(sessions=sessions, per_session_cap_usd=cap, worst_case_usd=worst, ceiling_usd=ceiling,
                fits=worst <= ceiling + 1e-9, max_sessions_at_cap=int(round(ceiling / cap, 6)),
                expected_usd=[round(sessions * t, 2) for t in typical],
                expected_at_historical_mean_usd=round(sessions * historical_mean, 2),
                worst_case_at_r8_cap_usd=round(sessions * 1.5, 2))


# ------------------------------------------------------------------------------- classification

_WRAPPERS = {"env", "time", "nice", "command", "exec", "sudo", "nohup"}
_UV_VALUE_OPTIONS = {"--with", "--python", "-p", "--directory", "--project", "--group", "--extra", "--env-file"}
_PY_VALUE_FLAGS = {"-W", "-X"}
_SEPARATORS = {"&&", "||", ";", "|", "&", "(", ")", ";;", "|&"}


def _tokens(command):
    text = command.replace("\n", " ; ")
    try:
        lexer = shlex.shlex(text, posix=True, punctuation_chars=True)
        lexer.whitespace_split = True
        return list(lexer)
    except ValueError:
        return re.findall(r"&&|\|\||<<-?|[;|&()<>]|[^\s;|&()<>]+", text)


def _segments(command):
    segment = []
    for token in _tokens(command):
        if token in _SEPARATORS:
            if segment:
                yield segment
            segment = []
        else:
            segment.append(token)
    if segment:
        yield segment


def _strip(argv):
    while argv:
        head = argv[0]
        if re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", head) or head in _WRAPPERS:
            argv = argv[1:]
        elif head == "timeout":
            argv = argv[2:]
        elif head in ("uv", "poetry", "pipenv", "hatch", "pdm") and argv[1:2] == ["run"]:
            argv = argv[2:]
            while argv and argv[0].startswith("-"):
                argv = argv[2:] if argv[0] in _UV_VALUE_OPTIONS else argv[1:]
        else:
            return argv
    return argv


def _python(args):
    i = 0
    while i < len(args):
        a = args[i]
        if a == "-m":
            module = args[i + 1] if i + 1 < len(args) else ""
            return {"pytest": "pytest", "py.test": "pytest", "unittest": "unittest", "tox": "tox", "nox": "tox"}.get(module)
        if a == "-c" or a.startswith("-c"):
            return "python-c"
        if a in ("-", "<", "<<", "<<-"):
            return "inline-python"
        if a in _PY_VALUE_FLAGS:
            i += 2
            continue
        if a.startswith("-"):
            i += 1
            continue
        name = Path(a).name
        return "test-script" if name.startswith("test") or name.endswith("_test.py") else None
    return "inline-python"  # a bare interpreter reads its program from stdin


def verification_kinds(command):
    """Every verification kind a Bash command runs (empty when it runs none)."""
    kinds = set()
    for segment in _segments(command):
        argv = _strip(segment)
        if not argv:
            continue
        program = Path(argv[0]).name
        if program in ("pytest", "py.test") or re.fullmatch(r"pytest-?[0-9.]+", program):
            kinds.add("pytest")
        elif program in ("tox", "nox"):
            kinds.add("tox")
        elif re.fullmatch(r"python(?:[0-9]+(?:\.[0-9]+)*)?", program):
            kind = _python(argv[1:])
            if kind:
                kinds.add(kind)
    return sorted(kinds)


def bash_commands(stream):
    return [b.get("input", {}).get("command") or "" for row in stream if row.get("type") == "assistant"
            for b in (row.get("message") or {}).get("content", []) if b.get("type") == "tool_use" and b.get("name") == "Bash"]




# ---------------------------------------------------------------------------------- per session

def requests(debug_text, stream_observations):
    """Debug-log dispatches (the primary count, retries included) and the per-session cross-check.

    Verified when the stream's distinct request ids equal the dispatches, or when the surplus of
    dispatches is fully explained by logged `[ERROR] API error (attempt ...)` entries."""
    import probe
    lines = debug_text.splitlines()
    dispatches = [m[1] for line in lines if (m := DEBUG_REQUEST.fullmatch(line))]
    errors = sum(1 for line in lines if API_ERROR.match(line))
    ids = []
    for item in stream_observations:
        for rid in probe.explicit_request_ids(item.get("value")):
            if rid not in ids:
                ids.append(rid)
    surplus = len(dispatches) - len(ids)
    return dict(requests=len(dispatches), stream_request_ids=len(ids), api_error_entries=errors,
                cross_check=surplus == 0, verified=0 <= surplus <= errors and len(set(dispatches)) == len(dispatches))


def censored(row):
    return row.get("result_status") != "success" or row.get("result_is_error") is not False


def session_row(session, assignment):
    """One analysis row from a collected session directory (debug log, streams, summary, collection)."""
    session = Path(session)
    read = lambda name: [json.loads(l) for l in (session / name).read_text().splitlines() if l.strip()] if (session / name).exists() else []  # noqa: E731,E741
    summary = json.loads((session / "summary.json").read_text()) if (session / "summary.json").exists() else {}
    collection = json.loads((session / "collection.json").read_text()) if (session / "collection.json").exists() else {}
    debug = (session / "claude-debug.log").read_text(errors="replace") if (session / "claude-debug.log").exists() else ""
    commands = bash_commands(read("stdout.jsonl"))
    return dict(assignment, status="collected" if summary and collection else "uncollected",
                result_status=summary.get("result_status"), result_is_error=summary.get("result_is_error"),
                num_turns=summary.get("num_turns"), cost_usd=summary.get("total_cost_usd"), usage=summary.get("usage"),
                session_id=(collection.get("provenance") or {}).get("session_id"),
                quality_passed=(collection.get("quality") or {}).get("passed") is True,
                verification_requests=sum(bool(verification_kinds(c)) for c in commands),
                verification_kinds=[verification_kinds(c) for c in commands if verification_kinds(c)],
                replay=collection.get("replay"), timeline=collection.get("timeline"),
                receipts=collection.get("all_journal_receipts") or [],
                **requests(debug, read("stream-observations.jsonl")))


# ------------------------------------------------------------------------------------- analysis

def effective(row, arm):
    """Requests and quality after censoring, conservative against Adage: only BEST is imputed upward."""
    if censored(row):
        observed = row.get("requests") or 0
        return dict(requests=max(observed, CAP_REQUESTS) if arm == "BEST" else observed, passed=False, censored=True)
    return dict(requests=row["requests"], passed=row.get("quality_passed") is True, censored=False)


def _ratio(pairs):
    a = sum(p[1] for p in pairs)
    return sum(p[0] for p in pairs) / a if a else math.inf


def bootstrap_bounds(by_task, seed=SEED, draws=BOOTSTRAP, confidence=CONFIDENCE):
    """One-sided percentile bounds of sum(x)/sum(y) resampling blocks with replacement within each task."""
    rng = random.Random(seed)
    stats = []
    for _ in range(draws):
        sample = []
        for pairs in by_task.values():
            sample.extend(pairs[rng.randrange(len(pairs))] for _ in pairs)
        stats.append(_ratio(sample))
    stats.sort()
    k = math.ceil(confidence * draws) - 1
    return dict(upper=stats[k], lower=stats[draws - 1 - k])


def sign_flip_p(pairs, margin=RATIO_MAX, seed=SEED, draws=BOOTSTRAP):
    """Report-only: paired sign-flip p-value for H0 mean(x - margin*y) >= 0 against H1 < 0."""
    d = [x - margin * y for x, y in pairs]
    observed = sum(d)
    rng = random.Random(seed)
    hits = sum(sum(v if rng.random() < 0.5 else -v for v in d) <= observed for _ in range(draws))
    return (1 + hits) / (1 + draws)


def compare(blocks, x, y, seed=SEED, draws=BOOTSTRAP):
    by_task = {}
    for b in blocks.values():
        by_task.setdefault(b["task"], []).append((b[x]["requests"], b[y]["requests"]))
    pairs = [p for ps in by_task.values() for p in ps]
    if not pairs:
        return dict(ratio=None, upper=None, lower=None, blocks=0, blocks_fewer=0, sign_flip_p=None, passed=False)
    bounds = bootstrap_bounds(by_task, seed=seed, draws=draws)
    return dict(ratio=round(_ratio(pairs), 4), upper=round(bounds["upper"], 4), lower=round(bounds["lower"], 4),
                blocks=len(pairs), blocks_fewer=sum(a < b for a, b in pairs),
                sign_flip_p=round(sign_flip_p(pairs, seed=seed, draws=draws), 4), passed=bounds["upper"] <= RATIO_MAX)


def control_rows(rows):
    import launch_canary
    out = []
    for c in rows:
        collected = c.get("status") == "collected"
        out.append(dict(order=c.get("order"), condition=c["condition"], collected=collected,
                        realized=bool(collected and launch_canary.realized(c)),
                        necessary_work_skipped=bool((c.get("replay") or {}).get("necessary_work_skipped"))))
    return out


def _receipts(row):
    """The learned context's runs (G2, G7). The finish-line gate's runs are reported apart: a gate
    pass is never the context firing or succeeding (receipt_replay version 3, revision 11)."""
    import receipt_replay
    return [e for e in (row.get("timeline") or {}).get("events", [])
            if e.get("kind") == "receipt" and receipt_replay.origin(e) == "context"]


def _ok(e):
    return e.get("phase") == "completed" and e.get("outcome") == "applied"


def _declared(path):
    return any(fnmatch.fnmatch(path, glob) for glob in CACHES)


def go_criteria(rows, blocks, included, eff, route=None):
    """G1-G9 for this stratum: applicable ones are evaluated, the others carry their reason."""
    best = [r for r in rows if r["arm"] == "BEST" and r["condition"] == "positive"]
    hooks_without_grant = [r for r in rows if r["arm"] in ("A0", "NOEXEC") and (r.get("receipts") or _receipts(r))]
    fired = [r for r in best if _receipts(r)]
    succeeded = [r for r in fired if any(_ok(e) for e in _receipts(r))]
    foreign = [r.get("order") for r in rows if r["arm"] == "BEST" for g in r.get("receipts") or []
               if (route and g.get("route") != route) or (r.get("session_id") and g.get("session") not in (None, r["session_id"]))]
    undeclared = sorted({c.get("path") for r in rows for g in r.get("receipts") or []
                         for c in (g.get("outcome") or {}).get("changed", []) if not _declared(c.get("path") or "")})
    skipped = [r.get("order") for r in rows if (r.get("replay") or {}).get("necessary_work_skipped")]
    unknown = [r.get("order") for r in rows if r.get("cost_usd") is None]
    executions = [(e, r) for r in best for e in _receipts(r) if _ok(e)]
    unnecessary = [e for e, r in executions if e.get("digest") != (r.get("timeline") or {}).get("final_digest")]
    failures = {a: sum(not eff[n][a]["passed"] for n in blocks) for a in ARMS}
    req = {a: sum(eff[n][a]["requests"] for n in included) for a in ARMS}
    cost = {a: sum(blocks[n][a].get("cost_usd") or 0 for n in included) for a in ARMS}
    values = {
        "G1": dict(passed=None, note="checked at freeze (route present, isolation audit passed)"),
        "G2": dict(fired=len(fired), sessions=len(best), successful=len(succeeded),
                   passed=bool(best) and len(fired) >= G2_FIRED_MIN * len(best)
                   and len(succeeded) >= G2_SUCCESS_MIN * len(fired)),
        "G5": dict(failures=failures, passed=failures["BEST"] <= min(failures["A0"], failures["NOEXEC"])),
        "G6": dict(necessary_work_skipped=skipped, executions_without_grant=[r.get("order") for r in hooks_without_grant],
                   foreign_or_unapproved_receipts=foreign, undeclared_writes=undeclared, unknown_costs=unknown,
                   passed=not (skipped or hooks_without_grant or foreign or undeclared or unknown)),
        "G7": dict(executions=len(executions), unnecessary=len(unnecessary),
                   passed=len(unnecessary) <= UNNECESSARY_MAX * len(executions)),
        "G8": dict(requests=dict(BEST=req["BEST"], NOEXEC=req["NOEXEC"]), cost_usd=dict(BEST=round(cost["BEST"], 6),
                   NOEXEC=round(cost["NOEXEC"], 6)),
                   passed=req["BEST"] <= req["NOEXEC"] and cost["BEST"] <= G8_COST_MAX_RATIO * cost["NOEXEC"] + 1e-9),
    }
    out = {}
    for g, spec in GO_CRITERIA.items():
        out[g] = dict(spec, **values.get(g, dict(passed=None)))
    out["G1"]["passed"] = True if route else None
    return out


def analyze(rows, best_arm=None, seed=SEED, draws=BOOTSTRAP, route="github-com-marshmallow-code-marshmallow/learned/run-175d85"):
    """The registered verdict. Every assigned row counts; nothing is rerun. Returns `outcome`: pass,
    fail, unverified, protocol-failure or incomplete. A row may name BEST by its development arm
    (`best_arm`, e.g. MOD): it is BEST."""
    rows = [dict(r, arm="BEST") if best_arm and best_arm not in ARMS and r.get("arm") == best_arm else r for r in rows]
    problems = []
    controls = control_rows([r for r in rows if r["condition"] != "positive"])
    for c in controls:
        if not c["realized"]:
            problems.append("control %s unrealized (protocol failure)" % c["condition"])
        if c["necessary_work_skipped"]:
            problems.append("control %s skipped necessary verification (protocol failure)" % c["condition"])
    if sorted(c["condition"] for c in controls) != sorted(CONTROLS):
        problems.append("the five registered controls are not all assigned (protocol failure)")
    blocks = {}
    for r in rows:
        if r["condition"] == "positive":
            blocks.setdefault(r["block"], dict(task=r["task"]))[r["arm"]] = r
    incomplete = [n for n, b in sorted(blocks.items()) if any(a not in b or b[a].get("status") != "collected" for a in ARMS)]
    tasks = sorted({b["task"] for b in blocks.values()})
    if incomplete or len(tasks) < 4 or any(sum(b["task"] == t for b in blocks.values()) < REPETITIONS for t in tasks):
        return dict(protocol=PROTOCOL, stratum=STRATUM, best_arm=best_arm, passed=False, outcome="incomplete",
                    problems=problems + ["incomplete blocks: %s" % incomplete], controls=controls)
    eff = {n: {a: effective(b[a], a) for a in ARMS} for n, b in blocks.items()}
    excluded = sorted(n for n, b in blocks.items() if not all(b[a].get("verified") for a in ARMS))
    included = [n for n in sorted(blocks) if n not in excluded]
    pick = lambda numbers: {n: dict(task=blocks[n]["task"], **eff[n]) for n in numbers}  # noqa: E731
    primary = compare(pick(included), "BEST", "A0", seed, draws)
    no_censored = [n for n in included if not any(eff[n][a]["censored"] for a in ARMS)]
    no_has_keys = [n for n in included if blocks[n]["task"] != SENSITIVITY_TASK]
    sensitivity = dict(without_censored_blocks=dict(compare(pick(no_censored), "BEST", "A0", seed, draws),
                                                    excluded_blocks=[n for n in included if n not in no_censored]),
                       without_has_keys=compare(pick(no_has_keys), "BEST", "A0", seed, draws))
    passes = {a: sum(eff[n][a]["passed"] for n in blocks) for a in ARMS}
    quality = passes["BEST"] >= passes["A0"] - QUALITY_MARGIN
    criteria = go_criteria(rows, blocks, included, eff, route)
    applicable_ok = all(v["passed"] is not False for v in criteria.values() if v["applies"])
    arms = {a: dict(sessions=len(blocks), requests_included=sum(eff[n][a]["requests"] for n in included),
                    mean_requests_included=round(sum(eff[n][a]["requests"] for n in included) / len(included), 3) if included else None,
                    observed_requests=sum(blocks[n][a].get("requests") or 0 for n in blocks), hidden_test_passes=passes[a],
                    censored=sum(eff[n][a]["censored"] for n in blocks),
                    verification_requests=sum(blocks[n][a].get("verification_requests") or 0 for n in blocks),
                    cost_usd=round(sum(blocks[n][a].get("cost_usd") or 0 for n in blocks), 6)) for a in ARMS}
    verification = dict(excluded_blocks=excluded, max_excluded=MAX_EXCLUDED_BLOCKS,
                        sessions=[dict(order=blocks[n][a].get("order"), block=n, arm=a, requests=blocks[n][a].get("requests"),
                                       stream_request_ids=blocks[n][a].get("stream_request_ids"),
                                       api_error_entries=blocks[n][a].get("api_error_entries"),
                                       cross_check=blocks[n][a].get("cross_check"), verified=blocks[n][a].get("verified"))
                                  for n in sorted(blocks) for a in ARMS])
    if any("protocol failure" in p for p in problems):
        outcome = "protocol-failure"
    elif len(excluded) > MAX_EXCLUDED_BLOCKS:
        outcome = "unverified"
        problems.append("%d of %d blocks have an unverified request count (more than %d)" % (
            len(excluded), len(blocks), MAX_EXCLUDED_BLOCKS))
    else:
        outcome = "pass" if primary["passed"] and quality and applicable_ok else "fail"
    warnings = []
    if primary["passed"] and not sensitivity["without_has_keys"]["passed"]:
        warnings.append("PROMINENT: the primary passes, but the sensitivity analysis without the has-keys blocks "
                        "fails (upper bound %s > %.2f)." % (sensitivity["without_has_keys"]["upper"], RATIO_MAX))
    if primary["passed"] and not sensitivity["without_censored_blocks"]["passed"]:
        warnings.append("The primary passes, but the sensitivity analysis without censored blocks fails.")
    return dict(protocol=PROTOCOL, stratum=STRATUM, best_arm=best_arm, outcome=outcome, passed=outcome == "pass",
                problems=problems, warnings=warnings,
                primary=dict(primary, threshold=RATIO_MAX, included_blocks=len(included)),
                quality=dict(best=passes["BEST"], a0=passes["A0"], margin=QUALITY_MARGIN, passed=quality),
                go_criteria=criteria, sensitivity=sensitivity, request_verification=verification, arms=arms,
                controls=controls,
                report_only=dict(noexec_vs_a0=compare(pick(included), "NOEXEC", "A0", seed, draws),
                                 best_vs_noexec=compare(pick(included), "BEST", "NOEXEC", seed, draws)),
                notes=["GO for this stratum: the primary test and quality, plus every applicable G criterion.",
                       "Model requests are never presented as GPU savings; CPU work is reported separately.",
                       "Censoring: every censored session is a quality failure; only BEST is imputed upward (to %d requests)."
                       % CAP_REQUESTS])


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("command", choices=("matrix", "select", "budget", "analyze"))
    parser.add_argument("path", type=Path, nargs="?")
    args = parser.parse_args()
    if args.command == "matrix":
        import heldout_tasks
        value = matrix(heldout_tasks.EVALUATION)
    elif args.command == "budget":
        import heldout_tasks
        value = budget(len(matrix(heldout_tasks.EVALUATION)))
    elif args.command == "select":
        value = dict(best=select_best(json.loads(args.path.read_text())))
    else:
        data = json.loads(args.path.read_text())
        value = analyze(data["rows"], data.get("best_arm"))
    print(json.dumps(value, indent=2))


if __name__ == "__main__":
    main()
