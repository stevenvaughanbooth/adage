#!/bin/bash
# report.sh <tag>: one Ŝ line per repository, pooled Python, and dead-end routes only.
D=${LD:-/tmp/adage-learner}
cd "$D" || exit 1
t=$1
for r in click attrs marshmallow vue-core urfave-cli; do
  echo "$r: $(python3 shat.py "ds/rb-$r" "ev-$r-$t.json" --quiet | tail -1)"
done
echo "python pooled: $(python3 shat.py --pair ds/rb-click ev-click-$t.json --pair ds/rb-attrs ev-attrs-$t.json --pair ds/rb-marshmallow ev-marshmallow-$t.json --quiet | tail -1)"
echo "python pooled, dead ends: $(python3 shat.py --pair ds/rb-click ev-click-$t.json --pair ds/rb-attrs ev-attrs-$t.json --pair ds/rb-marshmallow ev-marshmallow-$t.json --routes '^dead-end' --quiet | tail -1)"
echo "python pooled, v2: $(python3 shat.py --pair ds/rb-click ev-click-$t.json --pair ds/rb-attrs ev-attrs-$t.json --pair ds/rb-marshmallow ev-marshmallow-$t.json --v2 --quiet | tail -1)"
