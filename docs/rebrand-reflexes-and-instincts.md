# Rebrand: agent traces become reflexes

> Status: approved 2026-09-28. This brief is self-contained: it tells anyone working on Adage,
> human or agent, what changed in the brand and vocabulary, and the rules for applying it. The
> implementation is in progress on branch `claude/repo-maturity-roadmap-l95ohs`. There, the
> canonical word list is `docs/lexicon.md` and the brand system is Design Spec §17.

## The pitch

**Agent traces become reflexes.**

Adage reads the traces coding agents leave behind (Claude Code, OpenCode and Codex session
transcripts), finds the paths that actually get work done, and compiles them into **reflexes**:
small, typed, deterministic responses with their evidence attached. At each decision point, the
matching reflex tells the agent what to do next, so the agent spends turns on the task instead of
rediscovering the repository.

"Put your agent on rails" is retired as the tagline. It described the effect; the new line
describes the mechanism.

## The units

| Unit | What it is | Where it comes from |
| --- | --- | --- |
| **Reflex** | A learned response: a stimulus (a prompt that names a work item, a failed test run, an edit to `schema/*.json`), plus a response (run this, don't run that, read these files), plus typed inputs and evidence | Mined from real traces (`adage condition`), or grown on purpose from a skill (`adage train`: Adage drives your agent headlessly on a `SKILL.md`, `CONTRIBUTING.md` or runbook in a sandboxed clone, and compiles the sessions) |
| **Instinct** | An innate response that ships with Adage and works on day 0, with no history | Detected from the repository's own files: Makefile, `package.json`, CI config, lockfiles, generated-file markers |

Both are **compiled skills**. A skill costs inference every time an agent reads it; a reflex is
compiled once, is deterministic, can be reviewed, and carries its evidence. A learned reflex
always overrides the matching instinct. Instincts are never stronger than *prime* on their own.

Reflexes and instincts live in **tracks** on disk, and travel as signed, closed **packs** you can
review, commit and share. Packs never carry permission to execute. A person grants that per
machine (`adage delegate`), and it runs only inside an OS sandbox.

## The vocabulary

The theme is the nervous system. Traces are experience, reflexes are conditioned responses, and
instincts are innate ones.

**Nouns**

| Concept | Word |
| --- | --- |
| A session transcript | **trace** |
| A learned response | **reflex** |
| A shipped response | **instinct** |
| Stimulus → response path (in explanations) | **reflex arc** |
| What starts a reflex | **stimulus** (docs), **trigger** (CLI) |
| What the reflex tells the agent | **response** (docs), **directive** (CLI, where precision matters) |
| Files on disk / the signed, shareable artefact | **tracks** / **packs** |
| The comparison arm that receives nothing | **control** |
| The web view | **the map** |

**Strength** (the colours stay in the UI)

| Colour | Word | Meaning |
| --- | --- | --- |
| Green | **fire** | Do this: strong evidence |
| Yellow | **prime** | Consider this: suggestion |
| Red | **inhibit** | Don't do this: a known dead end |

**Lifecycle**

| Word | Meaning |
| --- | --- |
| **nascent** | Waits for a person's review |
| **forming** | Delivered while evidence builds |
| **firm** | Established |
| **dormant** | Held: its evidence thinned because it was being followed, not because it was wrong |
| **extinguished** | Retired |
| **ignored** | The agent didn't follow it |
| **misfired** | Followed, and the command failed |

**Verbs**

| Command | Does |
| --- | --- |
| `adage init` | Guided setup |
| `adage sense` | Read-only: which reflexes your traces would form |
| `adage condition` | Mine traces into reflexes and bind instincts |
| `adage train` | Grow reflexes from a skill |
| `adage review strengthen` / `weaken` / `extinguish` | A person's decisions about a reflex |
| `adage delegate` | Let approved reflexes act (prefetch, MCP tools), sandboxed |
| `adage map` | The web view |
| `adage watch`, `doctor`, `share`, `add`, `eval` | Unchanged |

## Rules for applying it

1. **No compatibility baggage.** Adage has not been released. Old verbs (`survey`, `learn`,
   `promote`, `demote`, `reject`, `graph --serve`, `exec enable`, `allow-exec`) are removed, not
   aliased.
2. **On-disk formats migrate.** Tracks, packs, index, review files and spools use the new words.
   No reader for the old values is kept. Committed fixtures are rewritten once by a migration
   script, which is then deleted.
3. **Old formats go.** v1 tracks give way to packs. The directive wrapper `<adage-signal>` becomes
   `<adage-reflex>`, and recorded traces in eval datasets are rewritten to match.
4. **Agent-facing text stays plain and factual.** The theme is for people, not the model: the
   text inside a directive says what to run and why, in plain words, with no themed vocabulary.
5. **Clarity wins.** Where a themed word is less clear than a plain one in a specific sentence,
   use the plain one.
6. **Evidence over enthusiasm.** Every claim about a reflex comes with its numbers ("fired in 412
   traces, saved 1.8 turns against control"), never "supercharges your agent".
7. "Reflex" and "instinct" are common nouns, not product names. The product is **Adage**
   (*Agentic DAG Engine*).

## Example lines

- *Agent traces become reflexes.*
- *Your agents already know the way. Adage turns it into reflexes.*
- *Skills cost inference every time. Reflexes are compiled once.*
