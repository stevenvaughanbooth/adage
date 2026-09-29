"""Generic command featurisation: shell *syntax* only, no tool names, no error strings.

What is fixed by hand (and is shell grammar, not a taxonomy): heredoc bodies and long quoted
strings are data (`<STR>`); `&& || ; | &` separate simple commands; a segment after `|` consumes
the previous one's output; the first word of a simple command is its head; a word starting with
`-` is a flag. Everything else is learned from the corpus: the vocabulary (a token is kept only if
it recurs across sessions), BPE sub-word pieces (HuggingFace `tokenizers`, trained on the fit
corpus), and every weight.
"""
from __future__ import annotations

import re
import shlex
import sys
from collections import Counter, defaultdict
from pathlib import Path

import numpy as np
import scipy.sparse as sp

sys.path.insert(0, str(Path(__file__).resolve().parents[2] / "abstraction"))
from shell import strip_heredocs  # noqa: E402  (generic: drops heredoc bodies)

NUM = re.compile(r"^\d+$")


def lex(cmd: str) -> list[tuple[list[str], bool]]:
    """[(argv, after_pipe)] for each simple command."""
    cmd, _ = strip_heredocs(cmd)
    # redirections that duplicate descriptors are not separators (`2>&1`, `&>`, `|&`)
    cmd = re.sub(r"\d*>&\d*", " ", cmd).replace("&>", ">").replace("|&", "|")
    try:
        lx = shlex.shlex(cmd.replace("\n", " ; "), posix=True, punctuation_chars=";&|")
        lx.whitespace_split = True
        lx.commenters = ""
        toks = list(lx)
    except ValueError:
        toks = cmd.replace("\n", " ; ").split()
    segs, cur, pipe, nxt_pipe = [], [], False, False
    for t in toks + [";"]:
        if t and set(t) <= set(";&|"):
            if cur:
                segs.append((cur, pipe))
            cur, pipe = [], t == "|"
            continue
        if "\n" in t or (len(t) > 40 and " " in t):
            t = "<STR>"
        elif NUM.match(t):
            t = "<N>"
        elif re.match(r"^\d?>", t) or t in ("<", ">", ">>"):
            continue
        cur.append(t)
    return segs


def text(cmd: str) -> str:
    """The normalised command line (for text models)."""
    return " && ".join(("| " if p else "") + " ".join(a) for a, p in lex(cmd))


class BPE:
    def __init__(self, vocab_size: int = 2000):
        self.vocab_size = vocab_size
        self.tok = None

    def fit(self, cmds: list[str]):
        from tokenizers import Tokenizer, models, pre_tokenizers, trainers
        tk = Tokenizer(models.BPE(unk_token="[UNK]"))
        tk.pre_tokenizer = pre_tokenizers.Sequence([pre_tokenizers.WhitespaceSplit(), pre_tokenizers.Split(re.escape("/"), "isolated")])
        tr = trainers.BpeTrainer(vocab_size=self.vocab_size, min_frequency=3, special_tokens=["[UNK]"], show_progress=False)
        tk.train_from_iterator([" ".join(a) for c in cmds for a, _ in lex(c)], tr)
        self.tok = tk
        return self

    def pieces(self, tok: str) -> list[str]:
        return [p for p in self.tok.encode(tok).tokens if p not in ("/", "[UNK]")]


def raw_features(cmd: str, bpe: BPE | None) -> list[str]:
    """Positional token features of one command (before vocabulary pruning)."""
    out = []
    for argv, pipe in lex(cmd):
        pre = "p" if pipe else ""
        head = argv[0]
        hb = head.rsplit("/", 1)[-1]
        out += [f"{pre}h:{head}", f"{pre}hb:{hb}"]
        if len(argv) > 1:
            out.append(f"{pre}hs:{hb} {argv[1]}")
        for t in argv[1:]:
            if t.startswith("-"):
                out.append(f"{pre}f:{t.split('=')[0]}")
            else:
                out.append(f"{pre}t:{t}")
                if "/" in t:
                    out.append(f"{pre}tb:{t.rsplit('/', 1)[-1]}")
        if bpe is not None and not pipe:
            for t in argv:
                out += [f"b:{p}" for p in bpe.pieces(t)]
    return out


class Featuriser:
    """TF-IDF over positional tokens and BPE pieces; vocabulary = features seen in >= min_sess sessions."""

    def __init__(self, min_sess: int = 2, bpe: bool = True):
        self.min_sess, self.use_bpe = min_sess, bpe

    def fit(self, cmds: list[str], sess: list[str]):
        self.bpe = BPE().fit(cmds) if self.use_bpe else None
        df = defaultdict(set)
        for c, s in zip(cmds, sess):
            for f in set(raw_features(c, self.bpe)):
                df[f].add(s)
        keep = sorted(f for f, ss in df.items() if len(ss) >= self.min_sess)
        self.vocab = {f: i for i, f in enumerate(keep)}
        n = len(set(sess))
        self.idf = np.array([np.log((1 + n) / (1 + len(df[f]))) + 1 for f in keep], np.float32)
        return self

    def transform(self, cmds: list[str]) -> sp.csr_matrix:
        rows, cols, vals = [], [], []
        for i, c in enumerate(cmds):
            cnt = Counter(self.vocab[f] for f in raw_features(c, self.bpe) if f in self.vocab)
            for j, v in cnt.items():
                rows.append(i)
                cols.append(j)
                vals.append((1 + np.log(v)) * self.idf[j])
        X = sp.csr_matrix((vals, (rows, cols)), shape=(len(cmds), len(self.vocab)), dtype=np.float32)
        nrm = np.sqrt(np.asarray(X.multiply(X).sum(1)).ravel())
        return sp.diags(1 / np.maximum(nrm, 1e-9)) @ X


OUT_TOK = re.compile(r"[A-Za-z_][A-Za-z_.]{2,}")


def out_tokens(out: str) -> list[str]:
    """Words of the first 400 characters of a call's output (digits and paths dropped)."""
    return OUT_TOK.findall(out[:400])


def out_template(out: str) -> str:
    """Drain-style template of the first non-empty output line: variable-looking words masked."""
    for ln in out.splitlines():
        ln = ln.strip()
        if ln:
            ws = ln.split()[:8]
            return " ".join("<*>" if re.search(r"[\d/\\]|^['\"]", w) or len(w) > 25 else w for w in ws)
    return "<empty>"
