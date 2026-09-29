#!/bin/sh
# Exercises plugin/claude-code/scripts/session-start: how it finds the binary (bundled and verified,
# then PATH, including npm's launcher), that every path exits 0, that a failure is one JSON
# `systemMessage` line on stdout (what Claude Code shows the person) with the other hooks pointed
# at scripts/quiet, and that `delegate on` without the adage-exec plugin keeps serving.
#
#   plugin/test/session-start.test.sh [path/to/adage]     (default: target/debug/adage)
#
# Unix only (Windows runs hooks differently; see SECURITY.md and the plan's §4.4).
set -eu
cd "$(dirname "$0")/../.."
REPO=$(pwd)
BIN=$(cd "$(dirname "${1:-target/debug/adage}")" && pwd)/$(basename "${1:-target/debug/adage}")
[ -x "$BIN" ] || { echo "no binary at $BIN; build it first (cargo build -p adage-cli)"; exit 1; }
SS="$REPO/plugin/claude-code/scripts/session-start"
W=$(cd "$(mktemp -d)" && pwd -P) # physical path: node reports resolved paths
trap 'rm -rf "$W"' EXIT
export ADAGE_HOME="$W/adage-home"
unset ADAGE_DISABLE || true
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) T=aarch64-apple-darwin ;; Darwin-x86_64) T=x86_64-apple-darwin ;;
  Linux-x86_64) T=x86_64-unknown-linux-gnu ;; Linux-aarch64) T=aarch64-unknown-linux-gnu ;;
  *) echo "unsupported platform for this test"; exit 1 ;;
esac
# A PATH with the basics but no adage.
BASE="/usr/bin:/bin:/usr/sbin:/sbin"
NODE=$(command -v node || true)
PAYLOAD='{"session_id":"ss-test","hook_event_name":"SessionStart","source":"startup"}'
fails=0
check() { # check <name> <condition...>
  name=$1; shift
  if "$@"; then echo "ok   $name"; else echo "FAIL $name"; fails=$((fails + 1)); fi
}
run() { # run <root> <data> <path>: run the hook, recording exit status, stdout and stderr
  set +e
  echo "$PAYLOAD" | PATH="$3" "$SS" "$1" "$2" >"$W/out" 2>"$W/err"
  code=$?
  set -e
}
target_of() { readlink "$1" 2>/dev/null || true; }

# 1. Bundled and verified.
R1="$W/root1"; mkdir -p "$R1/dist/$T"; cp "$BIN" "$R1/dist/$T/adage"
(cd "$R1/dist" && { shasum -a 256 "$T/adage" 2>/dev/null || sha256sum "$T/adage"; } >SHA256SUMS)
run "$R1" "$W/data1" "$BASE"
check "bundled: exits 0" [ "$code" = 0 ]
check "bundled: links adage-<sha>" sh -c "readlink '$W/data1/bin/adage' | grep -q '^adage-[0-9a-f]\{64\}\$'"
check "bundled: binary runs" sh -c "'$W/data1/bin/adage' version | grep -q '^adage '"

# 2. Bundled binary tampered with: ignored, with a hint; PATH has nothing: fail open.
R2="$W/root2"; cp -R "$R1" "$R2"; printf 'x' >>"$R2/dist/$T/adage"
mkdir -p "$R2/scripts"; cp "$REPO/plugin/claude-code/scripts/quiet" "$R2/scripts/quiet"
run "$R2" "$W/data2" "$BASE"
check "tampered: exits 0" [ "$code" = 0 ]
check "tampered: one systemMessage line on stdout" sh -c "[ \$(wc -l <'$W/out') = 1 ] && grep -q '^{\"systemMessage\":\"adage: the bundled binary for .* does not match its checksum' '$W/out'"
check "tampered: stdout is JSON" node -e "JSON.parse(require('fs').readFileSync('$W/out','utf8'))"
check "tampered: nothing on stderr" [ ! -s "$W/err" ]
check "tampered: reason in \$DATA/status" grep -q "does not match its checksum" "$W/data2/status"
check "tampered: other hooks run scripts/quiet" [ "$(target_of "$W/data2/bin/adage")" = "$R2/scripts/quiet" ]

# 2b. No bundled binary and nothing on PATH: the install hint, as a systemMessage.
R2B="$W/root2b"; mkdir -p "$R2B/scripts"; cp "$REPO/plugin/claude-code/scripts/quiet" "$R2B/scripts/quiet"
run "$R2B" "$W/data2b" "$BASE"
check "no binary: exits 0" [ "$code" = 0 ]
check "no binary: install hint" grep -q '"systemMessage":"adage: no binary for .*; install with brew install .* or npm i -g' "$W/out"
check "no binary: a quiet hook still answers" sh -c "echo '{}' | '$W/data2b/bin/adage' hook claude UserPromptSubmit; [ \$? = 0 ]"

# 3. No bundled binary; adage on PATH (Homebrew, installers, cargo): linked at its PATH location.
R3="$W/root3"; mkdir -p "$R3/scripts"; cp "$REPO/plugin/claude-code/scripts/quiet" "$R3/scripts/quiet"
mkdir -p "$W/pathbin"; ln -s "$BIN" "$W/pathbin/adage"
run "$R3" "$W/data3" "$W/pathbin:$BASE"
check "PATH: exits 0" [ "$code" = 0 ]
check "PATH: linked to the PATH entry" [ "$(target_of "$W/data3/bin/adage")" = "$W/pathbin/adage" ]

# 4. adage on PATH is npm's launcher: hooks get the platform binary, not node.
if [ -n "$NODE" ]; then
  # "<target> <os>-<cpu>" for this machine, from the npm platform table.
  NT=$(node -e 'import("'"$REPO"'/npm/config.mjs").then(c => { const p = c.PLATFORMS.find(p => p.os === process.platform && p.cpu === process.arch); console.log(p.target + " " + p.os + "-" + p.cpu) })')
  mkdir -p "$W/bins/${NT% *}"; cp "$BIN" "$W/bins/${NT% *}/adage"
  node "$REPO/npm/scripts/stage.mjs" --version 0.0.0-test --out "$W/npm" --allow-missing --bins "$W/bins" \
    --name adage-cli-ss-test >/dev/null
  NM="$W/global/lib/node_modules"; mkdir -p "$NM" "$W/global/bin"
  cp -R "$W/npm/cli" "$NM/adage-cli-ss-test"
  cp -R "$W/npm/${NT#* }" "$NM/adage-cli-ss-test/node_modules-tmp"
  mkdir -p "$NM/adage-cli-ss-test/node_modules"
  mv "$NM/adage-cli-ss-test/node_modules-tmp" "$NM/adage-cli-ss-test/node_modules/adage-cli-ss-test-${NT#* }"
  ln -s "../lib/node_modules/adage-cli-ss-test/bin/adage.js" "$W/global/bin/adage"
  run "$R3" "$W/data4" "$W/global/bin:$(dirname "$NODE"):$BASE"
  check "npm: exits 0" [ "$code" = 0 ]
  check "npm: linked to the platform binary" \
    [ "$(target_of "$W/data4/bin/adage")" = "$NM/adage-cli-ss-test/node_modules/adage-cli-ss-test-${NT#* }/bin/adage" ]
else
  echo "skip npm launcher case: node not found"
fi

# 5. Missing arguments: fail open.
run "" "" "$BASE"
check "no args: exits 0 silently" sh -c "[ '$code' = 0 ] && [ ! -s '$W/out' ]"

# 6. Execution on or off: exactly one of the two plugins serves (Track C D2).
XS="$REPO/plugin/claude-code-exec/scripts/session-start"
XR="$REPO/plugin/claude-code-exec"
runx() { set +e; echo "$PAYLOAD" | PATH="$2" "$XS" "$XR" "$1" >"$W/out" 2>"$W/err"; code=$?; set -e; }
runx "$W/xdata1" "$W/pathbin:$BASE"
check "exec off: exec plugin exits 0 silently" sh -c "[ '$code' = 0 ] && [ ! -s '$W/out' ]"
check "exec off: exec plugin is quiet" [ "$(target_of "$W/xdata1/bin/adage-exec")" = "$XR/scripts/quiet" ]
mkdir -p "$ADAGE_HOME/exec"; echo '{}' >"$ADAGE_HOME/exec/enabled"
# The adage-exec plugin is not installed: the Adage plugin keeps serving, and says why.
rm -f "$ADAGE_HOME/exec/plugin-seen"
run "$R3" "$W/data7" "$W/pathbin:$BASE"
check "exec on, no exec plugin: exits 0" [ "$code" = 0 ]
check "exec on, no exec plugin: still serves" [ "$(target_of "$W/data7/bin/adage")" = "$W/pathbin/adage" ]
check "exec on, no exec plugin: says so" grep -q "delegation is on but the adage-exec plugin isn't installed" "$W/out"
# Installed (beside this plugin, as in a checkout): the Adage plugin goes quiet.
mkdir -p "$W/claude-code-exec"
run "$R3" "$W/data6" "$W/pathbin:$BASE"
check "exec on: adage plugin exits 0 silently" sh -c "[ '$code' = 0 ] && [ ! -s '$W/out' ]"
check "exec on: adage plugin is quiet" [ "$(target_of "$W/data6/bin/adage")" = "$R3/scripts/quiet" ]
mkdir -p "$W/xpath"; ln -s "$BIN" "$W/xpath/adage"
if [ -x "$(dirname "$BIN")/adage-exec" ]; then ln -s "$(dirname "$BIN")/adage-exec" "$W/xpath/adage-exec"; fi
runx "$W/xdata2" "$W/xpath:$BASE"
check "exec on: exec plugin exits 0" [ "$code" = 0 ]
check "exec on: exec plugin serves" sh -c "readlink '$W/xdata2/bin/adage-exec' | grep -q '/adage'"
rm -f "$ADAGE_HOME/exec/enabled"

[ "$fails" = 0 ] && echo "session-start ok" || { echo "$fails failure(s)"; exit 1; }
