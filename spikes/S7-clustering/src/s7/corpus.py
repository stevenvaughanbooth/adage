"""S7 corpora: S2's labelled benchmark, and synthetic scale-ups of it.

Scale-ups reuse S2's 14 intent families and templates (spikes/S2-embeddings/data/gen_data.py) and
paraphrase them further: openers, closers, context tails, word drops and typos, and fresh IDs,
paths, and names. Noise is >= 60% of every scaled corpus and comes in two kinds:

* ``oneoff`` — content-dominated, one-off prompts (a frame with several random content words from
  the model's own pinned vocabulary). Geometrically diffuse; the correct answer is "no family".
* ``chatter`` — S2's noise templates (acknowledgements, trivia), paraphrased. These repeat, so they
  are *textually* dense: a density method will rightly find clusters here. They carry no
  consistent behaviour, which is what the purity step (Design §8.2.1 step 5) must catch.

Every prompt also carries a synthetic ``route`` — what the session did next — so the behavioural
split/merge can be measured. Route design (deliberately mixed):

* start / check / close / triage ticket share nouns and a ``{ticket}`` placeholder but lead to four
  different routes (split candidates if embeddings lump them).
* add_feature + refactor_code, and write_docs + explain_code, lead to the same route (merge
  candidates).
* update_dependencies is behaviourally split 50/50 by something the text cannot show (the repo's
  package manager): it must be judged impure and never trigger.
* Every family follows its route with p = 0.85; otherwise a random route. Noise follows a random
  route or none.
"""
from __future__ import annotations

import importlib.util
import json
import random
import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
S2 = ROOT.parent / "S2-embeddings"


def _load_s2_gen():
    spec = importlib.util.spec_from_file_location("s2_gen_data", S2 / "data" / "gen_data.py")
    mod = importlib.util.module_from_spec(spec)
    import sys
    sys.modules["s2_gen_data"] = mod
    spec.loader.exec_module(mod)
    return mod


G = _load_s2_gen()
FAMILY_KEYS = [f.key for f in G.FAMILIES]

ROUTES = {
    "start_ticket": "lookup",
    "check_ticket_status": "status",
    "close_ticket": "close",
    "triage_tickets": "triage-list",
    "fix_failing_test": "run-tests",
    "investigate_ci_failure": "ci-logs",
    "review_pr": "pr-diff",
    "add_feature": "read-arch",
    "refactor_code": "read-arch",
    "update_dependencies": None,  # 50/50 npm-outdated / pip-list: text cannot tell which
    "write_docs": "read-docs",
    "explain_code": "read-docs",
    "deploy_question": "deploy-runbook",
    "setup_environment": "setup-script",
}
ALL_ROUTES = sorted({r for r in ROUTES.values() if r} | {"npm-outdated", "pip-list"})
FOLLOW_P = 0.85


def route_for(rng: random.Random, family: str | None) -> str:
    if family is None:
        return rng.choice(ALL_ROUTES) if rng.random() < 0.5 else "none"
    r = ROUTES[family]
    if r is None:
        return "npm-outdated" if rng.random() < 0.5 else "pip-list"
    if rng.random() < FOLLOW_P:
        return r
    return rng.choice(ALL_ROUTES)


def behaviour_truth(family: str | None) -> str | None:
    """The behavioural family a perfect pipeline would emit: the family's route, or None when the
    family is noise or behaviourally impure."""
    if family is None:
        return None
    return ROUTES[family]


OPENERS = [
    "", "", "", "", "hey, ", "please ", "can you ", "ok so ", "quick one: ", "when you get a chance, ",
    "urgent: ", "hi -- ", "alright, ", "next up: ", "so ", "I need you to ", "could you ", "morning! ",
    "fyi ", "one thing: ",
]
CLOSERS = [
    "", "", "", "", " thanks", " please", " asap", " when you can", " -- ping me when done", ". thx",
    " before standup", " today", " for the release", " if you have time", ", cheers", " (low priority)",
    " -- thanks!", ", no rush",
]
CONTEXT = [
    "the customer reported it yesterday.", "this came up in the retro.", "product is asking about it.",
    "I'm out tomorrow so let's move fast.", "keep the change small.", "we talked about this on slack.",
    "there's a thread about it somewhere.", "it's been bugging me for weeks.", "legal wants it before friday.",
    "don't touch the payments code though.", "the last attempt got reverted.", "it's on the sprint board.",
    "sarah owns this area, loop her in.", "the dashboard looked odd this morning.", "we're in a code freeze next week.",
]

_TYPO_KEEP = re.compile(r"[{}]")


def perturb(rng: random.Random, text: str) -> str:
    words = text.split(" ")
    out = []
    for w in words:
        r = rng.random()
        if r < 0.04 and len(words) > 3:
            continue  # dropped word
        if r < 0.07 and len(w) > 3 and not _TYPO_KEEP.search(w):
            i = rng.randrange(len(w) - 1)
            w = w[:i] + w[i + 1] + w[i] + w[i + 2:]  # adjacent swap
        out.append(w)
    return " ".join(out)


def fresh_fill(rng: random.Random, template: str) -> str:
    """S2's slot pools, plus freshly generated IDs, paths and names for variety."""
    pkg, ver = rng.choice(G.PACKAGES)
    prefix = rng.choice(["W", "ENG", "BUG", "OPS", "SUP", "PROJ", "DATA", "INFRA"])
    parts = rng.choice(["src", "lib", "app", "pkg", "internal", "services", "web/src"])
    stem = rng.choice(["auth", "billing", "orders", "cart", "search", "users", "invoice", "queue", "cache"])
    ext = rng.choice(["py", "ts", "go", "rb", "rs", "tsx", "java"])
    verbs = ["compute", "load", "parse", "sync", "render", "apply", "fetch", "merge", "build", "check"]
    nouns = ["totals", "config", "tokens", "orders", "cache", "session", "invoice", "rates", "users", "events"]
    return template.format(
        ticket=f"{prefix}-{rng.randrange(1, 99999)}",
        pr=rng.randrange(1, 20000),
        path=f"{parts}/{stem}/{rng.choice(nouns)}.{ext}",
        branch=rng.choice(G.BRANCHES),
        pkg=pkg,
        ver=f"{rng.randrange(0, 30)}.{rng.randrange(0, 30)}.{rng.randrange(0, 30)}" if rng.random() < 0.5 else ver,
        service=rng.choice(G.SERVICES),
        fn=f"{rng.choice(verbs)}_{rng.choice(nouns)}",
        test=f"test_{rng.choice(nouns)}_{rng.choice(verbs)}",
        sha="".join(rng.choice("0123456789abcdef") for _ in range(rng.choice([7, 8, 12]))),
        url=rng.choice(G.URLS),
        env=rng.choice(G.ENV_VARS),
        error=rng.choice(G.ERRORS),
    )


def family_prompt(rng: random.Random, fam) -> str:
    core = fresh_fill(rng, rng.choice(fam.templates))
    text = rng.choice(OPENERS) + core + rng.choice(CLOSERS)
    if rng.random() < 0.25:
        text += " " + rng.choice(CONTEXT)
    return perturb(rng, text)


# One-off noise: frames dominated by random content words from the model's pinned vocabulary.
ONEOFF_FRAMES = [
    "{a} {b} {c}", "what about the {a} {b} and {c}", "{a} {b}, {c} {d}", "thinking of {a} {b} {c} {d}",
    "any idea on {a} {b} {c}", "{a} or {b}? {c} {d}", "notes: {a} {b} {c} {d} {e}", "{a} {b} {c}?",
    "how is {a} {b} related to {c}", "{a} {b} {c} {d}",
]


def load_vocab() -> list[str]:
    """Whole-word, alphabetic tokens from potion's tokenizer (pinned by digest), excluding every
    word that appears in any family template, so one-offs never secretly belong to a family."""
    tok = json.loads((ROOT / "models" / "potion-base-8m" / "tokenizer.json").read_text())
    fam_words = set()
    for f in G.FAMILIES:
        for t in f.templates:
            fam_words.update(re.findall(r"[a-z]+", t.lower()))
    words = [w for w in tok["model"]["vocab"] if w.isalpha() and w.isascii() and len(w) >= 4 and w not in fam_words]
    return sorted(words)


def oneoff_prompt(rng: random.Random, vocab: list[str]) -> str:
    frame = rng.choice(ONEOFF_FRAMES)
    return frame.format(**{k: rng.choice(vocab) for k in "abcde"})


def chatter_prompt(rng: random.Random) -> str:
    base = rng.choice(G.NOISE_TEMPLATES)
    base = fresh_fill(rng, base) if "{" in base else base
    return perturb(rng, rng.choice(G.NOISE_PREFIXES) + base + rng.choice(CLOSERS))


def s2_benchmark() -> list[dict]:
    """S2's labelled set as-is (922 rows, 25% noise), with synthetic routes attached."""
    rng = random.Random(7)
    rows = []
    for line in open(S2 / "data" / "prompts.jsonl"):
        r = json.loads(line)
        fam = r["family"]
        rows.append({
            "id": r["id"], "text": r["text"], "family": fam, "noise_kind": "chatter" if r["is_noise"] else None,
            "route": route_for(rng, fam), "repo": "r00",
        })
    return rows


def scaled(n: int, seed: int = 20260922, noise_share: float = 0.62, chatter_share: float = 0.2,
           repos: int | None = None) -> list[dict]:
    rng = random.Random(seed)
    vocab = load_vocab()
    repos = repos or max(1, n // 10_000)
    rows = []
    n_noise = int(round(n * noise_share))
    n_fam = n - n_noise
    for i in range(n_fam):
        fam = G.FAMILIES[i % len(G.FAMILIES)]
        rows.append({"text": family_prompt(rng, fam), "family": fam.key, "noise_kind": None})
    for i in range(n_noise):
        if rng.random() < chatter_share:
            rows.append({"text": chatter_prompt(rng), "family": None, "noise_kind": "chatter"})
        else:
            rows.append({"text": oneoff_prompt(rng, vocab), "family": None, "noise_kind": "oneoff"})
    rng.shuffle(rows)
    for i, r in enumerate(rows):
        r["id"] = f"p{i:07d}"
        r["route"] = route_for(rng, r["family"])
        r["repo"] = f"r{rng.randrange(repos):02d}"
    return rows
