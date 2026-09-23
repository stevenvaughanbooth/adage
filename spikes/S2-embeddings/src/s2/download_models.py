#!/usr/bin/env python3
"""Download the pinned model files this spike needs into ./models/ (git-
ignored). Budget: <200 MB total, approved for these three official HF repos.

To stay under budget we do NOT download every variant the SPIKE mentions:
  - potion-base-8M only (skip a "larger potion variant" -- would add ~130 MB)
  - bge-small-en-v1.5: fp32 ONNX only; int8 is produced locally by
    quantize_int8.py (no extra download)
  - all-MiniLM-L6-v2: the arm64 pre-quantized int8 ONNX only (skip fp32,
    which would add ~90 MB) -- used as a single-point v1-design baseline

See FINDINGS.md "what we could not do" for the budget trade-off.

Usage:
    python src/s2/download_models.py
"""
from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path

from huggingface_hub import hf_hub_download

ROOT = Path(__file__).resolve().parents[2]
MODELS_DIR = ROOT / "models"

# (repo_id, filename, local_subdir)
FILES = [
    ("minishlab/potion-base-8M", "model.safetensors", "potion-base-8m"),
    ("minishlab/potion-base-8M", "config.json", "potion-base-8m"),
    ("minishlab/potion-base-8M", "tokenizer.json", "potion-base-8m"),
    ("minishlab/potion-base-8M", "tokenizer_config.json", "potion-base-8m"),
    ("minishlab/potion-base-8M", "modules.json", "potion-base-8m"),
    ("BAAI/bge-small-en-v1.5", "onnx/model.onnx", "bge-small-en-v1.5"),
    ("BAAI/bge-small-en-v1.5", "tokenizer.json", "bge-small-en-v1.5"),
    ("BAAI/bge-small-en-v1.5", "tokenizer_config.json", "bge-small-en-v1.5"),
    ("BAAI/bge-small-en-v1.5", "config.json", "bge-small-en-v1.5"),
    ("sentence-transformers/all-MiniLM-L6-v2", "onnx/model_qint8_arm64.onnx", "all-MiniLM-L6-v2"),
    ("sentence-transformers/all-MiniLM-L6-v2", "tokenizer.json", "all-MiniLM-L6-v2"),
    ("sentence-transformers/all-MiniLM-L6-v2", "tokenizer_config.json", "all-MiniLM-L6-v2"),
    ("sentence-transformers/all-MiniLM-L6-v2", "config.json", "all-MiniLM-L6-v2"),
]


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def main() -> None:
    MODELS_DIR.mkdir(exist_ok=True)
    digests = {}
    total_bytes = 0
    for repo_id, filename, subdir in FILES:
        dest_dir = MODELS_DIR / subdir
        dest_dir.mkdir(exist_ok=True)
        local_name = os.path.basename(filename)
        dest_path = dest_dir / local_name
        if not dest_path.exists():
            downloaded = hf_hub_download(repo_id=repo_id, filename=filename)
            data = Path(downloaded).read_bytes()
            dest_path.write_bytes(data)
        digest = sha256_of(dest_path)
        size = dest_path.stat().st_size
        total_bytes += size
        digests.setdefault(subdir, {})[local_name] = {
            "repo_id": repo_id,
            "hf_path": filename,
            "sha256": digest,
            "bytes": size,
        }
        print(f"{subdir}/{local_name}: {size/1e6:.1f} MB  sha256={digest[:16]}...")

    (MODELS_DIR / "MANIFEST.json").write_text(json.dumps(digests, indent=2))
    print(f"\ntotal downloaded: {total_bytes/1e6:.1f} MB (budget: 200 MB)")
    if total_bytes > 200e6:
        raise SystemExit("OVER BUDGET: exceeds 200 MB")


if __name__ == "__main__":
    main()
