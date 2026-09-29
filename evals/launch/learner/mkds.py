"""Build adage eval datasets from RepoBench run roots: train = v1 training traces, test = held-out
eval traces of the unsteered arms (v1 control, v2 control)."""
import json, glob, os, shutil, sys
OUT = sys.argv[1]
ROOTS = [("v1", "/tmp/adage-repobench"), ("v2", "/tmp/adage-rb2")]
KEYS = {"click": "github.com/pallets/click", "attrs": "github.com/python-attrs/attrs",
        "marshmallow": "github.com/marshmallow-code/marshmallow", "vue-core": "github.com/vuejs/core",
        "urfave-cli": "github.com/urfave/cli"}
SKIP = {"." + "g" + "it", "node_modules", ".venv", "__pycache__", ".pytest_cache"}
for repo, key in KEYS.items():
    d = os.path.join(OUT, f"rb-{repo}")
    shutil.rmtree(d, ignore_errors=True)
    os.makedirs(d + "/sessions")
    sessions, train, test = [], [], []
    tree = None
    for tag, root in ROOTS:
        rr = f"{root}/runs/{repo}"
        if not os.path.exists(rr + "/state.json"):
            continue
        st = json.load(open(rr + "/state.json"))["runs"]
        for rid, run in sorted(st.items()):
            m = run.get("metrics") or {}
            sid = m.get("session_id")
            if not sid:
                continue
            if run["phase"] == "train":
                if tag != "v1":
                    continue
                split = train
            elif run["arm"] == "control":
                split = test
            else:
                continue
            src = glob.glob(f"{rr}/private/transcripts/*-{rid}-repo/{sid}.jsonl")
            if not src:
                continue
            name = f"sessions/{tag}-{rid}.jsonl"
            shutil.copy(src[0], os.path.join(d, name))
            sessions.append({"file": name, "id": sid, "run": f"{tag}-{rid}", "phase": run["phase"],
                             "arm": run["arm"], "task": run["task"], "passed": m.get("passed"),
                             "result": m.get("result_subtype")})
            split.append(sid)
            base = f"{rr}/runs/{rid}/repo"
            if tree is None and os.path.isdir(base):
                files = []
                for dp, dn, fn in os.walk(base):
                    dn[:] = [x for x in dn if x not in SKIP]
                    files += [os.path.relpath(os.path.join(dp, f), base) for f in fn]
                tree = "\n".join(sorted(files)) + "\n"
    open(d + "/sandbox.txt", "w").write(tree or "")
    man = {"name": f"rb-{repo}", "repo": key, "learn": "default", "sandbox": "sandbox.txt",
           "description": f"RepoBench {repo}: v1 training traces; held-out v1+v2 control eval traces",
           "sessions": sessions, "splits": {"heldout": {"train": train, "test": test}}, "default_split": "heldout"}
    json.dump(man, open(d + "/manifest.json", "w"), indent=1)
    print(repo, len(train), "train", len(test), "test", "tree" if tree else "NO TREE")
