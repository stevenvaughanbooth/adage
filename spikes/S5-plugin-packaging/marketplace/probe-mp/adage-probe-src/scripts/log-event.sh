#!/usr/bin/env bash
# S5 probe logger. Records one JSON line per hook firing.
# argv: EVENT LOGFILE [NOTE]
set -u
EVENT="${1:-unknown}"
LOGFILE="${2:?logfile required}"
NOTE="${3:-}"

STDIN_CONTENT="$(cat)"
TS="$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ" 2>/dev/null || date -u +"%Y-%m-%dT%H:%M:%SZ")"
DIGEST="$(printf '%s' "$STDIN_CONTENT" | shasum -a 256 | awk '{print $1}')"
LEN="${#STDIN_CONTENT}"

ADAGE_MARKER="$(command -v adage-marker 2>/dev/null || echo NOTFOUND)"
PACK_MARKER="$(command -v pack-marker 2>/dev/null || echo NOTFOUND)"

# Try to enumerate plugins from inside the hook (question 4).
PLUGIN_LIST_JSON="$(claude plugin list --json 2>&1)"
PLUGIN_LIST_RC=$?

jq -n \
  --arg ts "$TS" \
  --arg event "$EVENT" \
  --arg note "$NOTE" \
  --arg pid "$$" \
  --arg ppid "$PPID" \
  --arg plugin_root "${CLAUDE_PLUGIN_ROOT:-}" \
  --arg plugin_data "${CLAUDE_PLUGIN_DATA:-}" \
  --arg project_dir "${CLAUDE_PROJECT_DIR:-}" \
  --arg path "${PATH:-}" \
  --arg stdin_digest "$DIGEST" \
  --arg stdin_len "$LEN" \
  --arg stdin_raw "$STDIN_CONTENT" \
  --arg adage_marker "$ADAGE_MARKER" \
  --arg pack_marker "$PACK_MARKER" \
  --arg plugin_list_rc "$PLUGIN_LIST_RC" \
  --arg plugin_list_json "$PLUGIN_LIST_JSON" \
  '{ts:$ts, event:$event, note:$note, pid:$pid, ppid:$ppid,
    plugin_root:$plugin_root, plugin_data:$plugin_data, project_dir:$project_dir,
    path:$path, stdin_digest:$stdin_digest, stdin_len:($stdin_len|tonumber),
    stdin_raw:$stdin_raw, adage_marker:$adage_marker, pack_marker:$pack_marker,
    plugin_list_rc:($plugin_list_rc|tonumber), plugin_list_json:$plugin_list_json}' \
  >> "$LOGFILE" 2>>"${LOGFILE}.err"

exit 0
