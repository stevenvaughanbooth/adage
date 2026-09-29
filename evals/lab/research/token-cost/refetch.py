"""Natural experiments: when an output WAS truncated in the traces, how often did the agent go back
for the elided part? Calibrates the miss rate of a trimming policy.

(1) Claude Code's own Read truncation (a `read_truncation_notice` attachment, or a whole-file
    Read whose text stops before the file's end): did a later Read of that file start beyond the
    shown range?
(2) The agent's own `| head -N` / `| tail -N` on a Bash command: did it re-run the same command
    (same stripped prefix before the pipe) within the next 5 calls, without that pipe or with a
    larger N?
(3) The agent's own ranged Read: did a later Read of the same file cover lines just after the
    range (continuation, start within [end-20, end+200])?
The regexes here are a MEASUREMENT instrument (hand-written), not part of a policy.
"""
import json
import os
import pickle
import re

from common import harness_sets, fam
from policies import wilson

raw = pickle.load(open("/tmp/adage-research/token-cost/sessions.pkl", "rb"))
PIPE = re.compile(r"^(.*?)\|\s*(head|tail)\s+(?:-n\s*)?-?(\d+)\s*$", re.S)


def num(x, d):
    try:
        return int(x)
    except (TypeError, ValueError):
        return d


def main():
    out = {}
    for model in ("haiku", "sonnet"):
        held, train = harness_sets(raw, model)
        c = dict(cc_trunc=[0, 0], pipe=[0, 0], ranged=[0, 0])
        for s in held + train:
            res = s["results"]
            for i, r in enumerate(res):
                inp = r["input"] or {}
                later = res[i + 1:]
                if r["name"] == "Read" and not inp.get("offset") and not inp.get("limit"):
                    m = re.search(r"showing lines \d+-(\d+) of (\d+)", r["text"][-400:])
                    if m or "PARTIAL view" in r["text"][:300]:
                        end = int(m.group(1)) if m else 0
                        c["cc_trunc"][1] += 1
                        c["cc_trunc"][0] += any(x["name"] == "Read" and (x["input"] or {}).get("file_path") == inp.get("file_path")
                                                and num((x["input"] or {}).get("offset"), 0) >= end - 50 for x in later)
                if r["name"] == "Bash":
                    cmd = inp.get("command", "").strip()
                    m = PIPE.match(cmd)
                    if m and r["text"].count("\n") + 1 >= num(m.group(3), 0) - 1:  # the cap was binding
                        base, n = m.group(1).strip(), num(m.group(3), 0)
                        c["pipe"][1] += 1
                        hit = False
                        for x in later[:5]:
                            if x["name"] != "Bash":
                                continue
                            xc = (x["input"] or {}).get("command", "").strip()
                            if xc.startswith(base[:40]):
                                mm = PIPE.match(xc)
                                if not mm or num(mm.group(3), 0) > n or mm.group(2) != m.group(2):
                                    hit = True
                        c["pipe"][0] += hit
                if r["name"] == "Read" and inp.get("limit") and not r["is_error"]:
                    st = num(inp.get("offset"), 1)
                    lim = num(inp.get("limit"), 0)
                    if r["text"].count("\n") + 1 < lim - 2:
                        continue  # reached the file end: nothing elided
                    end = st + lim
                    c["ranged"][1] += 1
                    c["ranged"][0] += any(x["name"] == "Read" and (x["input"] or {}).get("file_path") == inp.get("file_path")
                                          and end - 20 <= num((x["input"] or {}).get("offset"), 1) <= end + 200 for x in later)
        out[model] = {k: dict(refetched=v[0], n=v[1], rate=wilson(v[0], v[1])) for k, v in c.items()}
    json.dump(out, open("refetch.json", "w"), indent=1)
    print(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
