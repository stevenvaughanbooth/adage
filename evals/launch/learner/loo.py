"""Leave-one-repo-out priors: for each target repo, fit value priors on the OTHER repos' training
traces (outcome_priors example) and write priors-<target>.json and params-<target>.json (an
`adage eval --params` overlay)."""
import os
import json, subprocess, sys
D = os.environ.get("LD", "/tmp/adage-learner") + ""
BIN = sys.argv[1] if len(sys.argv) > 1 else f"{D}/outcome_priors"
REPOS = ["click", "attrs", "marshmallow", "vue-core", "urfave-cli"]
# urfave-cli is the generality check: never a source of priors for another repository.
SOURCES = ["click", "attrs", "marshmallow", "vue-core"]


def group(repo):
    m = json.load(open(f"{D}/ds/rb-{repo}/manifest.json"))
    tr = set(m["splits"]["heldout"]["train"])
    files = [f"{D}/ds/rb-{repo}/{s['file']}" for s in m["sessions"] if s["id"] in tr]
    return ["--repo", m["repo"], "--root", f"{D}/roots/{repo}", *files]


for target in REPOS:
    args = [BIN]
    for other in SOURCES:
        if other != target:
            args += group(other)
    out = subprocess.run(args, capture_output=True, text=True, check=True).stdout
    open(f"{D}/priors-{target}.json", "w").write(out)
    json.dump({"learn": {"outcome_priors": json.loads(out)}}, open(f"{D}/params-{target}.json", "w"))
    cells = json.loads(out)["cells"]
    for k in sorted(cells):
        if k.startswith("py|venv|no-module:pytest") or k.startswith("*|*|*"):
            c = cells[k]
            print(target, k, "n=%d mean=%.2f kappa=%s" % (c["n"], c["sum"] / c["n"], c.get("kappa")))
