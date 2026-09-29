#!/usr/bin/env python3
"""Write the showcase's runner configs and its budget (no tokens):

  configs/<family>.json         Claude Haiku 4.5, the primary: 4 training sessions (the user's history,
                                product installed, learning by itself), then 8 held-out variants x 2
                                repeats x {control, adage}, paired by (variant, repeat). Exactly the GO
                                validation's protocol (evals/launch/go-validation/make_configs.py).
  configs/<family>-sonnet.json  Claude Sonnet 5, the smaller arm on mm-validator and chi-middleware: the
                                first 6 held-out variants x 1 repeat x {control, adage}, on the index the
                                product learned from that family's Haiku training sessions (copied, no
                                new training)
  budget.json                   per-session estimates, sessions and dollars per stage, caps

  python3 make_configs.py
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import families as F  # noqa: E402

RUNS = "/tmp/adage-showcase/runs"
HAIKU = "claude-haiku-4-5-20251001"
SONNET = "claude-sonnet-5"
SEED = 20260929
GLOBAL_CAP = 60.0
SONNET_FAMILIES = ["mm-validator", "chi-middleware"]
SONNET_HELD_OUT = 6
PESSIMISTIC = 1.3   # pessimistic / central, per session (the GO validation used 1.33-1.5)
SONNET_FACTOR = 1.4  # Sonnet 5 session $ / Haiku 4.5 session $: 2x the per-token price, ~0.7x the tokens

# Per-session $ (central): training ~ a control session; held out ~ the mean of control and adage.
EST = {
    "click-type": dict(train=0.40, held=0.37, basis="research live: control $0.399, card $0.334 (12 each)"),
    "mm-validator": dict(train=0.235, held=0.21, basis="research live: control $0.235, card $0.181 (12 each)"),
    "go-flag": dict(train=0.35, held=0.29, basis="confirmation live: control $0.347 (6/12 hit 60 turns), card $0.230"),
    "http-header": dict(train=0.33, held=0.30,
                        basis="data-expansion http bug fixes $0.481 (HeaderMap internals, 6 sessions); a header "
                              "constant is a smaller edit but compiles the crate several times; bytes $0.329"),
    "chi-middleware": dict(train=0.26, held=0.23,
                           basis="data-expansion chi bug fixes $0.214 (10 sessions); families cost 1.0-1.5x "
                                 "their repository's bug fixes"),
}

VERIFY = ["pytest", "go test", "cargo test", "cargo build", "cargo check", "make test", "tox", "nox", "unittest",
          "go vet", "go build", "gofmt", "cargo clippy", "cargo fmt"]
BASH_EDIT = ["sed -i", "cat >", "cat <<", "tee ", "write_text(", "patch -p", "git apply", "perl -pi"]
REGEN = {"go-flag": ["go doc", "make generate", "v3approve", "make v3approve"]}


def root(fid: str, sonnet: bool = False) -> str:
    return f"{RUNS}/{fid}{'-sonnet' if sonnet else ''}"


def all_roots() -> list[str]:
    return [root(f) for f in F.ORDER] + [root(f, True) for f in SONNET_FAMILIES]


def budget() -> dict:
    fam = {}
    for fid in F.ORDER:
        e = EST[fid]
        tr = len(F.train_tasks(fid)) * e["train"]
        ho = len(F.held_out_tasks(fid)) * 2 * 2 * e["held"]
        fam[fid] = {"per_session_usd": {"train": e["train"], "held_out": e["held"]}, "basis": e["basis"],
                    "sessions": {"train": 4, "held_out": 32}, "central_usd": round(tr + ho, 2),
                    "repeat1_usd": round(tr + ho / 2, 2), "cap_usd": round((tr + ho) * PESSIMISTIC, 2)}
    son = {}
    for fid in SONNET_FAMILIES:
        s = EST[fid]["held"] * SONNET_FACTOR
        son[fid] = {"per_session_usd": round(s, 3), "sessions": SONNET_HELD_OUT * 2,
                    "central_usd": round(SONNET_HELD_OUT * 2 * s, 2),
                    "cap_usd": round(SONNET_HELD_OUT * 2 * s * PESSIMISTIC, 2)}
    haiku = round(sum(x["central_usd"] for x in fam.values()), 2)
    sonnet = round(sum(x["central_usd"] for x in son.values()), 2)
    return {
        "global_cap_usd": GLOBAL_CAP, "pessimistic_factor": PESSIMISTIC, "sonnet_factor": SONNET_FACTOR,
        "pricing_per_mtok": {"claude-haiku-4-5": [1.0, 5.0], "claude-sonnet-5": [2.0, 10.0]},
        "haiku": fam, "sonnet": son,
        "sessions": {"haiku_train": 20, "haiku_held_out": 160, "sonnet_held_out": 24, "total": 204},
        "central_usd": {"haiku": haiku, "sonnet": sonnet, "total": round(haiku + sonnet, 2)},
        "pessimistic_usd": {"haiku": round(haiku * PESSIMISTIC, 2), "sonnet": round(sonnet * PESSIMISTIC, 2),
                            "total": round((haiku + sonnet) * PESSIMISTIC, 2)},
        "stages_central_usd": {
            "A_training": round(sum(len(F.train_tasks(f)) * EST[f]["train"] for f in F.ORDER), 2),
            "B_held_out_repeat_1": round(sum(16 * EST[f]["held"] for f in F.ORDER), 2),
            "C_held_out_repeat_2": round(sum(16 * EST[f]["held"] for f in F.ORDER), 2),
            "D_sonnet": sonnet},
        "offline_usd": 0.0,
        "note": "The global cap is hard: sc_run.py sums the spend recorded in every showcase root before each "
                "--execute and clamps each runner's cap to what remains. At the pessimistic factor the Haiku "
                "stages alone would exceed it; the stage order (every family's repeat 1 before any repeat 2, "
                "Sonnet last) makes a binding cap cost repeats and the Sonnet arm, never whole families.",
    }


def base_config(fid: str) -> dict:
    f = F.FAMILIES[fid]
    return {
        "repo_key": f["repo_key"],
        "workdir": "repo",
        "sandbox": ["python3", "{repo}/evals/showcase/sc_sandbox.py", "{task}", "{dir}", "--arm", "{arm}"],
        "score": ["python3", "{repo}/evals/showcase/sc_score.py", "{task}", "{dir}"],
        "prompts": ["{id}"],
        "task_prompts": {t: F.prompt(t) for t in F.order(fid)},
        "train": {"tasks": F.train_tasks(fid), "repeats": 1},
        "seed": SEED,
        "max_turns": 60,
        "concurrency": 2,
        "host_auth": True,
        "confine": True,
        "learn_args": [],
        "forward_env": list(f.get("forward_env") or []),
        "confine_expose": list(f.get("confine_expose") or []),
        "classify": {"lookup": [], "regen": REGEN.get(fid, []), "verify": VERIFY, "bash_edit": BASH_EDIT,
                     "check_flags": []},
        "obligation": {"paths": None},
        "labels": {"lookup": "lookup", "regen": "regenerate"},
    }


def haiku_config(fid: str, b: dict) -> dict:
    cfg = base_config(fid)
    cfg.update({
        "name": f"showcase-{fid}",
        "description": f"Adage showcase, family {fid} (Claude Haiku 4.5, primary). Training = the user's history "
                       "with the product installed and learning by itself; held out = control (hooks, directives "
                       "withheld) vs adage (the full product), paired. See evals/showcase/prereg.json.",
        "eval": {"tasks": F.held_out_tasks(fid), "repeats": 2, "arms": ["control", "adage"]},
        "model": HAIKU,
        "budget_usd": b["haiku"][fid]["cap_usd"],
        "go_validation": {"train_as_user": True, "learner_wait_s": 900, "global_cap_usd": GLOBAL_CAP,
                          "spend_roots": all_roots()},
    })
    return cfg


def sonnet_config(fid: str, b: dict) -> dict:
    cfg = base_config(fid)
    cfg.update({
        "name": f"showcase-{fid}-sonnet",
        "description": f"Adage showcase, family {fid}, the Claude Sonnet 5 arm: held-out control vs adage on the "
                       "index the product learned from the family's Haiku training sessions. See prereg.json.",
        "eval": {"tasks": F.held_out_tasks(fid)[:SONNET_HELD_OUT], "repeats": 1, "arms": ["control", "adage"]},
        "model": SONNET,
        "budget_usd": b["sonnet"][fid]["cap_usd"],
        "go_validation": {"seed_home_from": root(fid), "global_cap_usd": GLOBAL_CAP, "spend_roots": all_roots(),
                          "min_remaining_usd": round(b["sonnet"][fid]["central_usd"] * 0.5, 2)},
    })
    return cfg


def main() -> int:
    out = HERE / "configs"
    out.mkdir(exist_ok=True)
    b = budget()
    (HERE / "budget.json").write_text(json.dumps(b, indent=1) + "\n")
    for fid in F.ORDER:
        for name, cfg in [(fid, haiku_config(fid, b))] + ([(f"{fid}-sonnet", sonnet_config(fid, b))]
                                                           if fid in SONNET_FAMILIES else []):
            (out / f"{name}.json").write_text(json.dumps(cfg, indent=1) + "\n")
            print(f"configs/{name}.json: {cfg['model']}, train {len(cfg['train']['tasks'])}, eval "
                  f"{len(cfg['eval']['tasks'])} x {cfg['eval']['repeats']} x {cfg['eval']['arms']}, cap ${cfg['budget_usd']}")
    print(json.dumps({k: b[k] for k in ("sessions", "central_usd", "pessimistic_usd", "stages_central_usd")}, indent=1))
    return 0


if __name__ == "__main__":
    sys.exit(main())
