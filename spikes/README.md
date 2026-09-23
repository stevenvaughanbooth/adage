# Spikes

Time-boxed experiments that settle architecture decisions by measurement rather than argument.
Each spike answers one question, informs named decisions in *Adage — Architecture* (§15.3), and
ends with a recommendation.

## Board

| Spike | Question | Informs | Wave | Status |
| --- | --- | --- | --- | --- |
| [S1 · Hook latency](S1-hook-latency/SPIKE.md) | Can a hook client spawn, read stdin, and round-trip a socket inside the budget on macOS? | A2, A3 | 1 | ✅ Pass — 2.16 ms p95 exec-form + socket; pre-run binary at SessionStart |
| [S2 · Embeddings](S2-embeddings/SPIKE.md) | Are static embeddings good enough for intent families; what do transformer embeddings cost? | A6, A1 | 1 | ✅ Done — static 0.852 vs bge-small 0.930 top-1; static 17 ms load in Rust |
| [S3 · Daemonless state](S3-daemonless-state/SPIKE.md) | Can per-session state files and append-only spools replace a daemon's memory correctly? | A1, A5 | 1 | ✅ Pass — 0.54 ms p95 decide at 16 sessions; no daemon needed for state |
| [S4 · Directive wording](S4-directive-wording/SPIKE.md) | Which directive wordings are followed, and which trip injection defences? | D§9, F3 | 1 | ⏸ Partial (21 valid runs) — superseded in practice by the demo: the factual wording was followed in 36/36 Adage runs with zero suspicion or refusal language |
| [S5 · Plugin packaging](S5-plugin-packaging/SPIKE.md) | How do plugin roots, data dirs, SessionStart ordering, async hooks, and plugin enumeration behave? | A8, F4, F5 | 1 | ✅ Done — all 8 questions measured; recipe in FINDINGS |
| [S6 · SSH signing](S6-ssh-signing/SPIKE.md) | Can tracks be signed with agent-held keys and verified natively with namespaces? | A7 | 1 | ✅ Native — signs via ssh-agent and verifies in Rust |
| [S7 · Clustering at scale](S7-clustering/SPIKE.md) | Which clustering method is deterministic and scales to hundreds of thousands of prompts? | Intent pipeline | 2 | ✅ Pass on determinism, scale, purity; ❌ ARI ≥ 0.8 unreachable (supervised oracle 0.75) — **exact kNN + deterministic Louvain**, per repo: ARI 0.42, 300k in 8.0 min / 768 MB global, 17 s per repo; purity step lifts directive precision 0.57 → 0.87 |
| [S8 · Normaliser coverage](S8-normaliser/SPIKE.md) | How far do tree-sitter-bash and per-command grammars get on real transcripts? | Normaliser | 2 | ✅ Pass — 98.2% normalised, 315 fixtures; promoted to adage-core |

## Conventions

Every spike folder contains:

- `SPIKE.md` — the brief: question, decision it informs, hypothesis, method, pass/fail criteria, budget.
- `FINDINGS.md` — written at the end: what was measured, the numbers, the verdict, the recommendation,
  and anything surprising.
- Code and scripts needed to reproduce every number, with a one-line command to rerun.
- `results/` — raw measurement output (small text/CSV/JSON only).

Rules:

- **Reproducible.** Every number in `FINDINGS.md` comes from a script in the folder.
- **Contained.** Spikes write only inside their own folder. No changes to global harness settings,
  no globally installed plugins, no commits — commits are made by the coordinator.
- **Large files stay out of git.** Models, sandboxes, and build output go in git-ignored
  `models/`, `sandbox/`, `target/`, `.venv/`.
- **Honest.** A spike that disproves its hypothesis is a success. Report what the data says.

## Environment

Apple M3 Pro, 18 GB, macOS. Rust 1.98 (rustup, user-level), uv 0.12, Node 26, Docker 29,
Claude Code 2.1.221.
