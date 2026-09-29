#!/usr/bin/env python3
"""The live runner (evals/live/runner.py) with the procedure card delivered by a hook.

  python3 run.py <config.json> <phases...> --execute --host-auth --root R --adage BIN

Same runner, same arms, one change: the settings file. Both eval arms get Adage's hooks (directives
withheld, as the RepoBench control) PLUS card_hook.py on UserPromptSubmit and PostToolUse:
  control    -> card hook in `log` mode (detects and records, never injects)
  agents-md  -> card hook in `deliver` mode (the card-by-hook arm; no CLAUDE.md is committed)
Training sessions have no hooks. The hook, its index and the embedding model are mounted read-only
into the confined session (config `confine_expose`); nothing of the benchmark (hidden checks) is.
The config's `card_index` names the index file.
"""

from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[3] / "live"))
import runner  # noqa: E402

CFG = json.loads(Path(sys.argv[1]).read_text())
HOOK = Path(CFG["card_hook"])
INDEX = Path(CFG["card_index"])

_kind = runner.settings_kind


def settings_kind(arm):
    return {"control": "card-log", "agents-md": "card-deliver"}.get(arm, _kind(arm))


def settings(self, kind):
    if not kind.startswith("card-"):
        return _settings(self, kind)
    p = self.private / f"settings-{kind}.json"
    with self.lock:
        if kind not in self.written:
            body = json.loads(subprocess.run([str(self.adage), "hooks-config", str(self.adage)], capture_output=True,
                                             text=True, check=True).stdout)
            mode = kind.split("-", 1)[1]
            cmd = f"python3 {HOOK} {INDEX} {mode}"
            for event in ("UserPromptSubmit", "PostToolUse"):
                body["hooks"].setdefault(event, []).append(
                    {"hooks": [{"type": "command", "command": cmd, "timeout": 20}]})
            tmp = p.with_suffix(".tmp")
            tmp.write_text(json.dumps(body, indent=1))
            tmp.replace(p)
            self.written.add(kind)
    return p


_settings = runner.Live.settings
runner.settings_kind = settings_kind
runner.Live.settings = settings

if __name__ == "__main__":
    sys.exit(runner.main(sys.argv[1:]))
