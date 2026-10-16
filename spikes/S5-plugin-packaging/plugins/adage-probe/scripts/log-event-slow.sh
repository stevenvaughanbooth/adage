#!/usr/bin/env bash
# Same as log-event.sh but sleeps first, to detect async blocking / concurrency.
# argv: EVENT LOGFILE SLEEP_SECONDS
set -u
EVENT="${1:-unknown}"
LOGFILE="${2:?logfile required}"
SLEEP_S="${3:-3}"

STDIN_CONTENT="$(cat)"
START_TS="$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ" 2>/dev/null || date -u +"%Y-%m-%dT%H:%M:%SZ")"
DIGEST="$(printf '%s' "$STDIN_CONTENT" | shasum -a 256 | awk '{print $1}')"

sleep "$SLEEP_S"

END_TS="$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ" 2>/dev/null || date -u +"%Y-%m-%dT%H:%M:%SZ")"

jq -n \
  --arg start_ts "$START_TS" \
  --arg end_ts "$END_TS" \
  --arg event "$EVENT" \
  --arg pid "$$" \
  --arg sleep_s "$SLEEP_S" \
  --arg stdin_digest "$DIGEST" \
  '{start_ts:$start_ts, end_ts:$end_ts, event:$event, pid:$pid, sleep_s:$sleep_s, stdin_digest:$stdin_digest}' \
  >> "$LOGFILE" 2>>"${LOGFILE}.err"

exit 0
