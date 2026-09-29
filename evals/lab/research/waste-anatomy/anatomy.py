"""Decompose every session into segments and price every call under the cumulative-context model.

Input: the cache written by `load.py`. Output: `/tmp/adage-research/waste-anatomy/calls.pkl`, one
record per tool call (and per tool-less request) with its segment, hindsight flags, tokens and its
lifetime dollar cost, plus the per-request arrays needed for counterfactual re-pricing.

Instruments, and which are hand-coded (declared, measurement only; nothing here is a product
feature):

- HAND-CODED role instrument (`role`): tool name (Read/Grep/Glob/LS -> explore, Edit/Write/
  MultiEdit -> edit), the lab's `normalize.py` explore flag for shell, and three command regexes:
  TEST_RX (test runners), SETUP_RX (installers/env probes), `git` prefix -> vcs. Same kind of
  instrument as the architecture review's regex classes.
- HAND-CODED test-path regex (`is_test_path`), to split the final diff into source and tests.
- LEARNED failure detector (`fail_model`): logistic regression on TF-IDF char n-grams of a shell
  result, trained on the TRAIN split's `is_error` labels, applied to held-out results. It catches
  failures a pipe hides (`pytest ... | head` exits 0).
- LEARNED stoplist: document frequency of tokens over TRAIN-split tool results; tokens in >2% of
  results are not evidence of provenance.
- HINDSIGHT (no hand-coding): the final diff (replayed Edit/Write calls against the task's base
  commit, intersected with the scorer's changed files), edit survival, the provenance graph (a
  later call's input or visible text re-uses a rare token first introduced by an earlier result),
  the needed slice (backward closure of provenance from surviving edits, plus the tool's own
  read-before-edit rule), re-invocation after failure (char-3-gram cosine between commands), exact
  re-reads and re-runs.
"""

from __future__ import annotations

import gzip
import json
import math
import pickle
import re
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))  # evals/lab
from normalize import normalize_bash  # noqa: E402

CACHE = Path("/tmp/adage-research/waste-anatomy")
PRICES = {  # $/MTok: input, cache write 1h, cache read, output. Verified against total_cost_usd.
    "haiku": dict(pin=1.0, pw=2.0, pr=0.10, pout=5.0),
    "sonnet": dict(pin=2.0, pw=4.0, pr=0.20, pout=10.0),
}
CHARS_PER_TOKEN = 3.08  # fitted: context growth vs result characters (R^2 0.91 with call overhead)

TEST_RX = re.compile(r"\b(pytest|py\.test|unittest|go test|vitest|jest|(npm|pnpm|yarn)( run)? test|tox|nox)\b")
SETUP_RX = re.compile(r"\b(pip3? (install|list|show|freeze)|uv (sync|pip|venv|add)|npm (install|ci|i)\b|"
                      r"pnpm (install|i)\b|go (mod|get)|which|--version|venv|virtualenv|poetry|conda|"
                      r"pip-compile|ensurepip)\b")
TOK = re.compile(r"[A-Za-z_][A-Za-z0-9_]{3,}")


def is_test_path(p: str) -> bool:
    return bool(re.search(r"(^|/)(tests?|__tests__|spec)(/|$)|(^|/)test_[\w]+\.py$|_test\.(go|py)$|"
                          r"\.(spec|test)\.[jt]sx?$", p))


def family(model: str) -> str:
    return "sonnet" if "sonnet" in (model or "") else "haiku"


def toks(s: str) -> set[str]:
    return set(TOK.findall(s or ""))


def c3(s: str) -> Counter:
    s = re.sub(r"\s+", " ", s)[:400]
    return Counter(s[i:i + 3] for i in range(max(0, len(s) - 2)))


def cos(a: Counter, b: Counter) -> float:
    if not a or not b:
        return 0.0
    num = sum(v * b.get(k, 0) for k, v in a.items())
    return num / math.sqrt(sum(v * v for v in a.values()) * sum(v * v for v in b.values()))


def consumer_text(call: dict) -> str:
    i = call["input"]
    if not isinstance(i, dict):
        return str(i)
    return " ".join(str(v) for v in i.values())


# ---------------------------------------------------------------- final diff by replay

def replay(sess: dict) -> tuple[dict, dict]:
    """Replay successful Edit/Write/MultiEdit calls per repo-relative file from the base commit.
    Returns (final added lines per file, per-call survival {call_id: bool})."""
    orig = sess.get("originals") or {}
    content: dict[str, str | None] = {}
    edits: dict[str, list] = defaultdict(list)  # file -> [(call_id, added lines)]
    for q in sess["requests"]:
        for c in q["calls"]:
            if c["name"] not in ("Edit", "Write", "MultiEdit") or c["err"]:
                continue
            i = c["input"] if isinstance(c["input"], dict) else {}
            fp = i.get("file_path") or ""
            if fp not in content:
                content[fp] = orig.get(fp)
            cur = content[fp]
            added = set()
            if c["name"] == "Write":
                new = i.get("content") or ""
                old_lines = set((cur or "").splitlines())
                added = {l.strip() for l in new.splitlines() if l.strip() and l not in old_lines}
                content[fp] = new
            else:
                ops = i.get("edits") if c["name"] == "MultiEdit" else [i]
                for op in ops or []:
                    o, n = op.get("old_string", ""), op.get("new_string", "")
                    ol = set(o.splitlines())
                    added |= {l.strip() for l in n.splitlines() if l.strip() and l not in ol}
                    if cur is not None and o and o in cur:
                        cur = cur.replace(o, n) if op.get("replace_all") else cur.replace(o, n, 1)
                content[fp] = cur
            edits[fp].append((c["id"], added))
    changed = set(sess.get("changed_files") or [])
    final_added, survive = {}, {}
    for fp, lst in edits.items():
        o, f = orig.get(fp), content.get(fp)
        in_diff = fp in changed or (o is not None and f is not None and o != f)
        if fp.startswith("/") or not in_diff:
            fa = set()
        elif f is None:
            fa = set().union(*[a for _, a in lst]) if lst else set()
        else:
            ol = set((o or "").splitlines())
            fa = {l.strip() for l in f.splitlines() if l.strip() and l not in ol}
        final_added[fp] = fa
        for cid, added in lst:
            nontriv = {l for l in added if len(l) >= 3}
            survive[cid] = bool(fa) and (bool(nontriv & fa) if nontriv else bool(fa))
    return final_added, survive


# ---------------------------------------------------------------- learned pieces (train split)

def fit_learned(train: list[dict]):
    from sklearn.feature_extraction.text import TfidfVectorizer
    from sklearn.linear_model import LogisticRegression
    df = Counter()
    ndoc = 0
    X, y = [], []
    for s in train:
        for q in s["requests"]:
            for c in q["calls"]:
                ndoc += 1
                df.update(toks(c["res"]))
                if c["name"] == "Bash":
                    X.append(res_view(c["res"]))
                    y.append(int(c["err"]))
    stop = {t for t, n in df.items() if n / ndoc > 0.02}
    vec = TfidfVectorizer(analyzer="char_wb", ngram_range=(3, 5), min_df=2, max_features=60000, sublinear_tf=True)
    Xv = vec.fit_transform(X)
    clf = LogisticRegression(C=4.0, max_iter=2000, class_weight="balanced").fit(Xv, y)
    return stop, vec, clf


def res_view(r: str) -> str:
    r = r or ""
    return (r[:300] + " ... " + r[-1200:]) if len(r) > 1500 else r


# ---------------------------------------------------------------- per session

def analyse(sess: dict, stop: set, vec, clf) -> dict:
    fam = family(sess["requests"][0]["model"])
    P = PRICES[fam]
    R = sess["requests"]
    n = len(R)
    C = np.array([q["usage"].get("in", 0) + q["usage"].get("cw", 0) + q["usage"].get("cr", 0) for q in R], float)
    W = np.array([q["usage"].get("cw", 0) for q in R], float)
    I = np.array([q["usage"].get("in", 0) for q in R], float)
    O = np.array([q["usage"].get("out", 0) for q in R], float)
    D = np.r_[np.diff(C), 0.0]  # tokens appended after request k (its results + next overhead)
    D = np.maximum(D, 0)
    actual = float(((C - W - I) * P["pr"] + W * P["pw"] + I * P["pin"] + O * P["pout"]).sum() / 1e6)

    final_added, survive = replay(sess)
    prompt_t = toks(sess["prompt"])

    calls = []
    for k, q in enumerate(R):
        for c in q["calls"]:
            calls.append({"k": k, "c": c})
    # token attribution of D_k: result tokens by characters, the rest is request overhead
    for k, q in enumerate(R):
        cs = [x for x in calls if x["k"] == k]
        rt = np.array([len(x["c"]["res"]) / CHARS_PER_TOKEN for x in cs])
        tot = rt.sum()
        if tot > D[k] and tot > 0:
            rt *= D[k] / tot
        over = D[k] - rt.sum()
        for x, t in zip(cs, rt):
            x["res_tok"] = float(t)
            x["over_tok"] = float(over / len(cs))
        q["_over_unassigned"] = float(over) if not cs else 0.0

    # ---- roles (HAND-CODED instrument)
    for x in calls:
        c = x["c"]
        nm, i = c["name"], c["input"] if isinstance(c["input"], dict) else {}
        cmd = i.get("command", "") if nm == "Bash" else ""
        x["cmd"] = cmd
        if nm in ("Edit", "Write", "MultiEdit", "NotebookEdit"):
            role = "edit"
        elif nm in ("Read", "Grep", "Glob", "LS", "NotebookRead"):
            role = "explore"
        elif nm == "Bash":
            key, _, _, explore = normalize_bash(cmd)
            if TEST_RX.search(cmd):
                role = "test"
            elif SETUP_RX.search(cmd):
                role = "setup"
            elif key.startswith("Bash:git"):
                role = "vcs"
            elif explore:
                role = "explore"
            else:
                role = "exec"
        else:
            role = "other"
        x["role"] = role
        x["file"] = i.get("file_path") or ""

    # ---- learned failure
    bash = [x for x in calls if x["c"]["name"] == "Bash"]
    if bash:
        pf = clf.predict_proba(vec.transform([res_view(x["c"]["res"]) for x in bash]))[:, 1]
        for x, p in zip(bash, pf):
            x["p_fail"] = float(p)
    for x in calls:
        x.setdefault("p_fail", float(x["c"]["err"]))
        x["failed"] = bool(x["c"]["err"]) or x["p_fail"] > 0.5
        x["piped_hidden_fail"] = (not x["c"]["err"]) and x["p_fail"] > 0.5

    # ---- provenance: first-introduction of rare tokens
    seen = set(prompt_t)
    src = {}
    for idx, x in enumerate(calls):
        # the request text and inputs before this call's result are "seen" (agent wrote them)
        seen |= toks(consumer_text(x["c"]))
        new = (toks(x["c"]["res"]) - seen) - stop
        x["novel"] = len(new)
        for t in new:
            src[t] = idx
        seen |= toks(x["c"]["res"])
    text_t = [toks(q["text"]) for q in R]
    refs_by = defaultdict(set)  # consumer idx -> source idxs
    referenced = [0] * len(calls)
    for j, x in enumerate(calls):
        ct = (toks(consumer_text(x["c"])) | text_t[x["k"]]) - stop
        for t in ct:
            s_ = src.get(t)
            if s_ is not None and calls[s_]["k"] < x["k"]:
                refs_by[j].add(s_)
                referenced[s_] += 1
    # tool-less text requests (final summaries) also consume
    for x, r in zip(calls, referenced):
        x["referenced"] = r > 0

    # ---- surviving edits, needed slice
    for x in calls:
        x["survive"] = survive.get(x["c"]["id"], False) if x["role"] == "edit" else False
        x["src_edit"] = x["survive"] and not is_test_path(x["file"])
    seeds = [j for j, x in enumerate(calls) if x["survive"]]
    seeds_src = [j for j, x in enumerate(calls) if x["src_edit"]]

    def closure(seed):
        need = set(seed)
        stack = list(seed)
        while stack:
            j = stack.pop()
            x = calls[j]
            for s_ in refs_by[j]:
                if s_ not in need:
                    need.add(s_)
                    stack.append(s_)
            if x["role"] == "edit" and x["c"]["name"] != "Write":  # read-before-edit rule
                for s_ in range(j - 1, -1, -1):
                    y = calls[s_]
                    if y["c"]["name"] == "Read" and y["file"] == x["file"] and not y["c"]["err"]:
                        if s_ not in need:
                            need.add(s_)
                            stack.append(s_)
                        break
        return need

    need = closure(seeds)
    need_src = closure(seeds_src)
    for j, x in enumerate(calls):
        x["needed"] = j in need
        x["needed_src"] = j in need_src

    # ---- hindsight overlays
    last_surv = max([j for j in seeds], default=-1)
    last_edit_before = []
    le = None
    for j, x in enumerate(calls):
        last_edit_before.append(le)
        if x["role"] == "edit" and not x["c"]["err"]:
            le = j
    # redundant: exact re-read / exact re-run without an intervening edit (to that file / anywhere)
    read_cover = defaultdict(list)
    run_seen = {}
    edit_count = 0
    file_edit_ver = Counter()
    for j, x in enumerate(calls):
        c = x["c"]
        i = c["input"] if isinstance(c["input"], dict) else {}
        red = False
        if c["name"] == "Read" and not c["err"]:
            off = int(i.get("offset") or 1)
            lim = int(i.get("limit") or 10 ** 6)
            span = (off, off + lim, file_edit_ver[x["file"]])
            for a, b, v in read_cover[x["file"]]:
                if v == span[2] and a <= span[0] and span[1] <= b:
                    red = True
            read_cover[x["file"]].append(span)
        elif c["name"] in ("Bash", "Grep", "Glob"):
            # an identical call with no edit since, whose earlier twin did not fail (a retry after
            # a failure is the friction's resolution, not redundancy)
            key = (c["name"], json.dumps(i, sort_keys=True), edit_count)
            prev = run_seen.get(key)
            red = prev is not None and not calls[prev]["failed"] and not any(
                calls[t]["role"] == "setup" for t in range(prev + 1, j))
            run_seen[key] = j
        if x["role"] == "edit" and not c["err"]:
            edit_count += 1
            file_edit_ver[x["file"]] += 1
        x["redundant"] = red

    # friction: failed exec/test/setup call re-invoked in similar form, before any edit, and the
    # re-invocation did not fail. Calls in between that are neither edits nor needed are the detour.
    friction = set()
    abandoned = set()
    execs = [j for j, x in enumerate(calls) if x["role"] in ("test", "exec", "setup") and x["cmd"]]
    for a_i, j in enumerate(execs):
        x = calls[j]
        if not x["failed"]:
            continue
        cj = c3(x["cmd"])
        resolved = None
        for m in execs[a_i + 1:]:
            if any(calls[t]["role"] == "edit" and not calls[t]["c"]["err"] for t in range(j + 1, m)):
                break
            if cos(cj, c3(calls[m]["cmd"])) >= 0.35:
                if not calls[m]["failed"]:
                    resolved = m
                    break
        if resolved is not None:
            friction.add(j)
            for t in range(j + 1, resolved):
                y = calls[t]
                if y["role"] in ("setup", "exec", "test", "explore", "other") and not y["needed"]:
                    friction.add(t)
        else:
            nxt_edit = any(calls[t]["role"] == "edit" for t in range(j + 1, len(calls)))
            if not nxt_edit:
                abandoned.add(j)
    for j, x in enumerate(calls):
        if x["role"] == "setup":
            friction.add(j)
    # dead-end edits and the runs that exercised them
    deadend = set()
    for j, x in enumerate(calls):
        if x["role"] == "edit" and not x["survive"]:
            deadend.add(j)
    for j, x in enumerate(calls):
        if x["role"] in ("test", "exec") and not x["needed"]:
            le_ = last_edit_before[j]
            if le_ is not None and le_ in deadend and j > last_surv:
                deadend.add(j)

    # primary segment (precedence)
    for j, x in enumerate(calls):
        if j in friction:
            seg = "env_friction"
        elif x["redundant"]:
            seg = "redundant"
        elif x["role"] == "edit":
            seg = "edit_final" if x["survive"] else "edit_deadend"
        elif j in deadend:
            seg = "deadend_runs"
        elif x["role"] in ("test", "exec") and j > last_surv and last_surv >= 0:
            seg = "verify"
        elif x["role"] == "test":
            seg = "test"
        elif x["role"] == "exec":
            seg = "repro_exec"
        elif x["role"] == "explore":
            seg = "orient"
        elif x["role"] == "vcs":
            seg = "vcs"
        else:
            seg = "other"
        x["seg"] = seg
        x["abandoned_exec"] = j in abandoned

    # ---- lifetime $ per call (exact linear decomposition of the model cost)
    S0 = C[0]
    later = np.array([n - k - 1 for k in range(n)], float)  # requests after k
    ncall_k = Counter(x["k"] for x in calls)
    rec = []
    for j, x in enumerate(calls):
        k = x["k"]
        m = ncall_k[k]
        own = (S0 * P["pr"] + I[k] * P["pin"] + O[k] * P["pout"]) / m / 1e6
        tok = x["res_tok"] + x["over_tok"]
        carry_w = tok * P["pw"] / 1e6 if k + 1 < n else 0.0
        carry_r = tok * P["pr"] * max(later[k] - 1, 0) / 1e6
        c = x["c"]
        rec.append(dict(
            idx=j, k=k, name=c["name"], role=x["role"], seg=x["seg"], err=c["err"], p_fail=x["p_fail"],
            failed=x["failed"], hidden_fail=x["piped_hidden_fail"], redundant=x["redundant"],
            survive=x["survive"], src_edit=x["src_edit"], needed=x["needed"], needed_src=x["needed_src"],
            referenced=x["referenced"], novel=x["novel"], abandoned=x["abandoned_exec"],
            res_tok=x["res_tok"], over_tok=x["over_tok"], own_usd=own, carry_usd=carry_w + carry_r,
            later=float(later[k]), cmd=x["cmd"][:200], file=x["file"], res_lines=len(c["res"].splitlines()),
            res=c["res"] if x["role"] in ("test", "exec", "setup") else "",
        ))
        x["idx"] = j
    # line-level hindsight for trimming: which result lines contain a token consumed later
    # suffix union of everything the agent wrote after request k (inputs and visible text)
    wrote = [set() for _ in range(n + 1)]
    for x in calls:
        wrote[x["k"]] |= toks(consumer_text(x["c"]))
    for k in range(n):
        wrote[k] |= text_t[k]
    suffix = [set() for _ in range(n + 1)]
    for k in range(n - 1, -1, -1):
        suffix[k] = suffix[k + 1] | wrote[k]
    for j, x in enumerate(calls):
        later_t = suffix[x["k"] + 1]
        lines = x["c"]["res"].splitlines()
        flags = [bool((toks(l) - stop - prompt_t) & later_t) for l in lines]
        keep = sum(len(l) + 1 for l, f in zip(lines, flags) if f)
        rec[j]["ref_chars"] = keep
        rec[j]["line_len"] = np.array([len(l) + 1 for l in lines], np.int32)
        rec[j]["line_ref"] = np.flatnonzero(flags).astype(np.int32)
        rec[j]["res_chars"] = len(x["c"]["res"])
    toolless = [k for k in range(n) if not ncall_k[k]]
    toolless_usd = sum((S0 * P["pr"] + I[k] * P["pin"] + O[k] * P["pout"]) / 1e6
                       + R[k]["_over_unassigned"] * (P["pw"] + P["pr"] * max(later[k] - 1, 0)) / 1e6
                       for k in toolless)
    return dict(
        sid=sess["sid"], repo=sess["repo"], task=sess["task"], base=sess["base"], arm=sess["arm"],
        fam=fam, cap=sess["cap"], passed=sess["passed"], censored=sess["censored"],
        cost_reported=sess["cost_reported"], cost_actual=actual, n_req=n, C=C.tolist(), W=W.tolist(),
        I=I.tolist(), O=O.tolist(), D=D.tolist(), prices=P, calls=rec, toolless_usd=toolless_usd,
        toolless=toolless, n_final_files=len([f for f, a in final_added.items() if a]),
        prompt=sess["prompt"][:2000],
    )


def main():
    sessions = [json.loads(l) for l in gzip.open(CACHE / "sessions.jsonl.gz", "rt")]
    sessions = [s for s in sessions if s["state_sid"] == s["sid"]]
    train = [s for s in sessions if s["arm"] == "train"]
    stop, vec, clf = fit_learned(train)
    out = []
    for s in sessions:
        if s["arm"] not in ("control", "train", "adage", "agents-md"):
            continue
        out.append(analyse(s, stop, vec, clf))
    with (CACHE / "calls.pkl").open("wb") as f:
        pickle.dump(out, f)
    print("analysed", len(out), file=sys.stderr)


if __name__ == "__main__":
    main()
