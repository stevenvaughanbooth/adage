#!/bin/sh
# Build the Adage binaries for this machine and stage them, with checksums, into the Claude Code
# plugins: adage into plugin/claude-code, adage-exec into plugin/claude-code-exec.
set -e
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$(dirname "$0")/.."
cargo build --release --locked -p adage-cli -p adage-exec
T=$(rustc -vV | sed -n 's/^host: //p')
# SHA-256 lines as shasum prints them: sha256sum (Linux) or shasum (macOS).
if command -v sha256sum >/dev/null 2>&1; then SUM="sha256sum"; else SUM="shasum -a 256"; fi
mkdir -p "plugin/claude-code/dist/$T"
cp target/release/adage "plugin/claude-code/dist/$T/adage"
(cd plugin/claude-code/dist && find . -name adage -type f | sed 's|^\./||' | xargs $SUM > SHA256SUMS)
echo "staged plugin/claude-code/dist/$T/adage"; cat plugin/claude-code/dist/SHA256SUMS
# The adage-exec plugin (Track C §3.2): the executing binary, staged the same way.
mkdir -p "plugin/claude-code-exec/dist/$T"
cp target/release/adage-exec "plugin/claude-code-exec/dist/$T/adage-exec"
(cd plugin/claude-code-exec/dist && find . -name adage-exec -type f | sed 's|^\./||' | xargs $SUM > SHA256SUMS)
echo "staged plugin/claude-code-exec/dist/$T/adage-exec"
