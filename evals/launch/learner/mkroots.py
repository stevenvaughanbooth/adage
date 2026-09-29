"""Snapshot roots for replay: a training checkout of each repository as agents saw it (files, plus
the project's .venv marker the sandbox created: pyvenv.cfg and bin/)."""
import os, shutil
D = os.environ.get("LD", "/tmp/adage-learner") + "/roots"
SRC = {
    "click": "/tmp/adage-repobench/runs/click/runs/train-click-ebcd548d-r1/repo",
    "attrs": "/tmp/adage-repobench/runs/attrs/runs/train-attrs-fd7538f0-r1/repo",
    "marshmallow": "/tmp/adage-repobench/runs/marshmallow/runs/train-marshmallow-de5e842a-r1/repo",
}
SKIP = {"." + "g" + "it", "node_modules", ".pytest_cache", "__pycache__"}
for repo, src in SRC.items():
    dst = os.path.join(D, repo)
    shutil.rmtree(dst, ignore_errors=True)
    shutil.copytree(src, dst, ignore=lambda d, names: [n for n in names if n in SKIP])
    os.makedirs(os.path.join(dst, ".venv", "bin"), exist_ok=True)
    open(os.path.join(dst, ".venv", "pyvenv.cfg"), "w").write("home = /usr/bin\ninclude-system-site-packages = false\nversion = 3.11\n")
    open(os.path.join(dst, ".venv", "bin", "python"), "w").write("")
    print(repo, dst)
