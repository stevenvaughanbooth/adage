from data import *
from learner import authoring
from verifier import files_of
from collections import Counter
import random
c=corpus()
for m in ("haiku","sonnet"):
    S=c.held_out(model=m); cnt=Counter(); ex=[]
    for s in S:
        R=records(s); A=s.acts; prev=None; last_edit=-1
        for k,r in enumerate(R):
            if authoring(r): last_edit=k; continue
            a=A[k]
            if a.run is None: continue
            if prev is not None and last_edit<prev:
                p=A[prev]
                if p.env_friction or not p.executed: cnt["prev run was friction / ran nothing"]+=1
                elif R[prev].key==r.key: cnt["identical re-run"]+=1
                elif files_of(a)==files_of(p): cnt["same files, other selector/flags/pipe"]+=1
                elif files_of(a) and files_of(p) and files_of(a)<files_of(p): cnt["narrower"]+=1
                else: cnt["different/broader files"]+=1; ex.append((R[prev].text[:70], r.text[:70]))
            prev=k
    print(m,{k:round(v/len(S),2) for k,v in cnt.items()})
    random.seed(1)
    for e in random.sample(ex,6): print("   ",e)
