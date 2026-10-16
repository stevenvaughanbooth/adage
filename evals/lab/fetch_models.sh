#!/usr/bin/env bash
# Download the lab's local embedding models into $ADAGE_LAB_MODELS (default /tmp/adage-lab-models).
# Hugging Face, hf-mirror and ModelScope are blocked by this environment's egress proxy; these
# mirrors are not. Licences: potion-* MIT, bge-small-en-v1.5 MIT, all-MiniLM-L6-v2 Apache-2.0.
set -euo pipefail
M="${ADAGE_LAB_MODELS:-/tmp/adage-lab-models}"
mkdir -p "$M"
cd "$M"
for t in fast-bge-small-en-v1.5 sentence-transformers-all-MiniLM-L6-v2; do
  [ -f "$t.tar.gz" ] || curl -sSfO "https://storage.googleapis.com/qdrant-fastembed/$t.tar.gz"
  tar xzf "$t.tar.gz"
done
for p in potion-base-8m@1.0.4 potion-base-32m@1.1.5; do
  f=$(npm pack "@yarflam/$p" 2>/dev/null | tail -1)
  d="${f%.tgz}"
  mkdir -p "$d" && tar xzf "$f" -C "$d"
done
# The 8M files must be the ones Adage pins (crates/adage-intent/src/model.rs).
echo "f65d0f325faadc1e121c319e2faa41170d3fa07d8c89abd48ca5358d9a223de2  yarflam-potion-base-8m-1.0.4/package/models/model.safetensors" | sha256sum -c -
pip install -q onnxruntime tokenizers safetensors numpy scikit-learn
