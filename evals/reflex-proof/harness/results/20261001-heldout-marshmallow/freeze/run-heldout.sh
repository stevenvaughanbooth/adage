#!/bin/bash
# Held-out study, launch-gate-1: run by the person, in their own terminal (the grants are theirs).
# Paid: the native session (<= 0.50 USD) and live (65 sessions at a 0.40 USD cap, 26 USD ceiling).
set -euo pipefail
A=/private/tmp/adage-real-repo-gate-v1
C=$A/cohorts/launch-gate-v8
H=$C/heldout-1
APPROVE_SHA=3213ff49f25564fd9df095dec57b90949333b2c6f91b05e8d2537dfd37765a03
LOG=$H/run-heldout-$(date -u +%Y%m%dT%H%M%SZ).log
exec > >(tee -a "$LOG") 2>&1
step() { printf '\n=== %s  %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"; }
trap 'echo "STOPPED (exit $?) at line $LINENO: $BASH_COMMAND. Log: $LOG. Never rerun an outcome; for a partial record: python3 heldout_cohort.py report $H"' ERR
for v in CLAUDECODE OPENCODE CODEX_SANDBOX CODEX_SANDBOX_NETWORK_DISABLED; do
  if [ -n "${!v:-}" ]; then echo "$v is set: run this from a normal terminal, not an agent session." >&2; exit 1; fi
done
[ -t 0 ] || { echo "stdin must be your terminal: the grants are issued by you." >&2; exit 1; }
export PYTHONDONTWRITEBYTECODE=1
cd /private/tmp/adage-heldout-src/evals/reflex-proof/harness
test -z "$(git status --porcelain --untracked-files=no)" || { echo "the clone has edits; stop." >&2; exit 1; }
test "$(shasum -a 256 "$H/approve-heldout.sh" | cut -d' ' -f1)" = "$APPROVE_SHA" || { echo "approve-heldout.sh differs from the reviewed one; stop." >&2; exit 1; }
step "1/7 native confinement session (paid, <= 0.50 USD)"
python3 real_repo_gate.py run-session "$C/sessions/native-confinement/manifest.json"
step "2/7 native review (must pass)"
python3 curve_sessions.py review-native "$C"
step "3/7 prepare (offline, about 30 min, 65 checkouts)"
python3 heldout_cohort.py prepare "$H"
step "4/7 grants: approve-heldout.sh sha256 $APPROVE_SHA"
sh "$H/approve-heldout.sh"
step "5/7 verify grants"
python3 heldout_cohort.py verify-grants "$H" > "$H/verify-grants.json"
cat "$H/verify-grants.json"
python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1]))["passed"] else "grants did not verify")' "$H/verify-grants.json"
step "6/7 live (paid: 65 sessions, 0.40 USD cap, 26 USD ceiling; controls first; stops on any lapse)"
python3 heldout_cohort.py live "$H"
step "7/7 report"
python3 heldout_cohort.py report "$H"
step "done. Log: $LOG"
