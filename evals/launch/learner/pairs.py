"""For each dataset session: pairs (last edited source file, test files named by the next test run),
the runner prefix of each test run, and whether it failed."""
import os
import json, re, sys, collections
D = os.environ.get("LD", "/tmp/adage-learner") + ""
TEST = re.compile(r"(pytest|vitest|npm test|npm run test|pnpm test|go test|tox|nox|jest)")


def calls(f):
    out, res = [], {}
    for line in open(f):
        try:
            j = json.loads(line)
        except Exception:
            continue
        c = (j.get("message") or {}).get("content")
        if not isinstance(c, list):
            continue
        for b in c:
            if b.get("type") == "tool_use":
                i = b.get("input", {})
                out.append((b["id"], b["name"], i.get("command") or i.get("file_path") or ""))
            if b.get("type") == "tool_result":
                cc = b.get("content")
                if isinstance(cc, list):
                    cc = " ".join(x.get("text", "") for x in cc if isinstance(x, dict))
                res[b["tool_use_id"]] = (bool(b.get("is_error")), str(cc))
    return [(n, c, res.get(i, (None, ""))) for i, n, c in out]


repo = sys.argv[1]
split = sys.argv[2] if len(sys.argv) > 2 else "train"
m = json.load(open(f"{D}/ds/rb-{repo}/manifest.json"))
ids = set(m["splits"]["heldout"][split])
runners = collections.Counter()
pairs = collections.Counter()
for s in m["sessions"]:
    if s["id"] not in ids:
        continue
    last = None
    for n, c, (err, out) in calls(f"{D}/ds/rb-{repo}/{s['file']}"):
        c = re.sub(r"/tmp/adage-\S+?/repo/?", "", c)
        if n in ("Edit", "Write", "MultiEdit"):
            if not c.startswith("/"):
                last = c
            continue
        if n != "Bash" or not TEST.search(c):
            continue
        first = c.split("|")[0].split("2>&1")[0].strip()
        segs = [x.strip() for x in re.split(r"&&|;", first)]
        t = next((x for x in segs if TEST.search(x)), first)
        runner = t.split(" tests")[0].split(" packages")[0].split(" --")[0][:50]
        runners[(runner, "ERR" if err else "ok")] += 1
        files = re.findall(r"((?:tests|packages|src)/[\w/.\-]+?\.(?:py|ts|go))", t)
        if last and files:
            for tf in files:
                pairs[(last, tf)] += 1
print("RUNNERS")
for (r, e), n in runners.most_common(25):
    print(f"{n:4d} {e:3s} {r}")
print("PAIRS (last edit -> test file)")
for (a, b), n in pairs.most_common(40):
    print(f"{n:4d} {a} -> {b}")
