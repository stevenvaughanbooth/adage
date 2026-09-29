"""Parse RepoBench Claude Code transcripts into per-call usage + per-tool-result records.

One API call = one unique assistant message id (Claude Code writes one JSONL row per content
block, each repeating the message's usage). Each tool result is attached to the first call that
sees it (the next API call after it), which is where it is written to the prompt cache; every
later call re-reads it from the cache.

Output: /tmp/adage-research/token-cost/sessions.pkl  (list of dicts)
"""
from __future__ import annotations

import glob
import json
import pickle
import re
import sys
from pathlib import Path

LAB = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(LAB))
from normalize import normalize_tool, strip_root  # noqa: E402

OUT = Path("/tmp/adage-research/token-cost")
BASES = [Path("/tmp/adage-repobench/runs"), Path("/tmp/adage-rb2/runs")]
REPOS = ["attrs", "click", "marshmallow", "urfave-cli", "vue-core", "click-sonnet5", "vue-core-sonnet5"]
RUN = re.compile(r"^(train|eval)-(.+)-([0-9a-f]{8})-r(\d+)(?:-(.+))?$")
SPLITS = LAB.parent / "repobench" / "splits"


def base_repo(r):
    return r.replace("-sonnet5", "")


def text_of(cc):
    if isinstance(cc, str):
        return cc
    out = []
    for y in cc or []:
        if isinstance(y, dict):
            out.append(y.get("text", "") if y.get("type") == "text" else json.dumps(y)[:2000])
    return "\n".join(out)


def parse(path: Path):
    rows = [json.loads(l) for l in path.open() if l.strip()]
    calls, results, agent = [], [], []  # agent: (call_idx, text) the agent wrote at that call
    mids = {}
    uses = {}
    pending_other = 0  # chars of non-tool-result content (attachments, prompt) since the last call
    prompt = ""
    for r in rows:
        t = r.get("type")
        m = r.get("message") if isinstance(r.get("message"), dict) else None
        if t == "assistant" and m and not r.get("isSidechain"):
            mid = m.get("id")
            if mid not in mids:
                u = m.get("usage") or {}
                cc = u.get("cache_creation") or {}
                mids[mid] = len(calls)
                calls.append(dict(model=m.get("model"), ts=r.get("timestamp"), inp=u.get("input_tokens", 0) or 0,
                                  cw1h=cc.get("ephemeral_1h_input_tokens", 0) or 0,
                                  cw5m=cc.get("ephemeral_5m_input_tokens", 0) or 0,
                                  cw=u.get("cache_creation_input_tokens", 0) or 0,
                                  cr=u.get("cache_read_input_tokens", 0) or 0,
                                  out=u.get("output_tokens", 0) or 0,
                                  think=(u.get("output_tokens_details") or {}).get("thinking_tokens", 0) or 0,
                                  res_chars=0, other_chars=pending_other, ntools=0))
                pending_other = 0
            ci = mids[mid]
            for x in m.get("content") or []:
                if not isinstance(x, dict):
                    continue
                if x.get("type") == "text":
                    agent.append((ci, x.get("text", "")))
                elif x.get("type") == "tool_use":
                    inp = x.get("input") or {}
                    uses[x.get("id")] = (ci, x.get("name", ""), inp)
                    calls[ci]["ntools"] += 1
                    agent.append((ci, json.dumps(inp, ensure_ascii=False)))
        elif t == "user" and m:
            c = m.get("content")
            if isinstance(c, str):
                if not prompt:
                    prompt = c
                pending_other += len(c)
                continue
            for x in c or []:
                if not isinstance(x, dict):
                    continue
                if x.get("type") == "tool_result":
                    txt = text_of(x.get("content"))
                    ci, name, inp = uses.get(x.get("tool_use_id"), (None, "", {}))
                    results.append(dict(use_call=ci, seen_call=len(calls), name=name, input=inp,
                                        text=txt, chars=len(txt), is_error=bool(x.get("is_error"))))
                elif x.get("type") == "text":
                    if not prompt:
                        prompt = x.get("text", "")
                    pending_other += len(x.get("text", ""))
        elif t == "attachment":
            a = r.get("attachment") or {}
            if a.get("type") in ("prompt_snapshot",):
                continue
            pending_other += len(a.get("text", "") or "") + len(json.dumps(r.get("rendered", ""))) if r.get("rendered") else len(a.get("text", "") or "")
    # attribute result chars to the call that first sees them
    for res in results:
        if res["seen_call"] < len(calls):
            calls[res["seen_call"]]["res_chars"] += res["chars"]
        a = normalize_tool(res["name"], res["input"]) if res["name"] else None
        res["key"] = a.key if a else "?"
        res["template"] = a.template if a else "?"
        res["full"] = a.full if a else "?"
        res["explore"] = a.explore if a else False
        res["edit"] = a.edit if a else False
    cost_state = None
    for r in rows:
        if r.get("type") == "cost-state":
            cost_state = r.get("totalCostUSD")
    return prompt, calls, results, agent, cost_state


def main():
    splits = {r: json.loads((SPLITS / f"{r}.json").read_text())["repos"][r] for r in
              ["attrs", "click", "marshmallow", "urfave-cli", "vue-core"]}
    seen, out = set(), []
    for base in BASES:
        for repo in REPOS:
            try:
                state = json.loads((base / repo / "state.json").read_text()).get("runs", {})
            except Exception:
                state = {}
            for f in sorted(glob.glob(str(base / repo / "private/transcripts/*/*.jsonl"))):
                f = Path(f)
                sid = f.stem
                if sid in seen:
                    continue
                d = f.parent.name
                rid = re.sub(r"^.*-runs-(?=(train|eval)-)", "", d)
                rid = re.sub(r"-repo$", "", rid)
                mm = RUN.match(rid)
                if not mm:
                    continue
                phase, _, h, rnd, arm = mm.groups()
                prompt, calls, results, agent, cs = parse(f)
                if not calls:
                    continue
                seen.add(sid)
                st = state.get(rid, {})
                met = st.get("metrics") or {}
                passed = met.get("hidden_passed", met.get("passed"))
                if passed is None and st.get("status") in ("passed", "failed"):
                    passed = st["status"] == "passed"
                br = base_repo(repo)
                task = f"{br}-{h}"
                sp = splits[br]
                split = ("train" if task in sp["train"] else "test" if task in sp["test"] else
                         "buffer" if task in sp["buffer"] else "other")
                model = max(set(c["model"] for c in calls if c["model"]),
                            key=lambda mo: sum(c["model"] == mo for c in calls))
                out.append(dict(sid=sid, repo=br, dataset=repo, run=rid, phase=phase, arm=arm or "train",
                                task=task, split=split, passed=passed, model=model, prompt=prompt[:2000],
                                calls=calls, results=results, agent=agent, cc_cost=cs, path=str(f)))
    OUT.mkdir(parents=True, exist_ok=True)
    with open(OUT / "sessions.pkl", "wb") as fh:
        pickle.dump(out, fh)
    import collections
    print(len(out), collections.Counter((s["dataset"], s["model"][:12], s["arm"], s["split"]) for s in out))


if __name__ == "__main__":
    main()
