#!/bin/bash
# The offline embedding ablation: adage eval embeddings, per embedder.
S=/tmp/claude-0/-home-user-adage/d6ae021f-492b-5125-b4ff-8e8a0676a66b/scratchpad
A=${ADAGE_BIN:-/home/user/adage/target/debug/adage}
mkdir -p $S/empty-models $S/out
M="$S/replay/click.json $S/replay/attrs.json $S/replay/marshmallow.json $S/replay/vue-core.json"
for e in hashed bundled potion-base-8m potion-code-16m-v2; do
  case $e in
    hashed|bundled) dir=$S/empty-models; sel=$e;;
    *) dir=$S/home1/models; sel=$e;;
  esac
  [ $e = bundled ] && sel=auto
  echo "=== $e"
  ADAGE_EMBED_MODEL=$sel $A eval embeddings $M --models $dir --json > $S/out/embed-$e.json
  ADAGE_EMBED_MODEL=$sel $A eval embeddings $M --models $dir
done
