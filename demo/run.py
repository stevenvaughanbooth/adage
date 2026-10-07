#!/usr/bin/env python3
"""Adage demo runner — real headless Claude Code agents, with and without Adage.

  python3 demo/run.py train   # baseline agents on training work items; Adage off
  python3 demo/run.py learn   # adage condition over the training transcripts -> tracks
  python3 demo/run.py eval    # held-out work items; arms from --arms / DEMO_ARMS (default control,adage)
  python3 demo/run.py all
  python3 demo/run.py one W-2041 mod [--debug]   # smoke test: one held-out run in one arm

Every run gets a fresh copy of the sandbox repository, a real `claude -p` session, and a hidden
check afterwards. Live state is written to $DEMO_ROOT/state.json for the dashboard
(demo/dashboard/serve.py).

Arms (ARMS below; docs/claude-mod/architecture.md §9, test-plan §7.2):

  a0           no Adage hooks at all: no --settings, no plugin, no ADAGE_* in the environment
  control      hooks from `adage hooks-config` via --settings, ADAGE_ARM=control (computes, says nothing)
  adage        the same hooks, treated
  mod-observe  the plugin only (--plugin-dir, never --settings from hooks-config), treated,
               ADAGE_MOD_OBSERVE=1
  mod          the plugin only, treated

The plugin arms load a copy of the plugin (default plugin/claude-code, --plugin-dir) staged under
$DEMO_ROOT/plugins/ with the binaries to serve it, exactly as plugin/build.sh stages them: adage
(default ADAGE_BIN, --plugin-bin) at dist/<target>/adage, and adage-exec (default ADAGE_EXEC_BIN,
else the adage-exec beside that adage) at dist/<target>/adage-exec, with their dist/SHA256SUMS. One
plugin ships both (plan A8): its session-start shim links bin/hook to adage-exec while
$ADAGE_HOME/exec/enabled exists (what a predict grant needs), else to adage. Nothing is downloaded
and plugin/ is never written. --no-stage loads --plugin-dir as it is (after plugin/build.sh).

Every session runs under --debug with its debug log at runs/<id>/claude-debug.log; `requests` is
counted from it as evals/reflex-proof/harness/heldout_analysis.requests() counts it
(demo/adage_metrics.py).

Environment: DEMO_ROOT (default /tmp/adage-demo), DEMO_MODEL, DEMO_CONCURRENCY, DEMO_REPEATS,
DEMO_TRAIN_REPEATS, DEMO_MAX_TURNS, DEMO_ARMS, ADAGE_BIN (default target/release/adage),
ADAGE_EXEC_BIN, DEMO_PLUGIN_DIR, DEMO_PLUGIN_BIN.
"""

import argparse
import hashlib
import json
import os
import platform
import re
import shutil
import subprocess
import sys
import threading
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
sys.path.insert(0, str(HERE))
from report import redact_home  # noqa: E402 - demo/report.py
import adage_metrics  # noqa: E402 - demo/adage_metrics.py
SANDBOX = HERE / "sandbox"
ROOT = Path(os.environ.get("DEMO_ROOT", "/tmp/adage-demo"))
ADAGE_HOME = ROOT / "adage-home"
ADAGE_BIN = Path(os.environ.get("ADAGE_BIN") or REPO / "target" / "release" / "adage")
PLUGIN_DIR = Path(os.environ.get("DEMO_PLUGIN_DIR") or REPO / "plugin" / "claude-code")
PLUGIN_BIN = Path(os.environ["DEMO_PLUGIN_BIN"]) if os.environ.get("DEMO_PLUGIN_BIN") else None
# The executing binary staged beside adage in the one plugin; None: the adage-exec beside adage.
EXEC_BIN = Path(os.environ["ADAGE_EXEC_BIN"]) if os.environ.get("ADAGE_EXEC_BIN") else None
PLUGIN_STAGE = True
EVAL_ARMS = [a for a in os.environ.get("DEMO_ARMS", "control,adage").split(",") if a]
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
    """Claude Code settings for a run: Adage hooks (exec form) for the hook arms, nothing for training."""
    p = ROOT / f"settings-{arm}.json"
    if ARMS[arm]["hooks"] == "empty":
        p.write_text(json.dumps({}))
        return p
    out = subprocess.run([str(ADAGE_BIN), "hooks-config", str(ADAGE_BIN)], capture_output=True, text=True, check=True)
    p.write_text(out.stdout)
    return p


# ------------------------------------------------------------------------------------------- arms
#
# hooks: "none"     no hooks at all (no --settings, no plugin)
#        "empty"    --settings {} (training: the agent alone, recorded like the hook arms)
#        "settings" --settings from `adage hooks-config` (exec-form hooks)
#        "plugin"   --plugin-dir only: the plugin's own hooks, never --settings as well, so no
#                   hook runs twice (implementation-plan §2.7.2)
# adage_arm: ADAGE_ARM for the session (None: no ADAGE_* variable at all).
# observe:   ADAGE_MOD_OBSERVE=1 (only the mod's observers register; architecture §9).
# Both plugin arms load the one plugin (plan A8); with $ADAGE_HOME/exec/enabled its hooks run the
# adage-exec staged inside it, which is what makes a predict grant answer a TurnStep.

ARMS = {
    "train": {"hooks": "empty", "adage_arm": "treated", "observe": False},
    "a0": {"hooks": "none", "adage_arm": None, "observe": False},
    "control": {"hooks": "settings", "adage_arm": "control", "observe": False},
    "adage": {"hooks": "settings", "adage_arm": "treated", "observe": False},
    "mod-observe": {"hooks": "plugin", "adage_arm": "treated", "observe": True},
    "mod": {"hooks": "plugin", "adage_arm": "treated", "observe": False},
}


def host_session_variable(name):
    """A variable that names the Claude Code session this runner itself runs in (when it does): its
    markers and identity. A child `claude -p` that inherits `CLAUDE_CODE_SESSION_ID` takes the
    host's session id as its own, so every run shares one id, one transcript name and one Adage
    spool; the `CLAUDE_CODE_REMOTE*` and messaging variables attach it to the host's session. The
    same rule as evals/live/runner.py's `forbidden_forward`."""
    return (name == "CLAUDECODE" or "SESSION_ID" in name
            or name in ("CLAUDE_CODE_CHILD_SESSION", "CLAUDE_CODE_ENTRYPOINT", "CLAUDE_PID")
            or name.startswith("CLAUDE_CODE_MESSAGING_") or name.startswith("CLAUDE_CODE_REMOTE"))


def arm_invocation(arm, base_env, *, settings_for=None, plugin_dir=None, adage_home=None, debug_file=None):
    """(claude flags, environment) that wire `arm`: the part of the command that differs by arm,
    plus the debug flags every arm gets. `settings_for(arm)` returns the --settings path (called
    only for the arms that pass one); `plugin_dir` is the staged plugin for the plugin arms. The
    environment never carries Adage's own variables but the arm's, nor the host session's
    (`host_session_variable`)."""
    spec = ARMS[arm]
    env = {k: v for k, v in base_env.items() if not k.startswith("ADAGE_") and not host_session_variable(k)}
    flags = []
    if spec["adage_arm"] is not None:
        env["ADAGE_HOME"] = str(adage_home or ADAGE_HOME)
        env["ADAGE_ARM"] = spec["adage_arm"]
        # Learning happens between rounds, never during one: no automatic learning.
        env["ADAGE_AUTO_LEARN"] = "0"
        if spec["observe"]:
            env["ADAGE_MOD_OBSERVE"] = "1"
    if spec["hooks"] in ("settings", "empty"):
        flags += ["--settings", str(settings_for(arm))]
    elif spec["hooks"] == "plugin":
        if plugin_dir is None:
            raise ValueError(f"arm {arm} needs a plugin directory")
        flags += ["--plugin-dir", str(plugin_dir)]
        # The mod finds the hook binary under its data directory, which it reads from
        # CLAUDE_PLUGIN_DATA in the engine's environment, or derives from an installed plugin's root
        # (plugins/cache/…). A --plugin-dir plugin has neither: the engine passes the variable to
        # command hooks only, so without this the mod stays inert (no TurnStep is ever asked) while
        # the command hooks run.
        env["CLAUDE_PLUGIN_DATA"] = str(plugin_data_dir(plugin_dir, env))
    if debug_file is not None:
        flags += ["--debug", "--debug-file", str(debug_file)]
    return flags, env


def plugin_data_dir(plugin_dir, env):
    """The data directory Claude Code gives a --plugin-dir plugin, the one its command hooks see as
    ${CLAUDE_PLUGIN_DATA}: `<config dir>/plugins/data/<name>-inline` (`<name>@inline`, every
    character but a letter, a digit, `-` or `_` as `-`), the name from .claude-plugin/plugin.json."""
    manifest = Path(plugin_dir) / ".claude-plugin" / "plugin.json"
    try:
        name = json.loads(manifest.read_text())["name"]
    except (OSError, ValueError, KeyError, TypeError):
        name = Path(plugin_dir).name
    return claude_projects(env).parent / "plugins" / "data" / re.sub(r"[^A-Za-z0-9\-_]", "-", f"{name}@inline")


def plugin_target():
    """The target triple plugin/build.sh stages for this host (rustc's host triple), from uname:
    the session-start shim resolves the same directory (on Linux it tries -musl first and falls
    back to -gnu when no musl binary is staged; staging never copies a dist/ from the source)."""
    s, m = platform.system(), platform.machine().lower()
    m = {"arm64": "aarch64", "amd64": "x86_64"}.get(m, m)
    if s == "Darwin":
        return f"{m}-apple-darwin"
    if s == "Linux":
        return f"{m}-unknown-linux-gnu"
    return f"{m}-pc-windows-msvc"


def stage_plugin(src, binary, dest, target=None, exec_binary=None):
    """A copy of the plugin at `src` in `dest`, serving `binary`: dist/<target>/adage, and
    dist/<target>/adage-exec from `exec_binary` (default: the adage-exec beside `binary`, when there
    is one), with the dist/SHA256SUMS lines the shim verifies them against (`<sha256>
    <target>/<name>`, as sha256sum prints them). The source's own dist/ is left behind, so a stale
    build.sh stage never wins."""
    src, dest, binary = Path(src), Path(dest), Path(binary)
    target = target or plugin_target()
    exec_binary = Path(exec_binary) if exec_binary else binary.with_name("adage-exec")
    if dest.exists():
        shutil.rmtree(dest)
    shutil.copytree(src, dest, ignore=shutil.ignore_patterns("dist"), symlinks=True)
    bin_dir = dest / "dist" / target
    bin_dir.mkdir(parents=True)
    lines = []
    for name, path in (("adage", binary), ("adage-exec", exec_binary)):
        if name == "adage-exec" and not path.is_file():
            continue
        shutil.copy2(path, bin_dir / name)
        (bin_dir / name).chmod(0o755)
        lines.append(f"{hashlib.sha256((bin_dir / name).read_bytes()).hexdigest()}  {target}/{name}\n")
    (dest / "dist" / "SHA256SUMS").write_text("".join(lines))
    return dest


_PLUGIN_LOCK = threading.Lock()
_STAGED = {}


def plugin_for_runs():
    """The plugin directory the plugin arms load: staged once per process under $DEMO_ROOT/plugins/
    (or --plugin-dir as it is, with --no-stage)."""
    if not PLUGIN_STAGE:
        return PLUGIN_DIR
    with _PLUGIN_LOCK:
        if "dir" not in _STAGED:
            _STAGED["dir"] = stage_plugin(PLUGIN_DIR, PLUGIN_BIN or ADAGE_BIN, ROOT / "plugins" / PLUGIN_DIR.name,
                                          exec_binary=EXEC_BIN)
        return _STAGED["dir"]


def claude_projects(env=None):
    """Where Claude Code writes its transcripts: $CLAUDE_CONFIG_DIR/projects, else ~/.claude/projects."""
    env = os.environ if env is None else env
    return Path(env["CLAUDE_CONFIG_DIR"]) / "projects" if env.get("CLAUDE_CONFIG_DIR") else Path.home() / ".claude" / "projects"


def project_slug(cwd):
    """The directory name Claude Code gives a working directory under projects/: every character
    but a letter or a digit becomes `-`."""
    return re.sub(r"[^A-Za-z0-9]", "-", str(cwd))


def transcript_for(session_id, cwd=None, projects=None):
    """The session's transcript: the one in its working directory's project directory, else the
    first project directory holding that session id."""
    projects = projects or claude_projects()
    if cwd is not None:
        p = projects / project_slug(cwd) / f"{session_id}.jsonl"
        if p.is_file():
            return p
    for p in projects.glob(f"*/{session_id}.jsonl"):
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

    debug_log = run_dir / "claude-debug.log"
    arm_flags, env = arm_invocation(arm, os.environ, settings_for=settings_file,
                                    plugin_dir=plugin_for_runs() if ARMS[arm]["hooks"] == "plugin" else None,
                                    debug_file=debug_log)
    # Every run is its own session, named here: its transcript, its spool and its outcome are
    # found by this id, so no two runs may share one (learn() refuses if they do).
    session_id = str(uuid.uuid4())
    cmd = [
        "claude", "-p", prompt,
        "--session-id", session_id,
        "--model", MODEL,
        "--max-turns", str(MAX_TURNS),
        # Partial messages carry the explicit request ids the `requests` cross-check reads.
        "--output-format", "stream-json", "--verbose", "--include-hook-events", "--include-partial-messages",
        "--setting-sources", "project,local",
        "--strict-mcp-config",
        *arm_flags,
        "--allowedTools", "Bash", "Read", "Edit", "Write", "Glob", "Grep", "LS", "MultiEdit",
        "--disallowedTools", "WebFetch", "WebSearch", "Agent", "Task",
    ]
    update_run(run_id, argv=[c if c != prompt else "<prompt>" for c in cmd], debug_log=str(debug_log))
    t0 = time.time()
    events, directives = [], []
    observations, parsed = [], []
    tools_by_id = {}
    stream_session, result = None, {}
    raw = open(run_dir / "stream.jsonl", "w")
    # stderr to a file, never an unread pipe: under --debug it can outgrow a pipe's buffer. No
    # stdin: `claude -p` otherwise waits for it before starting.
    err = open(run_dir / "stderr.log", "w")
    proc = subprocess.Popen(cmd, cwd=sandbox, stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=err,
                            text=True, env=env)
    assistant_turns = 0
    for line in proc.stdout:
        t = round(time.time() - t0, 2)
        raw.write(json.dumps({"t": t, "line": line.rstrip("\n")}) + "\n")
        try:
            m = json.loads(line)
        except ValueError:
            continue
        observations.append({"value": m})
        parsed.append(m)
        typ = m.get("type")
        if typ == "system" and m.get("subtype") == "init":
            stream_session = m.get("session_id")
        elif typ == "system" and "hook" in str(m.get("subtype", "")):
            text = json.dumps(m)
            if "adage-reflex" in text or "finish-line check" in text:
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
    err.close()
    elapsed = round(time.time() - t0, 2)
    # The session Claude Code says it ran is the one its files are under; a different id than the
    # one asked for is recorded, never papered over.
    asked_session, session_id = session_id, stream_session or session_id
    transcript = transcript_for(session_id, sandbox)

    first_edit = next((e for e in events if e["cat"] == "edit"), None)
    score = json.loads(subprocess.run([sys.executable, str(SANDBOX / "score.py"), str(sandbox), task],
                                      capture_output=True, text=True).stdout or "{}")
    spool = spool_directives(session_id) if session_id and arm not in ("train", "a0") else []
    try:
        debug_text = debug_log.read_text(errors="replace")
    except OSError:
        debug_text = ""
    spool_path = ADAGE_HOME / "sessions" / f"{session_id}.spool" if session_id else None
    metrics = derived_metrics(arm, events, parsed, observations, debug_text,
                              adage_metrics.read_spool(spool_path) if spool_path else [],
                              spool_exists=bool(spool_path and spool_path.exists()))
    metrics.update({
        "session_id": session_id,
        "session_id_as_asked": session_id == asked_session,
        "transcript": str(transcript) if transcript else None,
        "elapsed": elapsed,
        "turns": result.get("num_turns") or assistant_turns,
        "tool_turns": assistant_turns,
        "tool_calls": len(events),
        "turns_to_first_edit": first_edit["turn"] - 1 if first_edit else None,
        "time_to_first_edit": first_edit["t"] if first_edit else None,
        "explore_calls_before_edit": sum(1 for e in events if e["cat"] == "explore" and (not first_edit or e["t"] < first_edit["t"])),
        "ran_regen": any(e["cat"] == "regen" for e in events),
        "cost_usd": result.get("total_cost_usd"),
        "passed": bool(score.get("passed")),
        # score.py's real keys, all recorded (implementation-plan §2.7.2).
        "ci_passed": score.get("ci_passed"),
        "hidden_passed": score.get("hidden_passed"),
        "hidden_tests_run": score.get("hidden_tests_run"),
        "stale_generated": bool(score.get("stale_generated")),
        "ci_tail": score.get("ci_tail"),
        "hidden_tail": score.get("hidden_tail"),
        "result_subtype": result.get("subtype"),
        "adage": spool,
    })
    update_run(run_id, status="passed" if metrics["passed"] else "failed", metrics=metrics, t_end=time.time())
    return run_id, metrics


def derived_metrics(arm, events, stream, observations, debug_text, spool_records, spool_exists):
    """The E1 metrics of one finished run, from its timeline events, parsed stream lines, stream
    observations ([{"value": line}]), debug log text and spool records. Pure: unit-tested.

    requests, requests_verified   heldout_analysis.requests(): debug-log dispatches, cross-checked
                                  against the stream's distinct explicit request ids (unique)
    dead_end_hits                 dead ends hit: a verification that failed (as before)
    dead_end                      {"hit", "substituted"}: substituted = dead ends the spool says
                                  were replaced before they ran
    predicted_steps               spool turn_step records with predicted/predict set
    receipts_as_results           spool pretool records answered from a receipt
    directives                    directives delivered, from the spool (or, with no spool, the
                                  <adage-reflex blocks in the stream's hook responses)
    subagent_tool_calls           tool calls made inside subagents (stream)
    hooks_wired_once              a0: no spool at all; hook arms: exactly one session_start record
                                  (one hook invocation per event, T-E1-02); None for training"""
    rq = adage_metrics.requests(debug_text, observations)
    sp = adage_metrics.spool_metrics(spool_records)
    st = adage_metrics.stream_metrics(stream)
    hit = sum(1 for e in events if e.get("cat") == "verify-fail")
    if arm == "a0":
        wired = not spool_exists
    elif ARMS.get(arm, {}).get("hooks") in ("settings", "plugin"):
        wired = sp["session_starts"] == 1
    else:
        wired = None
    return {
        "requests": rq["requests"],
        "requests_verified": rq["verified"],
        "requests_check": {k: rq[k] for k in ("stream_request_ids", "api_error_entries", "cross_check",
                                              "requests_streamed", "requests_by_source")},
        "dead_end_hits": hit,
        "dead_end": {"hit": hit, "substituted": sp["dead_end_substituted"]},
        "dead_end_substituted": sp["dead_end_substituted"],
        "predicted_steps": sp["predicted_steps"],
        "receipts_as_results": sp["receipts_as_results"],
        "directives": sp["directives"] if spool_exists else st["directives_in_stream"],
        "directives_in_stream": st["directives_in_stream"],
        "subagent_tool_calls": st["subagent_tool_calls"],
        "session_starts": sp["session_starts"],
        "spool_exists": spool_exists,
        "hooks_wired_once": wired,
    }


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
                                "strength": x.get("strength"), "text": x.get("text", "")[:600]})
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
                      f"requests={m.get('requests')} verified={m.get('requests_verified')} "
                      f"t={m['elapsed']}s cost={m['cost_usd']}", flush=True)
            except Exception as e:  # noqa: BLE001 — one run's failure must not stop the demo
                print(f"  run failed: {e}", flush=True)


def train():
    plan = []
    for r in range(TRAIN_REPEATS):
        for i, task in enumerate(TRAIN):
            plan.append((f"train-{task}-r{r + 1}", "train", "train", task, PROMPTS[(i + r) % len(PROMPTS)]))
    phase_runs("train", plan)


def learn_inputs(runs, find=transcript_for):
    """(transcripts, outcomes, missing) for `adage condition` from finished runs: each run's own
    transcript (recorded at the end of the run, else looked up by session id and sandbox) and its
    outcome by session id; `missing` lists the runs with no transcript. Raises ValueError when two
    runs share a session id: their transcripts would collapse into one session (or another
    session's file be read in their place), so learning from them would be wrong, not just less."""
    by_sid = {}
    for r in runs:
        sid = r["metrics"].get("session_id")
        if sid:
            by_sid.setdefault(sid, []).append(r.get("id") or "?")
    shared = {sid: ids for sid, ids in by_sid.items() if len(ids) > 1}
    if shared:
        sid, ids = next(iter(shared.items()))
        raise ValueError(f"{sum(len(i) for i in shared.values())} runs share {len(shared)} session id(s), e.g. "
                         f"{sid} for {', '.join(ids[:3])}{'…' if len(ids) > 3 else ''}: run them again "
                         "(each run passes its own --session-id)")
    transcripts, outcomes, missing = [], {}, []
    for r in runs:
        m = r["metrics"]
        sid = m.get("session_id")
        recorded = Path(m["transcript"]).expanduser() if m.get("transcript") else None
        p = recorded if recorded and recorded.is_file() else (
            find(sid, ROOT / "runs" / r["id"] / "acme-billing" if r.get("id") else None) if sid else None)
        if p:
            transcripts.append(str(p))
            outcomes[sid] = m["passed"]
        else:
            missing.append(r.get("id") or sid or "?")
    return transcripts, outcomes, missing


def learn():
    STATE["phase"] = "learn"
    save_state()
    runs = [r for r in STATE["runs"].values() if r.get("metrics") and (r.get("phase") == "train" or ROUND > 1)]
    try:
        transcripts, outcomes, missing = learn_inputs(runs)
    except ValueError as e:
        sys.exit(f"learn: {e}")
    if missing:
        print(f"learn: no transcript for {len(missing)} run(s): {', '.join(missing)}", file=sys.stderr)
    if not transcripts:
        sys.exit(f"learn: no transcripts under {claude_projects()} for {len(runs)} run(s)")
    print(f"learn: {len(transcripts)} transcripts from {len(runs)} runs", flush=True)
    (ROOT / "outcomes.json").write_text(json.dumps(outcomes))
    env = dict(os.environ, ADAGE_HOME=str(ADAGE_HOME))
    if ROUND == 1:
        shutil.rmtree(ADAGE_HOME / "tracks", ignore_errors=True)
    out = subprocess.run([str(ADAGE_BIN), "condition", "--demo", "--json", "--repo", "acme-billing",
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
            # Interleave arms so all see the same conditions over time.
            tag = "eval" if ROUND == 1 else f"eval{ROUND}"
            for arm in EVAL_ARMS:
                plan.append((f"{tag}-{task}-r{r + 1}-{arm}", "eval", arm, task, prompt))
    phase_runs("eval", plan)
    STATE["phase"] = "done"
    save_state()


def parse_args(argv):
    p = argparse.ArgumentParser(description="Adage demo runner (see the module docstring).")
    p.add_argument("what", nargs="?", default="all", choices=["train", "learn", "eval", "all", "one"])
    p.add_argument("task", nargs="?", help="one: the work item (e.g. W-2041)")
    p.add_argument("arm", nargs="?", default="adage", help="one: the arm (default adage)")
    p.add_argument("--arms", help="eval: comma-separated arms (default DEMO_ARMS or control,adage)")
    p.add_argument("--debug", action="store_true",
                   help="accepted for the documented command line; every run is under --debug already")
    p.add_argument("--plugin-dir", help="the plugin the plugin arms load (default plugin/claude-code)")
    p.add_argument("--plugin-bin", help="the binary staged into the plugin (default ADAGE_BIN)")
    p.add_argument("--no-stage", action="store_true",
                   help="load --plugin-dir as it is (its dist/ staged by plugin/build.sh)")
    a = p.parse_args(argv)
    if a.what == "one" and not a.task:
        p.error("one needs a work item: run.py one W-2041 [arm]")
    arms = [x for x in (a.arms.split(",") if a.arms else EVAL_ARMS) if x]
    for arm in arms + ([a.arm] if a.what == "one" else []):
        if arm not in ARMS or arm == "train":
            p.error(f"unknown arm {arm!r} (arms: {', '.join(k for k in ARMS if k != 'train')})")
    a.arms = arms
    return a


def main(argv=None):
    global STATE, PLUGIN_DIR, PLUGIN_BIN, PLUGIN_STAGE, EVAL_ARMS
    args = parse_args(sys.argv[1:] if argv is None else argv)
    EVAL_ARMS = args.arms
    if args.plugin_dir:
        PLUGIN_DIR = Path(args.plugin_dir).resolve()
    if args.plugin_bin:
        PLUGIN_BIN = Path(args.plugin_bin).resolve()
    PLUGIN_STAGE = not args.no_stage
    ROOT.mkdir(parents=True, exist_ok=True)
    used = set(EVAL_ARMS if args.what != "one" else [args.arm])
    if not ADAGE_BIN.exists() and (args.what != "one" or used != {"a0"}):
        sys.exit(f"build first: cargo build --release  (missing {ADAGE_BIN}; or set ADAGE_BIN)")
    if any(ARMS[a]["hooks"] == "plugin" for a in used) and PLUGIN_STAGE and not (PLUGIN_BIN or ADAGE_BIN).exists():
        sys.exit(f"missing the plugin's binary {PLUGIN_BIN or ADAGE_BIN} (--plugin-bin)")
    STATE = load_state()
    STATE["model"] = MODEL
    STATE["round"] = ROUND
    what = args.what
    if what in ("train", "all"):
        train()
    if what in ("learn", "all"):
        learn()
    if what in ("eval", "all"):
        evaluate()
    if what == "one":
        # Smoke test: one held-out run in one arm. Its phase is `smoke`, not `eval`: report.py
        # compares the `eval` runs, and a smoke run is not one of the interleaved repeats.
        STATE["tracks"] = read_tracks()
        phase_runs("smoke", [(f"smoke-{args.task}-{args.arm}", "smoke", args.arm, args.task, PROMPTS[0])])


if __name__ == "__main__":
    main()
