#!/bin/sh
# Build the Adage binaries for this machine and stage them, with checksums, into the Claude Code
# plugin: adage, adage-exec and adage-train in plugin/claude-code/dist/<host triple>/ (one plugin,
# three binaries: plan A8, onboarding plan O1). Its session-start shim verifies each against
# dist/SHA256SUMS and links it into the plugin's data directory. The release stages every target
# the same way from dist's archives (scripts/stage-plugin.mjs).
#
#   plugin/build.sh [--no-build] [--from <dir>] [--dist <dir>] [--target <triple>]
#
#   --no-build         stage what --from already holds, without building (a dry run of the staging)
#   --from <dir>       where the binaries are (default target/release)
#   --dist <dir>       where to stage them (default plugin/claude-code/dist)
#   --target <triple>  the directory under dist/ (default the host triple, from `rustc -vV`)
#
# Nothing is written unless every binary is there: adage and adage-train always, adage-exec
# except on Windows (execution needs an OS sandbox Windows lacks).
set -e
abs() { case "$1" in /*) printf '%s' "$1" ;; *) printf '%s/%s' "$(pwd)" "$1" ;; esac; }
BUILD=1; FROM=""; DIST=""; T=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-build) BUILD="" ;;
    --from) FROM=$(abs "$2"); shift ;;
    --dist) DIST=$(abs "$2"); shift ;;
    --target) T="$2"; shift ;;
    *) echo "build.sh: unknown argument $1 (see the header of plugin/build.sh)" >&2; exit 2 ;;
  esac
  shift
done
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$(dirname "$0")/.."
REPO=$(pwd)
FROM=${FROM:-$REPO/target/release}; DIST=${DIST:-$REPO/plugin/claude-code/dist}
if [ -n "$BUILD" ]; then cargo build --release --locked -p adage-cli -p adage-exec -p adage-train; fi
[ -n "$T" ] || T=$(rustc -vV | sed -n 's/^host: //p')
[ -n "$T" ] || { echo "build.sh: no target triple (pass --target, or put rustc on PATH)" >&2; exit 1; }
X=""; NAMES="adage adage-exec adage-train"
case "$T" in *-windows-*) X=.exe; NAMES="adage adage-train" ;; esac
for n in $NAMES; do
  pkg=$n; [ "$n" = adage ] && pkg=adage-cli
  [ -f "$FROM/$n$X" ] || { echo "build.sh: no $n$X in $FROM; build it first (cargo build --release -p $pkg)" >&2; exit 1; }
done
# SHA-256 lines as shasum prints them: sha256sum (Linux) or shasum (macOS).
if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
mkdir -p "$DIST/$T"
for n in $NAMES; do cp "$FROM/$n$X" "$DIST/$T/$n$X"; chmod 755 "$DIST/$T/$n$X"; done
# One SHA256SUMS over every binary staged under dist/ (each target a developer staged), in the
# `<sha256>  <triple>/<name>` lines the shim greps; written aside, checked, then moved in place.
(
  cd "$DIST"
  find . -mindepth 2 -maxdepth 2 -type f \( -name adage -o -name adage-exec -o -name adage-train \
    -o -name adage.exe -o -name adage-exec.exe -o -name adage-train.exe \) \
    | sed 's|^\./||' | LC_ALL=C sort | xargs $SUM >.SHA256SUMS.tmp
  $SUM -c .SHA256SUMS.tmp >/dev/null
  mv .SHA256SUMS.tmp SHA256SUMS
)
# The binaries embed third-party data (the MIT embedding model): its notices travel with them.
cp "$REPO/LICENSE" "$REPO/NOTICE" "$DIST/"
echo "staged $DIST/$T: $NAMES"
cat "$DIST/SHA256SUMS"
