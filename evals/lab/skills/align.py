#!/usr/bin/env python3
"""Align agent trajectories to a skill's procedure graph, over abstract actions.

  python3 align.py <SKILL.md> <transcript.jsonl>... [--json] [--show N]

The alignment is a monotone profile alignment (a left-to-right profile HMM, Viterbi): the
procedure's steps are the match states, in order; each abstract action either matches the
current or a later step (a step may absorb several actions: repeated test runs), or is an
insertion (exploration the skill says nothing about). Skipping a required step costs; skipping a
conditional step costs nothing (whether it applied is decided afterwards from the trajectory:
an `after_edit` step applies when the session edited a matching file).

Match scores are over abstract actions, not strings: same role +3, same environment +1 (else -1:
a *deviation*: the right kind of step, run another way), same scope +0.5, the step's template
matched token-wise (slots as wildcards) +1.5. An action that runs a command the step forbids
aligns to the step (it is that step, done wrong) and is counted as an avoided-command hit.

Per step it reports coverage (aligned / applicable sessions), exact follows, deviations (by
abstract key, with their failure rate: an improvement if they never fail and the skill's way does),
avoided-command hits, and learned slot bindings (anti-unification of the aligned commands against
the session's context: prompt spans, edited paths). Insertions shared by most sessions are
reported as candidate steps the skill does not have.
"""

from __future__ import annotations

import fnmatch
import json
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

from abstract import Action, Session, read_session
from procedure import argv_of, parse, split_env

MATCH_MIN = 2.5
SKIP_REQUIRED = 2.0
INSERT = {"read": 0.0, "search": 0.0, "vcs": 0.0, "other": 0.0}
INSERT_DEFAULT = 0.2
EDIT_GLOBS_RE = re.compile(r"`([\w./*-]+/)`|`([\w./*-]+\*[\w./*-]*)`|under `([\w./-]+)`")


def template_re(t: str) -> re.Pattern:
    """A template as a token-wise pattern: slots match one token; extra trailing tokens allowed."""
    toks = []
    for tok in argv_of(t):
        parts = re.split(r"(\{\w+\})", tok)
        toks.append("".join(r"(?P<%s>[^\s]+?)" % p[1:-1] if re.fullmatch(r"\{\w+\}", p) else re.escape(p)
                            for p in parts))
    return re.compile(r"^" + r"\s+".join(toks) + r"(\s.*)?$")


def norm(cmd: str) -> str:
    a, _ = split_env(argv_of(cmd))
    a = [x[2:] if x.startswith("./") else x for x in a]
    return " ".join(a)


def edit_globs(step: dict) -> list[str]:
    gl = []
    for m in EDIT_GLOBS_RE.finditer(step["text"]):
        g = next(x for x in m.groups() if x)
        gl.append(g + "**" if g.endswith("/") else g)
    return gl


class Model:
    def __init__(self, graph: dict):
        self.g = graph
        self.steps = graph["steps"]
        for s in self.steps:
            s["_res"] = [template_re(c["template"]) for c in s["commands"]]
            s["_avoid"] = [template_re(a["command"]) for a in s["avoid"]]
            s["_globs"] = edit_globs(s) if s["role"] == "edit" else []

    def sim(self, a: Action, s: dict) -> tuple[float, str]:
        """(score, kind): kind is exact | deviation | avoided | role."""
        if s["role"] == "edit":
            if a.role != "edit":
                return 0.0, ""
            ok = not s["_globs"] or any(fnmatch.fnmatch(a.path, g) for g in s["_globs"])
            return (3.5 if ok else 2.6), "exact" if ok else "role"
        if not s["commands"]:
            return 0.0, ""
        cmd = norm(a.cmd)
        if a.cmd and any(r.match(cmd) for r in s["_avoid"]):
            return 3.0, "avoided"
        best = (0.0, "")
        for c, r in zip(s["commands"], s["_res"]):
            if a.role != c["role"]:
                continue
            sc = 3.0 + (1.0 if a.env == c["env"] else -1.0) + (0.5 if a.scope == c["scope"] else 0.0)
            exact = bool(a.cmd) and bool(r.match(cmd))
            sc += 1.5 if exact else 0.0
            kind = "exact" if exact else ("deviation" if a.env != c["env"] or a.scope != c["scope"] else "role")
            best = max(best, (sc, kind))
        return best

    def required(self, s: dict) -> bool:
        return s["precondition"]["kind"] in ("always", "before_finish") and s["role"] != "edit"

    def align(self, sess: Session) -> list[tuple[int | None, str]]:
        """For each action, the step index it aligns to (None: insertion) and the match kind."""
        n, m = len(sess.actions), len(self.steps)
        NEG = -1e9
        # f[j][used]: best score with pointer at step j (0..m-1), used = step j matched already.
        f = [[NEG, NEG] for _ in range(m + 1)]
        f[0][0] = 0.0
        back = []
        sims = [[self.sim(a, s) for s in self.steps] for a in sess.actions]

        def skip_cost(j0, used0, k):
            """Cost of moving the pointer from step j0 to step k (> j0), skipping those between."""
            c = 0.0
            for t in range(j0, k):
                if t == j0 and used0:
                    continue
                if self.required(self.steps[t]):
                    c += SKIP_REQUIRED
            return c

        for i, a in enumerate(sess.actions):
            g = [[NEG, NEG] for _ in range(m + 1)]
            bk = [[None, None] for _ in range(m + 1)]
            ins = INSERT.get(a.role, INSERT_DEFAULT)
            for j in range(m + 1):
                for u in (0, 1):
                    v = f[j][u]
                    if v == NEG:
                        continue
                    if v - ins > g[j][u]:
                        g[j][u], bk[j][u] = v - ins, (j, u, None)
                    for k in range(j, m):
                        sc, _ = sims[i][k]
                        if sc < MATCH_MIN:
                            continue
                        w = v + sc - (skip_cost(j, u, k) if k > j else 0.0)
                        if w > g[k][1]:
                            g[k][1], bk[k][1] = w, (j, u, k)
            f = g
            back.append(bk)
        # Close: steps after the pointer that were never matched.
        best, bj, bu = NEG, 0, 0
        for j in range(m + 1):
            for u in (0, 1):
                if f[j][u] == NEG:
                    continue
                tail = skip_cost(j, u, m) if j < m else 0.0
                if f[j][u] - tail > best:
                    best, bj, bu = f[j][u] - tail, j, u
        out: list[tuple[int | None, str]] = [(None, "")] * n
        j, u = bj, bu
        for i in range(n - 1, -1, -1):
            pj, pu, k = back[i][j][u]
            out[i] = (k, sims[i][k][1] if k is not None else "")
            j, u = pj, pu
        return out


def applies(step: dict, sess: Session) -> bool:
    pre = step["precondition"]
    if pre["kind"] == "after_edit":
        return any(a.role == "edit" and fnmatch.fnmatch(a.path, pre["glob"]) for a in sess.actions)
    return True


def after_last_edit(step: dict, sess: Session, idx: list[int]) -> bool:
    """For an after_edit / before_finish step: an aligned run after the last matching edit."""
    pre = step["precondition"]
    if pre["kind"] == "after_edit":
        edits = [a.i for a in sess.actions if a.role == "edit" and fnmatch.fnmatch(a.path, pre["glob"])]
    else:
        edits = [a.i for a in sess.actions if a.role == "edit"]
    ok = [i for i in idx if not sess.actions[i].failed]
    return bool(ok) and (not edits or max(ok) > max(edits))


def slot_bindings(step: dict, sessions_and_idx) -> dict:
    """Anti-unify the aligned concrete commands with the step's templates: for each slot, where its
    value came from (the prompt, an edited path's stem, a constant) and how often."""
    res = {}
    for c, r in zip(step["commands"], step["_res"]):
        if not c["slots"]:
            continue
        for slot in c["slots"]:
            src = Counter()
            vals = Counter()
            for sess, idx in sessions_and_idx:
                for i in idx:
                    m = r.match(norm(sess.actions[i].cmd))
                    if not m or m.groupdict().get(slot) is None:
                        continue
                    v = m.group(slot)
                    vals[v] += 1
                    edited = [Path(a.path).stem for a in sess.actions[:i] if a.role == "edit" and a.path]
                    if v in sess.prompt:
                        src["prompt"] += 1
                    elif v in edited:
                        src["edited-stem"] += 1
                    elif any(v == Path(a.path).stem for a in sess.actions[:i] if a.path):
                        src["read-stem"] += 1
                    else:
                        src["other"] += 1
            res[f"{c['template']}#{slot}"] = {"sources": dict(src), "values": dict(vals.most_common(6))}
    return res


def ngrams(keys: list[str], n: int) -> set:
    return {tuple(keys[i:i + n]) for i in range(len(keys) - n + 1)}


def analyse(graph: dict, sessions: list[Session]) -> dict:
    model = Model(graph)
    per = defaultdict(lambda: {"applicable": 0, "aligned": 0, "exact": 0, "deviation": Counter(),
                               "deviation_failed": Counter(), "avoided": 0, "avoided_failed": 0, "timely": 0,
                               "first_pos": [], "runs": 0, "runs_failed": 0})
    idx_by_step = defaultdict(list)
    inserts = Counter()
    aligned_sessions = []
    for sess in sessions:
        al = model.align(sess)
        aligned_sessions.append({"session": sess.id, "prompt": sess.prompt, "directives": sess.directives,
                                 "path": [(a.key(), a.cmd or a.path, k, kind, a.failed)
                                          for a, (k, kind) in zip(sess.actions, al)]})
        sess_ins = set()
        for a, (k, kind) in zip(sess.actions, al):
            if k is None and a.role not in ("read", "other"):
                sess_ins.add(a.key())
        inserts.update(sess_ins)
        for si, s in enumerate(model.steps):
            idx = [a.i for a, (k, _) in zip(sess.actions, al) if k == si]
            p = per[s["id"]]
            ap = applies(s, sess)
            p["applicable"] += ap
            if not idx:
                continue
            p["aligned"] += 1
            p["first_pos"].append(min(idx))
            kinds = {al[i][1] for i in idx}
            p["exact"] += "exact" in kinds
            for i in idx:
                a = sess.actions[i]
                p["runs"] += 1
                p["runs_failed"] += a.failed
                if al[i][1] == "deviation":
                    p["deviation"][a.key() + "  e.g. " + a.cmd[:60]] += 1
                    p["deviation_failed"][a.key() + "  e.g. " + a.cmd[:60]] += a.failed
                if al[i][1] == "avoided":
                    p["avoided"] += 1
                    p["avoided_failed"] += a.failed
            if s["precondition"]["kind"] in ("after_edit", "before_finish"):
                p["timely"] += after_last_edit(s, sess, idx)
            idx_by_step[s["id"]].append((sess, idx))
    steps = []
    for s in model.steps:
        p = per[s["id"]]
        fp = sorted(p["first_pos"])
        steps.append({
            "id": s["id"], "title": s["title"], "role": s["role"], "precondition": s["precondition"]["kind"],
            "applicable": p["applicable"], "aligned": p["aligned"], "exact": p["exact"],
            "coverage": round(p["aligned"] / p["applicable"], 3) if p["applicable"] else None,
            "timely": p["timely"] if s["precondition"]["kind"] in ("after_edit", "before_finish") else None,
            "runs": p["runs"], "runs_failed": p["runs_failed"],
            "deviations": [{"what": k, "n": v, "failed": p["deviation_failed"][k]}
                           for k, v in p["deviation"].most_common(5)],
            "avoided_hits": p["avoided"], "avoided_failed": p["avoided_failed"],
            "median_first_position": fp[len(fp) // 2] if fp else None,
            "slots": slot_bindings(s, idx_by_step[s["id"]]),
        })
    n = len(sessions)
    common = [{"action": k, "sessions": v} for k, v in inserts.most_common() if v >= max(2, n // 2)]
    return {"sessions": n, "steps": steps, "insertions_common": common, "alignments": aligned_sessions}


def render(r: dict) -> str:
    out = [f"{r['sessions']} sessions"]
    for s in r["steps"]:
        cov = f"{s['aligned']}/{s['applicable']}"
        extra = f" timely {s['timely']}" if s["timely"] is not None else ""
        out.append(f"{s['id']} {s['role']:<10} {s['precondition']:<13} aligned {cov:<6} exact {s['exact']:<3}"
                   f" runs {s['runs']} (failed {s['runs_failed']}){extra}  {s['title'][:50]}")
        for d in s["deviations"]:
            out.append(f"     deviation x{d['n']} (failed {d['failed']}): {d['what']}")
        if s["avoided_hits"]:
            out.append(f"     forbidden command used x{s['avoided_hits']} (failed {s['avoided_failed']})")
        for k, v in s["slots"].items():
            out.append(f"     slot {k}: {v['sources']} {list(v['values'])[:4]}")
    if r["insertions_common"]:
        out.append("not in the skill, in most sessions: " + ", ".join(f"{c['action']} ({c['sessions']})"
                                                                     for c in r["insertions_common"]))
    return "\n".join(out)


def main(argv):
    skill = argv[0]
    paths = [p for p in argv[1:] if p.endswith(".jsonl")]
    g = parse(skill).to_json()
    sessions = [read_session(p) for p in paths]
    r = analyse(g, sessions)
    if "--json" in argv:
        print(json.dumps(r, indent=1, default=str))
    else:
        print(render(r))
        if "--show" in argv:
            for a in r["alignments"][: int(argv[argv.index("--show") + 1])]:
                print(f"\n# {a['prompt'][:70]!r}")
                for key, what, k, kind, failed in a["path"]:
                    print(f"   {('s%d' % (k + 1)) if k is not None else '  .':>4} {kind:<9} {key:<32} "
                          f"{'FAIL ' if failed else ''}{what[:70]}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
