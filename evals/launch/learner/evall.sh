#!/bin/bash
# evall.sh <binary> <tag> [repos...]: adage eval (instincts off) on each repo with its LOO priors
# and snapshot root; JSON to ev-<repo>-<tag>.json, text to ev-<repo>-<tag>.txt.
D=${LD:-/tmp/adage-learner}
bin=$1; tag=$2; shift 2
repos=${@:-click attrs marshmallow vue-core urfave-cli}
cd "$D" || exit 1
for r in $repos; do
  extra=()
  [ -f "params-$r.json" ] && extra+=(--params "params-$r.json")
  [ -d "roots/$r" ] && extra+=(--sandbox-dir "roots/$r")
  ./ev.sh "$bin" "$r" "${extra[@]}" > "ev-$r-$tag.txt" 2>&1
  ./ev.sh "$bin" "$r" "${extra[@]}" --json > "ev-$r-$tag.json" 2>/dev/null
  echo "== $r"; sed -n '4,20p' "ev-$r-$tag.txt"
done
