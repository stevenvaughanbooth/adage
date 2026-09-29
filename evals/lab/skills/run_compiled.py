#!/usr/bin/env python3
"""The three-arm comparison with the skill compiled by the product: `adage train`, then the runner.

  python3 run_compiled.py <config.json> <root> [--execute] [--adage BIN]

1. Make one sandbox (the arm without the skill: every such sandbox has the same root commit, see
   sandbox.py) and compile the skill against it with `adage train <SKILL.md> --repo <sandbox>`,
   into <root>/private/adage-home: the deterministic compile and the sandboxed replay. The review
   is saved as <root>/compiled-review.txt.
2. Accept the staged pack into that home (`adage train accept <name>`), as the person who read the
   review would. This harness is a person's script, not an agent session: the child gets no agent
   markers.
3. Run the runner's held-out phase and report (arms from the config: neither, skill in context,
   compiled). The `adage` arm reads <root>/private/adage-home and nothing else: no learned
   reflexes, no instincts.

Steps 1 and 2 spend nothing. Step 3 needs --execute and spends money (the config's budget caps it).
"""

import json
import os
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
AGENT_MARKERS = ("CLAUDECODE", "CLAUDE_CODE_SESSION_ID", "CLAUDE_CODE_ENTRYPOINT", "SSH_AUTH_SOCK")


def person_env(**extra):
    env = {k: v for k, v in os.environ.items() if k not in AGENT_MARKERS and "SESSION_ID" not in k}
    env.update(extra)
    return env


def main(argv):
    cfg_path, root, rest = Path(argv[0]), Path(argv[1]).resolve(), argv[2:]
    adage = Path(rest[rest.index("--adage") + 1] if "--adage" in rest else REPO / "target/debug/adage").resolve()
    cfg = json.loads(cfg_path.read_text())
    skill = Path(cfg["sandbox"][3].replace("{repo}", str(REPO)))
    kind = cfg["sandbox"][2]
    home = root / "private" / "adage-home"
    sandbox = root / "private" / "compile" / cfg["workdir"]
    root.mkdir(parents=True, exist_ok=True)
    if not (home / "index.json").exists():
        sandbox.parent.mkdir(parents=True, exist_ok=True)
        if not sandbox.exists():
            subprocess.run(["python3", str(HERE / "sandbox.py"), kind, str(skill), str(sandbox),
                            cfg["eval"]["tasks"][0], "adage"], check=True)
        env = person_env(ADAGE_HOME=str(home))
        out = subprocess.run([str(adage), "train", str(skill), "--repo", str(sandbox)], cwd=sandbox, env=env,
                             capture_output=True, text=True)
        (root / "compiled-review.txt").write_text(out.stdout + out.stderr)
        print(out.stdout + out.stderr, flush=True)
        if out.returncode != 0:
            return out.returncode
        name = json.loads(next((home / "train" / "skills").glob("*/review.json")).read_text())["skill"]
        prefix = next((home / "train" / "skills").iterdir()).name
        acc = subprocess.run([str(adage), "train", "accept", prefix], cwd=sandbox, capture_output=True, text=True,
                             env=person_env(ADAGE_HOME=str(home), ADAGE_ALLOW_NONINTERACTIVE="1"))
        (root / "compiled-accept.txt").write_text(acc.stdout + acc.stderr)
        print(acc.stdout + acc.stderr, flush=True)
        if acc.returncode != 0:
            return acc.returncode
        print(f"compiled {name} into {home}", flush=True)
    cmd = ["python3", str(REPO / "evals/live/runner.py"), str(cfg_path), "eval", "report", "--root", str(root),
           "--adage", str(adage)]
    if "--execute" in rest:
        cmd.append("--execute")
    return subprocess.run(cmd, env=person_env()).returncode


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
