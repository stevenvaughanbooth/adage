"""The showcase's five recurring-task families (registry and task ids).

  family          repository @ base                 ecosystem            source
  click-type      pallets/click @ 06b2a67           Python (uv, pytest)  ported (recurring-workflows)
  mm-validator    marshmallow @ 7f0792bd            Python (uv, pytest)  ported (recurring-workflows)
  go-flag         urfave/cli v3 @ 58ec406e          Go                   ported (recurring-workflows/confirm)
  http-header     hyperium/http @ fdc6ab8           Rust (cargo)         new (data-expansion corpus repository)
  chi-middleware  go-chi/chi v5 @ 3d1777a           Go                   new (data-expansion corpus repository)

Every family has 12 variants in a fixed order: the first TRAIN_N (4) are the training sessions (the
user's history, learned by the product itself), the other 8 are held out. Each family also has 6
same-repository negative prompts (3 RepoBench one-off fixes in the family's files, 3 hard negatives)
for the false-fire probe. Task ids are `<family>--<variant>`.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[1]
sys.path.insert(0, str(HERE))

import fam_chi  # noqa: E402
import fam_http  # noqa: E402
import fam_python  # noqa: E402
import fam_urfave  # noqa: E402

TRAIN_N = 4
ORDER = ["click-type", "mm-validator", "go-flag", "http-header", "chi-middleware"]
FAMILIES = {f["id"]: f for f in (fam_python.CLICK, fam_python.MM, fam_urfave.FAMILY, fam_http.FAMILY,
                                 fam_chi.FAMILY)}
assert list(FAMILIES) == ORDER


def task_id(fid: str, vid: str) -> str:
    return f"{fid}--{vid}"


def parse_task(tid: str) -> tuple[dict, dict]:
    fid, vid = tid.split("--", 1)
    f = FAMILIES[fid]
    return f, next(v for v in f["variants"] if v["id"] == vid)


def order(fid: str) -> list[str]:
    return [task_id(fid, v["id"]) for v in FAMILIES[fid]["variants"]]


def train_tasks(fid: str) -> list[str]:
    return order(fid)[:TRAIN_N]


def held_out_tasks(fid: str) -> list[str]:
    return order(fid)[TRAIN_N:]


def name(tid: str) -> str:
    f, v = parse_task(tid)
    return v[f["name_key"]]


def prompt(tid: str) -> str:
    f, v = parse_task(tid)
    return f["prompt"](v)


def probe_edits(tid: str) -> list[str]:
    """The files a session of this variant would edit (the probe's grey-zone file signal)."""
    f, v = parse_task(tid)
    pe = f["probe_edits"]
    if callable(pe):
        return pe(v)
    if f["id"] == "go-flag":
        return [f"flag_{v['id']}.go", f"flag_{v['id']}_test.go", "docs/v3/examples/flags/basics.md"]
    return list(pe)


def negatives(fid: str) -> list[dict]:
    """The family's 6 negative prompts, each with the files its task edits. RepoBench ones take their
    prompt verbatim and their real fix's files from evals/repobench/tasks/<repo>.json."""
    f = FAMILIES[fid]
    tasks = {t["id"]: t for t in json.loads((REPO_ROOT / "evals/repobench/tasks" / f"{f['repo']}.json")
                                            .read_text())["tasks"]}
    out = []
    for n in f["negatives"]:
        if n["source"] == "repobench":
            t = tasks[n["id"]]
            out.append(dict(id=n["id"], source="repobench", prompt=t["prompt"],
                            edits=list(t["src_files"]) + list(t.get("test_files") or [])[:1]))
        else:
            out.append(dict(n))
    return out


if __name__ == "__main__":
    for fid in ORDER:
        f = FAMILIES[fid]
        print(f"== {fid} ({f['repo']} @ {f['base']}, {len(f['variants'])} variants, {len(f['negatives'])} negatives)")
        for i, tid in enumerate(order(fid)):
            print(f"  {'train' if i < TRAIN_N else 'held '} {tid} | {prompt(tid)[:150]!r}")
