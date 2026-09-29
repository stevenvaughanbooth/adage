"""(b) A learned "you're done" signal and a learned redundant re-run signal, with harm.

DONE signal. Event e = (command key, Drain template of the output's last non-empty line, numbers
masked) for every Bash result that follows an edit. Label from TRAINING traces: y = 1 if the agent
made no further successful edit afterwards in that session. P(y|e) is estimated with a
Beta(1,1) prior, support >= 3 training sessions; the signal fires at the first held-out result
whose P >= theta (theta chosen on train to maximise saved $ minus a harm charge). Nothing names a
test runner or a pass/fail string: which commands and which output shapes mean "done" is learned.
Counterfactual if the agent stops at the signal (one more call to write the answer): saved $ =
cost of every later call (later calls do not change earlier calls' cost). Harm: the signal fired
but the agent later made a successful edit (work that stopping would cut), and pass rates of
those sessions.

REDUNDANT RE-RUN. A Bash call whose exact command (sandbox root stripped) was already run with no successful
edit in between. Idempotency is learned per command key from training traces: the share of such
repeats whose masked output was identical; keys with share >= 0.8 over >= 3 repeats qualify.
PreToolUse would answer with a stub pointing at the earlier output (tokens saved), or a directive
could prevent the call (call-level upper bound). Harm: held-out repeats of qualifying keys whose
output actually differed.

Hand-written parts: the Edit/Write tool set (the lab normaliser's EDIT_TOOLS) and number masking.
"""
from __future__ import annotations

import collections
import json
import pickle
import re
import sys

import numpy as np

sys.path.insert(0, "/tmp/adage-research/token-cost/pkg/drain3-0.9.11")
from drain3.drain import Drain  # noqa: E402

from common import PRICES, boot, boot_ratio, call_cost, fam, session_cost  # noqa: E402
from policies import S, wilson  # noqa: E402
sys.path.insert(0, "/home/user/adage/evals/lab")
from normalize import strip_root  # noqa: E402


def ident(r):
    return strip_root((r["input"] or {}).get("command", "")).strip()

NUM = re.compile(r"(?<![A-Za-z_])[-+]?\d+(?:\.\d+)?%?")


def mask(t):
    return NUM.sub("<NUM>", t.strip())[:300]


def last_line(text):
    ls = [l for l in text.split("\n") if l.strip()]
    return mask(ls[-1]) if ls else "<empty>"


def attach_text():
    raw = pickle.load(open("/tmp/adage-research/token-cost/sessions.pkl", "rb"))
    by = {s["sid"]: s for s in raw}
    for s in S:
        for r, rr in zip(s["results"], by[s["sid"]]["results"]):
            r["_text"] = rr["text"]


def good_edit(r):
    return r["edit"] and not r["is_error"]


def events(s, drain, learn):
    """(result index, key, template id, y) for Bash results after the first successful edit."""
    res = s["results"]
    last_edit = max([i for i, r in enumerate(res) if good_edit(r)], default=-1)
    seen_edit = False
    out = []
    for i, r in enumerate(res):
        if good_edit(r):
            seen_edit = True
            continue
        if not seen_edit or not r["key"].startswith("Bash:"):
            continue
        ll = last_line(r["_text"])
        if learn:
            c, _ = drain.add_log_message(r["key"] + " || " + ll)
            cid = c.cluster_id
        else:
            c = drain.match(r["key"] + " || " + ll, "never")
            cid = c.cluster_id if c is not None else -1
        out.append((i, cid, int(i > last_edit)))
    return out


def saved_after(s, i):
    """$ of every call after the one that sees result i, keeping one call to write the answer."""
    k = s["results"][i]["seen_call"] + 1
    p = PRICES[fam(s["model"])]
    return sum(call_cost(c, p) for c in s["calls"][k:]), max(len(s["calls"]) - k, 0)


def done_signal(train, held, loro=False):
    out = {}
    groups = [(None, train, held)] if not loro else [
        (repo, [s for s in train if s["repo"] != repo], [s for s in held if s["repo"] == repo])
        for repo in sorted({s["repo"] for s in held})]
    rows = []
    for _, tr, te in groups:
        d = Drain(sim_th=0.5, depth=4)
        stat = collections.defaultdict(lambda: [0, 0, set()])
        for s in tr:
            for i, cid, y in events(s, d, True):
                stat[cid][0] += 1
                stat[cid][1] += y
                stat[cid][2].add(s["sid"])
        P = {cid: (u + 1) / (n + 2) for cid, (n, u, ss) in stat.items() if len(ss) >= 3}

        def fire(s, th):
            for i, cid, y in events(s, d, False):
                if P.get(cid, 0) >= th:
                    return i
            return None

        # choose theta on train: saved $ minus harm charge (a fired session with later edits
        # is charged its whole cost: a failed task has to be redone)
        best, bv = None, 0.0
        for th in (0.6, 0.7, 0.8, 0.9, 0.95):
            v = 0.0
            for s in tr:
                i = fire(s, th)
                if i is None:
                    continue
                later = any(good_edit(r) for r in s["results"][i + 1:])
                v += saved_after(s, i)[0] - (session_cost(s) if later else 0)
            if v > bv:
                best, bv = th, v
        for s in te:
            i = fire(s, best) if best else None
            if i is None:
                rows.append(dict(fired=False, saved=0.0, calls=0, later=False, passed=s["passed"], base=session_cost(s)))
                continue
            sv, nc = saved_after(s, i)
            later = any(good_edit(r) for r in s["results"][i + 1:])
            rows.append(dict(fired=True, saved=sv, calls=nc, later=later, passed=s["passed"], base=session_cost(s), th=best))
    sv = np.array([r["saved"] for r in rows])
    base = np.array([r["base"] for r in rows])
    safe = np.array([0 if r["later"] else r["saved"] for r in rows])
    fired = [r for r in rows if r["fired"]]
    harm = [r for r in fired if r["later"]]
    out = dict(n=len(rows), fired=wilson(len(fired), len(rows)),
               gross_usd=boot(sv), gross_pct=boot_ratio(sv, base),
               calls_cut_per_session=float(np.mean([r["calls"] for r in rows])),
               usd_if_harmful_fires_suppressed=boot(safe), pct_if_harmful_fires_suppressed=boot_ratio(safe, base),
               harm_rate_fired_then_edited=wilson(len(harm), len(fired)),
               pass_rate_fired_no_later_edit=wilson(sum(bool(r["passed"]) for r in fired if not r["later"]),
                                                    sum(1 for r in fired if not r["later"])),
               pass_rate_fired_later_edit=wilson(sum(bool(r["passed"]) for r in harm), len(harm)),
               thetas=sorted({r.get("th") for r in fired if r.get("th")}))
    return out


def rerun(train, held):
    rep = collections.defaultdict(lambda: [0, 0])
    for s in train:
        last = {}
        for r in s["results"]:
            if good_edit(r):
                last.clear()
                continue
            if not r["key"].startswith("Bash:"):
                continue
            m = mask(r["_text"])
            if ident(r) in last:
                rep[r["key"]][0] += 1
                rep[r["key"]][1] += last[ident(r)] == m
            last[ident(r)] = m
    idem = {k for k, (n, same) in rep.items() if n >= 3 and same / n >= 0.8}
    tok, call_ub, base, hits, diff = [], [], [], 0, 0
    for s in held:
        last = {}
        t = c = 0.0
        p = PRICES[fam(s["model"])]
        for r in s["results"]:
            if good_edit(r):
                last.clear()
                continue
            if not r["key"].startswith("Bash:"):
                continue
            m = mask(r["_text"])
            if ident(r) in last and r["key"] in idem:
                hits += 1
                diff += last[ident(r)] != m
                t += max(r["tok"] - 40, 0) * r["carry"]
                k = r["use_call"] if r["use_call"] is not None else r["seen_call"] - 1
                c += call_cost(s["calls"][k], p) + r["tok"] * r["carry"]
            last[ident(r)] = m
        tok.append(t); call_ub.append(c); base.append(session_cost(s))
    tok, call_ub, base = map(np.array, (tok, call_ub, base))
    return dict(idempotent_keys=sorted(idem), repeats_per_session=hits / len(held),
                token_stub_usd=boot(tok), token_stub_pct=boot_ratio(tok, base),
                call_level_upper_usd=boot(call_ub), call_level_upper_pct=boot_ratio(call_ub, base),
                harm_output_differed=wilson(diff, hits))


def main():
    attach_text()
    from common import harness_sets
    out = {}
    for f in ("haiku", "sonnet"):
        H, train = harness_sets(S, f)
        out[f] = dict(done_per_repo_pooled=done_signal(train, H),
                      done_leave_one_repo_out=done_signal(train, H, loro=True) if f == "haiku" else None,
                      rerun=rerun(train, H))
    json.dump(out, open("done.json", "w"), indent=1)
    print(json.dumps(out, indent=1, default=lambda x: round(x, 4)))


if __name__ == "__main__":
    main()
