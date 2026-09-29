#!/usr/bin/env python3
"""Dry run of the whole GO-validation pipeline, with no spend (no `claude` session, no model call).

  python3 dryrun.py --adage BIN [--out dryrun.json] [--skip-validate]

  1. validation   validation.json: every variant fails on the base and passes with its gold patch
                  (run validate.py first, or without --skip-validate this re-runs it: about 7 minutes)
  2. configs      both configs parse through gv_run.py + the runner's own dry run (the plan as JSON):
                  session counts, arms, prompts, isolation, host auth, confinement, no forbidden
                  variable forwarded, the training sessions' environment (ADAGE_HOME only), the
                  confinement of training sessions (no PID namespace, so the learner survives), the
                  learner-wait logic on a synthetic queue, the product check of BIN
  3. conditioning the product's own learner (`adage condition`, the same learn and card parameters as
                  the background `condition --auto`) over 4 SYNTHETIC training transcripts built from
                  the gold patches (the edits, `pnpm automd`, `pnpm vitest run`, `pnpm lint`), then
                  detect_check.py through the installed hook: does a card form, which held-out
                  prompts get it, does any negative
  4. analysis     analyze.py --synthetic: the pre-registered analysis on synthetic runs with known
                  effects, its operating characteristics, CI coverage and power

BIN should be the product under test. Until it is merged, the card-capable build of the card branch
works (worktree-agent-ae7d626216059442c: .claude/worktrees/agent-ae7d626216059442c/target/debug/adage).
"""

from __future__ import annotations

import argparse
import json
import os
import subprocess
import sys
import tempfile
import time
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(REPO / "evals" / "live"))
import family_ufo as fam  # noqa: E402

FORBIDDEN = ("CLAUDECODE", "CLAUDE_CODE_SESSION_ID")


def step_validation(skip: bool) -> dict:
    if not skip:
        subprocess.run([sys.executable, str(HERE / "validate.py")], check=False)
    v = json.loads((HERE / "validation.json").read_text())
    return {"ok": v["valid"] == v["of"] == len(fam.order()), "valid": v["valid"], "of": v["of"],
            "gold_check_seconds_max": max(r["gold_check_seconds"] for r in v["variants"]),
            "base_failed_checks": sorted({k for r in v["variants"] for k, x in r["base_checks"].items() if not x})}


def plan(cfg: str, phases: list[str], adage: Path, root: Path) -> dict:
    env = {k: v for k, v in os.environ.items() if k not in FORBIDDEN}
    out = subprocess.run([sys.executable, str(HERE / "gv_run.py"), str(HERE / "configs" / cfg), *phases, "--json",
                          "--host-auth", "--root", str(root), "--adage", str(adage)], capture_output=True, text=True,
                         env=env, cwd=HERE)
    if out.returncode != 0:
        raise SystemExit(f"gv_run.py {cfg}: {out.stderr[-2000:]}")
    first, _, rest = out.stdout.partition("\n")
    return {"product_check": json.loads(first.split(": ", 1)[1]), "plan": json.loads(rest)}


def step_configs(adage: Path) -> dict:
    import gv_run
    import runner

    res = {}
    with tempfile.TemporaryDirectory() as t:
        f = plan("ufo-util.json", ["train", "eval", "report"], adage, Path(t) / "ufo-util")
        r = plan("click-regression.json", ["learn", "eval", "report"], adage, Path(t) / "click-regression")
    fp, rp = f["plan"], r["plan"]
    fcfg = json.loads((HERE / "configs/ufo-util.json").read_text())
    rcfg = json.loads((HERE / "configs/click-regression.json").read_text())
    loaded = runner.load_config(HERE / "configs/ufo-util.json", {"host_auth": True})
    train_env = [s["env"] for s in fp["sessions"] if s["phase"] == "train"]
    eval_env = {s["arm"]: s["env"] for s in fp["sessions"] if s["phase"] == "eval"}
    prompts_ok = all(s["prompt"] == fam.prompt(fam.variant(s["task"])) for s in fp["sessions"])
    res["family"] = {
        "sessions": fp["totals"]["sessions"], "by_arm": fp["totals"]["by_arm"],
        "expected": {"train": 4, "control": 16, "adage": 16},
        "train_env": train_env[0] if train_env else None, "eval_env": eval_env,
        "train_settings": sorted({Path(s["command"][s["command"].index("--settings") + 1]).name
                                  for s in fp["sessions"] if s["phase"] == "train"}),
        "prompts_match_family": prompts_ok, "held_out_disjoint_from_train":
            not (set(fcfg["train"]["tasks"]) & set(fcfg["eval"]["tasks"])),
        "cap_usd": fp["totals"]["budget_usd"], "max_turns": fp["totals"]["max_turns_each"],
        "host_auth": loaded.get("host_auth"), "confine": loaded.get("confine"),
        "forward_env_forbidden": [k for k in loaded["forward_env"] if runner.forbidden_forward(k)],
        "forward_env_has_session_markers": [k for k in loaded["forward_env"] if k in FORBIDDEN],
        "learn_phase_refused": "train_as_user" in json.dumps(fcfg.get("go_validation")),
    }
    res["family"]["ok"] = (res["family"]["by_arm"] == {"adage": 16, "control": 16, "train": 4} and prompts_ok
                           and res["family"]["train_env"] == {"ADAGE_HOME": res["family"]["train_env"]["ADAGE_HOME"]}
                           and res["family"]["train_settings"] == ["settings-hooks.json"]
                           and eval_env["adage"].get("ADAGE_ARM") == "treated"
                           and eval_env["control"].get("ADAGE_ARM") == "control"
                           and eval_env["adage"].get("ADAGE_AUTO_LEARN") == "0"
                           and not res["family"]["forward_env_forbidden"] and loaded["confine"] and loaded["host_auth"])
    rl = runner.load_config(HERE / "configs/click-regression.json", {"host_auth": True})
    res["regression"] = {"sessions": rp["totals"]["sessions"], "by_arm": rp["totals"]["by_arm"],
                         "eval_tasks": rcfg["eval"]["tasks"], "learn": rp["learn"][:5], "learn_passes_outcomes": "--outcomes" in rp["learn"],
                         "cap_usd": rp["totals"]["budget_usd"], "max_turns": rp["totals"]["max_turns_each"],
                         "confine": rl.get("confine"), "host_auth": rl.get("host_auth"),
                         "forward_env_forbidden": [k for k in rl["forward_env"] if runner.forbidden_forward(k)],
                         "seed_from_exists": Path(rcfg["go_validation"]["seed_train_from"], "state.json").exists()}
    res["regression"]["ok"] = (rp["totals"]["by_arm"] == {"adage": 6, "control": 6} and rl["confine"]
                               and rl["host_auth"] and not res["regression"]["forward_env_forbidden"]
                               and res["regression"]["seed_from_exists"] and not res["regression"]["learn_passes_outcomes"])
    # confinement of a training session, and the learner wait on a synthetic queue
    gv_run.install({"train_as_user": True, "learner_wait_s": 5})
    with tempfile.TemporaryDirectory() as t:
        t = Path(t)
        env = {"PATH": os.environ.get("PATH", "")}
        tr = runner.confine_argv({}, t / "runs" / "train-x", env)
        ev = runner.confine_argv({}, t / "runs" / "eval-x", env)
        q = t / "home" / "learn-queue"
        q.mkdir(parents=True)
        (q / "github-com-unjs-ufo.json").write_text(json.dumps({"requested_at": 10}))
        busy1 = gv_run.learner_busy(t / "home")
        (q / "github-com-unjs-ufo.state.json").write_text(json.dumps({"outcome": "ok", "processed": 10}))
        busy2 = gv_run.learner_busy(t / "home")
        (q / "github-com-unjs-ufo.spawn.json").write_text(json.dumps({"at": 1, "after": 2}))
        busy3 = gv_run.learner_busy(t / "home")
    res["confinement"] = {"train_has_pid_namespace": "--unshare-pid" in tr, "eval_has_pid_namespace": "--unshare-pid" in ev,
                          "learner_busy_pending": busy1, "learner_busy_after_processed": busy2,
                          "learner_busy_with_spawn_file": busy3}
    res["confinement"]["ok"] = (not res["confinement"]["train_has_pid_namespace"] and res["confinement"]["eval_has_pid_namespace"]
                                and busy1 == ["github-com-unjs-ufo"] and busy2 == [] and busy3 == [])
    res["product_check"] = f["product_check"]
    res["ok"] = res["family"]["ok"] and res["regression"]["ok"] and res["confinement"]["ok"]
    return res


# ------------------------------------------------------------------- synthetic training history

def transcript(tid: str, i: int, cwd: str) -> list[dict]:
    """A training session in Claude Code's on-disk format, built from the gold patch."""
    v = fam.variant(tid)
    sid = f"00000000-0000-4000-8000-{i:012d}"
    lines, n = [], 0
    ts = f"2026-09-2{i}T10:00:00Z"

    def call(name, inp, out, err=False):
        nonlocal n
        n += 1
        tid_ = f"toolu_{i}_{n}"
        lines.append({"type": "assistant", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                      "message": {"id": f"msg_{i}_{n}", "model": "claude-haiku-4-5-20251001",
                                  "content": [{"type": "tool_use", "id": tid_, "name": name, "input": inp}]}})
        lines.append({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                      "message": {"role": "user", "content": [{"type": "tool_result", "tool_use_id": tid_,
                                                               "content": out, "is_error": err}]}})

    lines.append({"type": "user", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                  "message": {"role": "user", "content": fam.prompt(v)}})
    call("Bash", {"command": "ls"}, "README.md\npackage.json\nsrc\ntest")
    call("Read", {"file_path": f"{cwd}/src/utils.ts"}, "export function withoutHost(input: string) {")
    call("Grep", {"pattern": "automd", "path": cwd}, "README.md:<!-- automd:jsdocs src=./src defaultGroup=utils -->")
    anchor = '  return (parsed.pathname || "/") + parsed.search + parsed.hash;\n}'
    ex = "\n".join(" * " + x for x in fam.example_lines(v).splitlines())
    body = "\n".join("  " + x.strip() for x in v["body"].strip().splitlines())
    code = (f"\n\n/**\n * {v['desc']}\n *\n * @example\n *\n * ```js\n{ex}\n * ```\n *\n * @group utils\n */\n"
            f"export function {v['name']}({v['ts_sig']} {{\n{body}\n}}")
    call("Edit", {"file_path": f"{cwd}/src/utils.ts", "old_string": anchor, "new_string": anchor + code}, "ok")
    test = (f'\ndescribe("{v["name"]}", () => {{\n  test("examples", () => {{\n'
            f'    expect({v["name"]}({", ".join(fam.js(a) for a in v["cases"][0][0])})).toEqual({fam.js(v["cases"][0][1])});\n  }});\n}});\n')
    call("Edit", {"file_path": f"{cwd}/test/utilities.test.ts", "old_string": "  withoutHost,\n} from \"../src\";",
                  "new_string": f"  withoutHost,\n  {v['name']},\n}} from \"../src\";"}, "ok")
    call("Edit", {"file_path": f"{cwd}/test/utilities.test.ts", "old_string": "describe(\"withoutHost\", () => {",
                  "new_string": test + "\ndescribe(\"withoutHost\", () => {"}, "ok")
    call("Bash", {"command": "python -m pytest"}, "Exit code 1\n/usr/bin/python: No module named pytest", True)
    call("Bash", {"command": "pnpm vitest run test/utilities.test.ts"}, " Test Files  1 passed (1)\n      Tests  60 passed (60)")
    call("Bash", {"command": "pnpm automd"}, "Automd updated\n README.md updated")
    call("Bash", {"command": "pnpm lint"}, "Checking formatting...\nAll matched files use Prettier code style!")
    call("Bash", {"command": "pnpm vitest run"}, " Test Files  14 passed (14)\n      Tests  497 passed (497)")
    lines.append({"type": "assistant", "sessionId": sid, "cwd": cwd, "timestamp": ts,
                  "message": {"id": f"msg_{i}_end", "model": "claude-haiku-4-5-20251001", "stop_reason": "end_turn",
                              "content": [{"type": "text", "text": f"Added `{v['name']}` with tests and README docs."}]}})
    return lines


def step_conditioning(adage: Path, keep: Path | None) -> dict:
    with tempfile.TemporaryDirectory(prefix="gv-dry-") as t:
        t = Path(t)
        home, tx = t / "adage-home", t / "transcripts"
        tx.mkdir()
        files = []
        for i, tid in enumerate(fam.train_tasks(), start=1):
            cwd = f"/tmp/adage-launch/go-validation/runs/ufo-util/runs/train-{tid}-r1/repo"
            p = tx / f"s{i}.jsonl"
            p.write_text("\n".join(json.dumps(x) for x in transcript(tid, i, cwd)) + "\n")
            files.append(str(p))
        env = {k: v for k, v in os.environ.items() if k not in FORBIDDEN}
        env.update(ADAGE_HOME=str(home), HOME=str(t / "user"), XDG_DATA_HOME=str(t / "user/.local/share"))
        t0 = time.time()
        out = subprocess.run([str(adage), "condition", "--json", "--repo", "github.com/unjs/ufo", "--transcripts", *files],
                             capture_output=True, text=True, env=env)
        cond = {"exit": out.returncode, "seconds": round(time.time() - t0, 2), "stderr": out.stderr[-800:]}
        try:
            rep = json.loads(out.stdout)
            cond["report_keys"] = sorted(rep)[:30]
            cond["report_cards"] = {k: rep[k] for k in rep if "card" in k.lower() or "famil" in k.lower()}
        except ValueError:
            cond["stdout_tail"] = out.stdout[-800:]
        sense = subprocess.run([str(adage), "sense"], capture_output=True, text=True, env=env, cwd=t)
        cond["sense_recurring"] = [x.strip() for x in (sense.stdout + sense.stderr).splitlines() if "recurring" in x.lower()
                                   or "card" in x.lower()][:6]
        gate = t / "gate.json"
        g = subprocess.run([sys.executable, str(HERE / "detect_check.py"), "--home", str(home), "--adage", str(adage),
                            "--out", str(gate)], capture_output=True, text=True, env=env)
        cond["detect_exit"] = g.returncode
        cond["detect_stderr"] = g.stderr[-1500:]
        if gate.exists():
            gj = json.loads(gate.read_text())
            cond["gate"] = gj["summary"]
            cond["held_out"] = [{k: r[k] for k in ("task", "at_prompt", "at_edit", "delivered")} for r in gj["held_out"]]
            cond["negatives"] = [{k: r[k] for k in ("id", "at_prompt", "at_edit", "delivered")} for r in gj["negatives"]]
            cond["hook_events"] = gj.get("hook_events")
            if keep:
                keep.write_text(json.dumps(gj, indent=1) + "\n")
    cond["note"] = ("synthetic training transcripts built from the gold patches: this checks the mechanics (the "
                    "product learns a card for this family and the hook delivers it on these prompts), not what "
                    "real agents will do")
    return cond


def step_regression_conditioning(adage: Path) -> dict:
    """The regression slice's conditioning, exactly as gv_run.py does it (`adage condition`, default
    thresholds, no outcomes, the 10 archived click training transcripts), then its 6 held-out prompts
    through the installed hook: a card formed from one-off bug fixes must not reach them (G4c)."""
    import detect_check as dc
    import gv_run
    import runner
    sys.path.insert(0, str(REPO / "evals" / "repobench"))
    import common
    import sandbox as rb_sandbox

    cfgp = HERE / "configs" / "click-regression.json"
    raw = json.loads(cfgp.read_text())
    gv_run.install(raw["go_validation"])
    cfg = runner.load_config(cfgp, {"host_auth": True})
    out = {}
    with tempfile.TemporaryDirectory(prefix="gv-reg-") as t:
        t = Path(t)
        root = t / "click-regression"
        gv_run.seed(root, Path(raw["go_validation"]["seed_train_from"]), cfg)
        live = runner.Live(cfg, root, adage, 1)
        live.learn()
        routes = [e["route"]["id"] for e in json.loads((live.home / "index.json").read_text())["routes"]]
        out["cards"] = [r for r in routes if r.startswith("card-")]
        out["u11_routes"] = sum(1 for r in routes if r.startswith(("dead-end-", "instinct-u11")))
        rep = live.state["rounds"]["1"]["learn"]["repos"].get(cfg["repo_key"], {}).get("engine", {})
        out["card_families"], out["cards_compiled"] = rep.get("card_families"), rep.get("cards")
        r = common.repo("click")
        tasks = {x["id"]: x for x in common.load_tasks("click")["tasks"]}
        base = tasks[cfg["eval"]["tasks"][0]]["base"]
        def click_checkout(d):  # RepoBench's click mirror lives in its default cache, not the ufo one
            saved = os.environ.get("REPOBENCH_CACHE")
            os.environ["REPOBENCH_CACHE"] = raw["go_validation"].get("click_cache", "/tmp/adage-repobench")
            try:
                rb_sandbox.create(r, base, d, setup=False)
            finally:
                os.environ["REPOBENCH_CACHE"] = saved or ""

        probe = dc.Probe(adage, live.home, t / "probe", click_checkout)
        rows = []
        for tid in cfg["eval"]["tasks"]:
            edits = tasks[tid].get("src_files") or ["src/click/core.py"]
            x = probe.session(cfg["task_prompts"][tid], edits)
            rows.append({"task": tid, "at_prompt": x["at_prompt"], "at_edit": x["at_edit"], "delivered": x["delivered"]})
        import fnmatch
        files = [f["path"].replace("<x>", "*") for e in json.loads((live.home / "index.json").read_text())["routes"]
                 if e["route"]["id"].startswith("card-") for f in e["route"]["step"]["card"]["files"]]
        for x in rows:
            fix = list(tasks[x["task"]].get("src_files") or []) + list(tasks[x["task"]].get("test_files") or [])
            x["in_card_area"] = any(fnmatch.fnmatch(f, g) for f in fix for g in files)
        out["held_out_probe"] = rows
        out["card_delivered"] = sum(x["delivered"] for x in rows)
        out["false_fires"] = [x["task"] for x in rows if x["delivered"] and not x["in_card_area"]]
    out["ok"] = not out["false_fires"]
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--adage", required=True)
    ap.add_argument("--out", default=str(HERE / "dryrun.json"))
    ap.add_argument("--skip-validate", action="store_true")
    a = ap.parse_args(argv)
    adage = Path(a.adage).resolve()
    res = {"adage": str(adage), "when": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())}
    print("1. validation", flush=True)
    res["validation"] = step_validation(a.skip_validate)
    print("2. configs", flush=True)
    res["configs"] = step_configs(adage)
    print("3. conditioning + detection (synthetic history)", flush=True)
    res["conditioning"] = step_conditioning(adage, HERE / "dryrun_gate.json")
    print("3b. regression conditioning + probe (archived click history)", flush=True)
    res["regression_conditioning"] = step_regression_conditioning(adage)
    print("4. analysis on synthetic runs", flush=True)
    out = subprocess.run([sys.executable, str(HERE / "analyze.py"), "--synthetic"], capture_output=True, text=True)
    syn = json.loads(out.stdout)
    res["analysis"] = {"checks_pass": syn["checks_pass"], "checks": syn["checks"],
                       "operating_characteristics": syn["operating_characteristics"],
                       "ci90_coverage_8_clusters": syn["ci90_coverage_8_clusters"],
                       "power_G2_at_true_-20pct": syn["power_G2_at_true_-20pct"],
                       "illustrative": {k: {"usd": v["usd"], "verdict": v["verdict"]} for k, v in syn["scenarios"].items()}}
    g = res["conditioning"].get("gate") or {}
    res["ok"] = {"validation": res["validation"]["ok"], "configs": res["configs"]["ok"],
                 "card_formed": bool(g.get("gate_R2_card_formed")), "detection_gate": bool(g.get("gate_R3_detection")),
                 "no_false_fire": (g.get("false_fires") or [1])[0] == 0, "analysis": syn["checks_pass"],
                 "regression_no_false_fire": res["regression_conditioning"]["ok"]}
    Path(a.out).write_text(json.dumps(res, indent=1) + "\n")
    print(json.dumps(res["ok"], indent=1))
    return 0 if all(res["ok"].values()) else 1


if __name__ == "__main__":
    sys.exit(main())
