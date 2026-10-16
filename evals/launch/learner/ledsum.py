"""ledsum.py <tag> [repos...]: the drop ledger aggregated by gate, per repository (markdown)."""
import os
import json, sys, collections
D = os.environ.get("LD", "/tmp/adage-learner") + ""
tag = sys.argv[1]
repos = sys.argv[2:] or ["click", "attrs", "marshmallow", "vue-core", "urfave-cli"]

RULES = [
    ("kept", "kept"),
    ("score: support", "scorer: support < min_support"),
    ("score: days", "scorer: days"),
    ("score: consistency", "scorer: dead-end consistency (the runner passes elsewhere)"),
    ("score: contrast", "scorer: contrast < 0.3"),
    ("score: value", "scorer: value V <= 0"),
    ("is ambiguous", "compiler: no dominant or covering variant (ambiguous)"),
    ("variable argument", "compiler: variable argument (one test file per run)"),
    ("not a fixed, plain command", "compiler: not a fixed, plain command (inline code, quotes)"),
    ("housekeeping", "compiler: housekeeping"),
    ("is not an obligation, a sequence or an environment step", "compiler: not an obligation, sequence or environment step"),
    ("an observation after an edit", "compiler: an observation after an edit"),
    ("no precise glob", "compiler: glob precision"),
    ("a one-event context", "compiler: no trigger for a one-event context"),
    ("cannot be a context item", "compiler: context item not compilable"),
    ("has no single template", "compiler: flow-bound command has no template"),
    ("renderable signature", "compiler: failure has no renderable signature"),
    ("also passes where it runs", "compiler: runner also passes (not a dead end)"),
    ("directs an avoided runner", "compiler: interlock (directs an avoided runner)"),
    ("supporting traces, fewer than", "judge: support < min_support"),
    ("task(s), fewer than", "judge: fewer than 2 distinct tasks"),
    ("a check agents may run anyway", "judge: check agents may run anyway (contrast bound)"),
    ("a check agents already run", "judge: check agents already run after the edit"),
    ("contrast", "judge: contrast"),
    ("below the bar", "judge: rate below the bar, no outcome lift"),
    ("explained by", "judge: explained by another route"),
    ("valued on the same trigger", "value: replaced by a valued route on the same trigger"),
    ("dead end's working command", "judge: a dead end's working command"),
    ("already names", "judge: a dead end already names it"),
    ("owed by an obligation", "judge: sequence's first step owed"),
    ("passed the gates; no route", "no delivery shape (read or act context; context routes need MDL selection)"),
]


def cat(stage):
    for k, v in RULES:
        if k in stage:
            return v
    return "other: " + stage[:60]


rows = collections.defaultdict(collections.Counter)
totals = {}
routes = {}
for r in repos:
    d = json.load(open(f"{D}/led-{r}-{tag}.json"))
    totals[r] = (d["sessions"], len(d["candidates"]))
    for x in d["candidates"]:
        rows[cat(x["stage"])][r] += 1
    routes[r] = [(x["id"], x["strength"], x["trigger"], x["step"]) for x in d["routes"]]
print("| Gate that dropped the candidate | " + " | ".join(repos) + " |")
print("|---|" + "---:|" * len(repos))
print("| (traces, scored candidates) | " + " | ".join(f"{totals[r][0]}, {totals[r][1]}" for r in repos) + " |")
for k in sorted(rows, key=lambda k: -sum(rows[k].values())):
    print(f"| {k} | " + " | ".join(str(rows[k][r]) for r in repos) + " |")
print()
for r in repos:
    print(f"- {r}: " + ("; ".join(f"`{i}` {s} {json.dumps(t)} → {json.dumps(st)}" for i, s, t, st in routes[r]) or "none"))
