#!/bin/sh
# Type-check and lint (warnings denied) the workspace for other targets, without their C
# toolchains.
#
#   scripts/cross-check.sh [target ...]     (default: every release target)
#
# `cargo clippy`, like `cargo check`, never links, but the tree-sitter build scripts still compile
# their C sources for the target, which needs that target's headers (MSVC SDK, glibc, musl). This
# script points the
# `cc` crate at stub compiler and archiver scripts that create empty outputs, so what is proven is
# exactly this: all Rust code, including every `cfg(windows)` / `cfg(unix)` branch, compiles for
# the target. Linking and running are proven only by CI on native runners.
set -eu
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
cd "$(dirname "$0")/.."

TARGETS=${*:-"x86_64-pc-windows-msvc x86_64-pc-windows-gnu x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin"}

STUB=$(mktemp -d)
trap 'rm -rf "$STUB"' EXIT
cat >"$STUB/cc" <<'EOF'
#!/bin/sh
# Stub C compiler: create each requested output (-o FILE, -FoFILE, /FoFILE) empty.
prev=""
for a in "$@"; do
  case "$a" in
    -Fo*|/Fo*) : >"${a#???}" ;;
  esac
  if [ "$prev" = "-o" ]; then : >"$a"; fi
  prev="$a"
done
exit 0
EOF
cat >"$STUB/ar" <<'EOF'
#!/bin/sh
# Stub archiver: create the named archive (ar-style or lib.exe-style) empty.
for a in "$@"; do
  case "$a" in
    -out:*|-OUT:*|/out:*|/OUT:*) : >"${a#*:}" ;;
    *.a|*.lib) : >"$a"; exit 0 ;;
  esac
done
exit 0
EOF
chmod +x "$STUB/cc" "$STUB/ar"

for t in $TARGETS; do
  rustup target list --installed | grep -qx "$t" || rustup target add "$t"
  v=$(echo "$t" | tr '-' '_')
  echo "== cargo clippy --target $t"
  env "CC_$v=$STUB/cc" "AR_$v=$STUB/ar" "CFLAGS_$v=" \
    cargo clippy --workspace --all-targets --locked --target "$t" -- -D warnings
done
