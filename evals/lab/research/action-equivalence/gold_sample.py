"""Sample the commands for the hand-labelled gold set (the MEASUREMENT INSTRUMENT).

70 distinct Bash commands per repository, uniformly from held-out control-arm calls (seed 11).
They are labelled by hand in `gold_labels.tsv` (id, class) under the rubric in README.md §Gold.
"""
import json
import random

import data

S = data.load()
rng = random.Random(11)
rows = []
for repo in data.REPOS:
    calls = [r for s in S if s.repo == repo and s.arm == "control" for r in s.recs if r.tool == "Bash" and r.cmd.strip()]
    rng.shuffle(calls)
    seen, k = set(), 0
    for r in calls:
        if r.cmd in seen:
            continue
        seen.add(r.cmd)
        rows.append({"id": f"{repo[:2]}{k:02d}", "repo": repo, "skey": r.skey, "idx": r.idx, "cmd": r.cmd})
        k += 1
        if k == 70:
            break
json.dump(rows, open(data.HERE / "gold_commands.json", "w"), indent=0)
for x in rows:
    print(x["id"], x["cmd"].replace("\n", " ⏎ ")[:220])
