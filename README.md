# Adage

**Agentic DAG Engine** — put your agent on rails.

Adage learns the routes coding agents take to get work done, packages the good ones as shareable
**tracks**, and at each decision point tells the agent — deterministically — what to do next. It never
executes anything itself: the agent runs every command, through its own tools, under its harness's
own permissions.

## Status

Architecture phase. Nothing here is ready to use.

| Phase | Document | State |
| --- | --- | --- |
| 1 · Design | [Adage — Design Spec](docs/Adage%20—%20Design%20Spec.md) | Draft 6 — decisions closed except O9 (IP and data policy) |
| 2 · Architecture | [Adage — Architecture](docs/Adage%20—%20Architecture.md) | Draft 2 — reviewed; spikes in progress |
| 3 · Implementation plan | — | Not started |
| 4 · Slices | — | Not started |

## Layout

| Path | Contents |
| --- | --- |
| `docs/` | Current design and architecture |
| `docs/archive/` | Earlier design iterations (Interaction Compiler, Interaction Graph Compiler) |
| `spikes/` | Time-boxed experiments that settle architecture decisions — see [the board](spikes/README.md) |
| `research/poc/` | Early TypeScript proof of concept: transcript importer, shell normaliser, predictability analysis |
