#!/bin/bash
# loop.sh <tag>: copy the built binaries, fit LOO priors, ledgers over the training traces, replay
# every repo, and the interim Ŝ per repo.
D=${LD:-/tmp/adage-learner}
W=$(git -C "$(dirname "$0")" rev-parse --show-toplevel)
tag=$1
cd "$D" || exit 1
cp "$W/target/release/adage" "./adage-$tag"
cp "$W/target/release/examples/ledger" "./ledger-$tag"
cp "$W/target/release/examples/outcome_priors" ./outcome_priors
python3 loo.py ./outcome_priors > "loo-$tag.txt" 2>&1 || { cat "loo-$tag.txt"; exit 1; }
for r in click attrs marshmallow vue-core urfave-cli; do
  python3 led.py "./ledger-$tag" "$r" "led-$r-$tag.json" --root "roots/$r" --priors "priors-$r.json" 2>/dev/null | grep -E "sessions|VALUED|ROUTE|TESTMAP|CARD" > "led-$r-$tag.txt"
done
./evall.sh "./adage-$tag" "$tag" > "evall-$tag.txt" 2>&1
for r in click attrs marshmallow vue-core urfave-cli; do
  echo "== $r"; cat "led-$r-$tag.txt"; python3 shat.py "ds/rb-$r" "ev-$r-$tag.json"
done
