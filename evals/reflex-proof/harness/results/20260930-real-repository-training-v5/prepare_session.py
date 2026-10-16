import json,os,sys
from pathlib import Path
sys.path.insert(0,'evals/reflex-proof/harness')
import cohort_v5 as c
root=Path('/private/tmp/adage-real-repo-gate-v1/cohorts/v5');task=sys.argv[1]
# Fixture loading sets a preparation-only cache variable; restore the caller environment
# before policy generation, so actual native settings remain byte-for-byte comparable.
previous=dict(os.environ)
manifest=root/'sessions'/task/'manifest.json'
if manifest.exists():
 value=json.loads(manifest.read_text())
 if value.get('protocol_revision')==5: print(manifest);raise SystemExit(0)
else:
 value=c.gate.prepare_session(root,task,c.MODEL,native_probe=False,setup_checkout=c.pairs.frozen_setup)
os.environ.clear();os.environ.update(previous)
value['shared_sources'][str(Path(__file__))]=c.gate.sha(Path(__file__))
print(c.finalize_recipe(value,root,False))
