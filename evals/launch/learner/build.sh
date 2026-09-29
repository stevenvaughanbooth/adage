#!/bin/bash
# build.sh: the adage binary and the learner's examples, fast release profile.
cd $(git -C "$(dirname "$0")" rev-parse --show-toplevel) || exit 1
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_INCREMENTAL=0 CARGO_PROFILE_RELEASE_LTO=false CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16
cargo build --release -p adage-cli --bin adage --locked -j3 2>&1 | grep -E "^error|^warning|Finished" -A7 | head -30
cargo build --release -p adage-learn --example ledger --example outcome_priors --locked -j3 2>&1 | grep -E "^error|^warning|Finished" -A7 | head -30
