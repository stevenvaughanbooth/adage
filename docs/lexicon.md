# Adage terms

These names describe ordinary software behavior. Use plain language when introducing them;
readers should not need to learn a metaphor before they can use Adage. The CLI retains some
short labels, explained below.

## Core terms

| Term | Meaning |
| --- | --- |
| **Session history / trace** | The transcript an agent records: prompts, tool calls and the results available in that recording |
| **Reflex** | A reusable condition and response, with evidence; it can be learned from history or authored and reviewed |
| **Instinct** | A packaged reflex template that gets its facts from repository declarations rather than learned session history |
| **Candidate** | A pattern the learner has identified; it may need more evidence or review before it becomes active |
| **Trigger** | The situation in which a reflex applies, such as an edit, a failed command or a sequence of events |
| **Guidance / directive** | Text Adage delivers to the agent; the agent still decides how to use it |
| **Action** | A supported operation Adage performs under separate local execution permission |
| **Procedure card** | A reflex that supplies the files, commands and failed approaches associated with a recurring kind of task |
| **Pack** | A versioned, reviewable collection of reflexes, their requirements and optional evidence, used for sharing |
| **Marketplace** | A catalogue of packs pinned to specific content; the built-in catalogue ships with Adage |
| **Track** | The internal storage namespace for a set of reflexes and their learning/review state |
| **Family** | A group of similar tasks used in learning; related subagents are not automatically independent examples |
| **Control** | A session in which Adage withholds its interventions so an enabled measurement can compare outcomes |

A skill is instructions the agent interprets. A reflex makes a supported condition and response
explicit. `adage train` can extract some procedural steps from skills, but arbitrary judgment
remains with the model. A pack's signature establishes its origin and integrity, not that its
behavior is useful or correct.

## Evidence strength

These labels describe how strongly guidance is phrased. None grants execution permission.
Approval is a separate decision: an approved Prime reflex can already provide advice. Approving
it does not create evidence or make it Fire, and an effect ceiling may keep it at Prime.

| CLI label | Plain meaning |
| --- | --- |
| **fire** | Recommend the step based on qualifying evidence |
| **prime** | Suggest the step for the agent to consider |
| **inhibit** | Warn about a known failed approach |

## Lifecycle labels

| CLI label | Plain meaning |
| --- | --- |
| **nascent** | New; awaiting review |
| **forming** | Still being evaluated; can provide advice when available |
| **firm** | Established lifecycle stage; availability is shown separately |
| **dormant** | Stale |
| **extinguished** | Retired |
| **ignored** | The agent did not follow the guidance |
| **misfired** | The agent followed it and the recorded step failed |

These are observations about a reflex, not a guarantee about task quality. A passing command
can be incomplete verification; an ignored suggestion can be the right decision.

## Execution status

| Status | Meaning |
| --- | --- |
| **Suggest** | Guidance is available; automatic execution is off or has no matching grant |
| **Automatic** | Local permission and policy make an action eligible, subject to runtime checks |
| **Needs review** | Saved permission no longer matches the command or current policy |
| **Needs recovery** | An interrupted staged action pauses further application until inspected and acknowledged |
| **Grant saved** | Permission exists, but this view cannot validate the repository's scripts |
| **Disabled** | A switch prevents interventions |
| **Observe** | New sessions default to quiet controls; existing session assignments still apply |
| **Inactive** | The reflex is not active in the runtime index |
| **Unavailable** | Execution permission or sandbox state cannot be established |

An **execution receipt** records what ran and its result. **Unknown** means no confirmed outcome;
it must not be described as success. See [execution](execution.md) for the exact rules.

## Command names

`adage condition` learns from history; `adage sense` previews what it could learn.
`adage review` inspects evidence. `adage packs` manages instincts and imported packs.
`adage delegate` manages optional execution. See [usage](usage.md#commands) for the full reference.

The [terminology migration](rebrand-reflexes-and-instincts.md) records older names for historical
context. It is not a list of supported command aliases.
