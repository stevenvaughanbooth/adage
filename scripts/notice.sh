#!/bin/sh
# Generate the third-party NOTICE for the adage binary with cargo-about (plan §7 L2, L3).
#
#   scripts/notice.sh [output]        (default: NOTICE)
#
# Needs cargo-about: `cargo install --locked cargo-about --features cli`.
# Commit the regenerated NOTICE whenever dependencies change; dist ships it (and LICENSE) in every
# release archive.
set -eu
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$(dirname "$0")/.."
OUT=${1:-NOTICE}
command -v cargo-about >/dev/null 2>&1 || {
  echo "cargo-about not found: cargo install --locked cargo-about --features cli" >&2
  exit 1
}
cargo about generate --locked --fail -m crates/adage-cli/Cargo.toml -c about.toml -o "$OUT" about.hbs
# Registry license files can use CRLF or trailing spaces. Normalize only line endings and
# trailing whitespace so generated notices are reproducible and pass the repository's checks.
NORMALIZED=$(mktemp)
trap 'rm -f "$NORMALIZED"' EXIT
awk '{ sub(/[[:space:]]+$/, ""); print }' "$OUT" > "$NORMALIZED"
cat "$NORMALIZED" > "$OUT"
rm -f "$NORMALIZED"
echo "wrote $OUT"
# adage-exec and adage-train ship too; the NOTICE covers them only while every third-party crate
# they link is one adage links as well.
deps() { cargo tree --locked -p "$1" -e normal --prefix none --target all | sed 's/ (.*//' | grep -v '^adage' | sort -u; }
CLI_DEPS=$(mktemp)
trap 'rm -f "$CLI_DEPS"' EXIT
deps adage-cli > "$CLI_DEPS"
for bin in adage-exec adage-train; do
  extra=$(deps "$bin" | comm -13 "$CLI_DEPS" -)
  if [ -n "$extra" ]; then
    echo "$bin links crates adage does not; generate its notices too:" >&2
    echo "$extra" >&2
    exit 1
  fi
done
echo "adage-exec and adage-train link a subset of adage's crates: $OUT covers them"
