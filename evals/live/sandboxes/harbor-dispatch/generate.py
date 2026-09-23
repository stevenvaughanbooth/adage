#!/usr/bin/env python3
"""Generate the harbor-dispatch sandbox: a TypeScript service run directly on Node, with an ops queue
of incidents that only a repository-local tool can read.

  python3 generate.py <out_dir> [--seed N]

The output is a git repository with one commit. Generation is deterministic: the same seed gives
the same files, bytes and commit id. The seed only varies the decoy incidents in the ops queue
(their numbers, dates and order); the twelve task incidents are fixed.

Needs Node >= 23.6 (type stripping and `module.registerHooks`) on PATH; nothing is fetched.
"""

import json
import os
import random
import shutil
import subprocess
import sys
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
TEMPLATE = HERE / "template"
DEFAULT_SEED = 20260923
MAGIC = b"HDQ1"
REPORTERS = ["berth-planning", "yard-planning", "customer-service", "vessel-operations", "terminal-safety",
             "network-planning", "harbour-master", "stowage-planning"]
FIXED_DATE = "2026-09-20T09:00:00+00:00"


def node():
    exe = shutil.which("node")
    if not exe:
        raise SystemExit("generate.py: `node` is not on PATH (Node >= 23.6 is required)")
    out = subprocess.run([exe, "--version"], capture_output=True, text=True, check=True).stdout.strip()
    major, minor = (int(x) for x in out.lstrip("v").split(".")[:2])
    if (major, minor) < (23, 6):
        raise SystemExit(f"generate.py: Node {out} is too old (>= 23.6 needed for type stripping and registerHooks)")
    return exe


def incidents(seed):
    """The ops queue: the task incidents plus seeded decoys (closed, won't fix, duplicates)."""
    spec = json.loads((HERE / "incidents.json").read_text())
    rng = random.Random(seed)
    public = ("id", "title", "status", "priority", "area", "reported", "reporter", "description", "acceptance", "files")
    out = [{**{k: t[k] for k in public if k in t}, "status": "open"} for t in spec["tasks"]]
    taken = {int(t["id"].split("-")[1]) for t in spec["tasks"]}
    for d in spec["decoys"]:
        n = rng.choice([x for x in range(12, 300) if x not in taken])
        taken.add(n)
        month, day = rng.randint(1, 6), rng.randint(1, 28)
        out.append({
            "id": f"INC-{n}", "title": d["title"], "status": d["status"], "priority": rng.choice(["P2", "P3", "P4"]),
            "area": d["area"], "reported": f"2026-{month:02d}-{day:02d}", "reporter": rng.choice(REPORTERS),
            "description": d["description"], "acceptance": "None recorded.", "files": d["files"],
        })
    rng.shuffle(out)
    return out


def pack(items):
    body = json.dumps(items, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode()
    return MAGIC + zlib.compress(body, 9)


def git(out, *args):
    env = dict(os.environ, GIT_AUTHOR_NAME="Harbor Ops", GIT_AUTHOR_EMAIL="ops@harbor.example",
               GIT_COMMITTER_NAME="Harbor Ops", GIT_COMMITTER_EMAIL="ops@harbor.example",
               GIT_AUTHOR_DATE=FIXED_DATE, GIT_COMMITTER_DATE=FIXED_DATE, GIT_CONFIG_GLOBAL=os.devnull,
               GIT_CONFIG_NOSYSTEM="1")
    subprocess.run(["git", "-c", "init.defaultBranch=main", "-c", "commit.gpgsign=false", *args],
                   cwd=out, env=env, check=True, capture_output=True)


def generate(out_dir, seed=DEFAULT_SEED):
    out = Path(out_dir)
    if out.exists() and any(out.iterdir()):
        raise SystemExit(f"generate.py: {out} exists and is not empty")
    out.mkdir(parents=True, exist_ok=True)
    for src in sorted(TEMPLATE.rglob("*")):
        rel = src.relative_to(TEMPLATE)
        dest = out / rel
        if src.is_dir():
            dest.mkdir(parents=True, exist_ok=True)
        else:
            dest.parent.mkdir(parents=True, exist_ok=True)
            dest.write_bytes(src.read_bytes())
            dest.chmod(0o755 if rel.parts[0] == "bin" else 0o644)
    (out / ".ops").mkdir(exist_ok=True)
    (out / ".ops" / "incidents.pack").write_bytes(pack(incidents(seed)))
    (out / "src" / "generated").mkdir(parents=True, exist_ok=True)
    subprocess.run([node(), "tools/codegen.mjs"], cwd=out, check=True, capture_output=True)
    git(out, "init", "-q")
    git(out, "add", "-A")
    git(out, "commit", "-q", "-m", "harbor-dispatch 3.4.0")
    return out


def main(argv):
    seed = DEFAULT_SEED
    args = list(argv)
    if "--seed" in args:
        i = args.index("--seed")
        seed = int(args[i + 1])
        del args[i:i + 2]
    if len(args) != 1:
        raise SystemExit("usage: generate.py <out_dir> [--seed N]")
    generate(args[0], seed)
    print(f"generated harbor-dispatch in {args[0]} (seed {seed})")


if __name__ == "__main__":
    main(sys.argv[1:])
