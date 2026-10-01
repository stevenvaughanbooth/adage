#!/usr/bin/env python3
"""Post hoc, read-only audit of development canary-v8 (`dev-v8b`), written after its report.

It applies three eval-audit checks that the frozen `dev-arms-1` report did not make. It never
changes that report's verdict.
- Control realization: did each control produce its assigned condition? This uses
  `launch_canary.realized` from the study's own frozen source copy.
- Censoring: did any session end other than success/completed, or reach its turn or budget cap?
- Dispatch cross-check: do model requests (distinct assistant message ids) equal the requests
  in Claude Code's debug log?

It also records, from each session's Adage spool, when the learned verification ran or was
withheld and why, and when bytecode appeared in the shared Python runtime.

Usage: python3 posthoc_audit.py /private/tmp/adage-real-repo-gate-v1/cohorts/launch-gate-v5/dev-v8b
"""
import ast
import collections
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys

HARNESS = Path(__file__).resolve().parents[3]
REPO = HARNESS.parents[2]
FREEZE_COMMIT = "5b7fc3a"
DISPATCH = re.compile(r"^\S+ \[DEBUG\] \[API REQUEST\] /v1/messages x-client-request-id=([\w-]+) source=(\w+)$")
RUNTIME_LIB = "runtime/python/cpython-3.12.14-macos-aarch64-none/lib"
BASE_SITE = "bases/mm-validator/.venv/lib/python3.12/site-packages"


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def frozen_function(path, name, *constants):
    """One pure function (and the module constants it reads) from the study's frozen source copy,
    without importing that copy's module graph."""
    tree = ast.parse(Path(path).read_text())
    keep = [n for n in tree.body if (isinstance(n, ast.FunctionDef) and n.name == name)
            or (isinstance(n, ast.Assign) and any(getattr(t, "id", None) in constants for t in n.targets))]
    if sum(isinstance(n, ast.FunctionDef) for n in keep) != 1:
        raise SystemExit("%s not found in %s" % (name, path))
    namespace = {"json": json}
    exec(compile(ast.Module(body=keep, type_ignores=[]), str(path), "exec"), namespace)
    return namespace[name]


def iso_ms(ms):
    return datetime.datetime.fromtimestamp(ms / 1000, datetime.timezone.utc).isoformat(timespec="milliseconds")


def births(root):
    """Birth times (UTC, to the second) of every .pyc under root."""
    out = collections.Counter()
    for p in Path(root).rglob("*.pyc"):
        t = datetime.datetime.fromtimestamp(os.stat(p).st_birthtime, datetime.timezone.utc)
        out[t.isoformat(timespec="seconds")] += 1
    return dict(sorted(out.items()))


def frozen_sources(manifest):
    """Each frozen source hash against the same file at the freeze commit."""
    mismatches = []
    for path, digest in sorted(manifest["source_sha256"].items()):
        rel = Path(path).relative_to(REPO)
        blob = subprocess.run(["git", "-C", str(REPO), "show", f"{FREEZE_COMMIT}:{rel}"], capture_output=True)
        if blob.returncode or hashlib.sha256(blob.stdout).hexdigest() != digest:
            mismatches.append(str(rel))
    return dict(commit=FREEZE_COMMIT, files=len(manifest["source_sha256"]), mismatches=mismatches)


def transcript_window(session):
    stamps = []
    for line in (session / "native-transcript.jsonl").read_text(errors="replace").splitlines():
        try:
            row = json.loads(line)
        except json.JSONDecodeError:
            continue
        if isinstance(row, dict) and row.get("timestamp"):
            stamps.append(row["timestamp"])
    return [min(stamps), max(stamps)] if stamps else None


def learned_runs(session, spool_records):
    """Every learned-verification decision in the spool: ran (with outcome) or withheld (with reason)."""
    runs, gates = [], []
    for spool in sorted((session / "adage-home/sessions").glob("*.spool")):
        for record in spool_records(spool):
            for e in record.get("exec") or []:
                runs.append(dict(at=iso_ms(record["ts"]), event=record.get("event"), route=e.get("route"),
                                 outcome=e.get("outcome"), withheld=e.get("withheld"), reason=e.get("reason")))
            if record.get("kind") == "stop_gate":
                gates.append(dict(at=iso_ms(record["ts"]), blocked=record.get("blocked")))
    return runs, gates


def audit(root):
    root = Path(root).resolve(strict=True)
    manifest = json.loads((root / "manifest.json").read_text())
    # The rules come from the study's own frozen copy of the harness, never from later edits.
    frozen = root / "source"
    for path, digest in manifest["source_sha256"].items():
        if sha256(frozen / Path(path).relative_to(REPO)) != digest:
            raise SystemExit("the study's frozen source copy differs from its manifest: " + path)
    realized = frozen_function(frozen / "evals/reflex-proof/harness/launch_canary.py", "realized")
    spool_records = frozen_function(frozen / "evals/live/runner.py", "spool_records", "SPOOL_MAGIC")
    reports = sorted((root / "reports").glob("dev-arms-*.json"))
    rows = []
    for row in manifest["matrix"]:
        name = "%02d-%s-%s" % (row["order"], row["arm"], row["condition"]) + ("" if row["block"] is None else "-b%d" % row["block"])
        session = root / "sessions" / name
        c = json.loads((session / "collection.json").read_text())
        summary = json.loads((session / "summary.json").read_text())
        argv = json.loads((session / "run.json").read_text())["argv"]
        max_turns = int(argv[argv.index("--max-turns") + 1])
        budget = float(argv[argv.index("--max-budget-usd") + 1])
        sources = collections.Counter(m[2] for line in (session / "claude-debug.log").read_text(errors="replace").splitlines()
                                      if (m := DISPATCH.fullmatch(line)))
        censored = [why for why, hit in (
            ("not success", summary.get("result_status") != "success" or summary.get("result_is_error")),
            ("not completed", summary.get("terminal_reason") != "completed"),
            ("turn cap", (summary.get("num_turns") or 0) >= max_turns),
            ("budget cap", (c.get("cost_usd") or 0) >= budget)) if hit]
        hooks = manifest["arms"][row["arm"]]["hooks"]
        runs, gates = learned_runs(session, spool_records) if hooks else ([], [])
        applied = [e for e in (c.get("timeline") or {}).get("events", [])
                   if e.get("kind") == "receipt" and e.get("phase") == "completed" and e.get("outcome") == "applied"]
        final = (c.get("timeline") or {}).get("final_digest")
        rows.append(dict(
            session=name, order=row["order"], arm=row["arm"], condition=row["condition"], block=row.get("block"),
            collection_sha256=sha256(session / "collection.json"), transcript_utc=transcript_window(session),
            requests=c["requests"], debug_dispatches=dict(sources), dispatch_match=sum(sources.values()) == c["requests"],
            result_status=summary.get("result_status"), terminal_reason=summary.get("terminal_reason"),
            num_turns=summary.get("num_turns"), max_turns=max_turns, cost_usd=c.get("cost_usd"), budget_usd=budget,
            censored=censored, quality_passed=(c.get("quality") or {}).get("passed"),
            contract_emitted=c.get("contract_emitted") if hooks else None,
            learned_verification=runs, stop_gate=gates,
            receipts_applied=len(applied) if hooks else None,
            receipt_covers_final=any(e.get("digest") == final for e in applied) if hooks else None,
            repeat_attempts=(c.get("replay") or {}).get("repeat_attempts") if hooks else None,
            necessary_work_skipped=(c.get("replay") or {}).get("necessary_work_skipped") if hooks else None,
            realized=realized(dict(condition=row["condition"], timeline=c.get("timeline"),
                                   replay=c.get("replay"))) if hooks else None))
    controls = [r for r in rows if r["condition"] != "positive"]
    hooked = [r for r in rows if r["contract_emitted"] is not None]
    ran = [r["session"] for r in hooked if any(e["outcome"] for e in r["learned_verification"])]
    withheld = [r for r in hooked if any(e["withheld"] for e in r["learned_verification"])]
    reasons = collections.Counter(e["reason"] for r in hooked for e in r["learned_verification"] if e["withheld"])
    authority = root.parents[2]
    return dict(
        audit="dev-v8b post hoc audit (read-only). The study is invalid for its execution arms from session 09: "
              "those sessions ran with a lapsed grant, so the dev-arms-1 report is not evidence for or against "
              "Levers 1-2",
        root=str(root), manifest_sha256=sha256(root / "manifest.json"),
        reports={p.name: sha256(p) for p in reports},
        frozen_sources=frozen_sources(manifest),
        rules=dict(
            realized="launch_canary.realized (frozen copy): failed needs a receipt and no successful one; later-edit and "
                     "config-change need a successful receipt that no longer covers the final sources; broader and "
                     "rerun need a successful receipt and a replay",
            censored="result_status not success, terminal_reason not completed, num_turns at --max-turns, or cost at "
                     "--max-budget-usd",
            dispatch="debug-log [API REQUEST] /v1/messages lines, by source, against distinct assistant message ids"),
        sessions=rows,
        summary=dict(
            dispatch_cross_check=dict(sessions=len(rows), equal=sum(r["dispatch_match"] for r in rows),
                                      mismatches=[r["session"] for r in not_equal(rows)]),
            censored=[r["session"] for r in rows if r["censored"]],
            quality_passed=sum(r["quality_passed"] is True for r in rows),
            controls_realized=dict(realized=sum(bool(r["realized"]) for r in controls), total=len(controls),
                                   by_arm={a: {r["condition"]: r["realized"] for r in controls if r["arm"] == a}
                                           for a in ("A2", "A3")}),
            D3_note="D3 (no skipped necessary work) held in the report, but no control realized its condition and "
                    "every control ran with a lapsed grant, so D3 was never tested and v8 makes no safety claim",
            stop_gate_runs=sum(len(r["stop_gate"]) for r in rows),
            learned_verification=dict(
                sessions_with_hooks=len(hooked), ran=ran,
                withheld_sessions=[r["session"] for r in withheld], withheld_reasons=dict(reasons),
                contract_emitted=[r["session"] for r in hooked if r["contract_emitted"]],
                last_run=max((e["at"] for r in hooked for e in r["learned_verification"] if e["outcome"]), default=None),
                first_withheld=min((e["at"] for r in hooked for e in r["learned_verification"] if e["withheld"]), default=None)),
            shared_runtime_bytecode=dict(runtime_lib_pyc_births_utc=births(authority / RUNTIME_LIB),
                                         base_site_packages_pyc_births_utc=births(authority / BASE_SITE))))


def not_equal(rows):
    return [r for r in rows if not r["dispatch_match"]]


if __name__ == "__main__":
    print(json.dumps(audit(sys.argv[1]), indent=1, sort_keys=False))
