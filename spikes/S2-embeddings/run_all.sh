#!/usr/bin/env bash
# One-line rerun for every number in FINDINGS.md.
#
# Usage:
#   cd spikes/S2-embeddings && ./run_all.sh
#
# Notes:
#  - Needs network access once, to download the three pinned HF model
#    files into ./models/ (git-ignored, <200 MB total, verified by SHA-256
#    in models/MANIFEST.json). Re-runs are offline (skips existing files).
#  - Needs `source "$HOME/.cargo/env"` for the Rust step (done below).
#  - Other spikes may be running concurrently on this machine; cost
#    numbers are indicative, not clean-room. Each cost script prints
#    `uptime` so you can see the load at measurement time.
set -euo pipefail
cd "$(dirname "$0")"

echo "== venv sync =="
uv pip sync requirements.txt --python .venv/bin/python

echo "== download models (skips existing files) =="
.venv/bin/python src/s2/download_models.py

echo "== quantize bge-small to int8 (local, deterministic) =="
if [ ! -f models/bge-small-en-v1.5/model_int8_dynamic.onnx ]; then
  .venv/bin/python src/s2/quantize_int8.py
else
  echo "  (already exists, skipping)"
fi

echo "== generate labelled benchmark set =="
.venv/bin/python data/gen_data.py --out data/prompts.jsonl

echo "== export normalized sample for the Rust check =="
PYTHONPATH=src .venv/bin/python data/export_rust_sample.py

echo "== quality benchmark (family top-1, verb-contrast, open-set, clustering) =="
PYTHONPATH=src .venv/bin/python src/s2/bench_quality.py

echo "== cost benchmark, Python (cold load, latency, memory, determinism) =="
.venv/bin/python src/s2/run_cost.py

echo "== cost benchmark, Rust (model2vec-rs: confirms the in-process hook-client path) =="
source "$HOME/.cargo/env"
(cd rust && ./run_bench.sh 8)

echo
echo "Done. See results/quality.md, results/cost.md, results/rust_cost.jsonl, and FINDINGS.md."
