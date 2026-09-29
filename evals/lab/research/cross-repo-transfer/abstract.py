"""Repository-agnostic abstraction of shell actions, learned from the corpus and the file trees.

Nothing here lists tool names, programs or error strings. What is used:

- **Shell syntax** (a parser, not domain knowledge): `shlex` with punctuation splits a command
  into simple commands on the shell's own operators; heredoc bodies and quoted multi-word
  arguments are one `{code}` argument; redirections are dropped.
- **Main segment (learned).** Of the simple commands in a compound command, the "main" one is the
  one whose program most often appears *alone* in the corpus (`cd … &&`, `… | head`, `source …
  &&` are wrappers because agents almost never run them alone).
- **Tree-grounded slots.** A token whose first path component exists in the repository's own
  file tree is a path; each component is kept literally only when it is *shared vocabulary*
  (present in the trees of >= 2 repositories: `.venv`, `bin`, `python`, `tests`, `src`,
  `node_modules`), otherwise it becomes `*` (keeping the extension). `src/click/core.py` ->
  `src/*/*.py`; `.venv/bin/python` stays. A non-path token is kept when it occurs in the
  commands of >= 2 repositories, a repository-unique token that names a tree entry becomes
  `{name}`, anything else `{arg}`; numbers `{n}`.
- **Head (learned).** The command part of an abstracted argv is the longest prefix whose every
  next token is predictable from the prefix across >= 2 repositories (a variable-order prefix
  tree, like the Drain template's constant positions): `python -m pytest`, `.venv/bin/python
  -m pytest`, `npm run {name}`, `go test`. What follows is the agent's argument.
"""

from __future__ import annotations

import collections
import re
import shlex

OPS = {"&&", "||", ";", "|", "&", ";;", "(", ")", "|&"}
REDIR = {">", ">>", ">&", "<", "<<", "<<<", "&>", ">|", "<&", "&>>"}


def segments(cmd: str) -> list[list[str]]:
    first, _, rest = cmd.partition("\n")
    if "<<" in first:  # heredoc: its body is the argument
        cmd = re.sub(r"<<-?\s*['\"]?(\w+)['\"]?", " {code} ", first)
    try:
        lx = shlex.shlex(cmd, posix=True, punctuation_chars=True)
        lx.whitespace_split = True
        toks = list(lx)
    except ValueError:
        toks = cmd.split()
    segs, cur, skip, op, piped = [], [], False, None, set()
    for t in toks:
        if skip:
            skip = False
            continue
        if t in OPS or t == "\n":
            if cur:
                segs.append(cur)
            cur = []
            if t in ("|", "|&"):
                piped.add(len(segs))
            continue
        if t in REDIR or re.fullmatch(r"[<>&|]+", t):
            if cur and re.fullmatch(r"\d", cur[-1]):
                cur.pop()
            skip = t not in ("<<",) and True
            continue
        cur.append(t)
    if cur:
        segs.append(cur)
    out = []
    for k, s in enumerate(segs):
        while s and re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", s[0]):
            s = s[1:]
        if s:
            out.append(Seg(s, k in piped))
    return out


class Seg(list):
    """A simple command's argv; `.piped` is true when it reads another command's output."""

    def __init__(self, argv, piped=False):
        super().__init__(argv)
        self.piped = piped


class Vocab:
    """Learned from a set of repositories' sessions and trees (the pool at learning time)."""

    def __init__(self, sessions, trees: dict[str, set[str]]):
        self.trees = trees
        comp_repos = collections.defaultdict(set)
        for repo, t in trees.items():
            for p in t:
                for c in p.strip("/").split("/"):
                    comp_repos[c].add(repo)
        repos_in_pool = {s.repo for s in sessions}
        # "Shared" means seen in >= 2 repositories; a single-repository pool (a repo-local
        # learner) falls back to >= 2 tasks of that repository.
        self.unit = (lambda s: s.repo) if len(repos_in_pool) >= 2 else (lambda s: s.task)
        if len(repos_in_pool) >= 2:
            self.shared_comp = {c for c, rs in comp_repos.items() if len(rs) >= 2}
        else:
            self.shared_comp = {c for p in trees.get(next(iter(repos_in_pool), ""), ()) for c in p.strip("/").split("/")}
        alone, total = collections.Counter(), collections.Counter()
        tok_repos = collections.defaultdict(set)
        cmds = []
        for s in sessions:
            for c in s.calls:
                if c.tool != "Bash":
                    continue
                segs = segments(c.inp.get("command", ""))
                for sg in segs:
                    h = sg[0].rsplit("/", 1)[-1]
                    total[h] += 1
                    if len(segs) == 1:
                        alone[h] += 1
                    for t in sg:
                        if len(t) < 40 and "\n" not in t:
                            tok_repos[t].add(self.unit(s))
                cmds.append((s.repo, segs, self.unit(s)))
        self.alone = {h: (alone[h] + 0.5) / (total[h] + 1) for h in total}
        self.shared_tok = {t for t, rs in tok_repos.items() if len(rs) >= 2}
        # prefix tree over abstracted argvs of main segments, with the repos each edge was seen in
        self.edge = collections.defaultdict(lambda: collections.defaultdict(set))
        self.node_n = collections.Counter()
        for repo, segs, u in cmds:
            if not segs:
                continue
            argv = self.abstract_argv(self.main(segs), repo)
            for k in range(min(len(argv), 5)):
                pre = tuple(argv[:k])
                self.node_n[pre] += 1
                self.edge[pre][argv[k]].add(u)
        self._edge_n = collections.Counter()
        for repo, segs, _ in cmds:
            if segs:
                argv = self.abstract_argv(self.main(segs), repo)
                for k in range(min(len(argv), 5)):
                    self._edge_n[(tuple(argv[:k]), argv[k])] += 1

    def main(self, segs):
        cand = [s for s in segs if not getattr(s, "piped", False)] or segs
        return max(cand, key=lambda s: self.alone.get(s[0].rsplit("/", 1)[-1], 0.5))

    def is_path(self, tok: str, repo: str) -> bool:
        t = tok.split("::")[0].removeprefix("./").strip("/")
        if not t or " " in t:
            return False
        first = t.split("/")[0]
        tree = self.trees.get(repo, set())
        return (first in {p.strip("/").split("/")[0] for p in tree} and ("/" in t or "." in t or first + "/" in tree)) \
            or (t + "/") in tree or t in tree

    def abstract_tok(self, tok: str, repo: str) -> str:
        if "\n" in tok or " " in tok or len(tok) > 60 or tok == "{code}":
            return "{code}"
        if re.fullmatch(r"-?\d+(\.\d+)?", tok):
            return "{n}"
        if self.is_path(tok, repo):
            base, sep, tid = tok.partition("::")
            comps = base.removeprefix("./").strip("/").split("/")
            out = []
            for c in comps:
                if c in self.shared_comp:
                    out.append(c)
                else:
                    ext = re.search(r"(\.[A-Za-z]{1,5})$", c)
                    out.append("*" + (ext.group(1) if ext else ""))
            return "/".join(out) + ("::{arg}" if sep else "")
        if tok in self.shared_tok:
            return tok
        if re.search(r"\.[A-Za-z]{1,5}$", tok) and "/" not in tok:
            return "*" + re.search(r"(\.[A-Za-z]{1,5})$", tok).group(1)
        if tok.startswith("-"):
            return "{flag}"
        comps = {c for p in self.trees.get(repo, ()) for c in p.strip("/").split("/")}
        return "{name}" if tok in comps else "{arg}"

    def abstract_argv(self, argv, repo):
        out = [self.abstract_tok(t, repo) for t in argv]
        if out and not self.is_path(argv[0], repo) and out[0] != "{code}":
            # the program: keep its name (an absolute path keeps only its shared components)
            comps = argv[0].strip("/").split("/")
            out[0] = "/".join(c for c in comps if c in self.shared_tok or c in self.shared_comp or c == comps[-1])
        return out

    def head(self, argv: list[str]) -> tuple[str, ...]:
        """Longest prefix whose next token is a constant: seen after this prefix in >= 2
        repositories and carrying >= 20% of the prefix's continuations."""
        h = []
        for k, t in enumerate(argv[:4]):
            pre = tuple(argv[:k])
            if k == 0:
                h.append(t)
                continue
            if t.startswith("{") or "*" in t:
                break
            rs = self.edge.get(pre, {}).get(t, set())
            share = self._edge_n[(pre, t)] / max(1, self.node_n[pre])
            if (len(rs) >= 2 and share >= 0.2) or (share >= 0.3 and self._edge_n[(pre, t)] >= 5):
                h.append(t)
            else:
                break
        return tuple(h)

    def action(self, cmd: str, repo: str):
        """(abstracted argv of the main segment, head). The head keeps the segments chained
        before the main one (`source .venv/bin/activate && python -m pytest`): they set up
        the environment the main command runs in. Segments after it (filters) are dropped."""
        segs = segments(cmd)
        if not segs:
            return (), ""
        m = self.main(segs)
        argv = self.abstract_argv(m, repo)
        pre = []
        for sg in segs[:next(i for i, x in enumerate(segs) if x is m)]:
            a = self.abstract_argv(sg, repo)
            pre.append(" ".join(a[:3]))
        return tuple(argv), " && ".join(pre + [" ".join(self.head(argv))])
