#!/usr/bin/env python3
"""Dry run of the whole showcase pipeline with no spend (no `claude` session, no model call).

  python3 dryrun.py [--adage BIN] [--out dryrun.json] [--revalidate] [--families F ...]

  1. validation    validation.json: every variant fails on the base and passes with its gold patch
                   (--revalidate re-runs validate.py: about 25 minutes)
  2. configs       every config through sc_run.py -> gv_run.py -> the runner's own plan (--json): session
                   counts per arm, prompts, held-out disjoint from training, model, turn cap, host auth,
                   confinement, nothing forbidden forwarded, the training environment (ADAGE_HOME only),
                   the Sonnet configs' refusal of train/learn and of an unlearned home, the product check
  3. conditioning  (needs BIN) per family, the product's own learner (`adage condition`, as the background
                   `condition --auto` runs it) over 4 SYNTHETIC training transcripts built from the gold
                   patches (the real diff as Edit/Write calls, the family's test command, a final reply),
                   then sc_detect.py's probe through the installed hook: does a card form, which held-out
                   prompts get it, does any negative; then the Sonnet arm's home seeding from that home
  4. analysis      analyze.py --synthetic (operating characteristics, checks) and --power, then the full
                   analysis + render.py on one plasmode dataset -> dryrun/ (RESULTS.synthetic.md,
                   results.synthetic.json: the website's schema filled with SYNTHETIC numbers)

Without BIN (or when it is missing), step 3 is skipped and reported as skipped.
"""

from __future__ import annotations

import argparse
import json
import os
import random
import re
import shutil
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[1]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "evals" / "live"))
import sc_sandbox  # noqa: E402,F401  (REPOBENCH_CACHE first)
import families as F  # noqa: E402
import make_configs as M  # noqa: E402

FORBIDDEN = ("CLAUDECODE", "CLAUDE_CODE_SESSION_ID")
DEFAULT_BIN = "/tmp/adage-go-target/release/adage"
TEST_CMD = {"click-type": ".venv/bin/python -m pytest -q tests/test_types.py",
            "mm-validator": ".venv/bin/python -m pytest -q tests/test_validate.py",
            "go-flag": "go test -count=1 .", "http-header": "cargo test -q",
            "chi-middleware": "go test -count=1 ./middleware/"}
TEST_OUT = {"click-type": "2252 passed in 6.1s", "mm-validator": "412 passed in 1.9s",
            "go-flag": "ok  \tgithub.com/urfave/cli/v3\t3.1s", "http-header": "test result: ok. 421 passed; 0 failed",
            "chi-middleware": "ok  \tgithub.com/go-chi/chi/v5/middleware\t1.2s"}


def env0() -> dict:
    return {k: v for k, v in os.environ.items() if k not in FORBIDDEN}


# ---------------------------------------------------------------------------------- 1. validation

def step_validation(revalidate: bool) -> dict:
    if revalidate:
        subprocess.run([sys.executable, str(HERE / "validate.py")], check=False)
    v = json.loads((HERE / "validation.json").read_text())
    return {"ok": v["valid"] == v["of"] == 60, "valid": v["valid"], "of": v["of"],
            "families": {k: [x["valid"], x["of"]] for k, x in v["families"].items()},
            "gold_check_seconds_max": {k: x["gold_check_seconds_max"] for k, x in v["families"].items()},
            "base_failed_checks": sorted({f"{r['family']}:{k}" for r in v["variants"]
                                          for k, x in r["base_checks"].items() if not x})}


# ------------------------------------------------------------------------------------- 2. configs

def plan(cfg: Path, phases: list[str], adage: str, root: Path) -> tuple[int, dict | str]:
    out = subprocess.run([sys.executable, str(HERE / "sc_run.py"), str(cfg), *phases, "--json", "--host-auth", "--root",
                          str(root), "--adage", adage], capture_output=True, text=True, env=env0(), cwd=HERE)
    if out.returncode != 0:
        return out.returncode, (out.stderr or out.stdout)[-600:]
    first, _, rest = out.stdout.partition("\n")
    return 0, {"product_check": json.loads(first.split(": ", 1)[1]), "plan": json.loads(rest)}


def step_configs(adage: str) -> dict:
    import runner
    res = {}
    with tempfile.TemporaryDirectory() as t:
        for fid in F.ORDER:
            p = HERE / "configs" / f"{fid}.json"
            code, x = plan(p, ["train", "eval", "report"], adage, Path(t) / fid)
            if code:
                res[fid] = {"ok": False, "error": x}
                continue
            fp = x["plan"]
            cfg = runner.load_config(p, {"host_auth": True})
            train_env = [s["env"] for s in fp["sessions"] if s["phase"] == "train"]
            eval_env = {s["arm"]: s["env"] for s in fp["sessions"] if s["phase"] == "eval"}
            row = {"sessions": fp["totals"]["sessions"], "by_arm": fp["totals"]["by_arm"], "model": fp["model"],
                   "cap_usd": fp["totals"]["budget_usd"], "max_turns": fp["totals"]["max_turns_each"],
                   "prompts_match": all(s["prompt"] == F.prompt(s["task"]) for s in fp["sessions"]),
                   "disjoint": not (set(cfg["train"]["tasks"]) & set(cfg["eval"]["tasks"])),
                   "train_env": train_env[0] if train_env else None, "eval_env": eval_env,
                   "forward_env_forbidden": [k for k in cfg["forward_env"] if runner.forbidden_forward(k)],
                   "confine": cfg.get("confine"), "host_auth": cfg.get("host_auth"),
                   "confine_expose": cfg.get("confine_expose"), "product_check": x["product_check"]}
            row["ok"] = (row["by_arm"] == {"adage": 16, "control": 16, "train": 4} and row["prompts_match"]
                         and row["disjoint"] and row["train_env"] == {"ADAGE_HOME": row["train_env"]["ADAGE_HOME"]}
                         and eval_env["adage"].get("ADAGE_ARM") == "treated" and eval_env["control"].get("ADAGE_ARM") == "control"
                         and eval_env["adage"].get("ADAGE_AUTO_LEARN") == "0" and not row["forward_env_forbidden"]
                         and row["confine"] and row["host_auth"] and row["max_turns"] == 60
                         and row["model"] == M.HAIKU)
            res[fid] = row
        for fid in M.SONNET_FAMILIES:
            p = HERE / "configs" / f"{fid}-sonnet.json"
            code, x = plan(p, ["eval", "report"], adage, Path(t) / f"{fid}-sonnet")
            refused_train, msg = plan(p, ["train", "eval"], adage, Path(t) / f"{fid}-sonnet-x")
            if code:
                res[f"{fid}-sonnet"] = {"ok": False, "error": x}
                continue
            fp = x["plan"]
            eval_env = {s["arm"]: s["env"] for s in fp["sessions"]}
            row = {"sessions": fp["totals"]["sessions"], "by_arm": fp["totals"]["by_arm"], "model": fp["model"],
                   "cap_usd": fp["totals"]["budget_usd"],
                   "tasks": sorted({s["task"] for s in fp["sessions"]}) == sorted(F.held_out_tasks(fid)[:6]),
                   "train_refused": refused_train != 0 and "Sonnet arm" in str(msg),
                   "eval_env_learning_frozen": all(e.get("ADAGE_AUTO_LEARN") == "0" for e in eval_env.values())}
            row["ok"] = (row["by_arm"] == {"adage": 6, "control": 6} and row["model"] == M.SONNET and row["tasks"]
                         and row["train_refused"] and row["eval_env_learning_frozen"])
            res[f"{fid}-sonnet"] = row
    res["global_cap_usd"] = M.GLOBAL_CAP
    res["ok"] = all(v["ok"] for k, v in res.items() if isinstance(v, dict))
    return res


# ------------------------------------------------------------------ 3. synthetic training history

def hunks(d: Path) -> list[dict]:
    """The gold patch as Claude Code calls: an Edit per diff hunk of a modified file (old = context +
    removed lines, new = context + added lines), a Write per new file."""
    subprocess.run(["git", "-C", str(d), "add", "-A", "-N", "."], capture_output=True)
    diff = subprocess.run(["git", "-C", str(d), "diff", "-U2", "--no-color"], capture_output=True, text=True).stdout
    calls, cur, new_file = [], None, False
    for block in re.split(r"^diff --git ", diff, flags=re.M)[1:]:
        m = re.match(r"a/(\S+) b/(\S+)", block)
        path = m.group(2)
        if "\nnew file mode" in block.split("@@", 1)[0]:
            calls.append({"name": "Write", "path": path, "content": (d / path).read_text()})
            continue
        for h in re.split(r"^@@[^\n]*@@[^\n]*\n", block, flags=re.M)[1:]:
            old, new = [], []
            for line in h.splitlines():
                if line.startswith("\\"):
                    continue
                tag, text = line[:1], line[1:]
                if tag in (" ", "-"):
                    old.append(text)
                if tag in (" ", "+"):
                    new.append(text)
            if old != new:
                calls.append({"name": "Edit", "path": path, "old": "\n".join(old), "new": "\n".join(new)})
    return calls


def transcript(fid: str, tid: str, i: int, cwd: str, calls: list[dict]) -> list[dict]:
    sid = f"00000000-0000-4000-8000-{i:012d}"
    ts = f"2026-09-2{i}T10:00:00Z"
    lines, n = [], 0

    def call(name, inp, out, err=False):
        nonlocal n
        n += 1
        tu = f"toolu_{i}_{n}"
        lines.append({"type": "assistant", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                      "message": {"id": f"msg_{i}_{n}", "model": M.HAIKU,
                                  "content": [{"type": "tool_use", "id": tu, "name": name, "input": inp}]}})
        lines.append({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                      "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tu,
                                                               "content": out, "is_error": err}]}})

    lines.append({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                  "message": {"role": "user", "content": F.prompt(tid)}})
    call("Bash", {"command": "ls"}, "README.md\nsrc\ntests")
    for c in calls:
        if c["name"] == "Edit":
            call("Read", {"file_path": f"{cwd}/{c['path']}"}, c["old"][:400])
            call("Edit", {"file_path": f"{cwd}/{c['path']}", "old_string": c["old"], "new_string": c["new"]},
                 f"The file {cwd}/{c['path']} has been updated.")
        else:
            call("Write", {"file_path": f"{cwd}/{c['path']}", "content": c["content"]},
                 f"File created successfully at: {cwd}/{c['path']}")
    call("Bash", {"command": TEST_CMD[fid]}, TEST_OUT[fid])
    lines.append({"type": "assistant", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                  "message": {"id": f"msg_{i}_end", "model": M.HAIKU, "stop_reason": "end_turn",
                              "content": [{"type": "text", "text": f"Done: {F.name(tid)} with tests and docs."}]}})
    return lines


def step_conditioning(adage: Path, fid: str, keep: Path) -> dict:
    import sc_detect
    import sc_run
    f = F.FAMILIES[fid]
    out: dict = {"family": fid}
    with tempfile.TemporaryDirectory(prefix=f"sc-dry-{fid}-") as t:
        t = Path(t)
        home, tx = t / "adage-home", t / "transcripts"
        tx.mkdir()
        files = []
        for i, tid in enumerate(F.train_tasks(fid), start=1):
            d = t / "work" / "repo"
            sc_sandbox.create(tid, d, setup=(fid == "go-flag"))
            f["gold"](F.parse_task(tid)[1], d)
            calls = hunks(d)
            cwd = f"{M.root(fid)}/runs/train-{tid}-r1/repo"
            p = tx / f"s{i}.jsonl"
            p.write_text("\n".join(json.dumps(x) for x in transcript(fid, tid, i, cwd, calls)) + "\n")
            files.append(str(p))
            out.setdefault("gold_calls", {})[tid] = [f"{c['name']} {c['path']}" for c in calls]
            shutil.rmtree(d, ignore_errors=True)
        env = env0()
        env.update(ADAGE_HOME=str(home), HOME=str(t / "user"), XDG_DATA_HOME=str(t / "user/.local/share"))
        t0 = time.time()
        r = subprocess.run([str(adage), "condition", "--json", "--repo", f["repo_key"], "--transcripts", *files],
                           capture_output=True, text=True, env=env)
        out["condition"] = {"exit": r.returncode, "seconds": round(time.time() - t0, 2), "stderr_tail": r.stderr[-600:]}
        try:
            rep = json.loads(r.stdout)
            eng = ((rep.get("repos") or {}).get(f["repo_key"]) or {}).get("engine") or {}
            out["condition"]["card_families"], out["condition"]["cards"] = eng.get("card_families"), eng.get("cards")
        except ValueError:
            out["condition"]["stdout_tail"] = r.stdout[-600:]
        g = sc_detect.probe(fid, home, adage)
        keep.write_text(json.dumps(g, indent=1) + "\n")
        out["gate"] = g["summary"]
        out["held_out"] = [{k: x[k] for k in ("task", "at_prompt", "at_edit", "delivered")} for x in g["held_out"]]
        out["negatives"] = [{k: x[k] for k in ("id", "at_prompt", "at_edit", "delivered")} for x in g["negatives"]]
        idx = home / "index.json"
        cards = []
        if idx.exists():
            for e in json.loads(idx.read_text()).get("routes", []):
                rid = (e.get("route") or {}).get("id") or e.get("id") or ""
                card = ((e.get("route") or {}).get("step") or {}).get("card")
                if str(rid).startswith("card-") and card:
                    cards.append({"id": rid, "files": [x.get("path") for x in card.get("files", [])],
                                  "commands": [x.get("command") or x.get("cmd") for x in card.get("commands", [])][:4]})
        out["cards"] = cards
        if fid in M.SONNET_FAMILIES:  # the Sonnet arm's seeding from this (learned) home
            src_root = t / "haiku-root"
            (src_root / "private").mkdir(parents=True)
            shutil.copytree(home, src_root / "private" / "adage-home")
            dst = t / "sonnet-root"
            first = sc_run.seed_home(src_root, dst)
            again = sc_run.seed_home(src_root, dst)
            try:
                sc_run.seed_home(t / "no-such-root", t / "sonnet-root-2")
                refuses = False
            except SystemExit:
                refuses = True
            out["sonnet_seed"] = {"seeded": first["seeded"], "idempotent": not again["seeded"],
                                  "index_copied": (dst / "private/adage-home/index.json").exists(),
                                  "refuses_unlearned_home": refuses}
    s = out["gate"]
    # the pipeline worked (the learner ran, the probe ran); what the product did is a finding, not a failure
    out["ok"] = out["condition"]["exit"] == 0 and len(out["held_out"]) == 8 and len(out["negatives"]) == 6
    out["findings"] = ([] if s["card_formed"] else ["no card formed"]) + (
        [f"{s['false_fires'][0]}/{s['false_fires'][1]} negatives got the card ({', '.join(s['false_fire_ids'])})"]
        if s["false_fires"][0] else []) + (
        [f"held out delivered {s['held_out_delivered'][0]}/{s['held_out_delivered'][1]}"]
        if s["held_out_delivered"][0] < s["held_out_delivered"][1] else [])
    out["note"] = ("synthetic training transcripts built from the gold patches: this checks the mechanics (the product "
                   "learns a card for this family and the hook delivers it on these prompts), not what agents will do")
    return out


# ------------------------------------------------------------------------------------ 4. analysis

def step_analysis(outdir: Path) -> dict:
    import importlib.util
    spec = importlib.util.spec_from_file_location("sc_analyze", HERE / "analyze.py")
    A = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(A)
    syn = json.loads(subprocess.run([sys.executable, str(HERE / "analyze.py"), "--synthetic", "--out",
                                     str(HERE / "synthetic.json")], capture_output=True, text=True).stdout)
    pw = json.loads((HERE / "power.json").read_text())
    outdir.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory() as t:
        roots = A.synth_roots(Path(t), -0.20, random.Random(20260929))
        a = A.analyse(roots)
        it = A.interim(roots)
        (Path(t) / "analysis.json").write_text(json.dumps(a))
        r = subprocess.run([sys.executable, str(HERE / "render.py"), "--analysis", str(Path(t) / "analysis.json"),
                            "--out-dir", str(Path(t) / "out"), "--status", "synthetic"], capture_output=True, text=True)
        shutil.copy(Path(t) / "out" / "RESULTS.md", outdir / "RESULTS.synthetic.md")
        shutil.copy(Path(t) / "out" / "results.json", outdir / "results.synthetic.json")
    rj = json.loads((outdir / "results.synthetic.json").read_text())
    return {"ok": syn["checks_pass"] and r.returncode == 0 and rj["status"] == "synthetic",
            "synthetic_checks": syn["checks"], "operating_characteristics": syn["operating_characteristics"],
            "power_5x8x2": pw["designs"]["5x8x2"], "ci90_coverage_known_truth": pw["ci90_coverage_known_truth"],
            "sonnet_2x6x1": pw["sonnet_2x6x1"],
            "illustrative_primary": rj["primary"]["sentence"] + " [SYNTHETIC, true card effect -20%, 75% delivery]",
            "interim_on_illustrative": it["stop_for_harm"], "render": r.stdout.strip()[-300:],
            "outputs": ["dryrun/RESULTS.synthetic.md", "dryrun/results.synthetic.json"]}


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--adage", default=DEFAULT_BIN)
    ap.add_argument("--out", default=str(HERE / "dryrun.json"))
    ap.add_argument("--revalidate", action="store_true")
    ap.add_argument("--families", nargs="*", default=F.ORDER)
    a = ap.parse_args(argv)
    adage = Path(a.adage)
    have_bin = adage.exists()
    res = {"when": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()), "adage": str(adage),
           "adage_version": subprocess.run([str(adage), "--version"], capture_output=True, text=True).stdout.strip()
           if have_bin else None, "spend_usd": 0.0}
    print("1. validation", flush=True)
    res["validation"] = step_validation(a.revalidate)
    print("2. configs", flush=True)
    res["configs"] = step_configs(str(adage))
    print("3. conditioning + probe (synthetic history)", flush=True)
    gates = HERE / "dryrun"
    gates.mkdir(exist_ok=True)
    if have_bin:
        res["conditioning"] = {}
        for fid in a.families:
            print(f"   {fid}", flush=True)
            res["conditioning"][fid] = step_conditioning(adage, fid, gates / f"gate.{fid}.json")
    else:
        res["conditioning"] = {"skipped": f"no binary at {adage}"}
    print("4. analysis on synthetic runs", flush=True)
    res["analysis"] = step_analysis(gates)
    cond = res["conditioning"]
    res["ok"] = {"validation": res["validation"]["ok"], "configs": res["configs"]["ok"],
                 "conditioning": ("skipped" if "skipped" in cond else {f: c["ok"] for f, c in cond.items()}),
                 "findings": ({} if "skipped" in cond else {f: c["findings"] for f, c in cond.items() if c["findings"]}),
                 "analysis": res["analysis"]["ok"]}
    Path(a.out).write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps(res["ok"], indent=1))
    flat = [res["ok"]["validation"], res["ok"]["configs"], res["ok"]["analysis"]] + (
        list(res["ok"]["conditioning"].values()) if isinstance(res["ok"]["conditioning"], dict) else [])
    return 0 if all(flat) else 1


if __name__ == "__main__":
    sys.exit(main())
