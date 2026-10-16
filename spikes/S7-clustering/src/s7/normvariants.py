"""Does the normaliser move the ceiling? Oracle ARI and top-1 under normalisation variants.
Writes results/norm_variants.json."""
from __future__ import annotations

import json
import re
from pathlib import Path

from s7 import corpus, embed
from s7.ceiling import ceiling

ROOT = Path(__file__).resolve().parents[2]
PH = re.compile(r"\{([a-z_]+)\}")


def first_sentence(t: str) -> str:
    parts = re.split(r"(?<=[.!?])\s+|\n+| -- ", t.strip())
    return parts[0] if parts and parts[0] else t


VARIANTS = {
    "s2": lambda t: embed.normalize(t),
    "s2_bare_placeholders": lambda t: PH.sub(r"\1", embed.normalize(t)),
    "s2_drop_placeholders": lambda t: " ".join(PH.sub(" ", embed.normalize(t)).split()),
    "s2_first_sentence": lambda t: first_sentence(embed.normalize(t)),
    "raw": lambda t: t,
}


def main():
    out = {}
    for cname, rows in [("s2_benchmark", corpus.s2_benchmark()), ("scaled_10k", corpus.scaled(10_000))]:
        for v, f in VARIANTS.items():
            E = embed.embed([f(r["text"]) for r in rows])
            out[f"{cname}/{v}"] = c = ceiling(rows, E)
            print(cname, v, c["top1"], c["oracle"]["ari"], c["oracle"]["purity"])
    (ROOT / "results" / "norm_variants.json").write_text(json.dumps(out, indent=1))


if __name__ == "__main__":
    main()
