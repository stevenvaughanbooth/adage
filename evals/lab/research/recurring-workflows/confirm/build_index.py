#!/usr/bin/env python3
"""Build the card hook's runtime index (hookenv/index.json) from a family's prior sessions.

  python3 build_index.py --root RUNNER_ROOT --family go-flag [--first N] --out hookenv/index.json

For the family's training runs in RUNNER_ROOT (chronological; `--first N` keeps only the first N):
the card (procedure.py, token budget 800) and the embeddings of their prompts (potion-base-8M).
tau / tau_low are the pre-registered detector's threshold, chosen on the training families only
(detection.json, potion-8M, n4 or n1 to match the library size). The index keys families by the
repository key the hook computes from `origin`.
"""

from __future__ import annotations

import argparse
import glob
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE.parents[3] / "repobench"))
import card_hook  # noqa: E402
import common  # noqa: E402
import families  # noqa: E402
import procedure  # noqa: E402

MODEL = "/tmp/adage-lab-models/yarflam-potion-base-8m-1.0.4/package/models"


def main(argv=None) -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--root", required=True)
    ap.add_argument("--family", required=True)
    ap.add_argument("--first", type=int)
    ap.add_argument("--out", required=True)
    ap.add_argument("--card-out")
    ap.add_argument("--max-tokens", type=int, default=800)
    a = ap.parse_args(argv)
    st = json.loads((Path(a.root) / "state.json").read_text())
    runs = sorted([r for r in st["runs"].values() if r.get("phase") == "train" and (r.get("metrics") or {}).get("session_id")],
                  key=lambda r: r["t0"])
    if a.first:
        runs = runs[: a.first]
    paths = [glob.glob(f"{a.root}/private/transcripts/*/{r['metrics']['session_id']}.jsonl")[0] for r in runs]
    passed = {r["metrics"]["session_id"]: bool(r["metrics"]["passed"]) for r in runs}
    views = [procedure.session_view(p) for p in paths]
    card = procedure.learn(views, passed)
    text = procedure.render(card, max_tokens=a.max_tokens)
    prompts = [r["prompt"] for r in runs]
    emb = card_hook.Static(Path(MODEL))
    det = json.loads((HERE / "detection.json").read_text())["potion-8M"]["n1" if len(runs) == 1 else "n4"]
    f = families.FAMILIES[a.family]
    key = common.repo_key(common.repo(f["repo"]))
    files = [p for p, _, _ in card["files"] if "<x>" not in p]
    patterns = [procedure.pattern_glob(p) for p, _, _ in card["files"] if "<x>" in p]
    index = {"model_dir": MODEL, "tau": det["tau"], "tau_low": det["tau_low"],
             "repos": {key: {"families": [{"name": a.family, "prompts": prompts, "emb": emb.encode(prompts).tolist(),
                                           "card": text, "files": files, "patterns": patterns, "sessions": len(runs)}]}}}
    Path(a.out).parent.mkdir(parents=True, exist_ok=True)
    Path(a.out).write_text(json.dumps(index))
    if a.card_out:
        Path(a.card_out).write_text(text)
    print(text)
    print(f"-- {len(runs)} sessions, key {key}, tau {det['tau']}, files {files}, patterns {patterns}, ~{len(text) // 4} tokens",
          file=sys.stderr)
    return 0


if __name__ == "__main__":
    sys.exit(main())
