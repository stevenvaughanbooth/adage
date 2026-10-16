#!/usr/bin/env python3
"""Export a normalized-text sample for the Rust cold-load/latency check
(S2's Rust confirmation step), one prompt per line, so main.rs doesn't need
to reimplement the normaliser just to get realistic-length input strings.

Usage:
    PYTHONPATH=src .venv/bin/python data/export_rust_sample.py
"""
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "src"))
from s2.normalize import normalize  # noqa: E402

rows = [json.loads(l) for l in open(ROOT / "data" / "prompts.jsonl")]
texts = [normalize(r["text"]).text for r in rows if not r["is_noise"]]
out = ROOT / "data" / "normalized_sample.txt"
out.write_text("\n".join(texts) + "\n")
print(f"wrote {len(texts)} lines -> {out}")
