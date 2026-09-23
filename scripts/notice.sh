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
echo "wrote $OUT"
