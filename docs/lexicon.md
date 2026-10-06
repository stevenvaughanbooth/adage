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
| What a reflex does when it matches | **response**; **directive** for advice, **action** for execution | directive used for every behavior |
| A versioned collection of reflexes | **reflex set** | track (product explanations) |
| An unqualified behavior proposed by learning | **candidate** | |
| Internal files and storage identifiers | **tracks** | (unchanged implementation term) |
| Signed, shareable artefact | **pack** | shared track |
| An instinct's route, with blanks its detector fills from a repository's files | **template** | |
| The instinct packs Adage ships (`adage/python-env`, …) | **base instincts**, **base packs** | core instincts, built-in routes |
| A repository's list of the packs it offers, pinned by digest | **marketplace** (`.adage/marketplace.toml`) | |
| Holdout arm | **control** | holdout |
| Traces of similar tasks | **family** (of similar tasks) | cluster |
| What a family's traces had in common, said at the next such task | **procedure card**: a reflex, whose response is the card | recipe, playbook |
| The web view (`adage map`) | **the map** | signal box |

## Strength (formerly aspects)

| Colour | Word | Meaning |
| --- | --- | --- |
| Green | **fire** | Do this: strong evidence |
| Yellow | **prime** | Consider this: suggestion |
| Red | **inhibit** | Don't do this: a known dead end |

Strength describes evidence and how advice is phrased. **Fire does not mean a script is allowed
to execute.** Accepting evidence and granting execution are separate decisions.

## Execution status

| Status | Meaning |
| --- | --- |
| **Suggest** | The agent receives applicable guidance; an execution grant is absent or automatic execution is off |
| **Automatic** | A matching local grant (or explicit inert-observe policy) and execution enabled make the route eligible, subject to runtime checks |
| **Needs review** | Saved execution authority no longer matches the executable or current policy |
| **Needs recovery** | An interrupted E2 operation pauses further application in this worktree until inspected and acknowledged |
| **Grant saved** | A grant exists, but the current view cannot validate that repository's scripts |
| **Disabled** | A kill switch prevents interventions |
| **Observe** | New sessions default to quiet controls; existing session assignments and explicit evaluation arm overrides still apply |
| **Inactive** | The route is not active in the runtime index |
| **Unavailable** | Required execution authority or sandbox state cannot be read or established |

An **execution receipt** records what actually happened. **Unknown** means an attempt may have
had an effect without a confirmed terminal receipt; it is never rendered as success or retried
automatically. [How Adage works](how-it-works.md) defines the product contract behind these terms.

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
| | `adage train cards` | Replay the commands of a repository's procedure cards on a clean checkout |
| | `adage packs` | The packs Adage binds: base instincts and added packs; `on`, `off`, `show` |
| | `adage marketplace` | Where packs come from: the adage marketplace, built in; `show`, `update` |
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
