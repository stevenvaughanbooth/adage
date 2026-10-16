#!/usr/bin/env bash
# Runs the model2vec-rs cold-load/latency/determinism check N times and
# writes the raw JSON lines to ../results/rust_cost.jsonl.
#
# Usage: source "$HOME/.cargo/env" && ./run_bench.sh [n_trials]
set -euo pipefail
cd "$(dirname "$0")"

N="${1:-5}"
OUT="../results/rust_cost.jsonl"

cargo build --release
: > "$OUT"
echo "[system load] $(uptime)" | tee -a "$OUT.note"
for i in $(seq 1 "$N"); do
  ./target/release/s2_rust_bench ../models/potion-base-8m ../data/normalized_sample.txt | tee -a "$OUT"
done
echo "wrote $N trials to $OUT"
