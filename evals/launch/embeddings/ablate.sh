#!/bin/bash
# Replay ablation: embedder x case store x mode, the four RepoBench v2 manifests (control arm).
S=/tmp/claude-0/-home-user-adage/d6ae021f-492b-5125-b4ff-8e8a0676a66b/scratchpad
A=${ADAGE_BIN:-$S/adage-abl}
mkdir -p $S/empty-models $S/abl
for e in ${EMBEDDERS:-hashed bundled potion-base-8m potion-code-16m-v2}; do
  case $e in
    hashed) dir=$S/empty-models; sel=hashed;;
    bundled) dir=$S/empty-models; sel=auto;;
    *) dir=$S/home1/models; sel=$e;;
  esac
  for c in ${CASES:-off on ungated}; do
    for r in click attrs marshmallow vue-core; do
      for m in post-hoc pre-emptive; do
        f=$S/abl/$e-$c-$r-$m.json
        [ -s $f ] && continue
        ADAGE_EMBED_MODEL=$sel $A eval replay-savings $S/replay/$r.json --policy learned --arm control --mode $m \
          --models $dir --cases $c --json > $f 2> $f.err || echo "FAILED $f: $(tail -2 $f.err)"
      done
    done
    echo "done $e $c"
  done
done
