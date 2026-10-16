# Adage documentation

Adage is an experiment in learning reusable behavior from coding-agent sessions. Start with one
repository, inspect the evidence, and measure whether the guidance helps. Automatic execution is
a separate, optional feature.

## Start here

| Guide | Use it for |
| --- | --- |
| [First session](first-reflex.md) | Install, connect an agent, learn from existing history and review the result |
| [Usage](usage.md) | Commands, settings, updating and removal |
| [How Adage works](how-it-works.md) | What happens automatically, what needs approval and what is still limited |
| [Terms](lexicon.md) | Plain meanings of reflex, instinct, pack and the labels shown in the CLI |
| [Harness support](harnesses.md) | Differences between Claude Code, Codex and OpenCode |

## Learn, reuse and share

| Guide | Use it for |
| --- | --- |
| [Live learning](live-learning.md) | History sources, repository scope and background learning |
| [Instincts](instincts.md) | Built-in and optional rules that use repository declarations |
| [Packs and the marketplace](marketplace.md) | Inspect, export, import and host packs; current distribution limits |
| [Pack format](packs.md) | Manifests, route files, evidence and signatures |
| [Compiling skills](training.md) | Extract supported procedures, inspect replay results and accept a pack |
| [Optional execution](execution.md) | Approve one action, inspect its results and revoke permission |

## Evidence and experiments

[Benchmarks and evidence](benchmarks.md) records what worked and what needs improvement.
Our working hypothesis is that specialized workflows can yield better reflexes with greater
benefit. Help test it with representative traces, improved automatic detection and comparisons
on new tasks; the records distinguish learning coverage from live-agent benefit.

- [Evaluation methods](evals.md) and [evidence repository layout](../evals/README.md).
- [Learning infrastructure](learning-infrastructure.md) and [learning experiments](learning-experiments.md).
- [Registered launch protocol](launch-gate-protocol.md) and [launch evidence index](../evals/reflex-proof/LAUNCH-STATUS.md).

## Contributing

[CONTRIBUTING.md](../CONTRIBUTING.md) covers setup, checks and the DCO sign-off. Use the current
[architecture](architecture.md), [repository map](repository.md), [testing guide](testing.md)
and [release procedure](release.md). Read a crate's `AGENT.md` before changing its implementation.

## Design history

These documents record decisions, proposals and earlier implementation states. They are useful
context, but may describe planned or superseded behavior. Use the operational guides above for
current setup and capabilities.

- [Original design spec](Adage%20—%20Design%20Spec.md), [architecture snapshot](Adage%20—%20Architecture.md)
  and [implementation plan](Adage%20—%20Implementation%20Plan.md).
- [Zero-config design](zero-config.md), [product terminology migration](rebrand-reflexes-and-instincts.md)
  and [design work](design/everyday-ux.md).
- [Claude Code mod research](claude-mod/README.md), [archived designs](archive/), and
  [architecture spikes](../spikes/README.md).

Historical experiment records stay in place so their evidence links remain stable.
