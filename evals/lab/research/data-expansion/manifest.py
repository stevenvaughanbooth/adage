#!/usr/bin/env python3
"""Write corpus.json: every session of the data-expansion corpus, with paths, splits, outcomes and
per-session token/cost totals (per-call usage is in each session's calls.jsonl).

  python3 manifest.py            # -> evals/lab/research/data-expansion/corpus.json

Reads the runner roots under /tmp/adage-research/data-expansion/runs, the task files and splits in
evals/repobench, and the configs here. Costs $0; safe to re-run while sessions are running (a
session still running is listed with status "running").
"""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
RB = HERE.parents[2] / "repobench"
sys.path.insert(0, str(RB))
import common  # noqa: E402
import spend_total  # noqa: E402

DX = Path("/tmp/adage-research/data-expansion")
NEW = ["go-toml", "chi", "bytes", "http", "serde-json", "itertools", "ufo", "packaging", "janet", "jq"]


def split_of(rid: str, tid: str, cfg: dict) -> str:
    p = RB / "splits" / f"{rid}.json"
    if p.exists():
        sp = json.loads(p.read_text())["repos"][rid]
        for k in ("train", "buffer", "test"):
            if tid in sp[k]:
                return k
    if tid in cfg.get("train", {}).get("tasks", []):
        return "train-extra"  # older than the published split (click/vue-core extra training)
    return "unsplit"


def active(root: Path) -> bool:
    """Is a runner working in this root now? (a `running` session in an idle root was cut off by an
    infrastructure failure, e.g. the shared disk filling up, and is not part of the corpus)"""
    import subprocess
    out = subprocess.run(["ps", "-eo", "args"], capture_output=True, text=True).stdout
    return any("runner.py" in line and f"--root {root}" in line for line in out.splitlines())


def session_rows(root: Path, cfg: dict, rid: str) -> list[dict]:
    st = json.loads((root / "state.json").read_text())
    rows = []
    for run_id, r in sorted(st["runs"].items()):
        m, sc = r.get("metrics") or {}, r.get("score") or {}
        rd = root / "runs" / run_id
        sid = m.get("session_id")
        tr = list((root / "private" / "transcripts").glob(f"*/{sid}.jsonl")) if sid else []
        tok = {"input": 0, "output": 0, "cache_read": 0, "cache_write": 0, "calls": 0}
        if (rd / "calls.jsonl").exists():
            for line in (rd / "calls.jsonl").read_text().splitlines():
                c = json.loads(line)
                tok["calls"] += 1
                for k in ("input", "output", "cache_read", "cache_write"):
                    tok[k] += c[k]
        rep = run_id.rsplit("-r", 1)[-1].split("-")[0]
        rows.append({
            "run_id": run_id, "root": str(root), "repo": rid, "task": r.get("task"),
            "split": split_of(rid, r.get("task"), cfg), "phase": r.get("phase"), "arm": r.get("arm"),
            "repeat": int(rep) if rep.isdigit() else None, "model": st.get("model"), "max_turns": st.get("max_turns"),
            "status": ("abandoned" if r.get("status") == "running" and not active(root) else r.get("status")), "run_status": m.get("status") or r.get("run_status"),
            "status_reason": m.get("status_reason"),
            "passed": m.get("passed"), "n_tests": sc.get("n_tests"), "touched_gold": sc.get("touched_gold"),
            "changed_files": sc.get("changed_files"), "hidden_command": sc.get("command"),
            "cost_usd": m.get("cost_usd"), "tool_calls": m.get("tool_calls"), "turns": m.get("turns"),
            "elapsed_s": m.get("elapsed"), "result_subtype": m.get("result_subtype"),
            "harness_version": m.get("harness_version"), "session_id": sid, "tokens": tok,
            "transcript": str(tr[0]) if tr else None,
            "stream": str(rd / "stream.jsonl") if (rd / "stream.jsonl").exists() else None,
            "calls": str(rd / "calls.jsonl") if (rd / "calls.jsonl").exists() else None,
            "diff": str(rd / "agent.diff") if (rd / "agent.diff").exists() else None,
        })
    return rows


def repo_entry(rid: str) -> dict:
    r = common.repo(rid)
    tp = RB / "tasks" / f"{rid}.json"
    tasks = json.loads(tp.read_text())["tasks"] if tp.exists() else []
    sp = RB / "splits" / f"{rid}.json"
    spj = json.loads(sp.read_text()) if sp.exists() else None
    val = DX / "cache" / "validate" / f"{rid}.validation.json"
    v = json.loads(val.read_text()) if val.exists() else []
    return {
        "url": r["url"], "lang": r["lang"], "repo_key": common.repo_key(r), "hidden": r["hidden"],
        "setup": r.get("setup"), "tasks_file": str(tp), "n_validated": len(tasks),
        "n_prompted": sum(bool(t.get("prompt")) for t in tasks),
        "validation": {"tried": len(v), "ok": sum(x["ok"] for x in v)} if v else None,
        "split_file": str(sp) if spj else None, "split_digest": spj["digest"] if spj else None,
        "split": {k: spj["repos"][rid][k] for k in ("train", "buffer", "test")} if spj else None,
        "tasks": [{"id": t["id"], "date": t["date"], "src_files": t["src_files"], "test_files": t["test_files"],
                   "gold_files": t.get("gold_files"), "src_lines": None, "fixed_seconds": t.get("fixed_seconds"),
                   "n_tests": t.get("n_tests"), "hidden_command": t.get("hidden_command"),
                   "has_prompt": bool(t.get("prompt"))} for t in tasks] if spj else None,
    }


def main() -> int:
    repos, sessions = {}, []
    for rid in NEW:
        if (RB / "tasks" / f"{rid}.json").exists():
            repos[rid] = repo_entry(rid)
    for cfgp in sorted((HERE / "configs").glob("*.json")):
        cfg = json.loads(cfgp.read_text())
        root = DX / "runs" / cfgp.stem
        rid = cfgp.stem.replace("-train2", "")
        if rid in repos:
            repos[rid]["config"] = str(cfgp)
            repos[rid]["root"] = str(root)
        if (root / "state.json").exists():
            sessions += session_rows(root, cfg, rid)
    done = [s for s in sessions if s["status"] in ("passed", "failed")]
    by = {}
    for s in done:
        k = f"{s['repo']}/{s['split']}"
        b = by.setdefault(k, {"sessions": 0, "passed": 0, "cost_usd": 0.0, "tool_calls": 0})
        b["sessions"] += 1
        b["passed"] += bool(s["passed"])
        b["cost_usd"] = round(b["cost_usd"] + (s["cost_usd"] or 0), 3)
        b["tool_calls"] += s["tool_calls"] or 0
    out = {
        "schema": "adage-dx-corpus/1",
        "generated": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "about": ("Data-expansion corpus (research sprint 2026-09-29): new RepoBench-style repositories "
                  "(hidden-test tasks mined from real fixes, validated fail-at-base/pass-with-fix) and extra "
                  "click/vue-core training sessions. Claude Haiku 4.5, 60 max turns, isolated, --host-auth, "
                  "confined. Arms: `train` = no hooks; `control` = Adage hooks on, directives withheld "
                  "(logged in state.json). Per-call usage: each session's `calls` file (one JSON line per model "
                  "call: input/output/cache_read/cache_write tokens, list-price usd, cum_usd, tool uses). "
                  "Outcome: `passed` = hidden tests pass after the session. Splits: `train`/`test` follow "
                  "evals/repobench/splits (chronological); `train-extra` = click/vue-core tasks older than "
                  "their published train split."),
        "cache_root": str(DX), "spend": spend_total.total(),
        "repos": repos, "summary": by,
        "n_sessions": len(sessions), "n_finished": len(done), "sessions": sessions,
    }
    (HERE / "corpus.json").write_text(json.dumps(out, indent=1) + "\n")
    print(f"corpus.json: {len(done)}/{len(sessions)} sessions finished; spend ${out['spend']['total']:.2f}")
    for k, b in sorted(by.items()):
        print(f"  {k:<24} {b['passed']}/{b['sessions']} passed  ${b['cost_usd']:.2f}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
