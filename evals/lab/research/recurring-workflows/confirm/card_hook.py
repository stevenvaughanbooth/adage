#!/usr/bin/env python3
"""Claude Code hook: deliver a learned procedure card when a prompt belongs to a known recurring family.

  settings.json:  UserPromptSubmit / PostToolUse -> python3 card_hook.py <index.json> <deliver|log>

UserPromptSubmit: embed the prompt (potion-base-8M, model2vec static: mean of token vectors,
L2-normalised), score it against each family of THIS repository (repo key from `git remote get-url
origin`, normalised as Adage does): the max cosine to the family's prior-session prompts. If the best
score >= tau, the card goes out as `additionalContext`. If it lands in the grey zone [tau_low, tau),
the family is remembered for the session, and PostToolUse delivers the card the first time the
agent edits (Edit/Write/MultiEdit) one of the family's card files. `log` mode makes every decision
and records it but never injects (the control arm). Decisions go to $HOME/.rw-card/log.jsonl.
A hook failure never blocks the agent: every error exits 0 with no output.
"""

from __future__ import annotations

import json
import os
import subprocess
import sys
import time
from pathlib import Path

import numpy as np


class Static:
    """model2vec: mean of the token vectors (tokenizer.json + model.safetensors), L2-normalised."""

    def __init__(self, d: Path):
        from safetensors.numpy import load_file
        from tokenizers import Tokenizer
        d = Path(d)
        t = load_file(str(d / "model.safetensors"))
        self.E = t.get("embeddings", next(iter(t.values()))).astype(np.float32)
        tok = d / "tokenizer.json" if (d / "tokenizer.json").exists() else d / "tokenizer" / "tokenizer.json"
        self.tok = Tokenizer.from_file(str(tok))

    def encode(self, texts):
        out = np.zeros((len(texts), self.E.shape[1]), np.float32)
        for i, enc in enumerate(self.tok.encode_batch(list(texts), add_special_tokens=False)):
            ids = [j for j in enc.ids if j < len(self.E)][:512]
            if ids:
                out[i] = self.E[ids].mean(0)
        n = np.linalg.norm(out, axis=1, keepdims=True)
        return out / np.maximum(n, 1e-9)


def normalise_origin(url: str) -> str:
    u = url.strip()
    for suf in (".git", "/"):
        while u.endswith(suf):
            u = u[: -len(suf)]
    if u.startswith("git@"):
        u = u[4:].replace(":", "/", 1)
    return u.split("://")[-1].lower()


def repo_key(cwd: str) -> str:
    try:
        out = subprocess.run(["git", "-C", cwd, "remote", "get-url", "origin"], capture_output=True, text=True, timeout=5)
        return normalise_origin(out.stdout) if out.returncode == 0 else ""
    except Exception:  # noqa: BLE001
        return ""


def rel(path: str, cwd: str) -> str:
    try:
        return os.path.relpath(path, cwd) if os.path.isabs(path) else path
    except ValueError:
        return path


def file_matches(path: str, files, patterns) -> bool:
    """An exact card file, or a card pattern (`flag_*.go`: a name that varied by session) in the same directory."""
    import fnmatch
    return path in files or any(os.path.dirname(path) == os.path.dirname(g) and fnmatch.fnmatch(path, g)
                                for g in patterns)


def score(index: dict, repo: str, prompt: str):
    fams = (index["repos"].get(repo) or {}).get("families") or []
    if not fams:
        return None, 0.0
    emb = Static(Path(index["model_dir"]))
    x = emb.encode([prompt])[0]
    best, best_s = None, -1.0
    for f in fams:
        s = float((np.asarray(f["emb"], np.float32) @ x).max())
        if s > best_s:
            best, best_s = f, s
    return best, best_s


def state_path() -> Path:
    p = Path(os.environ.get("HOME", "/tmp")) / ".rw-card"
    p.mkdir(parents=True, exist_ok=True)
    return p


def log(rec: dict) -> None:
    with open(state_path() / "log.jsonl", "a") as fh:
        fh.write(json.dumps({"ts": time.time(), **rec}) + "\n")


def emit(event: str, text: str) -> None:
    print(json.dumps({"hookSpecificOutput": {"hookEventName": event, "additionalContext": text}}))


def main(argv) -> int:
    index_path, mode = argv[1], argv[2]
    ev = json.loads(sys.stdin.read() or "{}")
    event = ev.get("hook_event_name", "")
    sid = ev.get("session_id", "")
    cwd = ev.get("cwd") or os.getcwd()
    index = json.loads(Path(index_path).read_text())
    st_file = state_path() / f"{sid}.json"
    st = json.loads(st_file.read_text()) if st_file.exists() else {}
    if event == "UserPromptSubmit":
        repo = repo_key(cwd)
        t0 = time.time()
        fam, s = score(index, repo, ev.get("prompt", ""))
        decision = "none"
        if fam is not None and s >= index["tau"]:
            decision = "fire"
        elif fam is not None and s >= index["tau_low"]:
            decision = "grey"
        st = {"family": fam["name"] if fam else None, "score": s, "decision": decision, "delivered": False}
        log({"event": event, "sid": sid, "repo": repo, "family": st["family"], "score": round(s, 4),
             "decision": decision, "mode": mode, "ms": round((time.time() - t0) * 1000)})
        if decision == "fire":
            st["delivered"] = True
            if mode == "deliver":
                emit(event, fam["card"])
        st_file.write_text(json.dumps(st))
        return 0
    if event == "PostToolUse" and st.get("decision") == "grey" and not st.get("delivered"):
        if ev.get("tool_name") not in ("Edit", "Write", "MultiEdit"):
            return 0
        p = rel((ev.get("tool_input") or {}).get("file_path", ""), cwd)
        fam = next((f for r in index["repos"].values() for f in r["families"] if f["name"] == st["family"]), None)
        if fam and file_matches(p, fam["files"], fam["patterns"]):
            st["delivered"] = True
            st_file.write_text(json.dumps(st))
            log({"event": event, "sid": sid, "family": fam["name"], "file": p, "decision": "fire-file", "mode": mode})
            if mode == "deliver":
                emit(event, fam["card"])
    return 0


if __name__ == "__main__":
    try:
        sys.exit(main(sys.argv))
    except Exception as e:  # noqa: BLE001 - a hook must never break the session
        try:
            log({"event": "error", "error": str(e)[:300]})
        except Exception:  # noqa: BLE001
            pass
        sys.exit(0)
