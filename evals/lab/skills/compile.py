#!/usr/bin/env python3
"""Procedure graph + aligned sessions -> a compiled skill: a collection of typed reflexes.

  python3 compile.py <SKILL.md> --repo-key K --sessions 'glob/*.jsonl' [--adage BIN] [--home DIR]
                     [--verify acme|click] [--out compiled.json]

A compiled skill (`adage-compiled-skill/0`) holds, per procedure step, the reflexes it becomes in
Adage's route model (route v1 today, the shapes of docs/packs.md): run (prompt-triggered, typed
slots bound from the prompt), read-batch (the lookup's linked files), obligation (after an edit,
checked at Stop), sequence (after an edit, numbered steps), avoid (a forbidden command, with its
failure signature). Each reflex carries its evidence from the alignment (applicable sessions,
sessions that did it on time) and a verification record: its command replayed in a fresh sandbox
against the step's expected observation. Steps that cannot compile say why (a gap), so the
compiled skill is also a coverage report of what stays text.

`--home` writes the routes as one track (`<K>/skill-<name>`) into an ADAGE_HOME and compiles its
index with the adage binary (`adage condition --no-instincts` over an empty transcript), which
validates every route exactly as a learned or imported one: nothing here bypasses the safety checks.
"""

from __future__ import annotations

import fnmatch
import glob
import json
import math
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from abstract import read_session
from align import analyse, edit_globs
from procedure import parse

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
SCHEMA = "adage-compiled-skill/0"
READ_LINKED = re.compile(r"\bread (the )?(linked|listed) files\b", re.I)


def wilson(k: int, n: int) -> float:
    if n == 0:
        return 0.0
    z, p = 1.96, k / n
    return (p + z * z / (2 * n) - z * math.sqrt(p * (1 - p) / n + z * z / (4 * n * n))) / (1 + z * z / n)


def evidence(stat: dict | None, k: int | None = None) -> dict:
    n = (stat or {}).get("applicable") or 0
    k = k if k is not None else ((stat or {}).get("timely") if (stat or {}).get("timely") is not None
                                 else (stat or {}).get("exact", 0))
    k = min(k or 0, n)
    return {"opportunities": n, "support": k, "days": 1 if n else 0, "successes": 0, "deviations_failed": 0,
            "rate": round(k / n, 4) if n else 0.0, "lower_bound": round(wilson(k, n), 4), "median_position": 0.0,
            "turns_saved": 0.0, "provenance": "authored", "models": ["claude-haiku-4-5-20251001"]}


def route(rid, summary, trigger, step, effect, strength, slots=(), exit_="intent_changes", ev=None):
    return {"id": rid, "summary": summary, "trigger": trigger, "step": step, "slots": list(slots), "effect": effect,
            "strength": strength, "state": "firm", "exit": [{"when": exit_}], "evidence": ev or evidence(None),
            "authored": True}


def plain(t: str) -> bool:
    return bool(re.fullmatch(r"[A-Za-z0-9 _./:=@,+{}-]+", t)) and len(t) <= 200


def code_glob(graph: dict) -> str | None:
    """The glob of the code an edit step changes (`under acme/` -> acme/*.py), else the first
    after_edit glob of a verify step."""
    for s in graph["steps"]:
        if s["role"] == "edit":
            for g in edit_globs(s):
                if g.endswith("/**"):
                    return g[:-3] + "/*.py"
                return g
    for s in graph["steps"]:
        if s["role"] == "verify" and s["precondition"]["kind"] == "after_edit":
            return s["precondition"]["glob"]
    return None


def build(graph: dict, analysis: dict | None) -> dict:
    stats = {s["id"]: s for s in (analysis or {}).get("steps", [])}
    name = graph["skill"]["name"]
    reflexes, gaps, proposed = [], [], []
    code = code_glob(graph)
    steps = graph["steps"]
    for i, s in enumerate(steps):
        st = stats.get(s["id"])
        made = []
        if not s["compilable"]:
            gaps.append({"step": s["id"], "title": s["title"], "why": s["why_not"]})
            continue
        # avoid: every forbidden command of the step, in one route (runners joined with " / ").
        if s["avoid"]:
            av = [a["command"] for a in s["avoid"] if plain(a["command"])]
            instead = s["avoid"][0]["instead"]
            sig = next((a["signature"] for a in s["avoid"] if a["signature"]), "")
            if av and plain(instead) and "{" not in instead and sig:
                made.append(route(f"skill-{s['id']}-avoid", "A command the skill forbids here, and what to run instead",
                                  {"on": "session_start"},
                                  {"do": "avoid", "avoid": " / ".join(av), "instead": instead, "signature": sig},
                                  "verify", "inhibit", exit_="intent_changes",
                                  ev=evidence(st, (st or {}).get("applicable", 0) - (st or {}).get("avoided_hits", 0)
                                              if st else None)))
        for c in s["commands"]:
            t = c["template"]
            if not plain(t):
                gaps.append({"step": s["id"], "title": s["title"], "why": f"`{t}` is not one plain command"})
                continue
            slots = [n for n in c["slots"]]
            pre = s["precondition"]
            if c["role"] == "lookup" and slots:
                sl = graph["slots"][slots[0]]
                if sl["source"] != "prompt":
                    gaps.append({"step": s["id"], "title": s["title"], "why": f"slot {{{slots[0]}}} not in the prompt"})
                    continue
                made.append(route(f"skill-{s['id']}-lookup", s["title"], {"on": "prompt", "span": "ticket"},
                                  {"do": "run", "command": t}, "observe", "fire",
                                  [{"name": slots[0], "source": {"from": "prompt", "span": "ticket"},
                                    "contract": {"kind": "pattern", "regex": sl["pattern"]}}], ev=evidence(st)))
                if READ_LINKED.search(s["text"]):
                    key = "exec " + re.sub(r"\{\w+\}", "{id}", t)
                    made.append(route(f"skill-{s['id']}-read", "Read the files the lookup lists, together",
                                      {"on": "after_command", "command": key},
                                      {"do": "read_batch", "slot": "files", "max": 5}, "observe", "prime",
                                      [{"name": "files", "source": {"from": "result_field", "field": "*paths"},
                                        "contract": {"kind": "repo_path"}}], ev=evidence(st)))
                continue
            if slots:
                srcs = {graph["slots"][n]["source"] for n in slots}
                why = (f"slot {{{slots[0]}}} is bound from the edited file's name, and no Adage slot source binds an "
                       "edited file's stem (EditSegment binds a directory segment)") if "edit" in srcs else \
                    f"slot {{{slots[0]}}} is the agent's choice"
                gaps.append({"step": s["id"], "title": s["title"], "why": why, "command": t})
                continue
            effect = {"verify": "verify", "regenerate": "regenerate", "lookup": "observe"}.get(c["role"], "environment")
            if pre["kind"] == "after_edit":
                glob_ = pre["glob"]
            elif pre["kind"] == "before_finish" or (c["role"] == "verify" and code):
                glob_ = code
            else:
                glob_ = None
            if glob_ is None and effect == "observe":
                # A fixed observe command with no stimulus but the task itself: an intent route
                # (Adage's `intent` trigger: a local-embedding centroid of the skill's prompts, no slots).
                # Its centroid needs the embedding model at install, so it is proposed, not written.
                proposed.append({"from_step": s["id"], "route": route(
                    f"skill-{s['id']}-intent", s["title"],
                    {"on": "intent", "family": name, "model": "potion-base-8m@<pinned>", "centroid": "<at install>",
                     "threshold": 0.6}, {"do": "run", "command": t}, "observe", "prime", ev=evidence(st))})
                continue
            if glob_ is None:
                gaps.append({"step": s["id"], "title": s["title"],
                             "why": f"`{t}` has no stimulus: it follows the agent's own writing of a file"
                                    if c["role"] == "run" else f"`{t}` has no stimulus Adage can see"})
                continue
            strength = "fire" if effect in ("verify", "regenerate") else "prime"
            made.append(route(f"skill-{s['id']}-{c['role']}", s["title"], {"on": "after_edit", "paths": glob_},
                              {"do": "run", "command": t}, effect, strength, exit_="exploratory_change",
                              ev=evidence(st)))
        # sequence: a verify step followed by a finishing verify step after the same edit.
        nxt = steps[i + 1] if i + 1 < len(steps) else None
        if (s["role"] == "verify" and nxt and nxt["role"] == "verify" and nxt["precondition"]["kind"] == "before_finish"
                and s["commands"] and nxt["commands"] and code):
            a, b = s["commands"][0]["template"], nxt["commands"][0]["template"]
            if plain(a) and plain(b) and "{" not in a + b:
                made.append(route(f"skill-{s['id']}-{nxt['id']}-seq", f"{s['title']}, then {nxt['title']}",
                                  {"on": "after_edit", "paths": code}, {"do": "sequence", "commands": [a, b]},
                                  "verify", "fire", exit_="exploratory_change", ev=evidence(stats.get(nxt["id"]))))
        for r in made:
            reflexes.append({"from_step": s["id"], "route": r})
    compilable = [s for s in steps if s["compilable"]]
    covered = {r["from_step"] for r in reflexes + proposed}
    return {"schema": SCHEMA, "skill": graph["skill"], "trigger": graph["trigger"], "slots": graph["slots"],
            "reflexes": reflexes, "proposed": proposed, "gaps": gaps,
            "coverage": {"steps": len(steps), "compilable": len(compilable), "compiled": len(covered),
                         "advice_blocks": len(graph["advice"])},
            "alignment": {k: v for k, v in (analysis or {}).items() if k != "alignments"}}


# --------------------------------------------------------------------------- adage install

def write_track(compiled: dict, home: Path, repo_key: str) -> Path:
    name = compiled["skill"]["name"]
    d = home / "tracks" / repo_key.replace("/", "_") / f"skill-{name}"
    shutil.rmtree(d, ignore_errors=True)
    (d / "routes").mkdir(parents=True)
    (d / "track.toml").write_text(
        f'[track]\nname = "{repo_key.split("/")[-1]}/skill-{name}"\nversion = "0.1.0"\n'
        f'description = "Compiled from the skill {name} (sha256 {compiled["skill"]["sha256"][:12]})"\n'
        f'provenance = "authored"\n\n[scope]\nrepo = "{repo_key}"\n\n[gauge]\nharness = ["claude-code"]\n'
        f'models = ["claude-haiku-4-5-20251001"]\n')
    for r in compiled["reflexes"]:
        (d / "routes" / f"{r['route']['id']}.yaml").write_text(json.dumps(r["route"], indent=1) + "\n")
    return d


def compile_index(adage: str, home: Path, repo_key: str) -> dict:
    """Compile the index with the adage binary, validating every route. Returns the index's routes."""
    with tempfile.TemporaryDirectory() as t:
        dummy = Path(t) / "empty.jsonl"
        dummy.write_text(json.dumps({"type": "user", "message": {"role": "user", "content": "noop"},
                                     "cwd": "/nonexistent", "sessionId": "noop"}) + "\n")
        out = subprocess.run([adage, "condition", "--no-instincts", "--json", "--repo", repo_key, "--transcripts",
                              str(dummy)], capture_output=True, text=True, env={"ADAGE_HOME": str(home),
                                                                                "PATH": "/usr/bin:/bin"})
    if out.returncode != 0:
        raise SystemExit(f"adage condition refused the compiled track:\n{out.stderr}")
    idx = json.loads((home / "index.json").read_text())
    return {"routes": [(r["track"], r["route"]["id"], r["route"]["state"], r["route"]["strength"])
                       for r in idx["routes"]], "log": out.stderr.strip()[-2000:]}


# ------------------------------------------------------------------------------ verification

def sandbox(kind: str, d: Path, task: str) -> None:
    if kind == "acme":
        subprocess.run(["python3", str(REPO / "demo/sandbox/generate.py"), str(d)], check=True, capture_output=True)
    else:
        subprocess.run(["python3", str(REPO / "evals/repobench/sandbox.py"), "click", task, str(d), "--arm", "off"],
                       check=True, capture_output=True)


def run(cmd: str, cwd: Path) -> tuple[int, str]:
    p = subprocess.run(cmd, shell=True, cwd=cwd, capture_output=True, text=True, timeout=600,
                       env={"PATH": "/usr/local/bin:/usr/bin:/bin", "HOME": str(cwd)})
    return p.returncode, (p.stdout + p.stderr)


def verify(compiled: dict, graph: dict, kind: str, task: str) -> list[dict]:
    """Replay each reflex's commands in a fresh sandbox: the directed command must exit 0 and show
    what the step expects; a forbidden command must fail with its signature. Slots are bound from
    their example (or the first value the alignment saw)."""
    steps = {s["id"]: s for s in graph["steps"]}
    out = []
    with tempfile.TemporaryDirectory() as t:
        d = Path(t) / "sb"
        sandbox(kind, d, task)
        for r in compiled["reflexes"]:
            rt, st = r["route"], steps[r["from_step"]]
            step = rt["step"]
            rec = {"route": rt["id"], "checks": []}
            if step["do"] == "avoid":
                for a in step["avoid"].split(" / "):
                    code, text = run(a, d)
                    rec["checks"].append({"cmd": a, "exit": code, "signature_seen": step["signature"] in text,
                                          "ok": code != 0 and step["signature"] in text})
                code, text = run(step["instead"], d)
                rec["checks"].append({"cmd": step["instead"], "exit": code, "ok": code == 0})
            elif step["do"] in ("run", "sequence"):
                for cmd in step.get("commands") or [step["command"]]:
                    for sl in rt["slots"]:
                        ex = compiled["slots"].get(sl["name"], {}).get("example") or "W-1041"
                        cmd = cmd.replace("{" + sl["name"] + "}", ex if kind == "acme" else ex)
                    if "repro.py" in cmd and not (d / "repro.py").exists():
                        (d / "repro.py").write_text("import click\nprint(click.__name__)\n")
                    code, text = run(cmd, d)
                    want = st["expect"]["contains"]
                    rec["checks"].append({"cmd": cmd, "exit": code, "expect": want,
                                          "ok": code == 0 and all(w in text for w in want)})
                    if rt["effect"] == "regenerate":
                        diff = subprocess.run(["git", "status", "--porcelain"], cwd=d, capture_output=True, text=True)
                        rec["checks"].append({"cmd": "git status (idempotent on a clean tree)",
                                              "ok": diff.stdout.strip() == ""})
            else:
                rec["checks"].append({"cmd": "(read_batch: nothing to replay)", "ok": True})
            rec["ok"] = all(c["ok"] for c in rec["checks"])
            out.append(rec)
    return out


def signature_of(text: str) -> str:
    """A short, renderable failure signature from a replayed command's output."""
    for pat in (r"No module named [\w.]+", r"RuntimeError: [^\n]{0,80}", r"[\w.-]+: (?:command )?not found",
                r"\w+Error: [^\n]{0,80}"):
        m = re.search(pat, text)
        if m:
            return re.sub(r"[`<>\"\\]", "", m.group(0)).strip()
    return ""


def refine(compiled: dict, verification: list[dict], kind: str, task: str) -> list[dict]:
    """Act on the replay: a directed command that failed on a clean checkout is dropped (the skill
    is stale there: a gap with the evidence); an avoid route keeps only runners that fail with its
    signature, and a runner that fails differently gets a route of its own with the observed one."""
    changes = []
    by_id = {v["route"]: v for v in verification}
    keep = []
    for r in compiled["reflexes"]:
        rt, v = r["route"], by_id.get(r["route"]["id"])
        if not v or v["ok"]:
            keep.append(r)
            continue
        if rt["step"]["do"] == "avoid":
            same = [c["cmd"] for c in v["checks"][:-1] if c["ok"]]
            other = [c for c in v["checks"][:-1] if not c["ok"] and c["exit"] not in (0, None)]
            if same:
                rt["step"]["avoid"] = " / ".join(same)
                keep.append(r)
            for c in other:
                with tempfile.TemporaryDirectory() as t:
                    d = Path(t) / "sb"
                    sandbox(kind, d, task)
                    _, text = run(c["cmd"], d)
                sig = signature_of(text)
                if sig:
                    nr = json.loads(json.dumps(r))
                    nr["route"]["id"] = rt["id"] + "-" + re.sub(r"[^a-z0-9]+", "-", c["cmd"].lower()).strip("-")[:20]
                    nr["route"]["step"].update(avoid=c["cmd"], signature=sig)
                    keep.append(nr)
                    changes.append(f"{rt['id']}: `{c['cmd']}` fails with `{sig}`, not the skill's signature: own route")
            if not v["checks"][-1]["ok"]:
                changes.append(f"{rt['id']}: the instead command `{rt['step']['instead']}` alone exits "
                               f"{v['checks'][-1]['exit']} on a clean checkout (kept: the skill's instead is scoped)")
            continue
        failed = [c for c in v["checks"] if not c["ok"]]
        compiled["gaps"].append({"step": r["from_step"], "title": rt["summary"],
                                 "why": f"replay failed on a clean checkout: `{failed[0]['cmd']}` exited "
                                        f"{failed[0].get('exit')} (the skill's instruction is stale here)"})
        changes.append(f"{rt['id']}: dropped (replay `{failed[0]['cmd']}` exited {failed[0].get('exit')})")
    compiled["reflexes"] = keep
    compiled["coverage"]["compiled"] = len({r["from_step"] for r in keep})
    return changes


def main(argv):
    skill = argv[0]
    opt = {argv[i]: argv[i + 1] for i in range(1, len(argv) - 1) if argv[i].startswith("--")}
    graph = parse(skill).to_json()
    paths = sorted(glob.glob(opt["--sessions"])) if "--sessions" in opt else []
    analysis = analyse(graph, [read_session(p) for p in paths]) if paths else None
    compiled = build(graph, analysis)
    if "--verify" in opt:
        task = opt.get("--task", "click-0cd28d65")
        compiled["verification"] = verify(compiled, graph, opt["--verify"], task)
        compiled["refined"] = refine(compiled, compiled["verification"], opt["--verify"], task)
        for c in compiled["refined"]:
            print("refined:", c)
    if "--home" in opt:
        home = Path(opt["--home"])
        write_track(compiled, home, opt["--repo-key"])
        compiled["index"] = compile_index(opt.get("--adage", str(REPO / "target/debug/adage")), home, opt["--repo-key"])
    text = json.dumps(compiled, indent=1, default=str)
    if "--out" in opt:
        Path(opt["--out"]).write_text(text + "\n")
    for r in compiled["reflexes"]:
        rt = r["route"]
        what = rt["step"].get("command") or rt["step"].get("commands") or rt["step"].get("avoid") or rt["step"]["do"]
        ev = rt["evidence"]
        print(f"{r['from_step']:<4} {rt['id']:<24} {rt['trigger']['on']:<14} {rt['strength']:<8} "
              f"{ev['support']}/{ev['opportunities']}  {what}")
    for g in compiled["gaps"]:
        print(f"{g['step']:<4} GAP {g['title'][:40]}: {g['why']}")
    for r in compiled["proposed"]:
        print(f"{r['from_step']:<4} PROPOSED {r['route']['id']} (intent trigger; centroid at install): {r['route']['step']['command']}")
    print("coverage", compiled["coverage"])
    for v in compiled.get("verification", []):
        print("verify", v["route"], "ok" if v["ok"] else "FAILED", [(c["cmd"], c.get("exit")) for c in v["checks"]])
    if "index" in compiled:
        print("index", compiled["index"]["routes"])
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
