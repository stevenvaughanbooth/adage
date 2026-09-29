"""led.py <ledger-binary> <repo> <out.json> [--demo] [--split train|test|all]: the drop ledger over a
dataset's sessions (default: the train split), with a summary of drop reasons."""
import os
import collections, json, re, subprocess, sys
D = os.environ.get("LD", "/tmp/adage-learner") + ""
binary, repo, out = sys.argv[1:4]
extra = [a for a in sys.argv[4:] if a == "--demo"]
for f in ("--root", "--priors"):
    if f in sys.argv:
        extra += [f, sys.argv[sys.argv.index(f) + 1]]
split = "train"
if "--split" in sys.argv:
    split = sys.argv[sys.argv.index("--split") + 1]
m = json.load(open(f"{D}/ds/rb-{repo}/manifest.json"))
if split == "all":
    ids = {s["id"] for s in m["sessions"]}
else:
    ids = set(m["splits"]["heldout"][split])
files = [f"{D}/ds/rb-{repo}/{s['file']}" for s in m["sessions"] if s["id"] in ids]
res = subprocess.run([binary, "--repo", m["repo"], *extra, *files], capture_output=True, text=True)
if res.returncode != 0:
    sys.exit(res.stderr)
sys.stderr.write(res.stderr); open(out, "w").write(res.stdout)
d = json.loads(res.stdout)
e = d["engine"]
print(f"{repo}: {d['sessions']} sessions, {len(d['candidates'])} scored candidates, {e['reflexes']} selected, "
      f"{e['compiled']} compiled, {e['routes']} routes")


def reason(stage):
    s = re.sub(r"[a-z-]+-[0-9a-f]{6,}: ", "", stage)
    s = re.sub(r"\d+(\.\d+)?", "N", s)
    s = re.sub(r'"[^"]*"', "X", s)
    s = re.sub(r"\S+/\S+", "P", s)
    return s[:110]


c = collections.Counter((x["shape"], reason(x["stage"])) for x in d["candidates"])
for (shape, r), n in sorted(c.items(), key=lambda kv: -kv[1]):
    print(f"{n:5d}  {shape:10s} {r}")
for v in e.get("valued", []):
    a = v["advantage"]
    print("VALUED", v["key"]["trigger"], v["failed"], "->", v["command"], "A=%.2f sd=%.2f P=%.2f lb=%.2f Q0=%.2f Qs=%.2f n0=%d ns=%d ran=%d decl=%s" % (
        a["mean"], a["sd"], a["p_positive"], a["lower"], a["q_null"], a["q_sat"], v["n_null"], v["n_sat"], v["ran_tests"], v["declared"]), v["strength"])
t = e.get("testmap")
if t:
    a = t["advantage"]
    print("TESTMAP", t["glob"], t["command"], "files", t["map_files"], "A=%.2f sd=%.2f P=%.2f lb=%.2f Q0=%.2f Qs=%.2f n0=%d ns=%d verified=%s" % (
        a["mean"], a["sd"], a["p_positive"], a["lower"], a["q_null"], a["q_sat"], t["n_null"], t["n_sat"], t["verified"]), t["strength"])
if e.get("card"):
    print("CARD", e["card"])
for r in d["routes"]:
    print("ROUTE", r["id"], r["strength"], json.dumps(r["trigger"]), json.dumps(r["step"]))
