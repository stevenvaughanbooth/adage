"""Score the learned-outcome reflex learner in the common harness, next to U11.

Test set: `corpus.held_out(model="haiku")` (v1+v2 held-out control sessions; the drivers' set).
Protocols: `split` (the repository's own training sessions) and `loro` (every session of the
other repositories). Modes: post-hoc (PostToolUse) and predictive (PreToolUse, no hindsight).

  python3 run_harness.py -> results/harness.json
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parent))
sys.path.insert(0, str(HERE))

from harness import load, score  # noqa: E402
from harness.drivers import loro, split  # noqa: E402
from harness.leakage import scan_memory  # noqa: E402
from harness.policies.u11 import U11  # noqa: E402

import policy as P  # noqa: E402


def main():
    corpus = load(include_dx=False)  # pinned: RepoBench v1+v2 only (test set 6a795a362ccb)
    test = corpus.held_out(model="haiku")
    res = {}

    def keep(name, r):
        res[name] = r.to_json()
        res[name]["headline"] = r.headline()
        print(r.headline(), flush=True)

    keep("U11 post-hoc", score(U11("post-hoc"), test, name="U11 post-hoc"))
    keep("U11 predictive split", split(lambda tr: U11("predictive", tr), corpus, model="haiku",
                                       name="U11 predictive [split]"))
    # two pre-declared reflex filters: min_rate 0.7 (the lab's precision target) and 0.5
    for mr in (0.5, 0.7):
        for mode in ("post-hoc", "predictive"):
            fit = P.fitter(mode, min_rate=mr)
            keep(f"learned {mode} split min_rate={mr}",
                 split(fit, corpus, model="haiku", name=f"learned-outcomes {mode} [split] min_rate={mr}"))
            keep(f"learned {mode} loro min_rate={mr}",
                 loro(fit, corpus, model="haiku", name=f"learned-outcomes {mode} [loro] min_rate={mr}"))
    keep("learned post-hoc loro min_rate=0.7 p_follow=0.5",
         loro(P.fitter("post-hoc", min_rate=0.7), corpus, model="haiku",
              name="learned-outcomes post-hoc [loro] min_rate=0.7 p_follow=0.5", p_follow=0.5))
    keep("learned post-hoc split p_follow=0.5",
         split(P.fitter("post-hoc"), corpus, model="haiku", name="learned-outcomes post-hoc [split] p_follow=0.5",
               p_follow=0.5))
    mem = []
    res["reflexes"] = {}
    for repo in sorted({s.repo for s in test}):
        pol = P.LearnedOutcomePolicy(corpus.train(repo=repo))
        mem += pol.memory_texts()
        res["reflexes"][repo] = [{"response": r["response"][:200], "support": r["support"], "rate": round(r["rate"], 2)}
                                 for r in pol.reflexes]
    res["scan_memory"] = str(scan_memory(mem, test, corpus.train()))
    (HERE / "results").mkdir(exist_ok=True)
    (HERE / "results" / "harness.json").write_text(json.dumps(res, indent=1, default=str))


if __name__ == "__main__":
    main()
