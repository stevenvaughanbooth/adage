#!/usr/bin/env bash
# One-line rerun for every number in FINDINGS.md.
#
#   cd spikes/S7-clustering && ./run_all.sh
#
# Needs network once (potion-base-8M, 31 MB, pinned revision, SHA-256 verified into ./models/,
# git-ignored). Everything else is offline and seeded. Expect ~1.5 h on an M3 Pro: the 300k global
# kNN alone is ~8 min, and the Python HDBSCAN baselines at 100k run to their timeouts.
set -euo pipefail
cd "$(dirname "$0")"
export PYTHONPATH=src
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"

echo "== venv =="
[ -d .venv ] || uv venv --python 3.11 .venv
uv pip sync requirements.txt --python .venv/bin/python

echo "== model (pinned, verified) =="
.venv/bin/python src/s7/fetch_model.py

echo "== Rust clusterer =="
(cd rust && cargo build --release)

mkdir -p .cache results
echo "== ceilings and normalisation variants (supervised references) =="
.venv/bin/python -W ignore -m s7.ceiling
.venv/bin/python -W ignore -m s7.normvariants

echo "== parameter sweeps on the development corpus (seed 1) =="
.venv/bin/python -W ignore -m s7.sweep > .cache/sweep.log
.venv/bin/python -W ignore -m s7.sweep2 > .cache/sweep2.log
.venv/bin/python -W ignore -m s7.louvain_py > .cache/sweep_louvain.log

echo "== the Rust Louvain reproduces the Python reference exactly =="
.venv/bin/python -W ignore -m s7.check_rust

echo "== benchmark: quality + determinism, scale, purity (test corpora, seed 20260922) =="
.venv/bin/python -W ignore -m s7.bench quality scale perrepo purity
