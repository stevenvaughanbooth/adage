"""Shared helpers: per-repo priors, cached discovery events, $ under the cumulative-context model."""
import glob, os, subprocess
import kd_load, facts, kd_embed

_PRIORS = {}


def prior(S, repo):
    if repo not in _PRIORS:
        _PRIORS[repo] = facts.Prior([s for s in S if s.arm == "train"], exclude_repo=repo)
    return _PRIORS[repo]


STRICT = os.environ.get("KD_STRICT") == "1"
_PATHS = {}


def repo_paths(repo):
    """Every tracked path of the repository, with its directory prefixes and basename (from
    `git ls-files` of a checkout: a fact of the repository, not a hand-written list)."""
    if repo not in _PATHS:
        d = sorted(glob.glob(f"/tmp/adage-repobench/runs/{repo}/runs/eval-*-control/repo"))[0]
        files = subprocess.run(["git", "-C", d, "ls-files"], capture_output=True, text=True).stdout
        _PATHS[repo] = facts.atoms(files)
    return _PATHS[repo]


def events_for(S, tau=0.8, failed=None, strict=None):
    """Discovery events per session. strict: keep only locator events whose needed atoms are all
    repository paths (where things live) and failure chains (what fails and what works); code
    identifiers read out of files are dropped, since a card naming them cannot replace reading."""
    strict = STRICT if strict is None else strict
    V = kd_embed.session_vecs(S)
    out = {}
    for s in S:
        M = V[s.sid]
        ev = facts.extract(s, prior(S, s.repo), lambda i, j, M=M: float(M[i] @ M[j]), tau=tau,
                           failed=None if failed is None else failed[s.sid])
        if strict:
            P = repo_paths(s.repo)
            ev = [e for e in ev if e.kind == "chain" or e.need <= P]
        out[s.sid] = ev
    return out


def call_tokens(s):
    """Tokens each call added to the context: the next request's newly written tokens (its tool
    results + the previous assistant turn), shared among that request's calls by result size."""
    if getattr(s, "_tok", None) is not None:
        return s._tok
    by = {}
    for k, c in enumerate(s.calls):
        by.setdefault(c.req, []).append(k)
    tok = [0.0] * len(s.calls)
    for r, ks in by.items():
        new = s.req_new[r + 1] if r + 1 < len(s.req_new) else sum((s.calls[k].out_chars + len(s.calls[k].inp)) / 4 for k in ks)
        w = [s.calls[k].out_chars + len(s.calls[k].inp) + 1 for k in ks]
        for k, x in zip(ks, w):
            tok[k] = new * x / sum(w)
    s._tok = tok
    return tok


def dollars(s, ks):
    """$ saved by removing calls ks from session s, cumulative-context model (Haiku 4.5 rates):
    a request whose calls are all removed is not made (its whole cost is saved; a partly removed
    request saves its removed calls' share); every kept later request reads fewer cached tokens,
    and the removed tokens are never written to the cache."""
    _, _, cw, cr = kd_load.RATES["haiku"]
    ks = set(ks)
    if not ks:
        return 0.0
    tok = call_tokens(s)
    by = {}
    for k, c in enumerate(s.calls):
        by.setdefault(c.req, []).append(k)
    removed_req = {r for r, v in by.items() if all(k in ks for k in v)}
    tot = sum(s.calls[k].cost for k in ks)
    for k in ks:
        r0 = s.calls[k].req
        later_kept = sum(1 for r in range(r0 + 1, s.n_requests) if r not in removed_req)
        tot += tok[k] * (cw + cr * later_kept) / 1e6
    return min(tot, s.cost)


def carry(s, tokens, from_req=0):
    """$ to carry `tokens` of delivered text from request `from_req` to the end of the session."""
    _, _, cw, cr = kd_load.RATES["haiku"]
    return tokens * (cw + cr * max(0, s.n_requests - from_req)) / 1e6
