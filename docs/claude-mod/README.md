# Adage in Claude Code as a mod

Four documents, one proposal (2026-10-02, against Claude Code 2.1.287, the release that turned
mods on by default), its implementation ([impl/README.md](impl/README.md): every chunk of the
plan, built against 2.1.288 on the branch `claude/mods-poc-m98u3o`, with the proof of concept
that preceded it under [poc/](poc/README.md)), and the brief for evaluating it live
([evaluation-plan.md](evaluation-plan.md)). Nothing has run in a live Claude Code session yet;
[spike S9](../../spikes/S9-claude-mods/SPIKE.md)'s questions are answered by that evaluation. Each document was reviewed once against the code by
two independent readers before this revision; the header of each says what the review changed.

| Document | Answers | Read it when |
| --- | --- | --- |
| [architecture.md](architecture.md) | What a mod lets Adage do that the command hooks cannot, and how it fits beside them without moving the decision core: what the mod gives inference, prediction and attachment (§1b), the request Adage does not send, chains, the finish line at the last step, the dead end that never runs, receipts, conventions attached to the tool, noticed patterns, skills, decisions made in the session, invariants, hosts | You want to know *what* and *why* |
| [ux-design.md](ux-design.md) | Every place Adage draws, every string, every state, the band, the pane, the commands, the settings, the degraded states, the copy deck, the usability check | You want to know *what the person sees* |
| [implementation-plan.md](implementation-plan.md) | What changes in the learner, the harness, the tracks, training, the CLI, the mod and the evals; twenty-nine chunks with files and lines, contracts, tests, acceptance criteria, estimates and dependencies | You are going to build it |
| [test-plan.md](test-plan.md) | Levels, environments, fixtures, traceability, every test case by id, the live probes, the evaluation protocol, CI and release gates, entry and exit criteria | You are going to prove it |

How they relate: the architecture names a capability; the UX design says how it looks; the plan
says which chunk builds it and names the test cases; the test plan specifies those cases and the
criteria that close the chunk. The spike holds the questions that must be answered before the
chunks that depend on them (the plan's §0 and §3 say which).

Two things to hold in mind reading any of them: the Rust binary stays the only decider, the
learner and the sandbox, and the command hooks stay as the floor on every host; and the person-
only decisions (review, delegation, trust) are made by a person's answer in the session, in the
dialog Claude Code uses to ask a question, bound to the session by a single-use token the model
cannot reach by being steered (mod architecture §3.12: a provenance record and a guard against
steering, not a boundary against a same-user agent), or at a terminal, which stays an equal path. The goal that sets the bar is in ux-design §4.18: install the plugin and never run a
command.
