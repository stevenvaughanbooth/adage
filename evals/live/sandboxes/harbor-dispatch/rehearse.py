#!/usr/bin/env python3
"""Rehearse the harbor-dispatch pipeline end to end with scripted sessions. Spends no tokens.

  python3 rehearse.py <root> [--adage PATH]

A scripted "agent" (plain Python, no model) works each incident in a fresh sandbox the way unaided
agents do: it explores, looks the incident up, reads the listed files one per turn, applies the
reference fix, sometimes runs `npm test` first (the dead end), runs `npm run check`, and after
registry edits runs `npm run codegen` (once only after CI reports stale output). Every command
really runs in the sandbox, and each session is written as a Claude Code transcript with the real
output. The root gets the live runner's layout (runs/, transcripts/, state.json, adage-home/), so
the rest of the pipeline runs unchanged:

  1. `adage condition --demo` over the 12 training sessions; checks that all four conventions are learned
     (lookup with an INC contract, linked files, the npm-test dead end, the registry finish line);
  2. the runner's classification and metrics over every session, and the results page;
  3. `build_demo.py` turns the root into an `adage eval` dataset, and `adage eval --split round1`
     replays the 18 held-out sessions against what was learned.

This proves the sandbox is learnable and the tooling ingests it. It says nothing about how real
agents behave: that is what the live run measures.
"""

import json
import shutil
import subprocess
import sys
import time
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent.parent.parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "evals" / "live"))
sys.path.insert(0, str(REPO / "demo"))
import generate  # noqa: E402
import report  # noqa: E402
import runner  # noqa: E402
import score as scorer  # noqa: E402
import solutions  # noqa: E402

CONFIG = REPO / "evals" / "live" / "configs" / "harbor-dispatch.json"
SPEC = {t["id"]: t for t in json.loads((HERE / "incidents.json").read_text())["tasks"]}


class Session:
    """One scripted session, recorded as a Claude Code transcript and as runner events."""

    def __init__(self, run_id, cwd, prompt, t0):
        self.run_id, self.cwd, self.t0 = run_id, Path(cwd), t0
        self.sid = str(uuid.uuid5(uuid.NAMESPACE_URL, f"adage-rehearsal/{run_id}"))
        self.lines, self.events, self.turn = [], [], 0
        self.lines.append(self.rec("user", {"role": "user", "content": prompt}))

    def rec(self, typ, message, **extra):
        ts = time.strftime("%Y-%m-%dT%H:%M:%S.000Z", time.gmtime(self.t0 + self.turn * 4))
        return {"type": typ, "sessionId": self.sid, "cwd": str(self.cwd), "timestamp": ts, "message": message, **extra}

    def call(self, tool, inp, output, failed=False):
        self.turn += 1
        tid = f"toolu_{self.turn:03d}"
        self.lines.append(self.rec("assistant", {"id": f"msg_{self.turn:03d}", "model": "scripted-rehearsal", "role": "assistant",
                                                 "content": [{"type": "tool_use", "id": tid, "name": tool, "input": inp}]}))
        self.lines.append(self.rec("user", {"role": "user", "content": [
            {"type": "tool_result", "tool_use_id": tid, "content": output, "is_error": failed}]},
            toolUseResult={"stdout": output} if tool == "Bash" else {}))
        self.events.append({"t": self.turn * 4.0, "turn": self.turn, "tool": tool, "id": tid, "failed": failed,
                            "cat": runner.classify(CFG, tool, inp, failed=failed),
                            "cmd": inp.get("command") or inp.get("file_path") or ""})
        return output, failed

    def bash(self, cmd):
        p = subprocess.run(cmd, shell=True, cwd=self.cwd, capture_output=True, text=True, env=scorer.env())
        out = (p.stdout + p.stderr).rstrip("\n")
        if p.returncode != 0:
            out = f"Exit code {p.returncode}\n{out}"
        return self.call("Bash", {"command": cmd, "description": cmd}, out, p.returncode != 0)

    def read(self, rel):
        text = (self.cwd / rel).read_text()
        numbered = "\n".join(f"{i + 1:>6}\t{line}" for i, line in enumerate(text.splitlines()))
        return self.call("Read", {"file_path": str(self.cwd / rel)}, numbered)

    def edit(self, rel, old, new):
        p = self.cwd / rel
        p.write_text(p.read_text().replace(old, new, 1))
        return self.call("Edit", {"file_path": str(p), "old_string": old, "new_string": new},
                         f"The file {p} has been updated successfully.")

    def write(self, transcripts):
        d = transcripts / "-rehearsal"
        d.mkdir(parents=True, exist_ok=True)
        (d / f"{self.sid}.jsonl").write_text("".join(json.dumps(x) + "\n" for x in self.lines))


def work(s, task, i):
    """What an unaided agent typically does, with deterministic variation by session index."""
    s.bash("ls")
    s.read("README.md")
    if i % 3 == 0:
        s.bash("cat package.json")
    s.bash(f"./bin/fetch-incident {task}" + (" --json" if i % 5 == 4 else ""))
    for f in SPEC[task]["files"]:
        s.read(f)
    for f, old, new in solutions.EDITS[task]:
        s.edit(f, old, new)
    registry = task in solutions.REGISTRY
    if registry and i % 4 == 1:
        s.bash("npm run check")          # stale: CI says to regenerate
        s.bash("npm run codegen")
    elif registry:
        s.bash("npm run codegen")
    if i % 3 != 2:
        s.bash("npm test")               # the dead end
    s.bash("npm run check")


def main(argv):
    global CFG
    args = list(argv)
    adage = REPO / "target" / "release" / "adage"
    if "--adage" in args:
        i = args.index("--adage")
        adage = Path(args[i + 1])
        del args[i:i + 2]
    if len(args) != 1:
        raise SystemExit("usage: rehearse.py <root> [--adage PATH]")
    if not adage.exists():
        raise SystemExit(f"build first: cargo build --release -p adage-cli  (missing {adage})")
    root = Path(args[0]).resolve()
    shutil.rmtree(root, ignore_errors=True)
    CFG = runner.load_config(CONFIG, {})
    state = {"config": CFG["name"], "model": "scripted-rehearsal", "runs": {}, "rounds": {}, "labels": CFG["labels"],
             "phase": "done", "started": 1790000000.0}
    plan = runner.plan(CFG, ["train", "eval"], 1)
    plan = [p for p in plan if p[2] in ("train", "control")]
    t0 = 1790000000.0
    print(f"scripting {len(plan)} sessions in fresh sandboxes under {root}/runs")
    for i, (run_id, phase, arm, task, prompt_t) in enumerate(plan):
        workdir = root / "runs" / run_id / CFG["workdir"]
        generate.generate(workdir)
        start = t0 + i * 600
        s = Session(run_id, workdir, prompt_t.format(id=task), start)
        work(s, task, i)
        s.write(root / "transcripts")
        (root / "runs" / run_id / "stream.jsonl").write_text(json.dumps({"t": 0, "line": json.dumps(
            {"type": "system", "subtype": "init", "session_id": s.sid, "cwd": str(workdir)})}) + "\n")
        sc = scorer.score(workdir, task)
        result = {"num_turns": s.turn + 1, "total_cost_usd": 0.0, "subtype": "success"}
        m = runner.run_metrics(CFG, s.events, result, sc, workdir, s.turn * 4.0, s.sid, [])
        state["runs"][run_id] = {"id": run_id, "phase": phase, "arm": arm, "task": task, "prompt": prompt_t.format(id=task),
                                 "round": 1, "status": "passed" if m["passed"] else "failed", "t0": start,
                                 "t_end": start + s.turn * 4, "events": s.events, "directives": [], "metrics": m}
        print(f"  {run_id:<26} passed={m['passed']} turns={m['turns']} first_edit={m['turns_to_first_edit']} "
              f"dead_ends={m['dead_end_hits']} obligation={m['obligation_met']}")

    # 1. learn from the training sessions, as the runner does in round 1.
    home = root / "adage-home"
    train = [r for r in state["runs"].values() if r["phase"] == "train"]
    tx = [str(next((root / "transcripts").glob(f"*/{r['metrics']['session_id']}.jsonl"))) for r in train]
    (root / "outcomes.json").write_text(json.dumps({r["metrics"]["session_id"]: r["metrics"]["passed"] for r in train}))
    out = subprocess.run([str(adage), "condition", *CFG["learn_args"], "--json", "--repo", CFG["repo_key"],
                          "--outcomes", str(root / "outcomes.json"), "--transcripts", *tx],
                         capture_output=True, text=True, env=dict(scorer.env(), ADAGE_HOME=str(home)))
    if out.returncode != 0:
        raise SystemExit(out.stderr)
    tracks = json.loads((home / "index.json").read_text()).get("routes", [])
    state["rounds"]["1"] = {"learn": json.loads(out.stdout), "tracks": tracks, "sessions": len(tx)}
    state["learn"], state["tracks"] = state["rounds"]["1"]["learn"], tracks
    (root / "state.json").write_text(json.dumps(state))
    print(f"\nlearned {len(tracks)} routes from {len(tx)} training sessions:")
    for t in tracks:
        r = t["route"]
        slots = "; ".join(f"{x['name']}: {json.dumps(x['contract'])}" for x in r.get("slots", []))
        print(f"  {r['id']:<22} {r['strength']:<7} trigger={json.dumps(r['trigger'])}\n{'':<25}step={json.dumps(r['step'])}"
              + (f"\n{'':<25}slots={slots}" if slots else ""))
    lookup = next((t["route"] for t in tracks if t["route"]["id"] == "lookup-work-item"), None)
    dead = next((t["route"] for t in tracks if t["route"]["id"].startswith("dead-end")), None)
    finish = next((t["route"] for t in tracks if t["route"]["id"].startswith("finish-line")), None)
    linked = next((t["route"] for t in tracks if t["route"]["id"] == "read-linked-files"), None)
    checks = {
        "lookup: ./bin/fetch-incident {item}, contract ^INC-\\d{2,3}$": bool(lookup) and "fetch-incident {item}" in json.dumps(lookup["step"])
        and "^INC-\\\\d{2,3}$" in json.dumps(lookup["slots"]),
        "linked files after the lookup": bool(linked),
        "dead end: avoid npm test, use npm run check": bool(dead) and "npm test" in dead["step"].get("avoid", "")
        and dead["step"].get("instead") == "npm run check",
        "finish line: registry/*.csv -> npm run codegen": bool(finish) and json.dumps(finish["trigger"]).find("registry/*.csv") >= 0
        and finish["step"].get("command") == "npm run codegen",
    }
    for name, ok in checks.items():
        print(f"  {'ok ' if ok else 'BAD'}  {name}")

    # 2. the runner's report and results page over the scripted state.
    summary = runner.summarise(state, 1)
    page = report.build(state, root / "results")
    print(f"\nrunner summary (control arm only here; no directives were delivered): "
          f"{json.dumps({k: v.get('control') for k, v in summary['metrics'].items()})}")
    print(f"results page: {page / 'index.html'}")

    # 3. build an offline dataset and replay the held-out sessions.
    ds = root / "dataset"
    fresh = root / "sandbox-files"
    generate.generate(fresh)
    b = subprocess.run([sys.executable, str(REPO / "evals" / "datasets" / "tools" / "build_demo.py"), str(root), str(ds),
                        "--name", "harbor-rehearsal", "--repo", CFG["repo_key"], "--sandbox", str(fresh),
                        "--description", "Scripted rehearsal sessions on the harbor-dispatch sandbox (no model)"],
                       capture_output=True, text=True)
    print(b.stdout.strip() or b.stderr.strip())
    if b.returncode != 0:
        raise SystemExit(1)
    ev = subprocess.run([str(adage), "eval", str(ds), "--split", "round1"], capture_output=True, text=True)
    print(ev.stdout.strip() or ev.stderr.strip())
    ok = all(checks.values()) and ev.returncode == 0
    print("\nREHEARSAL OK" if ok else "\nREHEARSAL FAILED")
    return 0 if ok else 1


CFG = None

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
