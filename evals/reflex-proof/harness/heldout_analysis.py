#!/usr/bin/env python3
"""Pre-registered held-out analysis for launch-gate-1 revision 9 (draft). Offline and pure: no dispatch.

Stratum: same repository, same test file family, guided-learned route (run-175d85).

- Arms: A0 (no Adage hooks), NOEXEC (Adage hooks, BEST's binaries and card setting, no execution
  grant) and BEST, the development arm chosen by `select_best` from the development canary-v8
  report: the arm passing D1-D4 with the lowest mean requests. If none passes, nothing here runs.
- Matrix: four held-out tasks x five repetitions = 20 paired blocks; each block runs the three arms
  on one task and repetition in a seeded random order recorded in the matrix. The five safety
  controls run first, in BEST, on the development task with the validated canary suffixes.
- Primary metric: model requests per session, the count of `[API REQUEST]` dispatch lines in the
  debug log, which must match the stream's distinct request ids one to one.
- Primary test: the one-sided 95% upper bound of the paired ratio sum(BEST)/sum(A0) over blocks
  (percentile bootstrap over blocks, stratified by task) is at most 0.90, and BEST's hidden-test
  passes are at least A0's minus one.
- Censoring: a session without a successful result (max turns, budget cap, wall clock or any other
  stop) is a quality failure and counts max(observed, 60) requests. It is never dropped.
- Controls: every control session must be realized (launch_canary.realized) and keep necessary
  work; an unrealized control is a protocol failure, not a pass.
- Verification requests: Bash requests running pytest in any form (pytest, py.test, python -m
  pytest, uv run pytest), tox or nox, python -m unittest, inline Python checks (python -c, python
  reading stdin or a heredoc) or a test script. Reported, never gated.
"""
from __future__ import annotations

import argparse
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
REPETITIONS = 5
MAX_TURNS = 60
CAP_REQUESTS = MAX_TURNS
SESSION_CAP_USD = 0.33
CEILING_USD = 22.0
RATIO_MAX = 0.90
CONFIDENCE = 0.95
BOOTSTRAP = 20000
SEED = 20261001
QUALITY_MARGIN = 1
DEBUG_REQUEST = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")


def select_best(dev_report):
    """The development arm passing D1-D4 with the lowest mean requests (ties: lower mean cost, then
    the name). None means the held-out comparison is not run."""
    verdict, arms = dev_report.get("verdict", {}), dev_report.get("arms", {})
    passing = [a for a in CANDIDATES if verdict.get(a, {}).get("passed") is True
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
                fits=worst <= ceiling, max_sessions_at_cap=int(ceiling // cap),
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
    """Debug-log dispatches (primary) and the stream's distinct request ids (cross-check)."""
    import probe
    dispatches = [m[1] for line in debug_text.splitlines() if (m := DEBUG_REQUEST.fullmatch(line))]
    ids = []
    for item in stream_observations:
        for rid in probe.explicit_request_ids(item.get("value")):
            if rid not in ids:
                ids.append(rid)
    return dict(requests=len(dispatches), stream_request_ids=len(ids),
                cross_check=len(dispatches) == len(ids) and len(set(dispatches)) == len(dispatches))


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
    row = dict(assignment, status="collected" if summary else "uncollected",
               result_status=summary.get("result_status"), result_is_error=summary.get("result_is_error"),
               num_turns=summary.get("num_turns"), cost_usd=summary.get("total_cost_usd"), usage=summary.get("usage"),
               quality_passed=(collection.get("quality") or {}).get("passed") is True,
               verification_requests=sum(bool(verification_kinds(c)) for c in commands),
               verification_kinds=[verification_kinds(c) for c in commands if verification_kinds(c)],
               replay=collection.get("replay"), timeline=collection.get("timeline"),
               **requests(debug, read("stream-observations.jsonl")))
    return row


# ------------------------------------------------------------------------------------- analysis

def effective(row):
    if censored(row):
        return dict(requests=max(row.get("requests") or 0, CAP_REQUESTS), passed=False, censored=True)
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
    bounds = bootstrap_bounds(by_task, seed=seed, draws=draws)
    return dict(ratio=round(_ratio(pairs), 4), upper=round(bounds["upper"], 4), lower=round(bounds["lower"], 4),
                blocks=len(pairs), blocks_fewer=sum(a < b for a, b in pairs),
                sign_flip_p=round(sign_flip_p(pairs, seed=seed, draws=draws), 4))


def analyze(rows, best_arm=None, seed=SEED, draws=BOOTSTRAP):
    """The registered verdict. Every assigned row counts; nothing is dropped or rerun."""
    import launch_canary
    problems = []
    controls = [r for r in rows if r["condition"] != "positive"]
    control_rows = []
    for c in controls:
        collected = c.get("status") == "collected"
        realized = collected and launch_canary.realized(c)
        skipped = bool((c.get("replay") or {}).get("necessary_work_skipped"))
        control_rows.append(dict(order=c.get("order"), condition=c["condition"], collected=collected,
                                 realized=realized, necessary_work_skipped=skipped))
        if not realized:
            problems.append("control %s unrealized (protocol failure)" % c["condition"])
        if skipped:
            problems.append("control %s skipped necessary verification" % c["condition"])
    if sorted(c["condition"] for c in controls) != sorted(CONTROLS):
        problems.append("the five registered controls are not all assigned")
    blocks = {}
    for r in rows:
        if r["condition"] == "positive":
            blocks.setdefault(r["block"], dict(task=r["task"]))[r["arm"]] = r
    for number, b in sorted(blocks.items()):
        missing = [a for a in ARMS if a not in b or b[a].get("status") != "collected"]
        if missing:
            problems.append("block %s incomplete: %s" % (number, ", ".join(missing)))
    tasks = sorted({b["task"] for b in blocks.values()})
    if len(tasks) < 4 or any(sum(b["task"] == t for b in blocks.values()) < REPETITIONS for t in tasks):
        problems.append("fewer than four tasks x five repetitions")
    unverified = [r.get("order") for r in rows if r["condition"] == "positive" and r.get("status") == "collected"
                  and not r.get("cross_check")]
    if unverified:
        problems.append("request count cross-check failed for sessions %s" % unverified)
    if problems and any("incomplete" in p for p in problems):
        return dict(protocol=PROTOCOL, stratum=STRATUM, best_arm=best_arm, passed=False, problems=problems,
                    controls=control_rows)
    eff = {n: dict(task=b["task"], **{a: effective(b[a]) for a in ARMS}) for n, b in blocks.items()}
    arms = {}
    for a in ARMS:
        rs = [b[a] for b in blocks.values()]
        arms[a] = dict(sessions=len(rs), requests=sum(e[a]["requests"] for e in eff.values()),
                       mean_requests=round(sum(e[a]["requests"] for e in eff.values()) / len(rs), 3),
                       hidden_test_passes=sum(e[a]["passed"] for e in eff.values()),
                       censored=sum(e[a]["censored"] for e in eff.values()),
                       verification_requests=sum(r.get("verification_requests") or 0 for r in rs),
                       cost_usd=round(sum(r.get("cost_usd") or 0 for r in rs), 6),
                       unknown_costs=sum(r.get("cost_usd") is None for r in rs))
    primary = compare(eff, "BEST", "A0", seed, draws)
    quality = arms["BEST"]["hidden_test_passes"] >= arms["A0"]["hidden_test_passes"] - QUALITY_MARGIN
    if any(r.get("cost_usd") is None for r in rows):
        problems.append("unknown session costs")
    test = primary["upper"] <= RATIO_MAX
    return dict(protocol=PROTOCOL, stratum=STRATUM, best_arm=best_arm, problems=problems,
                passed=not problems and test and quality,
                primary=dict(primary, threshold=RATIO_MAX, passed=test),
                quality=dict(best=arms["BEST"]["hidden_test_passes"], a0=arms["A0"]["hidden_test_passes"],
                             margin=QUALITY_MARGIN, passed=quality),
                arms=arms, controls=control_rows,
                report_only=dict(noexec_vs_a0=compare(eff, "NOEXEC", "A0", seed, draws),
                                 best_vs_noexec=compare(eff, "BEST", "NOEXEC", seed, draws),
                                 uncensored_blocks=sum(not any(e[a]["censored"] for a in ARMS) for e in eff.values())),
                notes=["Model requests are never presented as GPU savings; CPU work is reported separately.",
                       "Censored sessions count as failures at max(observed, %d) requests." % CAP_REQUESTS])


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
