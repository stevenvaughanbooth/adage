"""potion-base-8M embeddings of call inputs (the pinned model Adage's runtime ships), cached."""
import os, sys
from pathlib import Path
import numpy as np
LAB = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(LAB))
os.environ.setdefault("ADAGE_LAB_CACHE", "/tmp/adage-research/knowledge-distillation/emb")
import embed  # noqa: E402

_M = None


def model():
    global _M
    if _M is None:
        _M = embed.Cached("potion-8M", embed.load("potion-8M"))
    return _M


def enc(texts):
    return model().encode([t[:600] for t in texts])


def save():
    model().save()


def session_vecs(sessions):
    """{sid: (n_calls x d) matrix of call-input embeddings}"""
    allt = [c.inp for s in sessions for c in s.calls]
    V = enc(allt)
    save()
    out, i = {}, 0
    for s in sessions:
        out[s.sid] = V[i:i + len(s.calls)]
        i += len(s.calls)
    return out
