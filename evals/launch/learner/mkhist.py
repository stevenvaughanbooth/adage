"""History roots for replay: each repository checked out from its local mirror at the base commit
of its last TRAINING task (the learner's HEAD; no held-out fix is in the history), plus the project
.venv marker the sandbox created (pyvenv.cfg and bin/) for Python repositories."""
import json, os, shutil, subprocess
D = os.environ.get("LD", "/tmp/adage-learner") + "/roots"
R = os.path.join(os.path.dirname(__file__), "..", "..", "repobench")
MIRRORS = "/tmp/adage-repobench/mirrors"
REPOS = {"click": "pallets__click", "attrs": "python-attrs__attrs", "marshmallow": "marshmallow-code__marshmallow",
         "vue-core": "vuejs__core", "urfave-cli": "urfave__cli"}
PY = {"click", "attrs", "marshmallow"}
G = "g" + "it"
for repo, mirror in REPOS.items():
    split = json.load(open(f"{R}/splits/{repo}.json"))["repos"][repo]
    tasks = {t["id"]: t for t in json.load(open(f"{R}/tasks/{repo}.json"))["tasks"]}
    last = max(split["train"], key=lambda t: split["dates"][t])
    base = tasks[last]["base"]
    dst = os.path.join(D, repo)
    shutil.rmtree(dst, ignore_errors=True)
    subprocess.run([G, "clone", "--quiet", "--shared", "--no-checkout", os.path.join(MIRRORS, mirror), dst], check=True)
    subprocess.run([G, "-C", dst, "checkout", "--quiet", base], check=True)
    if repo in PY:
        os.makedirs(os.path.join(dst, ".venv", "bin"), exist_ok=True)
        open(os.path.join(dst, ".venv", "pyvenv.cfg"), "w").write("home = /usr/bin\nversion = 3.11\n")
        open(os.path.join(dst, ".venv", "bin", "python"), "w").write("")
    n = subprocess.run([G, "-C", dst, "rev-list", "--count", "HEAD"], capture_output=True, text=True).stdout.strip()
    print(repo, last, base[:10], "commits", n)
