"""Compile abstract actions back to concrete, safe commands, and score them against what agents ran.

The abstract action `test-run | project-env | file | tests-of-edited` ("run the project's tests
for the file it edited, through the project environment") compiles, at directive time, from repo
facts and the session so far:

    python  .venv/bin/python -m pytest <tests>          (the project interpreter; pyvenv.cfg)
    node    node_modules/.bin/vitest run <tests>        (the `test` script's runner, never watch mode)
    go      go test ./<package of the edited file>      (Go runs packages, not files)

where <tests> = the tests edited in the session, then the file→test map of each edited source
file (most recent first: naming rule, then git co-change), at most 3. No argument is copied from a
trace. The compiled argv passes `safe()`: an allow-listed runner prefix, operands that are tracked
test files (or package directories) of the checkout, no shell metacharacters, nothing installed.

Evaluation points: every test run an agent made after at least one edit, in eval-phase sessions.
For each, the directive the grammar would have compiled *just before* that call is compared with:

- the agent's own run (`agree_agent`: the directive selects a file the agent's run selected; the
  agent's `-k`-only runs have no file and are counted apart);
- the task's hidden tests (`gold_hit`);
- the environment: the agent ran outside the project environment (`agent_env_wrong`), and that
  run hit a dead end (`preempted`: the directive would have avoided it).

The baselines compile the same point the only way they can:

- **LGG**: the dominant typed template among the repository's *training* test runs (the traces a
  learner sees), with its holes bound by value flow (a G1 slot): the hole is bindable when the
  agent's actual value appeared earlier in the session (prompt, an earlier output, an edited path).
- **Embedding**: the most common literal of the potion cluster (fit on training test runs, τ 0.85)
  the agent's command falls into.
"""
from __future__ import annotations

import collections
import json
import os
import re
import sys

import numpy as np

import corpus
import embed
import facts as F
import grammar as G
import lgg
import shell
import traces as T

META = re.compile(r"[;&|<>`$(){}\\\n*?!]")
PREFIX = {"python": [".venv/bin/python", "-m", "pytest"], "node": ["node_modules/.bin/vitest", "run"], "go": ["go", "test"]}


def compile_test_run(fx: F.RepoFacts, repo: str, base: str, edited_src: list, edited_tests: list, k: int = 3):
    """→ (argv, targets, basis) for the abstract `test-run|project|file|tests-of-edited` action."""
    if fx.lang not in PREFIX:
        return None
    argv = list(PREFIX[fx.lang])
    if fx.lang == "python" and not fx.venv:
        return None
    if fx.lang == "go":
        dirs = []
        for p in list(reversed(edited_src)) + list(edited_tests):
            d = os.path.dirname(p) or "."
            if d not in dirs:
                dirs.append(d)
        pk = ["./" + d if d != "." else "." for d in dirs[:k]] or ["./..."]
        return argv + pk, pk, "package"
    tg: list[str] = []
    for t in edited_tests:
        if t in fx.files and F.is_test_module(t) and t not in tg:
            tg.append(t)
    basis = "edited-test" if tg else ""
    for s in reversed(edited_src):
        for t in F.tests_for(repo, base, s, fx.files)[:2]:
            if t not in tg:
                tg.append(t)
                basis = basis or "map"
    tg = tg[:k]
    if not tg:
        # the package's test directory (vue: packages/<pkg>/__tests__), else nothing to target
        for s in reversed(edited_src):
            m = re.match(r"(packages/[^/]+)/", s)
            if m:
                return argv + [m.group(1)], [m.group(1)], "package"
        return None
    return argv + tg, tg, basis


def safe(argv: list[str], fx: F.RepoFacts) -> bool:
    pre = PREFIX.get(fx.lang)
    if not pre or argv[: len(pre)] != pre:
        return False
    for t in argv[len(pre):]:
        if META.search(t) or t.startswith("-") or ".." in t:
            return False
        if fx.lang == "go":
            if t not in (".", "./...") and not any(os.path.dirname(f) == t[2:] for f in fx.files):
                return False
        elif t not in fx.files and not any(f.startswith(t.rstrip("/") + "/") for f in fx.files):
            return False
        elif t in fx.files and not F.is_test(t):
            return False
    return True


def walk(l):
    """Yield (call, act, edited_src, edited_tests) with the edit state *before* each call."""
    edited_src, edited_tests = [], []
    for c, a in zip(l.sess.calls, l.acts):
        yield c, a, list(edited_src), list(edited_tests)
        if a.role == "edit" and a.outcome != "fail":
            p = a.targets[0] if a.targets else ""
            if F.is_test(p) and not p.startswith("/"):
                if p not in edited_tests:
                    edited_tests.append(p)
            elif G.area(p, l.facts) == "src":
                if p in edited_src:
                    edited_src.remove(p)
                edited_src.append(p)
        elif c.tool == "Bash":
            for s in shell.segments(c.command, l.sess.cwd):
                if s.writes and not s.writes.startswith("/") and F.is_test(s.writes) and s.writes not in edited_tests:
                    edited_tests.append(s.writes)


def x_files(a: G.Act) -> list[str]:
    return [t.split("::")[0] for t in a.targets]


def agrees(d_targets: list[str], a: G.Act, lang: str) -> bool | None:
    """Does the directive select what the agent's run selected? None: the agent's run names no file."""
    xs = x_files(a)
    if a.scope == "repo":
        return True  # the agent ran everything; the directive's files are inside it
    if not xs:
        return None
    for x in xs:
        for d in d_targets:
            dd = d[2:] if d.startswith("./") else d
            if lang == "go":
                xd = os.path.dirname(x) if x.endswith(".go") else x.lstrip("./")
                if dd in (".", "...") or xd in (dd, dd.rstrip("/")) or (dd == "." and xd == ""):
                    return True
                continue
            if x == dd or dd.startswith(x.rstrip("/") + "/") or x.startswith(dd.rstrip("/") + "/") or (lang == "node" and x in dd):
                return True
    return False


def flow_bindable(l, idx: int, a: G.Act) -> bool | None:
    """Would a G1 value-flow slot bind the agent's own target? (its value appeared earlier)."""
    vals = x_files(a) + a.selectors
    if not vals:
        return None
    ctx = l.sess.prompt + "\n"
    for c in l.sess.calls[:idx]:
        ctx += (c.out or "")[:20000] + "\n" + c.path + "\n" + str(c.input.get("command", ""))[:2000] + "\n"
    ctx = ctx.replace(l.sess.cwd.rstrip("/") + "/", "")
    return all(v and (v in ctx or os.path.basename(v) in ctx) for v in vals)


def last_test_path(l, idx: int) -> str | None:
    """The G1-style flow binding for a test-path hole: the test file most recently read or edited."""
    for c in reversed(l.sess.calls[:idx]):
        p = l.sess.rel(c.path) if c.tool in ("Read", "Edit", "Write", "MultiEdit") else ""
        if p and F.is_test(p) and not p.startswith("/"):
            return p
        if c.tool == "Bash":
            m = re.findall(r"(?:^|\s)((?:tests?|packages)/\S+?\.(?:py|ts))\b", c.command.replace(l.sess.cwd.rstrip("/") + "/", ""))
            if m:
                return m[-1]
    return None


PROJECT_ENV = G.PROJECT_ENV


def run():
    L = corpus.load()
    tasks = {r: T.tasks(r) for r in T.REPOS}
    by_repo = collections.defaultdict(list)
    for l in L:
        by_repo[l.sess.repo].append(l)

    # baselines fit on the training traces (what a learner has)
    lgg_dom, emb_fit = {}, {}
    for repo, ls in by_repo.items():
        sk = collections.Counter()
        texts = collections.Counter()
        for l in ls:
            if l.sess.phase != "train":
                continue
            for c, a in zip(l.sess.calls, l.acts):
                if a.role == "test-run":
                    sk[lgg.skeleton(c.command, l.sess.cwd, l.facts, loose=True)] += 1
                    texts[re.sub(r"\s+", " ", c.command.replace(l.sess.cwd.rstrip("/"), "$R"))[:600]] += 1
        lgg_dom[repo] = sk.most_common(3)
        tx = sorted(texts)
        if tx:
            X = embed.potion(tx)
            cl = embed.cluster(X, 0.85)
            ids, C = embed.centroids(X, cl)
            medoid = {}
            for cid in ids:
                mem = [(texts[t], t) for t, c in zip(tx, cl) if c == cid]
                medoid[int(cid)] = max(mem)[1]
            emb_fit[repo] = (ids, C, medoid)

    rows = []
    for repo, ls in sorted(by_repo.items()):
        for l in ls:
            if l.sess.phase != "eval":
                continue
            task = tasks[repo].get(l.sess.task, {})
            gold = set(task.get("test_files", []))
            for c, a, es, et in walk(l):
                if a.role != "test-run" or not (es or et):
                    continue
                comp = compile_test_run(l.facts, repo, l.base, es, et)
                row = {"repo": repo, "sid": l.sess.sid, "model": l.sess.model, "idx": c.idx, "x_env": a.env, "x_scope": a.scope,
                       "x_rel": a.relation, "x_outcome": a.outcome, "x_cmd": c.command[:200].replace(l.sess.cwd.rstrip("/") + "/", "")}
                if comp:
                    argv, tg, basis = comp
                    mo = compile_test_run(l.facts, repo, l.base, es, [])
                    row["gold_hit_map_only"] = bool(mo) and (bool(gold & set(mo[1])) if l.facts.lang != "go" else True)
                    row["agree_map_only"] = agrees(mo[1], a, l.facts.lang) if mo else False
                    row.update(compiled=" ".join(argv), basis=basis, safe=safe(argv, l.facts), agree=agrees(tg, a, l.facts.lang),
                               gold_hit=bool(gold & set(tg)) if l.facts.lang != "go" else any(os.path.dirname(g) in ("", *[t.strip("./") for t in tg]) for g in gold),
                               gold_at_base=bool(gold & l.facts.files))
                else:
                    row.update(compiled=None)
                row["agent_env_wrong"] = a.env not in PROJECT_ENV
                row["preempted"] = bool(comp) and a.outcome == "env-fail"
                # LGG baseline
                dom = lgg_dom.get(repo) or []
                row["lgg_template"] = dom[0][0] if dom else None
                row["lgg_bindable"] = flow_bindable(l, c.idx, a)
                if dom:
                    tpl = dom[0][0]
                    row["lgg_env_project"] = tpl.startswith((".venv/", "[.venv]", "uv run", "npm", "pnpm", "npx", "node_modules", "go ", "make"))
                    if re.search(r"<[PT]:test>", tpl):
                        v = last_test_path(l, c.idx)
                        row["lgg_bound"] = v
                        row["lgg_agree"] = agrees([v], a, l.facts.lang) if v else False
                    else:
                        row["lgg_agree"] = agrees([], a, l.facts.lang) if a.scope == "repo" else False
                # embedding baseline: the cluster's most common literal
                if repo in emb_fit:
                    ids, C, medoid = emb_fit[repo]
                    t = re.sub(r"\s+", " ", c.command.replace(l.sess.cwd.rstrip("/"), "$R"))[:600]
                    asg = embed.assign(embed.potion([t]), ids, C, 0.85)[0]
                    if asg is not None:
                        med = medoid[asg]
                        segs = shell.segments(med.replace("$R", l.sess.cwd.rstrip("/")), l.sess.cwd)
                        ma = min([G.classify_seg(s, l.facts, med) for s in segs] or [G.Act("other")], key=lambda z: G.ROLE_PRIORITY.index(z.role))
                        row["emb_literal"] = med[:200]
                        row["emb_agree"] = agrees([x.split("::")[0] for x in ma.targets], a, l.facts.lang) if ma.targets else (True if ma.scope == "repo" else None)
                        row["emb_env_project"] = ma.env in PROJECT_ENV
                    else:
                        row["emb_literal"] = None
                rows.append(row)
    return rows, lgg_dom


def summarise(rows, lgg_dom):
    out = {}
    groups = collections.defaultdict(list)
    for r in rows:
        groups[r["repo"]].append(r)
        groups["ALL"].append(r)
    for g, rs in groups.items():
        n = len(rs)
        comp = [r for r in rs if r.get("compiled")]
        ag = [r for r in comp if r.get("agree") is not None]
        eb = [r for r in rs if r.get("emb_literal")]
        eba = [r for r in eb if r.get("emb_agree") is not None]
        lb = [r for r in rs if r.get("lgg_bindable") is not None]
        out[g] = {
            "points": n,
            "compiled": len(comp) / n,
            "compiled_file_level": sum(1 for r in comp if r["basis"] in ("map", "edited-test")) / n,
            "safe": sum(r["safe"] for r in comp) / max(1, len(comp)),
            "agree_agent": sum(r["agree"] for r in ag) / max(1, len(ag)),
            "agree_agent_narrowed": sum(r["agree"] for r in ag if r["x_scope"] != "repo") / max(1, sum(1 for r in ag if r["x_scope"] != "repo")),
            "agent_named_no_file": 1 - len(ag) / max(1, len(comp)),
            "gold_hit": sum(r["gold_hit"] for r in comp) / max(1, len(comp)),
            "gold_hit_when_gold_at_base": sum(r["gold_hit"] for r in comp if r["gold_at_base"]) / max(1, sum(1 for r in comp if r["gold_at_base"])),
            "agent_env_wrong": sum(r["agent_env_wrong"] for r in rs) / n,
            "agent_env_fail": sum(r["x_outcome"] == "env-fail" for r in rs) / n,
            "preempted": sum(r["preempted"] for r in rs) / n,
            "gold_hit_map_only": sum(r.get("gold_hit_map_only", False) for r in comp) / max(1, len(comp)),
            "agree_map_only": sum(bool(r.get("agree_map_only")) for r in ag) / max(1, len(ag)),
            "basis": dict(collections.Counter(r["basis"] for r in comp)),
            "lgg_bindable": sum(r["lgg_bindable"] for r in lb) / max(1, len(lb)),
            "lgg_agree": sum(bool(r.get("lgg_agree")) for r in rs) / n,
            "lgg_env_project": sum(bool(r.get("lgg_env_project")) for r in rs) / n,
            "emb_assigned": len(eb) / n,
            "emb_agree": sum(r["emb_agree"] for r in eba) / max(1, len(eba)),
            "emb_env_project": sum(r["emb_env_project"] for r in eb) / max(1, len(eb)),
        }
    out["lgg_dominant_train_template"] = {k: v for k, v in lgg_dom.items()}
    return out


if __name__ == "__main__":
    rows, dom = run()
    here = os.path.dirname(os.path.abspath(__file__))
    os.makedirs(os.path.join(here, "results"), exist_ok=True)
    with open(os.path.join(here, "results", "compile_points.jsonl"), "w") as f:
        for r in rows:
            f.write(json.dumps(r) + "\n")
    s = summarise(rows, dom)
    with open(os.path.join(here, "results", "compile_back.json"), "w") as f:
        json.dump(s, f, indent=1)
    print(json.dumps(s, indent=1))
