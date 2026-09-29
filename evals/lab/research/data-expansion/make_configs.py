#!/usr/bin/env python3
"""Runner configs for the data-expansion corpus.

  python3 make_configs.py new <repo-id> [--n 10] [--repeats 2] [--budget 7]
  python3 make_configs.py extra <click|vue-core> [--n 9] [--budget 5]
  python3 make_configs.py small <repo-id> [--repeats 2] [--budget 3]

`new`: the latest --n validated, prompted tasks of a new repository, split chronologically with
evals/repobench/build.py (external protocol; --buffer 0 so 10 tasks give 5 train / 5 test). The
split goes to evals/repobench/splits/<repo>.json (with its SHA-256 pre-registration digest); the
config to configs/<repo>.json here: train (no hooks) and eval arm `control` only (hooks on,
directives withheld), both --repeats times, Claude Haiku 4.5, 60 turns (the v2 protocol).

`extra`: more training sessions for an existing repository, on validated tasks OLDER than its
published split (never a buffer or test task), newest first, one repeat. The eval block names the
published test tasks only so the runner accepts the config; only the train phase is run.
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
RB = HERE.parents[2] / "repobench"
sys.path.insert(0, str(RB))
import build  # noqa: E402
import common  # noqa: E402

CONFIGS = HERE / "configs"
MODEL = "claude-haiku-4-5-20251001"
MAX_TURNS = 60

# Per-language extras: rustup's toolchain lives under /root (hidden by confine), so sessions get
# RUSTUP_HOME and a read-only mount of it; their CARGO_HOME stays in the session's own home.
EXTRA = {"rust": {"forward_env": ["RUSTUP_HOME"], "confine_expose": ["/root/.rustup"]}}


def finish(cfg: dict, budget: float, name: str) -> dict:
    cfg.update(name=name, model=MODEL, max_turns=MAX_TURNS, concurrency=2, budget_usd=budget, learn_args=["--demo"])
    return cfg


def fix_size(rid: str, t: dict) -> int:
    """Lines the real fix changed in non-test files (git numstat on the mirror)."""
    m = common.mirror(common.repo(rid))
    out = common.git(m, "diff", "--numstat", t["base"], t["sha"], "--", *(t.get("gold_files") or t["src_files"]))
    return sum(int(a) + int(d) for a, d, *_ in (x.split("\t") for x in out.splitlines()) if a.isdigit() and d.isdigit())


def select_by_size(rid: str, n: int) -> list[dict]:
    """The n prompted tasks whose real fix is largest (harder tasks: a realistic pass/fail spread
    rather than one-line fixes), returned in fix-date order for the chronological split."""
    tasks = [t for t in common.load_tasks(rid)["tasks"] if t.get("prompt")]
    sized = sorted(tasks, key=lambda t: (-fix_size(rid, t), t["date"]))[:n]
    return sorted(sized, key=lambda t: (t["date"], t["sha"]))


def new(rid: str, n: int, repeats: int, budget: float, by_size: bool = False) -> Path:
    tasks = select_by_size(rid, n) if by_size else build.select(rid, n)
    sp = build.split(rid, tasks, 0.5, 0)
    (build.SPLITS / f"{rid}.json").write_text(json.dumps(sp, indent=1) + "\n")
    cfg = build.config(rid, sp, tasks, ["control"], repeats, repeats, MODEL, MAX_TURNS, budget)
    cfg = finish(cfg, budget, f"dx-{rid}")
    cfg.update(EXTRA.get(common.repo(rid)["lang"], {}))
    CONFIGS.mkdir(exist_ok=True)
    out = CONFIGS / f"{rid}.json"
    out.write_text(json.dumps(cfg, indent=1) + "\n")
    part = sp["repos"][rid]
    print(f"{rid}: train {len(part['train'])} test {len(part['test'])} x{repeats}; digest {sp['digest'][:12]} -> {out}")
    return out


def small(rid: str, repeats: int, budget: float) -> Path:
    """A repository with too few validated tasks for a chronological split (< 10): every prompted
    task is held out and worked `repeats` times in the control arm (unsteered sessions for
    cross-repository work; nothing is trained on this repository's own sessions)."""
    import hashlib
    tasks = sorted([t for t in common.load_tasks(rid)["tasks"] if t.get("prompt")], key=lambda t: (t["date"], t["sha"]))
    part = {"train": [], "buffer": [], "test": [t["id"] for t in tasks], "cutoff": tasks[0]["date"],
            "first_test": tasks[0]["date"], "note": "all held out: too few tasks for a chronological split"}
    sp = {"protocol": "all-held-out (data-expansion small repo)", "repos": {rid: part}, "excluded": {}}
    sp["digest"] = hashlib.sha256(json.dumps(sp, sort_keys=True).encode()).hexdigest()
    sp["repo_key"] = common.repo_key(common.repo(rid))
    (build.SPLITS / f"{rid}.json").write_text(json.dumps(sp, indent=1) + "\n")
    cfg = build.config(rid, sp, tasks, ["control"], 1, repeats, MODEL, MAX_TURNS, budget)
    cfg = finish(cfg, budget, f"dx-{rid}")
    cfg.update(EXTRA.get(common.repo(rid)["lang"], {}))
    CONFIGS.mkdir(exist_ok=True)
    out = CONFIGS / f"{rid}.json"
    out.write_text(json.dumps(cfg, indent=1) + "\n")
    print(f"{rid}: {len(tasks)} tasks, all held out, x{repeats} control -> {out}")
    return out


def extra(rid: str, n: int, budget: float) -> Path:
    base = json.loads((RB / "configs" / f"{rid}.json").read_text())
    sp = json.loads((RB / "splits" / f"{rid}.json").read_text())["repos"][rid]
    used = set(sp["train"]) | set(sp["buffer"]) | set(sp["test"])
    first = min(t["date"] for t in common.load_tasks(rid)["tasks"] if t["id"] in set(sp["train"]))
    older = [t for t in common.load_tasks(rid)["tasks"] if t["id"] not in used and t.get("prompt") and t["date"] < first]
    older.sort(key=lambda t: (t["date"], t["sha"]))
    pick = older[-n:]
    cfg = dict(base)
    cfg["task_prompts"] = {**{t["id"]: t["prompt"] for t in pick}, **{k: v for k, v in base["task_prompts"].items()
                                                                      if k in set(sp["test"])}}
    cfg["train"] = {"tasks": [t["id"] for t in pick], "repeats": 1}
    cfg["eval"] = {"tasks": list(sp["test"]), "repeats": 1, "arms": ["control"]}
    cfg = finish(cfg, budget, f"dx-{rid}-train2")
    CONFIGS.mkdir(exist_ok=True)
    out = CONFIGS / f"{rid}-train2.json"
    out.write_text(json.dumps(cfg, indent=1) + "\n")
    print(f"{rid}: {len(pick)} extra training tasks ({pick[0]['date'][:10]}..{pick[-1]['date'][:10]}, all before "
          f"{first[:10]}) -> {out}")
    return out


def main(argv=None) -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("mode", choices=["new", "extra", "small"])
    ap.add_argument("repo")
    ap.add_argument("--n", type=int)
    ap.add_argument("--repeats", type=int, default=2)
    ap.add_argument("--budget", type=float, default=7.0)
    ap.add_argument("--by-size", action="store_true", help="pick the n tasks with the largest real fix")
    a = ap.parse_args(argv)
    if a.mode == "new":
        new(a.repo, a.n or 10, a.repeats, a.budget, a.by_size)
    elif a.mode == "small":
        small(a.repo, a.repeats, a.budget)
    else:
        extra(a.repo, a.n or 9, a.budget)
    return 0


if __name__ == "__main__":
    sys.exit(main())
