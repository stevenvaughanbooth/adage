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
| `adage learn` | `adage condition` | `learn` stays as a documented alias; help reads "condition (learn)" |
| `adage-train run` | `adage train` | Grow reflexes from a skill |
| `adage review promote` / `demote` / `reject` | `adage review strengthen` / `weaken` / `extinguish` | |
| `adage graph --serve` | `adage map` | |
| `adage exec enable` / `review allow-exec` | `adage delegate` / `adage review delegate` | Reflexes that act |
| `adage watch`, `doctor`, `init`, `share`, `add`, `eval` | unchanged | Already fit |

## Rules

- Old verbs and state names remain hidden aliases for one release, with a one-line deprecation
  note on stderr (never on the hook path).
- On-disk formats keep their existing field values (`active`, `probation`, `green`, …) so
  existing tracks, packs and review files still read. The new words are presentation. A later
  format version may migrate them.
- The directive wrapper `<adage-signal …>` stays readable in old traces.
- Where a themed word is less clear than a plain one in a specific sentence, the plain one wins
  (Design Spec §17.3).
