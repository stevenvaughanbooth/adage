# diagnostics: where do test runs sit relative to edits and earlier runs (instrument, measurement only)
from data import *
from learner import authoring
from verifier import files_of
from collections import Counter
c=corpus()
for m in ("haiku","sonnet"):
    S=c.held_out(model=m); cnt=Counter(); n=0
    for s in S:
        R=records(s); A=s.acts; prev_runs=[]; last_edit=-1
        for k,r in enumerate(R):
            if authoring(r): last_edit=k; continue
            a=A[k]
            if a.run is None: continue
            n+=1
            first = not prev_runs
            # is there an edit since the previous test run?
            prev=prev_runs[-1] if prev_runs else -1
            edit_since = last_edit>prev
            covered_prev = any(R[p].key==r.key or (A[p].run.runner==a.run.runner and (not files_of(A[p]) or (files_of(a) and files_of(a)<=files_of(A[p])))) for p in prev_runs[-1:])
            covered_any = any(R[p].key==r.key or (A[p].run.runner==a.run.runner and (not files_of(A[p]) or (files_of(a) and files_of(a)<=files_of(A[p])))) for p in prev_runs)
            cnt["first run in session" if first else ("after edit" if edit_since else "no edit since previous run")]+=1
            if not first and edit_since: cnt["after edit & covered by previous run" if covered_prev else ("after edit & covered by some earlier run" if covered_any else "after edit & new targets")]+=1
            prev_runs.append(k)
    print(m,n,{k:(v,round(v/len(S),2)) for k,v in cnt.items()})
