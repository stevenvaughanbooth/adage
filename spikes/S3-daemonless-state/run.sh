#!/usr/bin/env bash
# Spike S3 — rerun every number in FINDINGS.md.
#   ./run.sh          full run (~15 min on an M3 Pro; longer under load)
#   ./run.sh quick    smaller matrix, as a smoke check
# Results land in results/. Every bench JSON records the load average before and after,
# because other workloads on the machine inflate tail latency.
#
# Two regimes:
#   paced  — jittered 10–30 ms think time between hook events per session (still ~10–100×
#            denser than a real session, where tools and the model take 100 ms–10 s)
#   stress — back-to-back events, 16 sessions ≈ 15–20k events/s from one driver process
set -euo pipefail
cd "$(dirname "$0")"
source "$HOME/.cargo/env" 2>/dev/null || true
cargo build --release -q
B=target/release/adage-s3
MODE=${1:-full}
if [[ $MODE == quick ]]; then T=8; R=1; SCALE=small; else T=16; R=2; SCALE=full; fi
PACE=20000
export S3_ROOT_BASE=${S3_ROOT_BASE:-target/bench}
rm -f results/bench-*.json

{ echo "date: $(date -u +%FT%TZ)"; echo "host: $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.perflevel0.physicalcpu)P+$(sysctl -n hw.perflevel1.physicalcpu)E cores, $(( $(sysctl -n hw.memsize) / 1073741824 )) GB";
  echo "os: macOS $(sw_vers -productVersion) (Darwin $(uname -r))"; echo "rustc: $(rustc --version)";
  echo "loadavg at start: $(sysctl -n vm.loadavg)"; echo "bench root: $S3_ROOT_BASE";
  echo "note: other spike agents were running concurrently during the recorded run"; } > results/environment.txt

echo "== cargo test (property + correctness tests)"
cargo test --release -q 2>&1 | grep -E 'test result|FAILED|panicked' | tee results/cargo-test.txt

echo "== correctness at scale"
$B correctness --scale $SCALE > /dev/null && echo "  -> results/correctness-$SCALE.json"

for n in 1 4 16; do
  for be in slots rename; do
    $B bench --exec file --backend $be --sessions $n --turns $T --reps $R --pace-us $PACE --label file-$be-$n-paced
    $B bench --exec proc --backend $be --sessions $n --turns $T --reps 1 --pace-us $PACE --label proc-$be-$n-paced
    $B bench --exec file --backend $be --sessions $n --turns 32 --reps $R --label file-$be-$n-stress
  done
  $B bench --exec daemon --sessions $n --turns $T --reps $R --pace-us $PACE --label daemon-$n-paced
  $B bench --exec daemon --sessions $n --turns 32 --reps $R --label daemon-$n-stress
  $B bench --exec proc --backend slots --sessions $n --turns $T --reps 1 --label proc-slots-$n-stress
done

echo "== durability cost (fsync / F_FULLFSYNC on every state write)"
for d in fsync full; do for n in 1 16; do
  $B bench --exec file --backend slots --durability $d --sessions $n --turns $T --reps 1 --pace-us $PACE --label file-slots-$n-paced-$d
done; done

echo "== control: observations joined before the batch decision (no overlap)"
$B bench --exec file --backend slots --sessions 16 --turns $T --reps 1 --pace-us $PACE --overlap 0 --label file-slots-16-paced-sequential

echo "== stall census: appends slower than 10 ms, stress vs paced (in-process driver)"
{ for p in 0 $PACE; do
    c=$(S3_TRACE_SLOW=1 $B bench --exec file --backend slots --sessions 16 --turns $T --reps 1 --pace-us $p --label stall-$p 2>&1 | grep -c slow-append || true)
    echo "pace_us=$p appends>10ms=$c $(grep -o '"events_per_s": [0-9]*' results/bench-stall-$p.json)"
  done; } | tee results/stalls.txt

echo "== disk: 16 sessions x ~1000 events, and zstd-compressed size"
$B bench --exec file --backend slots --sessions 16 --turns 32 --reps 1 --label disk > /dev/null
raw=$(cat "$S3_ROOT_BASE"/disk/r0/spool/*.log | wc -c | tr -d ' ')
zs=$(cat "$S3_ROOT_BASE"/disk/r0/spool/*.log | zstd -19 -q -c | wc -c | tr -d ' ')
ev=$(grep -o '"events":[0-9]*' results/bench-disk.json | head -1 | cut -d: -f2)
echo "{\"events\":$ev,\"spool_bytes\":$raw,\"zstd19_bytes\":$zs,\"zstd_kib_per_1000_events\":$(echo "scale=1; $zs*1000/$ev/1024" | bc)}" | tee results/disk-compression.json

echo "== code size (non-blank, non-comment lines)"
for f in src/store.rs src/spool.rs src/state.rs src/daemon.rs; do
  printf "%-16s %s\n" "$f" "$(grep -vE '^\s*(//|$)' $f | wc -l | tr -d ' ')"
done | tee results/loc.txt

python3 summarize.py > results/summary.md && echo "== summary -> results/summary.md"
