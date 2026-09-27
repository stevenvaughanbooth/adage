#!/usr/bin/env python3
"""Adage demo runner — real headless Claude Code agents, with and without Adage.

  python3 demo/run.py train   # baseline agents on training work items; Adage off
  python3 demo/run.py learn   # adage learn over the training transcripts -> tracks
  python3 demo/run.py eval    # held-out work items, two arms: holdout vs adage
  python3 demo/run.py all

Every run gets a fresh copy of the sandbox repository, a real `claude -p` session, and a hidden
check afterwards. Live state is written to $DEMO_ROOT/state.json for the dashboard
(demo/dashboard/serve.py). Both eval arms run Adage's hooks: the holdout arm computes what it
would have said and logs it, but says nothing — so hook overhead is identical between arms.

Environment: DEMO_ROOT (default /tmp/adage-demo), DEMO_MODEL, DEMO_CONCURRENCY, DEMO_REPEATS,
DEMO_TRAIN_REPEATS, DEMO_MAX_TURNS.
"""

import json
import os
import shutil
import subprocess
import sys
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))
from report import redact_home  # noqa: E402 - demo/report.py
SANDBOX = HERE / "sandbox"
ROOT = Path(os.environ.get("DEMO_ROOT", "/tmp/adage-demo"))
ADAGE_HOME = ROOT / "adage-home"
ADAGE_BIN = REPO / "target" / "release" / "adage"
MODEL = os.environ.get("DEMO_MODEL", "claude-sonnet-5")
CONCURRENCY = int(os.environ.get("DEMO_CONCURRENCY", "3"))
REPEATS = int(os.environ.get("DEMO_REPEATS", "3"))
TRAIN_REPEATS = int(os.environ.get("DEMO_TRAIN_REPEATS", "2"))
MAX_TURNS = int(os.environ.get("DEMO_MAX_TURNS", "30"))
ROUND = int(os.environ.get("DEMO_ROUND", "1"))

TRAIN = ["W-1041", "W-1042", "W-1043", "W-1044", "W-1045", "W-1046"]
HELD_OUT = ["W-2041", "W-2042", "W-2043", "W-2044", "W-2045", "W-2046"]
PROMPTS = [
    "Pick up {id} and get it done.",
    "Please work on {id}.",
    "Can you take care of {id}?",
]

EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
LOCK = threading.Lock()


# ---------------------------------------------------------------------------------------- state

def load_state():
    p = ROOT / "state.json"
    if p.exists():
        return json.loads(p.read_text())
    return {"model": MODEL, "phase": "idle", "runs": {}, "tracks": [], "learn": None, "started": time.time()}


STATE = None


def save_state():
    # The home directory is written as `~` (agents' commands sometimes name paths under it), so
    # the state can be shared or committed as it is.
    tmp = ROOT / "state.json.tmp"
    tmp.write_text(json.dumps(redact_home(STATE)))
    tmp.replace(ROOT / "state.json")


def update_run(run_id, **kv):
    with LOCK:
        STATE["runs"].setdefault(run_id, {}).update(kv)
        save_state()


# ------------------------------------------------------------------------------------ utilities

def classify(name, inp, failed=False):
    """A coarse category for the timeline."""
    if name in EDIT_TOOLS:
        return "edit"
    cmd = (inp or {}).get("command", "") if isinstance(inp, dict) else ""
    if name == "Bash" and any(s in cmd for s in ("sed -i", "cat >", "cat <<", "tee ", "> acme/", "> schema/", ">acme/", "write_text(")):
        return "edit"
    if name == "Bash":
        if "wi-lookup" in cmd:
            return "lookup"
        if "make gen" in cmd or "gen.py" in cmd:
            return "regen"
        if "unittest" in cmd or "make test" in cmd or "make ci" in cmd or "pytest" in cmd:
            return "verify-fail" if failed else "verify"
        if any(cmd.lstrip().startswith(c) for c in ("ls", "find", "cat", "grep", "rg", "head", "tail", "tree", "sed -n")):
            return "explore"
        return "exec"
    if name in ("Read", "Glob", "Grep", "LS"):
        return "explore"
    return "other"


def settings_file(arm):
    """Claude Code settings for a run: Adage hooks (exec form) for eval arms, nothing for training."""
    p = ROOT / f"settings-{arm}.json"
    if arm == "train":
        p.write_text(json.dumps({}))
        return p
    out = subprocess.run([str(ADAGE_BIN), "hooks-config", str(ADAGE_BIN)], capture_output=True, text=True, check=True)
    p.write_text(out.stdout)
    return p


def transcript_for(session_id):
    for p in (Path.home() / ".claude" / "projects").glob(f"*/{session_id}.jsonl"):
        return p
    return None


# ---------------------------------------------------------------------------------------- a run

def run_one(run_id, phase, arm, task, prompt_template):
    run_dir = ROOT / "runs" / run_id
    if run_dir.exists():
        shutil.rmtree(run_dir)
    sandbox = run_dir / "acme-billing"
    sandbox.mkdir(parents=True)
    subprocess.run([sys.executable, str(SANDBOX / "generate.py"), str(sandbox)], check=True, capture_output=True)
    prompt = prompt_template.format(id=task)
    update_run(run_id, id=run_id, phase=phase, arm=arm, task=task, prompt=prompt, status="running",
               t0=time.time(), events=[], directives=[], round=ROUND)

    env = dict(os.environ)
    env["ADAGE_HOME"] = str(ADAGE_HOME)
    env["ADAGE_ARM"] = "holdout" if arm == "holdout" else "treated"
    # Learning happens between rounds, never during one: no automatic learning.
    env["ADAGE_AUTO_LEARN"] = "0"
    cmd = [
        "claude", "-p", prompt,
        "--model", MODEL,
        "--max-turns", str(MAX_TURNS),
        "--output-format", "stream-json", "--verbose", "--include-hook-events",
        "--setting-sources", "project,local",
        "--strict-mcp-config",
        "--settings", str(settings_file(arm)),
        "--allowedTools", "Bash", "Read", "Edit", "Write", "Glob", "Grep", "LS", "MultiEdit",
        "--disallowedTools", "WebFetch", "WebSearch", "Agent", "Task",
    ]
    t0 = time.time()
    events, directives = [], []
    tools_by_id = {}
    session_id, result = None, {}
    raw = open(run_dir / "stream.jsonl", "w")
    proc = subprocess.Popen(cmd, cwd=sandbox, stdout=subprocess.PIPE, stderr=subprocess.PIPE, text=True, env=env)
    assistant_turns = 0
    for line in proc.stdout:
        t = round(time.time() - t0, 2)
        raw.write(json.dumps({"t": t, "line": line.rstrip("\n")}) + "\n")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        typ = m.get("type")
        if typ == "system" and m.get("subtype") == "init":
            session_id = m.get("session_id")
        elif typ == "system" and "hook" in str(m.get("subtype", "")):
            text = json.dumps(m)
            if "adage-signal" in text or "finish-line check" in text:
                directives.append({"t": t, "raw": text[:4000]})
                update_run(run_id, directives=directives)
        elif typ == "assistant":
            content = m.get("message", {}).get("content", [])
            uses = [c for c in content if c.get("type") == "tool_use"]
            if uses:
                assistant_turns += 1
            for u in uses:
                tools_by_id[u.get("id")] = u
                ev = {"t": t, "turn": assistant_turns, "tool": u.get("name"), "id": u.get("id"),
                      "cat": classify(u.get("name"), u.get("input")),
                      "cmd": (u.get("input") or {}).get("command") or (u.get("input") or {}).get("file_path") or ""}
                events.append(ev)
            update_run(run_id, events=events)
        elif typ == "user":
            for c in m.get("message", {}).get("content", []) or []:
                if isinstance(c, dict) and c.get("type") == "tool_result" and c.get("is_error"):
                    u = tools_by_id.get(c.get("tool_use_id"))
                    for ev in events:
                        if u and ev.get("id") == u.get("id"):
                            ev["failed"] = True
                            ev["cat"] = classify(u.get("name"), u.get("input"), failed=True)
            update_run(run_id, events=events)
        elif typ == "result":
            result = m
    proc.wait()
    raw.close()
    elapsed = round(time.time() - t0, 2)

    first_edit = next((e for e in events if e["cat"] == "edit"), None)
    score = json.loads(subprocess.run([sys.executable, str(SANDBOX / "score.py"), str(sandbox), task],
                                      capture_output=True, text=True).stdout or "{}")
    spool = spool_directives(session_id) if session_id and arm != "train" else []
    metrics = {
        "session_id": session_id,
        "elapsed": elapsed,
        "turns": result.get("num_turns") or assistant_turns,
        "tool_turns": assistant_turns,
        "tool_calls": len(events),
        "turns_to_first_edit": first_edit["turn"] - 1 if first_edit else None,
        "time_to_first_edit": first_edit["t"] if first_edit else None,
        "explore_calls_before_edit": sum(1 for e in events if e["cat"] == "explore" and (not first_edit or e["t"] < first_edit["t"])),
        "dead_end_hits": sum(1 for e in events if e["cat"] == "verify-fail"),
        "ran_regen": any(e["cat"] == "regen" for e in events),
        "cost_usd": result.get("total_cost_usd"),
        "passed": bool(score.get("passed")),
        "stale_generated": bool(score.get("stale_generated")),
        "result_subtype": result.get("subtype"),
        "adage": spool,
    }
    update_run(run_id, status="passed" if metrics["passed"] else "failed", metrics=metrics, t_end=time.time())
    return run_id, metrics


def spool_directives(session_id):
    """What Adage emitted (or would have) in this session, from its spool."""
    out = []
    p = ADAGE_HOME / "sessions" / f"{session_id}.spool"
    if not p.exists():
        return out
    try:
        r = subprocess.run([str(ADAGE_BIN), "report", "--json"], capture_output=True, text=True,
                           env=dict(os.environ, ADAGE_HOME=str(ADAGE_HOME)))
        _ = r
    except OSError:
        pass
    data = p.read_bytes()
    i = 0
    magic = bytes([0xAD, 0xA6, 0xE5, 0x01])
    while True:
        j = data.find(magic, i)
        if j < 0 or j + 12 > len(data):
            break
        n = int.from_bytes(data[j + 4:j + 8], "little")
        try:
            rec = json.loads(data[j + 12:j + 12 + n])
            d = rec.get("decision", {})
            for k in ("directives", "would_have"):
                for x in d.get(k, []):
                    out.append({"ts": rec.get("ts"), "event": rec.get("event"), "kind": k, "route": x.get("route"),
                                "aspect": x.get("aspect"), "text": x.get("text", "")[:600]})
            for jd in d.get("judgements", []):
                out.append({"ts": rec.get("ts"), "event": rec.get("event"), "kind": jd.get("kind"), "route": jd.get("route")})
            if d.get("continue_reason"):
                out.append({"ts": rec.get("ts"), "event": rec.get("event"), "kind": "stop_request", "text": d["continue_reason"]})
        except ValueError:
            pass
        i = j + 12 + n
    return out


# --------------------------------------------------------------------------------------- phases

def phase_runs(phase, plan):
    STATE["phase"] = phase
    save_state()
    with ThreadPoolExecutor(max_workers=CONCURRENCY) as ex:
        futs = [ex.submit(run_one, *p) for p in plan]
        for f in futs:
            try:
                rid, m = f.result()
                print(f"  {rid}: passed={m['passed']} turns={m['turns']} first_edit_turn={m['turns_to_first_edit']} "
                      f"t={m['elapsed']}s cost={m['cost_usd']}", flush=True)
            except Exception as e:  # noqa: BLE001 — one run's failure must not stop the demo
                print(f"  run failed: {e}", flush=True)


def train():
    plan = []
    for r in range(TRAIN_REPEATS):
        for i, task in enumerate(TRAIN):
            plan.append((f"train-{task}-r{r + 1}", "train", "train", task, PROMPTS[(i + r) % len(PROMPTS)]))
    phase_runs("train", plan)


def learn():
    STATE["phase"] = "learn"
    save_state()
    runs = [r for r in STATE["runs"].values() if r.get("metrics") and (r.get("phase") == "train" or ROUND > 1)]
    transcripts, outcomes = [], {}
    for r in runs:
        sid = r["metrics"]["session_id"]
        p = transcript_for(sid) if sid else None
        if p:
            transcripts.append(str(p))
            outcomes[sid] = r["metrics"]["passed"]
    (ROOT / "outcomes.json").write_text(json.dumps(outcomes))
    env = dict(os.environ, ADAGE_HOME=str(ADAGE_HOME))
    if ROUND == 1:
        shutil.rmtree(ADAGE_HOME / "tracks", ignore_errors=True)
    out = subprocess.run([str(ADAGE_BIN), "learn", "--demo", "--json", "--repo", "acme-billing",
                          "--outcomes", str(ROOT / "outcomes.json"), "--transcripts", *transcripts],
                         capture_output=True, text=True, env=env)
    if out.returncode != 0:
        print(out.stderr)
        sys.exit(1)
    rep = json.loads(out.stdout)
    STATE["learn"] = rep
    STATE["tracks"] = read_tracks()
    STATE.setdefault("rounds", {})[str(ROUND)] = {"learn": rep, "tracks": STATE["tracks"]}
    save_state()
    print(json.dumps(rep, indent=2))


def read_tracks():
    idx = ADAGE_HOME / "index.json"
    if not idx.exists():
        return []
    return json.loads(idx.read_text()).get("routes", [])


def evaluate():
    plan = []
    for r in range(REPEATS):
        for i, task in enumerate(HELD_OUT):
            prompt = PROMPTS[(i + r) % len(PROMPTS)]
            # Interleave arms so both see the same conditions over time.
            tag = "eval" if ROUND == 1 else f"eval{ROUND}"
            plan.append((f"{tag}-{task}-r{r + 1}-holdout", "eval", "holdout", task, prompt))
            plan.append((f"{tag}-{task}-r{r + 1}-adage", "eval", "adage", task, prompt))
    phase_runs("eval", plan)
    STATE["phase"] = "done"
    save_state()


def main():
    global STATE
    ROOT.mkdir(parents=True, exist_ok=True)
    if not ADAGE_BIN.exists():
        sys.exit(f"build first: cargo build --release  (missing {ADAGE_BIN})")
    STATE = load_state()
    STATE["model"] = MODEL
    STATE["round"] = ROUND
    what = sys.argv[1] if len(sys.argv) > 1 else "all"
    if what in ("train", "all"):
        train()
    if what in ("learn", "all"):
        learn()
    if what in ("eval", "all"):
        evaluate()
    if what == "one":
        # Smoke test: one held-out run with Adage.
        STATE["tracks"] = read_tracks()
        phase_runs("smoke", [("smoke-" + sys.argv[2], "eval", sys.argv[3] if len(sys.argv) > 3 else "adage",
                              sys.argv[2], PROMPTS[0])])


if __name__ == "__main__":
    main()
