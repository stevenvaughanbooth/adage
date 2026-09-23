#!/usr/bin/env python3
"""Download potion-base-8M (the static model the implementation plan settled on, S2) at a pinned
revision into ./models/ (git-ignored) and verify every file against S2's SHA-256 manifest.

Usage:
    .venv/bin/python src/s7/fetch_model.py
"""
from __future__ import annotations

import hashlib
import shutil
from pathlib import Path

from huggingface_hub import hf_hub_download

ROOT = Path(__file__).resolve().parents[2]
DEST = ROOT / "models" / "potion-base-8m"
REPO = "minishlab/potion-base-8M"
REVISION = "bf8b056651a2c21b8d2565580b8569da283cab23"
# Digests from spikes/S2-embeddings/models/MANIFEST.json.
FILES = {
    "model.safetensors": "f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2",
    "config.json": "2a6ac0e9aaa356a68a5688070db78fc3a464fefe85d2f06a1905ce3718687553",
    "tokenizer.json": "e67e803f624fb4d67dea1c730d06e1067e1b14d830e2c2202569e3ef0f70bb50",
    "tokenizer_config.json": "6725995e3ab3039857ff5bd99178a7cdf42863abb04449e7bb31feb1f55fe567",
    "modules.json": "a68dcbed0429dcdd5bfdca92b0b03cc30d09122c0a3fcf4758787d4b244e45b2",
}


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with open(p, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    DEST.mkdir(parents=True, exist_ok=True)
    for name, digest in FILES.items():
        out = DEST / name
        if not out.exists() or sha256(out) != digest:
            src = hf_hub_download(REPO, name, revision=REVISION)
            shutil.copyfile(src, out)
        got = sha256(out)
        if got != digest:
            raise SystemExit(f"{name}: sha256 {got} != pinned {digest}")
        print(f"ok  {name}  {digest[:16]}…")


if __name__ == "__main__":
    main()
