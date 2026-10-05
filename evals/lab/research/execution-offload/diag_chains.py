from data import *
from learner import PATH, authoring
from collections import Counter
c=corpus()
for m in ("haiku","sonnet"):
    S=c.held_out(model=m); cnt=Counter()
    for s in S:
        R=records(s); A=s.acts
        for k in range(len(R)-1):
            if A[k].run is None: continue
            fail = R[k].failed or " failed" in R[k].out or "FAIL" in R[k].out
            cnt["fail" if fail else "pass"]+=1
            nxt=R[k+1]
            paths=[p.group(1).lstrip("./") for p in PATH.finditer(R[k].out)]
            if nxt.tool=="Read":
                fp=nxt.args.get("file_path","")
                cnt[("fail" if fail else "pass")+": next Read"]+=1
                if fp in paths: cnt[("fail" if fail else "pass")+": next Read of printed path"]+=1
                if fp in paths[:5]: cnt[("fail" if fail else "pass")+": ...in first 5 printed"]+=1
                if fp in paths and set(nxt.args)<= {"file_path"}: cnt[("fail" if fail else "pass")+": ...whole file"]+=1
            if authoring(nxt): cnt[("fail" if fail else "pass")+": next edit"]+=1
            if nxt.tool=="Bash" and nxt.text.startswith("git"): cnt[("fail" if fail else "pass")+": next git"]+=1
            if A[k+1].run is not None: cnt[("fail" if fail else "pass")+": next test run"]+=1
    print(m,{k:round(v/len(S),2) for k,v in sorted(cnt.items())})
