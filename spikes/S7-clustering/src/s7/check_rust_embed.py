"""Does the production Rust embedder (crates/adage-intent: normaliser, WordPiece, pooling) agree
with the Python reference S2 and S7 measured (S2's normalize.py + model2vec)?

Needs `adage models install` to have been run into ADAGE_HOME (run_all.sh does it) and the
`embed` example built. Writes results/rust_embed_check.json."""
from __future__ import annotations

import json
import subprocess
import sys
from pathlib import Path

import numpy as np

from s7 import bench, corpus, embed

ROOT = Path(__file__).resolve().parents[2]
REPO = ROOT.parents[1]


def main():
    model_dir = sys.argv[1]
    rows = corpus.s2_benchmark() + corpus.scaled(10_000, seed=bench.SEED)[:3000]
    texts = [r["text"] for r in rows]
    exe = REPO / "target" / "release" / "examples" / "embed"
    p = subprocess.run([str(exe), model_dir], input="\n".join(json.dumps(t) for t in texts) + "\n",
                       capture_output=True, text=True, check=True)
    out = [json.loads(l) for l in p.stdout.splitlines()]
    py_norm = [embed.normalize(t) for t in texts]
    same_norm = sum(a == o["norm"] for a, o in zip(py_norm, out))
    # Compare vectors on identical normalised input, isolating tokenizer + pooling.
    ref = embed.embed([o["norm"] for o in out])
    cos = []
    for o, r in zip(out, ref):
        if o["vec"] is None:
            continue
        v = np.asarray(o["vec"], dtype=np.float32)
        cos.append(float(v @ r))
    cos = np.asarray(cos)
    import re
    one_letter = re.compile(r"\b[A-Z][A-Z0-9]{0,9}-\d{1,7}\b")
    diffs = [(t, a, o["norm"]) for t, a, o in zip(texts, py_norm, out) if a != o["norm"]]
    # The one intended difference: one-letter ticket prefixes (W-4821) become {ticket} too.
    explained = sum(one_letter.sub("{ticket}", a) == r for _, a, r in diffs)
    diffs = diffs[:4]
    res = {
        "n": len(texts),
        "normaliser_identical": same_norm,
        "normaliser_differences_explained_by_one_letter_ticket_fix": explained,
        "normaliser_differences_examples": diffs,
        "embedding_cosine_min": round(float(cos.min()), 6),
        "embedding_cosine_p01": round(float(np.quantile(cos, 0.01)), 6),
        "embedding_cosine_mean": round(float(cos.mean()), 6),
        "embedding_identical_share_1e-5": round(float((cos > 1 - 1e-5).mean()), 4),
    }
    print(json.dumps(res, indent=1))
    (ROOT / "results" / "rust_embed_check.json").write_text(json.dumps(res, indent=1))


if __name__ == "__main__":
    main()
