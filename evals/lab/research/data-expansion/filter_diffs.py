#!/usr/bin/env python3
"""Drop build and setup output from saved agent diffs (e.g. jq's autotools/libtool files, which its
exported tree does not .gitignore): keep a file's section only if the path is tracked at the task's
base commit, or is a new file under a source/test directory the repository defines. Idempotent.

  python3 filter_diffs.py <root> [...]
"""
import json, re, subprocess, sys
from pathlib import Path
HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[2] / "repobench"))
import common  # noqa: E402

def main(argv):
    for a in argv:
        root = Path(a)
        st = json.loads((root / "state.json").read_text())
        for rid, r in st["runs"].items():
            p = root / "runs" / rid / "agent.diff"
            if not p.exists() or not r.get("task"):
                continue
            repo_id = r["task"].rsplit("-", 1)[0]
            rep = common.repo(repo_id); t = common.task(repo_id, r["task"])
            tracked = set(common.git(common.mirror(rep), "ls-tree", "-r", "--name-only", t["base"]).split("\n"))
            parts = re.split(r"(?m)^(?=diff --git )", p.read_text(errors="replace"))
            keep = []
            for sec in parts:
                m = re.match(r"diff --git a/(\S+) b/", sec)
                if not m:
                    continue
                path = m.group(1)
                if path in tracked or common.is_source(rep, path) or (common.is_test(rep, path) and not path.endswith((".log", ".trs"))):
                    keep.append(sec)
            p.write_text("".join(keep))
    return 0

if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
