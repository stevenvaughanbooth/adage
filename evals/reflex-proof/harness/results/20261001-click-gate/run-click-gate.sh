#!/bin/bash
# Click launch-gate study, launch-gate-1 revision 13. Run by the person, in their own terminal: the
# grants are theirs, and paid sessions refuse to start inside an agent session.
#
#   0  budget check (read-only): the whole chain's worst case must fit the ledger less the $3 floor
#   1  cohort click-gate-v1 and its native recipe (offline)
#   2  native confinement session (PAID, <= 0.50 USD)          3  native review (must pass)
#   4  training recipes (offline)                               5  training: 10 guided sessions (PAID, <= 0.60 USD each)
#   6  learning, organic replay and isolation audit (offline)   7  the mode: execution, or advisory only (asks you)
#   8  freeze (offline)                                         9  prepare (offline, a few minutes)
#   10 grants: you read and run approve-click.sh (execution mode only; its sha256 is printed and checked)
#   11 verify grants (read-only)                                12 live (PAID: 65 or 40 sessions, <= 0.60 USD each)
#   13 report (the decision record)
#
# It stops on any error. Running it again resumes: a session already attempted is never dispatched
# again, and no outcome is ever rerun.
set -euo pipefail
A=/private/tmp/adage-real-repo-gate-v1
SRC=/private/tmp/adage-click-src
HARNESS=$SRC/evals/reflex-proof/harness
C=$A/cohorts/click-gate-v1
H=$C/heldout-1
CONTROLS=/private/tmp/adage-click-controls-2
DEV_REPORT=$A/cohorts/launch-gate-v6/dev-v9b/reports/dev-v9-1790870521952346000.json
EXTRA_AUDIT=/private/tmp/adage-learning-curve-v2
BIN=$SRC/target/release
BUILD=cfafd5e0c14cebcbfda0932e73f09122516c8034
ADAGE_SHA=101a1ecc669f3830e1611456cabc8f871fd0d7aa4922ccd00e3092d76bf92a45
EXEC_SHA=286455d71f01de57788c87d5bf1e1bad3fb3065bb65ae177f27c7d87e8bfea8c
TASKS_VALIDATION_SHA=0670e344b0fa72b0f81aa71f17a0196236868423eceeb7d535d7abb4cb64b0af
PROTOCOL_SHA=00ba79cc17932afbb22feddbb9c21889595f50b547e15acf560bb6a39a705d5c
LOG=/private/tmp/adage-click-gate-$(date -u +%Y%m%dT%H%M%SZ).log
exec > >(tee -a "$LOG") 2>&1
step() { printf '\n=== %s  %s\n' "$(date -u +%Y-%m-%dT%H:%M:%SZ)" "$*"; }
stop() { echo "STOP: $*" >&2; exit 1; }
trap 'echo "STOPPED (exit $?) at line $LINENO: $BASH_COMMAND. Log: $LOG. Run this script again to resume; nothing already attempted is dispatched again."' ERR
for v in CLAUDECODE OPENCODE CODEX_SANDBOX CODEX_SANDBOX_NETWORK_DISABLED; do
  if [ -n "${!v:-}" ]; then stop "$v is set: run this from a normal terminal, not an agent session."; fi
done
[ -t 0 ] || stop "stdin must be your terminal: the confirmations and the grants are yours."
export PYTHONDONTWRITEBYTECODE=1
cd "$HARNESS"
test -z "$(git status --porcelain --untracked-files=no)" || stop "the clone $SRC has edits; the frozen sources must be the committed ones."
sha() { shasum -a 256 "$1" | cut -d' ' -f1; }
test "$(sha launch_gate_protocol.json)" = "$PROTOCOL_SHA" || stop "launch_gate_protocol.json differs from the registered r13 protocol."
test "$(sha "$BIN/adage")" = "$ADAGE_SHA" || stop "$BIN/adage is not the PR #16 head build (cfafd5e); rebuild with cargo build --release --locked -p adage-cli -p adage-exec."
test "$(sha "$BIN/adage-exec")" = "$EXEC_SHA" || stop "$BIN/adage-exec is not the PR #16 head build (cfafd5e)."
test "$(sha "$CONTROLS/validation.json")" = "$TASKS_VALIDATION_SHA" || stop "the validated Click task controls differ from the frozen manifest."
test -f "$DEV_REPORT" || stop "the dev-v9b report (BEST's selection) is missing."
claude --version || true

step "0 budget (read-only): the whole chain's worst case must fit the ledger less the \$3 floor"
if ! python3 click_gate.py budget-check "$A" --phase start; then
  stop "the allocation does not cover the study's worst case (see shortfall_usd above). Raising the \$60 ledger ceiling is your decision: edit $A/evaluation-allocation.json and approve the matching change to real_repo_gate.budget_limit's \$60 limit, then run this again. Nothing was spent."
fi
read -r -p "Type 'run' to start (paid steps follow: native <= 0.50 USD, training <= 6.00 USD, held-out <= 39.00 USD): " answer
[ "$answer" = run ] || stop "not started at your request; nothing was spent."

step "1 cohort click-gate-v1 and its native recipe (offline)"
if [ ! -e "$C/manifest.json" ]; then python3 click_training.py prepare-cohort "$A" "$C"; fi
python3 click_training.py prepare-native "$C"

step "2 native confinement session (paid, <= 0.50 USD)"
if python3 click_gate.py attempted "$A" cohorts/click-gate-v1/sessions/native-confinement > /dev/null; then
  echo "already attempted; never dispatched again"
else
  python3 real_repo_gate.py run-session "$C/sessions/native-confinement/manifest.json"
fi

step "3 native review (must pass)"
if [ ! -e "$C/native-confinement-reviewed.json" ]; then python3 curve_sessions.py review-native "$C"; fi
python3 -c 'import json,sys; sys.exit(0 if json.load(open(sys.argv[1]))["passed"] else "the native review did not pass")' "$C/native-confinement-reviewed.json"

step "4 training recipes (offline, 10 checkouts)"
python3 click_training.py prepare-training "$C" > /dev/null

step "5 training: 10 guided sessions (paid, <= 0.60 USD each; stops before the \$3 floor)"
python3 click_training.py train "$C"
python3 click_training.py status "$C"

step "6 learning (offline): the fixed learner on the guided histories, the organic replay, the isolation audit"
python3 click_training.py learn "$C" --binary "$BIN/adage" --audit-authority "$EXTRA_AUDIT" > /dev/null
python3 - "$C/learning/learning.json" <<'PY'
import json, sys
r = json.load(open(sys.argv[1]))
print("guided: %s, %s sessions, missing %s" % (r["guided"]["status"], r["guided"]["sessions"], r["guided"]["missing"]))
for name in ("guided", "organic"):
    print("%s: audit %s; certified context routes:" % (name, (r[name].get("audit") or {}).get("passed")))
    for v in r[name].get("certified") or []:
        print("   %s  %s  eligible=%s  control_fit=%s" % (v["route"], v["command"], v["eligible"], v["control_fit"].get("fits")))
print("MODE: %s; route %s; command %s" % (r["mode"], r["route"], r["command"]))
PY

step "7 the mode"
if [ -e "$H/manifest.json" ]; then
  echo "already frozen; resuming"
else
  MODE=$(python3 click_gate.py mode "$C/learning/learning.json")
  ADVISORY_ONLY=()
  if [ "$MODE" = advisory ]; then
    echo "Learning produced no executable route that fits the registered rule (r13). As registered, the held-out"
    echo "study runs ADVISORY ONLY: A0 and NOEXEC in 20 paired blocks (40 sessions, <= 24.00 USD); BEST and its"
    echo "controls are skipped, and only the advisory co-primary is evaluated."
    read -r -p "Type 'advisory' to run the advisory-only held-out study, anything else to stop here: " answer
    [ "$answer" = advisory ] || stop "stopped before the held-out study at your request. The learning record stays."
  else
    if ! python3 click_gate.py budget-check "$A" --phase heldout --mode execution; then
      echo "Execution mode's held-out worst case (65 x 0.60 = 39.00 USD) does not fit the remaining allocation."
      read -r -p "Type 'advisory-only' to freeze advisory only (40 sessions), anything else to stop and raise the allocation: " answer
      [ "$answer" = advisory-only ] || stop "stopped before freeze; raise the allocation and run this again."
      ADVISORY_ONLY=(--advisory-only)
      MODE=advisory
    fi
  fi
  python3 click_gate.py budget-check "$A" --phase heldout --mode "$MODE" > /dev/null || stop "the $MODE held-out worst case does not fit the remaining allocation."

  step "8 freeze (offline)"
  python3 click_gate.py freeze "$H" --cohort "$C" --learning "$C/learning/learning.json" --controls "$CONTROLS" \
    --dev-report "$DEV_REPORT" --binaries "$BIN" --build-commit "$BUILD" --adage-sha256 "$ADAGE_SHA" \
    --adage-exec-sha256 "$EXEC_SHA" --audit-authority "$EXTRA_AUDIT" ${ADVISORY_ONLY[@]+"${ADVISORY_ONLY[@]}"}
fi
FROZEN_MODE=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["mode"])' "$H/manifest.json")

step "9 prepare (offline, a few minutes; makes the shared runtime library read-only)"
python3 click_gate.py prepare "$H"

step "10 grants (mode: $FROZEN_MODE)"
if [ "$FROZEN_MODE" = execution ]; then
  if python3 click_gate.py verify-grants "$H" > /dev/null 2>&1; then
    echo "the 25 BEST grants are already present and verified"
  else
    python3 click_gate.py approval-script "$H" > "$H/approval-script.json"
    APPROVE_SHA=$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["sha256"])' "$H/approval-script.json")
    test "$(sha "$H/approve-click.sh")" = "$APPROVE_SHA" || stop "approve-click.sh changed after it was written."
    echo "approve-click.sh: $H/approve-click.sh"
    echo "sha256: $APPROVE_SHA"
    echo "It enables E2 only in the 25 BEST homes and approves one learned pytest verification in each. Read it now."
    ${PAGER:-less} "$H/approve-click.sh"
    read -r -p "Type the first 12 characters of the sha256 above to issue the grants yourself: " answer
    [ "$answer" = "${APPROVE_SHA:0:12}" ] || stop "not confirmed; no grant was issued."
    test "$(sha "$H/approve-click.sh")" = "$APPROVE_SHA" || stop "approve-click.sh changed while you read it."
    sh "$H/approve-click.sh"
  fi
else
  echo "advisory only: no grant is issued"
fi

step "11 verify grants (read-only)"
python3 click_gate.py verify-grants "$H" > "$H/verify-grants.json"
cat "$H/verify-grants.json"

step "12 live (paid: controls first in execution mode; a failed control or lapsed grant drops BEST, A0 and NOEXEC continue)"
python3 click_gate.py live "$H"

step "13 report"
python3 click_gate.py report "$H" > "$H/report-latest.json"
python3 -c 'import json,sys; r=json.load(open(sys.argv[1])); print("OUTCOMES:", json.dumps(r["outcomes"]))' "$H/report-latest.json"
ls -1t "$H"/reports/click-heldout-*.md | head -1
step "done. Log: $LOG"
