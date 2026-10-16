#!/usr/bin/env bash
# Records argv as received, to compare exec-form vs shell-form substitution (Q8).
# argv[1] = logfile, argv[2..] = whatever the hook passed (may include literal
# "${CLAUDE_PLUGIN_ROOT}" or "$PATH" if the hook form does not expand them).
set -u
LOGFILE="$1"; shift
cat > /dev/null # drain stdin so the harness doesn't complain
jq -n --arg ts "$(date -u +%Y-%m-%dT%H:%M:%SZ)" --args '{ts:$ts, form:"see-argv", argv:$ARGS.positional}' "$@" \
  >> "$LOGFILE" 2>>"${LOGFILE}.err"
exit 0
