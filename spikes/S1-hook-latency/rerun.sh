#!/usr/bin/env bash
# S1 hook-latency spike: reproduce every measurement in FINDINGS.md.
#
# Usage: ./rerun.sh [scenario-id ...]
#   With no arguments, builds everything and runs all scenarios (A-G, plus
#   the Node-client comparisons). Pass specific scenario ids (as printed by
#   bench/run.js) to re-run only those.
#
# Writes results/<scenario>.json, results/summary.json, results/summary.csv,
# and appends an uptime/load snapshot to results/uptime.log before and after
# the run, per the spike-board rule to record machine load with every run.

set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"

# Rust is installed via rustup; make sure this shell can see it.
if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  source "$HOME/.cargo/env"
fi

echo "=== S1 rerun: $(date -u +%FT%TZ) ===" | tee -a results/uptime.log
uptime | tee -a results/uptime.log

echo "--- building client + server (release, LTO, stripped) ---"
cargo build --release

SOCK=/tmp/adage-s1-bench.sock
rm -f "$SOCK"

echo "--- starting echo server (warm, for scenarios D/F) ---"
./target/release/echoserver "$SOCK" >results/echoserver.log 2>&1 &
SERVER_PID=$!
trap 'kill "$SERVER_PID" 2>/dev/null || true; rm -f "$SOCK"' EXIT

# Give the server a moment to bind the socket.
for _ in $(seq 1 20); do
  [ -S "$SOCK" ] && break
  sleep 0.05
done
if [ ! -S "$SOCK" ]; then
  echo "echoserver did not bind $SOCK in time" >&2
  cat results/echoserver.log >&2 || true
  exit 1
fi

echo "--- running bench (node bench/run.js) ---"
ADAGE_S1_SOCK="$SOCK" node bench/run.js "$@"

kill "$SERVER_PID" 2>/dev/null || true
trap - EXIT
rm -f "$SOCK"

echo "=== S1 rerun done: $(date -u +%FT%TZ) ===" | tee -a results/uptime.log
uptime | tee -a results/uptime.log

echo
echo "Results: results/summary.csv, results/summary.json, results/<scenario>.json"
column -s, -t results/summary.csv 2>/dev/null || cat results/summary.csv
