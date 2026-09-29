# Adage lexicon

> Status: approved 2026-09-28. **Agent traces become reflexes.** Every user-facing word, from the
> CLI and TUI to the map page, docs and review screens, follows this table. Text the *agent* sees
> (directive templates) stays plain and factual: the theme is for people, not the model.

The theme is the nervous system. Traces are experience. Reflexes are conditioned responses learned
from them. Instincts are innate responses that ship with Adage.

## Nouns

| Concept | Word | Replaces |
| --- | --- | --- |
| A session transcript | **trace** | session, transcript (in user copy) |
| A learned route | **reflex** | route, learned route |
| A shipped route | **instinct** | built-in route |
| Trigger → response path, in docs | **reflex arc** | route (in explanations) |
| What starts a reflex | **stimulus** (docs), **trigger** (CLI) | trigger |
| What the reflex tells the agent | **response** (docs), **directive** (CLI, where precision matters) | directive |
| Files on disk | **tracks** | (unchanged) |
| Signed, shareable artefact | **pack** | shared track |
| Holdout arm | **control** | holdout |
| The web view (`graph --serve`) | **the map** | signal box |

## Strength (formerly aspects)

| Colour | Word | Meaning |
| --- | --- | --- |
| Green | **fire** | Do this: strong evidence |
| Yellow | **prime** | Consider this: suggestion |
| Red | **inhibit** | Don't do this: a known dead end |

## Lifecycle (formerly route states)

| State | Word |
| --- | --- |
| draft | **nascent** (waits for review) |
| probation | **forming** |
| active | **firm** |
| stale | **dormant** |
| retired | **extinguished** |
| derailed (judgement) | **ignored** |
| followed-and-failed | **misfired** |

## Verbs

| Command | Themed | Notes |
| --- | --- | --- |
| `adage survey` | `adage sense` | Read-only: which reflexes your traces would form |
| `adage learn` | `adage condition` | Removed: `learn` is gone |
| `adage-train run` | `adage train` | Compile a skill into reflexes (`adage train <SKILL.md>`); skill-steered sessions are `plan`, `--plan`, `report` |
| | `adage train accept` | Install a compiled skill's pack, as reviewed (the person accepts the review) |
| `adage review promote` / `demote` / `reject` | `adage review strengthen` / `weaken` / `extinguish` | |
| `adage graph --serve` | `adage map` | |
| `adage exec enable` / `review allow-exec` | `adage delegate` / `adage review delegate` | Reflexes that act |
| `adage watch`, `doctor`, `init`, `share`, `add`, `eval` | unchanged | Already fit |

## Rules

Adage has not been released, so there is no compatibility baggage (decision 2026-09-28):

- **No aliases.** Old verbs (`survey`, `learn`, `promote`, `demote`, `reject`, `graph --serve`,
  `exec enable`, `allow-exec`) are removed, not deprecated. `condition` is the only verb for
  learning.
- **On-disk formats migrate.** Track, pack, index, review and spool fields use the new words
  (`firm`, `forming`, `nascent`, `dormant`, `extinguished`; `fire`, `prime`, `inhibit`). No
  reader for the old values is kept. Committed fixtures and datasets are rewritten once by a
  migration script, which is then deleted.
- **Old formats go.** v1 tracks (`--format v1`, the v1 upgrader path in `add`) and the
  `<adage-signal>` wrapper are replaced by packs and a new wrapper, `<adage-reflex>`. Recorded
  traces in datasets are rewritten to the new wrapper, and fidelity baselines are re-recorded.
- **Agent-facing text stays plain and factual.** The theme is for people, not the model.
- Where a themed word is less clear than a plain one in a specific sentence, the plain one wins
  (Design Spec §17.3).
