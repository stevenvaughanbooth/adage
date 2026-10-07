#!/bin/sh
# Exercises plugin/claude-code/scripts/session-start, the one plugin's shim (plan A8): how it finds
# each binary (bundled and verified, then PATH, including npm's launcher), which one bin/hook runs
# (adage-exec with $ADAGE_HOME/exec/enabled, else adage), copies on a simulated Windows, that every
# path exits 0, and that a failure is one JSON `systemMessage` line on stdout (what Claude Code
# shows the person) with the other hooks pointed at scripts/quiet. Then the retired adage-exec
# plugin's alias shim (T-A8-04).
#
#   plugin/test/session-start.test.sh [path/to/adage]     (default: target/debug/adage; adage-exec
#                                                          is taken from beside it)
#
# Unix only (Windows runs hooks differently; see SECURITY.md and the plan's §4.4).
set -eu
cd "$(dirname "$0")/../.."
REPO=$(pwd)
BIN=$(cd "$(dirname "${1:-target/debug/adage}")" && pwd)/$(basename "${1:-target/debug/adage}")
[ -x "$BIN" ] || { echo "no binary at $BIN; build it first (cargo build -p adage-cli)"; exit 1; }
EXEC_BIN="$(dirname "$BIN")/adage-exec"
[ -x "$EXEC_BIN" ] || { echo "no adage-exec beside $BIN; build it first (cargo build -p adage-exec)"; exit 1; }
SS="$REPO/plugin/claude-code/scripts/session-start"
W=$(cd "$(mktemp -d)" && pwd -P) # physical path: node reports resolved paths
trap 'rm -rf "$W"' EXIT
# The shim copies every binary it installs, and debug builds are hundreds of megabytes: the
# fixtures use stripped copies when strip is there (the same code, and every check compares
# against these copies).
if command -v strip >/dev/null 2>&1 && strip -o "$W/adage" "$BIN" 2>/dev/null && strip -o "$W/adage-exec" "$EXEC_BIN" 2>/dev/null; then
  BIN="$W/adage"; EXEC_BIN="$W/adage-exec"
fi
export ADAGE_HOME="$W/adage-home"
export HOME="$W/home" XDG_CONFIG_HOME="$W/xdg" CODEX_HOME="$W/codex"
mkdir -p "$HOME" "$XDG_CONFIG_HOME" "$CODEX_HOME"
unset CLAUDE_CONFIG_DIR ADAGE_TEST_NO_SPAWN || true
unset ADAGE_DISABLE || true
export ADAGE_AUTO_LEARN=0
case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) T=aarch64-apple-darwin ;; Darwin-x86_64) T=x86_64-apple-darwin ;;
  Linux-x86_64) T=x86_64-unknown-linux-gnu ;; Linux-aarch64) T=aarch64-unknown-linux-gnu ;;
  *) echo "unsupported platform for this test"; exit 1 ;;
esac
# A PATH with the basics but no adage.
BASE="/usr/bin:/bin:/usr/sbin:/sbin"
NODE=$(command -v node || true)
START='{"session_id":"ss-test","hook_event_name":"SessionStart","source":"startup"}'
PAYLOAD=$START
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
sums() { (cd "$1/dist" && { shasum -a 256 $2 2>/dev/null || sha256sum $2; } >SHA256SUMS); }
marker_on() { mkdir -p "$ADAGE_HOME/exec"; echo '{}' >"$ADAGE_HOME/exec/enabled"; }
marker_off() { rm -f "$ADAGE_HOME/exec/enabled"; }
# The `bin` of every session_start record in a session's spool (CRC-framed: magic, length,
# CRC, JSON), one per line.
starts() {
  node -e '
    const b = require("fs").readFileSync(process.argv[1]); let i = 0;
    while (i + 12 <= b.length) {
      const n = b.readUInt32LE(i + 4), r = JSON.parse(b.subarray(i + 12, i + 12 + n)); i += 12 + n;
      if (r.kind === "session_start") console.log(r.bin);
    }' "$ADAGE_HOME/sessions/$1.spool" 2>/dev/null || true
}

# The bundled plugin: both binaries, verified.
R1="$W/root1"; mkdir -p "$R1/dist/$T" "$R1/scripts"
cp "$BIN" "$R1/dist/$T/adage"; cp "$EXEC_BIN" "$R1/dist/$T/adage-exec"
cp "$REPO/plugin/claude-code/scripts/quiet" "$R1/scripts/quiet"
sums "$R1" "$T/adage $T/adage-exec"

# 1. T-A8-01: without the marker, bin/hook is adage; both binaries are installed and verified.
marker_off
run "$R1" "$W/data1" "$BASE"
check "bundled: exits 0" [ "$code" = 0 ]
check "bundled: links adage-<sha>" sh -c "readlink '$W/data1/bin/adage' | grep -q '^adage-[0-9a-f]\{64\}\$'"
check "bundled: links adage-exec-<sha>" sh -c "readlink '$W/data1/bin/adage-exec' | grep -q '^adage-exec-[0-9a-f]\{64\}\$'"
check "bundled: the CLI binary runs" sh -c "'$W/data1/bin/adage' version | grep -q '^adage '"
check "marker off: bin/hook is adage" [ "$(target_of "$W/data1/bin/hook")" = adage ]

# 2. With the marker, bin/hook is adage-exec; bin/adage is still the CLI binary. Removing it
# re-makes the link at the next start.
marker_on
run "$R1" "$W/data1" "$BASE"
check "marker on: exits 0" [ "$code" = 0 ]
check "marker on: bin/hook is adage-exec" [ "$(target_of "$W/data1/bin/hook")" = adage-exec ]
check "marker on: bin/adage is the CLI binary" sh -c "'$W/data1/bin/adage' version | grep -q '^adage '"
check "marker on: says nothing extra" sh -c "! grep -q 'execution is on, but' '$W/out'"
marker_off
run "$R1" "$W/data1" "$BASE"
check "marker removed: bin/hook is adage again" [ "$(target_of "$W/data1/bin/hook")" = adage ]

# 3. T-A8-02: never both. A session started with the marker records one session_start, from
# adage-exec; without it, one from adage.
if [ -n "$NODE" ]; then
  mkdir -p "$W/billing/.git"
  for state in on off; do
    if [ "$state" = on ]; then marker_on; want='adage-exec-[0-9a-f]\{64\}'; else marker_off; want='adage-[0-9a-f]\{64\}'; fi
    PAYLOAD="{\"session_id\":\"never-both-$state\",\"cwd\":\"$W/billing\",\"hook_event_name\":\"SessionStart\",\"source\":\"startup\"}"
    run "$R1" "$W/data3" "$BASE"
    starts "never-both-$state" >"$W/starts"
    echo "observed start ($state): $(cat "$W/starts")"
    check "never both ($state): one session_start, from the hook binary" sh -c "[ \$(wc -l <'$W/starts') = 1 ] && grep -qx '$want' '$W/starts'"
  done
  marker_off
  PAYLOAD=$START
else
  echo "skip never-both: node not found"
fi

# Native providers and the portable fallback must produce the same digest.
printf 'abc' >"$W/digest input"
DIGEST=ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad
mkdir -p "$W/fallback-bin"
for provider in sha256sum openssl; do
  printf '#!/bin/sh\nexit 1\n' >"$W/fallback-bin/$provider"
  chmod +x "$W/fallback-bin/$provider"
done
sed -n '/^sha256()/p' "$SS" >"$W/digest-function"
GOT=$(PATH="$BASE" sh -c '. "$1"; sha256 "$2"' sh "$W/digest-function" "$W/digest input")
check "digest: native provider" [ "$GOT" = "$DIGEST" ]
if PATH="$BASE" command -v shasum >/dev/null 2>&1; then
  GOT=$(PATH="$W/fallback-bin:$BASE" sh -c '. "$1"; sha256 "$2"' sh "$W/digest-function" "$W/digest input")
  check "digest: portable fallback" [ "$GOT" = "$DIGEST" ]
fi

# 4. Bundled adage tampered with: ignored, with a hint; PATH has nothing: fail open.
R2="$W/root2"; cp -R "$R1" "$R2"; printf 'x' >>"$R2/dist/$T/adage"
run "$R2" "$W/data2" "$BASE"
check "tampered: exits 0" [ "$code" = 0 ]
check "tampered: one systemMessage line on stdout" sh -c "[ \$(wc -l <'$W/out') = 1 ] && grep -q '^{\"systemMessage\":\"adage: the bundled binary for .* does not match its checksum' '$W/out'"
check "tampered: stdout is JSON" node -e "JSON.parse(require('fs').readFileSync('$W/out','utf8'))"
check "tampered: nothing on stderr" [ ! -s "$W/err" ]
check "tampered: reason in \$DATA/status" grep -q "does not match its checksum" "$W/data2/status"
check "tampered: hooks run scripts/quiet" [ "$(target_of "$W/data2/bin/hook")" = "$R2/scripts/quiet" ]
check "tampered: the CLI entry is quiet too" [ "$(target_of "$W/data2/bin/adage")" = "$R2/scripts/quiet" ]

# 5. Bundled adage-exec tampered with, marker on: adage serves, and the person is told why.
R5="$W/root5"; cp -R "$R1" "$R5"; printf 'x' >>"$R5/dist/$T/adage-exec"
marker_on
run "$R5" "$W/data5" "$BASE"
check "exec tampered: exits 0" [ "$code" = 0 ]
check "exec tampered: adage serves" [ "$(target_of "$W/data5/bin/hook")" = adage ]
check "exec tampered: not installed" [ ! -e "$W/data5/bin/adage-exec" ]
check "exec tampered: says why" grep -q 'execution is on, but the bundled adage-exec does not match its checksum' "$W/out"
marker_off

# 6. No bundled binary and nothing on PATH: the install hint, as a systemMessage.
R6="$W/root6"; mkdir -p "$R6/scripts"; cp "$REPO/plugin/claude-code/scripts/quiet" "$R6/scripts/quiet"
run "$R6" "$W/data6" "$BASE"
check "no binary: exits 0" [ "$code" = 0 ]
check "no binary: install hint" grep -q '"systemMessage":"adage: no binary for .*; install with brew install .* or npm i -g' "$W/out"
check "no binary: a quiet hook still answers" sh -c "echo '{}' | '$W/data6/bin/hook' hook claude UserPromptSubmit; [ \$? = 0 ]"

# 7. No bundled binary; adage and adage-exec on PATH (Homebrew, installers, cargo): linked at
# their PATH locations, and the marker picks adage-exec.
mkdir -p "$W/pathbin"; ln -s "$BIN" "$W/pathbin/adage"; ln -s "$EXEC_BIN" "$W/pathbin/adage-exec"
run "$R6" "$W/data7" "$W/pathbin:$BASE"
check "PATH: exits 0" [ "$code" = 0 ]
check "PATH: adage linked to the PATH entry" [ "$(target_of "$W/data7/bin/adage")" = "$W/pathbin/adage" ]
check "PATH: adage-exec linked to the PATH entry" [ "$(target_of "$W/data7/bin/adage-exec")" = "$W/pathbin/adage-exec" ]
marker_on
run "$R6" "$W/data7" "$W/pathbin:$BASE"
check "PATH, marker on: bin/hook is adage-exec" [ "$(target_of "$W/data7/bin/hook")" = adage-exec ]
# adage-exec not on PATH but beside the adage on PATH (the old exec shim's fallback).
mkdir -p "$W/beside"; cp "$BIN" "$W/beside/adage"; cp "$EXEC_BIN" "$W/beside/adage-exec"
run "$R6" "$W/data7b" "$W/beside:$BASE"
check "PATH: adage-exec found beside adage" [ "$(target_of "$W/data7b/bin/adage-exec")" = "$W/beside/adage-exec" ]
# Neither: adage serves, and says why.
mkdir -p "$W/noexec"; ln -s "$BIN" "$W/noexec/adage"
run "$R6" "$W/data7c" "$W/noexec:$BASE"
check "PATH, no adage-exec: exits 0" [ "$code" = 0 ]
check "PATH, no adage-exec: adage serves" [ "$(target_of "$W/data7c/bin/hook")" = adage ]
check "PATH, no adage-exec: says why" grep -q 'execution is on, but no adage-exec binary was found' "$W/out"
marker_off

# 8. adage on PATH is npm's launcher: hooks get the platform binary, not node, and adage-exec from
# beside it.
if [ -n "$NODE" ]; then
  # "<target> <os>-<cpu>" for this machine, from the npm platform table.
  NT=$(node -e 'import("'"$REPO"'/npm/config.mjs").then(c => { const p = c.PLATFORMS.find(p => p.os === process.platform && p.cpu === process.arch); console.log(p.target + " " + p.os + "-" + p.cpu) })')
  mkdir -p "$W/bins/${NT% *}"; cp "$BIN" "$W/bins/${NT% *}/adage"; cp "$EXEC_BIN" "$W/bins/${NT% *}/adage-exec"
  node "$REPO/npm/scripts/stage.mjs" --version 0.0.0-test --out "$W/npm" --allow-missing --bins "$W/bins" \
    --name adage-cli-ss-test >/dev/null
  NM="$W/global/lib/node_modules"; mkdir -p "$NM" "$W/global/bin"
  cp -R "$W/npm/cli" "$NM/adage-cli-ss-test"
  cp -R "$W/npm/${NT#* }" "$NM/adage-cli-ss-test/node_modules-tmp"
  mkdir -p "$NM/adage-cli-ss-test/node_modules"
  mv "$NM/adage-cli-ss-test/node_modules-tmp" "$NM/adage-cli-ss-test/node_modules/adage-cli-ss-test-${NT#* }"
  ln -s "../lib/node_modules/adage-cli-ss-test/bin/adage.js" "$W/global/bin/adage"
  PLAT="$NM/adage-cli-ss-test/node_modules/adage-cli-ss-test-${NT#* }/bin"
  run "$R6" "$W/data8" "$W/global/bin:$(dirname "$NODE"):$BASE"
  check "npm: exits 0" [ "$code" = 0 ]
  check "npm: linked to the platform binary" [ "$(target_of "$W/data8/bin/adage")" = "$PLAT/adage" ]
  check "npm: adage-exec from beside it" [ "$(target_of "$W/data8/bin/adage-exec")" = "$PLAT/adage-exec" ]
else
  echo "skip npm launcher case: node not found"
fi

# 9. A simulated Windows (a `uname` stub): every entry is a copy, and bin/hook is copied again
# when the marker flips.
mkdir -p "$W/win-bin"
printf '#!/bin/sh\ncase "$1" in -m) echo x86_64 ;; *) echo MINGW64_NT-10.0 ;; esac\n' >"$W/win-bin/uname"
chmod +x "$W/win-bin/uname"
WT=x86_64-pc-windows-msvc
RW="$W/rootwin"; mkdir -p "$RW/dist/$WT" "$RW/scripts"; cp "$REPO/plugin/claude-code/scripts/quiet" "$RW/scripts/quiet"
cp "$BIN" "$RW/dist/$WT/adage.exe"; cp "$EXEC_BIN" "$RW/dist/$WT/adage-exec.exe"
sums "$RW" "$WT/adage.exe $WT/adage-exec.exe"
marker_off
run "$RW" "$W/datawin" "$W/win-bin:$BASE"
check "windows: exits 0" [ "$code" = 0 ]
check "windows: adage.exe is a copy" sh -c "[ -f '$W/datawin/bin/adage.exe' ] && [ ! -L '$W/datawin/bin/adage.exe' ] && cmp -s '$W/datawin/bin/adage.exe' '$BIN'"
check "windows: hook.exe is a copy of adage" sh -c "[ ! -L '$W/datawin/bin/hook.exe' ] && cmp -s '$W/datawin/bin/hook.exe' '$BIN'"
marker_on
run "$RW" "$W/datawin" "$W/win-bin:$BASE"
check "windows, marker on: hook.exe copied from adage-exec" cmp -s "$W/datawin/bin/hook.exe" "$EXEC_BIN"
marker_off
run "$RW" "$W/datawin" "$W/win-bin:$BASE"
check "windows, marker off: hook.exe copied from adage again" cmp -s "$W/datawin/bin/hook.exe" "$BIN"

# 10. Missing arguments: fail open.
run "" "" "$BASE"
check "no args: exits 0 silently" sh -c "[ '$code' = 0 ] && [ ! -s '$W/out' ]"

# 11. T-A8-04: the retired adage-exec plugin's alias registers SessionStart alone; its shim says
# its one line, relinks a stale bin/adage-exec to scripts/quiet, runs nothing and exits 0.
XR="$REPO/plugin/claude-code-exec"
check "alias: hooks.json registers SessionStart alone" node -e "
  const h = JSON.parse(require('fs').readFileSync('$XR/hooks/hooks.json', 'utf8'));
  process.exit(Object.keys(h.hooks).join() === 'SessionStart' && !('modules' in h) ? 0 : 1)"
mkdir -p "$W/xdata/bin"; ln -s "$EXEC_BIN" "$W/xdata/bin/adage-exec"
marker_on
PAYLOAD='{"session_id":"alias-test","hook_event_name":"SessionStart","source":"startup"}'
set +e
echo "$PAYLOAD" | PATH="$BASE" "$XR/scripts/session-start" "$XR" "$W/xdata" >"$W/out" 2>"$W/err"; code=$?
set -e
check "alias: exits 0" [ "$code" = 0 ]
check "alias: one line, uninstall this one" sh -c "[ \$(wc -l <'$W/out') = 1 ] && grep -q 'the adage plugin now includes execution; uninstall this one' '$W/out'"
check "alias: stdout is JSON" node -e "JSON.parse(require('fs').readFileSync('$W/out','utf8'))"
check "alias: the stale link now runs scripts/quiet" [ "$(target_of "$W/xdata/bin/adage-exec")" = "$XR/scripts/quiet" ]
check "alias: the stale entry answers nothing" sh -c "[ -z \"\$(echo '$PAYLOAD' | '$W/xdata/bin/adage-exec' hook claude UserPromptSubmit)\" ]"
check "alias: no record (no event answers twice)" [ ! -e "$ADAGE_HOME/sessions/alias-test.spool" ]
marker_off

[ "$fails" = 0 ] && echo "session-start ok" || { echo "$fails failure(s)"; exit 1; }
