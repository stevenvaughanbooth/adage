#!/usr/bin/env bash
# Run the data-expansion runner jobs one root at a time (each root runs at most 2 sessions at once),
# niced, with the hard spend cap and the disk checked before every root. Jobs are lines of
# $DX/queue.txt:
#   <config.json> <phases, comma-separated> <root>
# A job is done once its line is in $DX/queue.done; lines appended while this runs are picked up.
# A runner already running (started by an earlier queue) is waited for first.
# Disk: before each job, free space on / must be >= MIN_FREE_MB (default 1500); otherwise the
# finished sessions are post-processed (trees deleted) and the queue waits. While a job runs, a
# janitor post-processes finished sessions every 3 minutes (deletes their sandbox trees).
# Children never get CLAUDECODE or a session id: they are removed here, and the runner's isolation
# (--host-auth allowlist, confine) passes only its allowlist anyway.
set -u
DX=/tmp/adage-research/data-expansion
HERE=$(cd "$(dirname "$0")" && pwd)
ADAGE=/home/user/adage/target/debug/adage
CAP=${CAP:-45}
MIN_FREE_MB=${MIN_FREE_MB:-1500}
export REPOBENCH_CACHE=$DX/cache OMP_NUM_THREADS=1 GOFLAGS=-mod=mod RUSTUP_HOME=${RUSTUP_HOME:-/root/.rustup}
touch $DX/queue.txt $DX/queue.done
free_mb() { df -Pm / | awk 'NR==2 {print $4}'; }
janitor() { for r in $DX/runs/*/; do [ -f "$r/state.json" ] && python3 $HERE/postprocess.py "$r" >/dev/null 2>&1; done; }

while pgrep -f "evals/live/runner.py $HERE/configs" >/dev/null; do janitor; sleep 60; done
while true; do
  job=$(grep -vxF -f $DX/queue.done $DX/queue.txt | head -1)
  [ -z "$job" ] && break
  read -r cfg phases root <<<"$job"
  janitor
  while [ "$(free_mb)" -lt "$MIN_FREE_MB" ]; do
    echo "$(date -Is) disk: $(free_mb) MB free < $MIN_FREE_MB; waiting"
    sleep 120; janitor
  done
  spent=$(python3 $HERE/spend_total.py --quiet)
  echo "$(date -Is) job: $job (spent so far \$$spent, cap \$$CAP, free $(free_mb) MB)"
  if python3 -c "import sys; sys.exit(0 if float('$spent') < float('$CAP') - 1.0 else 1)"; then
    env -u CLAUDECODE -u CLAUDE_CODE_SESSION_ID -u CLAUDE_CODE_REMOTE_SESSION_ID -u CLAUDE_CODE_ENTRYPOINT \
      nice -n 10 python3 /home/user/adage/evals/live/runner.py "$cfg" ${phases//,/ } --execute --host-auth \
      --root "$root" --adage $ADAGE &
    rp=$!
    while kill -0 $rp 2>/dev/null; do sleep 180; janitor; echo "$(date -Is) free $(free_mb) MB"; done
    wait $rp
    echo "$(date -Is) runner exit $?"
  else
    echo "$(date -Is) SKIPPED: spend cap"
  fi
  python3 $HERE/postprocess.py "$root"
  python3 $HERE/manifest.py >/dev/null 2>&1
  echo "$job" >> $DX/queue.done
done
echo "$(date -Is) queue empty; total \$$(python3 $HERE/spend_total.py --quiet)"
