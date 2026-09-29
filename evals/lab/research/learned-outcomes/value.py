"""TD(λ) and Monte-Carlo value estimation over trace states, with session reward from state.json.

State after call i: (k-means cluster of the call's action embedding, k-means cluster of its output
embedding), 16 × 32 = 512 tabular states, plus a start state. Episode = one session.
Reward: −c per tool call (c = 1/30, about one session's worth of calls = one pass), and +1 at the
end if the hidden tests passed. γ = 1. The advantage of call i is

    A_i = −c + V(s_i) − V(s_{i−1})

(temporal credit assignment: how much the call moved the expected outcome, net of its cost).
Failure / waste score = −A_i. Fitted only on the training pool's sessions.
"""

from __future__ import annotations

import numpy as np
from sklearn.cluster import MiniBatchKMeans

C_STEP = 1 / 30
KA, KO = 16, 32


def states(a, fit, seed=0):
    EO, EA = a["EO"].astype(np.float32), a["EA"].astype(np.float32)
    ka = MiniBatchKMeans(KA, random_state=seed, n_init=3, batch_size=2048).fit(EA[fit]).predict(EA)
    ko = MiniBatchKMeans(KO, random_state=seed, n_init=3, batch_size=2048).fit(EO[fit]).predict(EO)
    return ka * KO + ko + 1  # 0 is the start state


def episodes(a, mask):
    sid = a["sid"]
    starts = np.r_[0, np.nonzero(sid[1:] != sid[:-1])[0] + 1, len(sid)]
    for s, e in zip(starts[:-1], starts[1:]):
        if mask[s] and a["passed"][s] >= 0:
            yield np.arange(s, e), float(a["passed"][s])


def td_lambda(a, st, fit, lam=0.8, alpha=0.05, epochs=30, seed=0):
    n = KA * KO + 1
    V = np.zeros(n)
    eps = list(episodes(a, fit))
    rng = np.random.default_rng(seed)
    V[:] = np.mean([r for _, r in eps])
    for ep in range(epochs):
        for k in rng.permutation(len(eps)):
            idx, R = eps[k]
            seq = np.r_[0, st[idx]]
            z = np.zeros(n)
            for t in range(len(seq)):
                s = seq[t]
                last = t == len(seq) - 1
                r = (R if last else -C_STEP) if t > 0 else 0.0
                # transition s_{t} -> s_{t+1}; terminal after the last call
                if last:
                    target_next, rew = 0.0, R
                else:
                    target_next, rew = V[seq[t + 1]], -C_STEP
                delta = rew + target_next - V[s]
                z *= lam
                z[s] += 1
                V += alpha * delta * z
    return V


def monte_carlo(a, st, fit):
    n = KA * KO + 1
    tot, cnt = np.zeros(n), np.zeros(n)
    for idx, R in episodes(a, fit):
        seq = np.r_[0, st[idx]]
        L = len(seq)
        for t, s in enumerate(seq):
            G = R - C_STEP * (L - 1 - t)
            tot[s] += G
            cnt[s] += 1
    prior = tot.sum() / cnt.sum()
    return (tot + 5 * prior) / (cnt + 5)  # shrink rare states to the mean


def advantage(a, st, V):
    adv = np.zeros(len(st))
    sid = a["sid"]
    for i in range(len(st)):
        prev = 0 if (i == 0 or sid[i] != sid[i - 1]) else st[i - 1]
        adv[i] = -C_STEP + V[st[i]] - V[prev]
    return adv
