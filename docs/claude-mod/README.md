# Adage in Claude Code as a mod

Four documents, one proposal (2026-10-02, against Claude Code 2.1.287, the release that turned
mods on by default). Nothing in them has run yet; [spike S9](../../spikes/S9-claude-mods/SPIKE.md)
is where the first measurements come from. Each document was reviewed once against the code by
two independent readers before this revision; the header of each says what the review changed.

| Document | Answers | Read it when |
| --- | --- | --- |
| [architecture.md](architecture.md) | What a mod lets Adage do that the command hooks cannot, and how it fits beside them without moving the decision core: four layers (observe, decide, measure, act in the model's place), the request Adage does not send, noticed patterns, skills, invariants, hosts | You want to know *what* and *why* |
| [ux-design.md](ux-design.md) | Every place Adage draws, every string, every state, the band, the pane, the commands, the settings, the degraded states, the copy deck, the usability check | You want to know *what the person sees* |
| [implementation-plan.md](implementation-plan.md) | What changes in the learner, the harness, the tracks, training, the CLI, the mod and the evals; twenty-five chunks with files and lines, contracts, tests, acceptance criteria, estimates and dependencies | You are going to build it |
| [test-plan.md](test-plan.md) | Levels, environments, fixtures, traceability, every test case by id, the live probes, the evaluation protocol, CI and release gates, entry and exit criteria | You are going to prove it |

How they relate: the architecture names a capability; the UX design says how it looks; the plan
says which chunk builds it and names the test cases; the test plan specifies those cases and the
criteria that close the chunk. The spike holds the questions that must be answered before the
chunks that depend on them (the plan's §0 and §3 say which).

Two things to hold in mind reading any of them: the Rust binary stays the only decider, the
learner and the sandbox, and the command hooks stay as the floor on every host; and the person-
only decisions (review, delegation, trust) are made by a person's press in the session, bound to
it by a token no agent can obtain (architecture §3.12), or at a terminal, which stays an equal
path. The goal that sets the bar is in ux-design §4.18: install the plugin and never run a
command.
