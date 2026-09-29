"""How many source repositories does day-0 transfer need? For each Python held-out repository,
re-run the day-0 fold (n = 0) with the pool restricted to one or two source repositories."""

from __future__ import annotations

import itertools
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PY = ["attrs", "click", "marshmallow"]


def main():
    rows = []
    for X in PY:
        srcs = [r for r in PY if r != X] + ["gen-py-clinic"]
        pools = [(s,) for s in srcs] + [tuple(r for r in PY if r != X)]
        for pool in pools:
            tag = "-pool-" + "+".join(pool)
            subprocess.run([sys.executable, str(HERE / "run.py"), X, "--ns=0", f"--pool={','.join(pool)}",
                            f"--tag={tag}", "--quiet"], check=True)
            r = json.loads((HERE / "results" / f"loro-{X}{tag}.json").read_text())["results"]["0"]["summary"]
            rows.append(dict(target=X, pool=list(pool), hier_pre=r["hier/pre"]["net_calls"],
                             hier_post=r["hier/post"]["net_calls"], oracle_pre=r["oracle/pre"]["net_calls"],
                             directives=r["hier/pre"]["directives"]))
            print(rows[-1], flush=True)
            (HERE / "results" / f"loro-{X}{tag}.json").unlink()
    (HERE / "results" / "poolsize.json").write_text(json.dumps(rows, indent=1))


if __name__ == "__main__":
    main()
