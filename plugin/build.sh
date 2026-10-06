#!/bin/sh
# Build the Adage binaries for this machine and stage them, with checksums, into the Claude Code
# plugin: adage and adage-exec both in plugin/claude-code (plan A8: one plugin; its session-start
# shim runs adage-exec while execution is on). The retired plugin/claude-code-exec alias bundles
# nothing.
set -e
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$(dirname "$0")/.."
cargo build --release --locked -p adage-cli -p adage-exec
T=$(rustc -vV | sed -n 's/^host: //p')
# SHA-256 lines as shasum prints them: sha256sum (Linux) or shasum (macOS).
if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
mkdir -p "plugin/claude-code/dist/$T"
cp target/release/adage target/release/adage-exec "plugin/claude-code/dist/$T/"
(cd plugin/claude-code/dist && find . -type f \( -name adage -o -name adage-exec \) | sed 's|^\./||' | sort | xargs $SUM > SHA256SUMS)
echo "staged plugin/claude-code/dist/$T/adage and adage-exec"; cat plugin/claude-code/dist/SHA256SUMS
# A stage from before A8 must not linger in the alias.
rm -rf plugin/claude-code-exec/dist
# The binaries embed third-party data (the MIT embedding model): its notices travel with them.
cp LICENSE NOTICE plugin/claude-code/dist/
