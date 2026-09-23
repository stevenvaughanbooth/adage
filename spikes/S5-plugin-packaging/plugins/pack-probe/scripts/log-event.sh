#!/usr/bin/env bash
# Same logger as adage-probe's, duplicated so pack-probe has no dependency on
# adage-probe's files (a real track pack couldn't rely on Adage's scripts
# existing at a known path).
set -u
EVENT="${1:-unknown}"
LOGFILE="${2:?logfile required}"

STDIN_CONTENT="$(cat)"
TS="$(date -u +"%Y-%m-%dT%H:%M:%S.%3NZ" 2>/dev/null || date -u +"%Y-%m-%dT%H:%M:%SZ")"
DIGEST="$(printf '%s' "$STDIN_CONTENT" | shasum -a 256 | awk '{print $1}')"

ADAGE_MARKER="$(command -v adage-marker 2>/dev/null || echo NOTFOUND)"
PACK_MARKER="$(command -v pack-marker 2>/dev/null || echo NOTFOUND)"
PLUGIN_LIST_JSON="$(claude plugin list --json 2>&1)"
PLUGIN_LIST_RC=$?

jq -n \
  --arg ts "$TS" \
  --arg event "$EVENT" \
  --arg plugin_root "${CLAUDE_PLUGIN_ROOT:-}" \
  --arg plugin_data "${CLAUDE_PLUGIN_DATA:-}" \
  --arg project_dir "${CLAUDE_PROJECT_DIR:-}" \
  --arg path "${PATH:-}" \
  --arg stdin_digest "$DIGEST" \
  --arg adage_marker "$ADAGE_MARKER" \
  --arg pack_marker "$PACK_MARKER" \
  --arg plugin_list_rc "$PLUGIN_LIST_RC" \
  --arg plugin_list_json "$PLUGIN_LIST_JSON" \
  '{ts:$ts, event:$event, plugin_root:$plugin_root, plugin_data:$plugin_data,
    project_dir:$project_dir, path:$path, stdin_digest:$stdin_digest,
    adage_marker:$adage_marker, pack_marker:$pack_marker,
    plugin_list_rc:($plugin_list_rc|tonumber), plugin_list_json:$plugin_list_json}' \
  >> "$LOGFILE" 2>>"${LOGFILE}.err"

exit 0
