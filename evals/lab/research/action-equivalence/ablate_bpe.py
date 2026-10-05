"""Ablation: rrr-first without BPE sub-word features (so the hook needs no BPE tokenizer)."""
import json

import common as C
import data
import env_split as ES
import eval_gold as E
import feats as FE
import methods as M


class NoBPE(M.OutcomeRRRFirst):
    name = "rrr-first-nobpe"

    def fit(self, calls):
        import numpy as np
        from scipy.sparse.linalg import svds
        cmds = [c[0].cmd for c in calls]
        self.fe = FE.Featuriser(bpe=False).fit(cmds, [c[3].key for c in calls])
        X = self.fe.transform(cmds)
        _, _, vt = svds(X.astype(np.float64), k=min(256, X.shape[1] - 1))
        self.V = vt.T.astype(np.float32)
        fc = M.first_in_session(calls)
        Z = self.sparse([c[0].cmd for c in fc]) @ self.V
        Y, _ = M.outcome_matrix(fc)
        Y = Y.toarray()
        B = np.linalg.solve(Z.T @ Z + self.lam * np.eye(Z.shape[1], dtype=np.float32), Z.T @ Y)
        _, _, v2 = np.linalg.svd(Z @ B, full_matrices=False)
        self.W = (self.V @ B @ v2[: self.rank].T).astype(np.float32)
        return self


S = C.sessions()
items = E.gold()
cmds = [x["cmd"] for x in items]
dev = [i for i, x in enumerate(items) if x["half"] == "dev"]
test = [i for i, x in enumerate(items) if x["half"] == "test"]
out = {}
for repo in (None,) + data.REPOS:
    m = M.Clustered(NoBPE()).fit(C.fit_calls(S, repo))
    sel_dev = [i for i in dev if items[i]["repo"] != repo]
    tau = E.best_tau(m.labeller, items, sel_dev)
    p = m.labeller(tau)(cmds)
    if repo is None:
        out["in_repo"] = {**E.evaluate(items, p, test), "tau": tau, "vocab": len(m.emb.fe.vocab),
                          "env_test": ES.stats(items, p, "test:py:proj", "test:py:sys"), "env_repro": ES.stats(items, p, "run:py:proj", "run:py:sys")}
        print(out["in_repo"], flush=True)
    else:
        rin = [i for i in test if items[i]["repo"] == repo]
        from itertools import combinations
        out.setdefault("loro", {})[repo] = E.pair_stats(items, p, list(combinations(rin, 2)))["F1"]
        print(repo, out["loro"][repo], flush=True)
json.dump(out, open(data.HERE / "results" / "ablate_bpe.json", "w"), indent=1)
