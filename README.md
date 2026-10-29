<p align="center">
  <img alt="Adage: learn reusable steps from coding-agent sessions" src="docs/assets/readme-header.jpg" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <img alt="Status: experimental" src="https://img.shields.io/badge/status-experimental-yellow.svg">
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-orange.svg">
</p>

# Adage

**Learn the repeatable parts of agent work. Reuse them next time.**

Adage is an experiment in a harness that learns from experience. It reads the session history
coding agents already leave on your machine, finds recurring steps, and makes them available in
later sessions as **reflexes**: a condition, a response, and the evidence behind it.

For example, an agent repeatedly tries a test command that fails because it bypasses your
repository's setup. The history shows that `make test` works. Adage can recognize that situation
and offer the known command. With separate approval, supported reflexes can also run a check or
fetch a result for the agent.

The question behind the project is simple: **which parts of an agent's work should it have to
reason through again?** Skills and workflows are useful instructions. Adage explores turning
some of their repeatable steps—and steps learned from actual work—into behavior the harness can
recognize and reuse. The model still handles new problems and judgment.

We think there is substantial value still trapped in this repeated work. Our working hypothesis
is that more specialized repositories and workflows offer clearer commands, conventions, checks
and recovery paths to learn, producing better reflexes with greater benefit. The open-source
experiment is to find that value, improve the learner so it discovers the patterns automatically,
and measure what changes on the next task.

Adage integrates with Claude Code, Codex and OpenCode. Their capabilities differ; Claude Code
has the most live evaluation coverage. See [harness support](docs/harnesses.md).

## Try it

Adage is experimental. Start in one repository and inspect what it learns. You can get useful
guidance without enabling automatic execution.

### Install

With Node.js 18 or later:

```sh
npm install -g @adage-sh/cli
```

npm selects prebuilt binaries for your platform; you do not need Rust or a Git checkout.
Then connect Adage from your repository:

```sh
cd /path/to/your/repository
adage init
```

`adage init` connects the agents it finds, shows its proposed changes, and offers to learn from
this repository's history. Codex also requires its own hook review in `/hooks`.

For a standalone install on macOS or Linux:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/stevenvaughanbooth/adage/releases/latest/download/adage-cli-installer.sh | sh
```

Follow the installer's PATH instructions, then run `adage init`. It offers to fetch matching
companions and checks their published checksums. Windows binaries and other archives are on the
[latest GitHub release](https://github.com/stevenvaughanbooth/adage/releases/latest).
The [installation guide](docs/usage.md#install) also covers the Claude Code plugin.

```sh
adage doctor                   # check the connection
adage condition --repo-root .  # learn from this repository's existing sessions
adage review                   # inspect reflexes and their evidence
adage ui                       # review decisions and explore recorded sessions
adage                         # see status and recent activity
```

In the terminal app, select a reflex and press `Enter` to review it. The list keeps a brief
summary and graph; the dialog shows the evidence, available decisions and saved result.
Approval is shown separately from yellow Prime or green Fire, which describe advice strength.

Continue your normal work. After sessions end, background learning can update the repository's
reflexes. `adage update` installs a newer published version. The [first-session guide](docs/first-reflex.md)
covers setup, an empty history, and what to look for in your next session.

## What a reflex does

A reflex matches a situation and supplies a known response. It can be a short piece of guidance,
a sequence of steps, or an approved action. It is not a new model or fine-tuned model weights.

| Situation | Possible response | Where it comes from |
| --- | --- | --- |
| A test command repeatedly fails, then the repository's wrapper succeeds | Suggest the wrapper instead of another failed attempt | Learned session history |
| Editing a schema is consistently followed by code generation | Remind the agent to run the generator | Learned history or a declared dependency |
| Similar tasks repeatedly use the same files and checks | Supply a procedure card with the relevant files and commands | Learned session history |
| A Python command uses the wrong environment | Use the verified project interpreter for supported calls | A built-in instinct |
| A test runner reports that it collected no tests | Ask the agent to inspect test discovery | An optional verification instinct |

These are supported kinds of behavior, not a guarantee that every repository will produce them.
Ambiguous commands, missing outcomes and weak evidence can leave a candidate inactive. A tool
returning successfully also does not prove that the task was completed correctly.

## From history to reuse

```text
Session history → learn patterns → inspect evidence → use matching reflexes
                         ↑                                  ↓
                         └──────── later sessions ──────────┘
```

1. **Read local history.** Import supported agent and subagent sessions, preserving who did what
   and the outcomes the recording actually contains. Related subagents are not counted as
   independent tasks.
2. **Find recurring behavior.** Look for useful commands, failed approaches, verification steps
   and sequences. The learner is deterministic: the same recorded evidence produces the same
   result, without asking a model to invent a rule.
3. **Keep the evidence attached.** `adage review` shows what a reflex is based on. Evidence
   thresholds and review determine what can become active; accepting a reflex does not authorize
   its commands to run.
4. **Match during work.** Hooks connect Adage to the agent's session. Matching runs locally on the
   CPU. Most reflexes supply guidance; supported rewrites and approved actions have separate rules.

No history is a valid starting point. Adage also includes **instincts**: packaged rules that read
facts from files such as `package.json`, a Makefile or a lockfile. They can help before learning
has enough examples. `adage packs` shows which apply and lets you turn them off.

## Skills, reflexes and packs

- A **skill** describes how to do work. The agent interprets it and makes decisions.
- A **reflex** makes a supported part of that work explicit: when it applies, what to do, and why.
  It can be learned from history or authored and reviewed.
- An **instinct** is a packaged reflex template that adapts to facts declared by a repository.
- A **pack** is the versioned, reviewable format used to share reflexes and instincts.

`adage train SKILL.md` can extract supported steps from a skill and attempt a sandbox replay.
It reports what was verified, what was stale and what could not be replayed. Instructions that
require judgment remain instructions; Adage does not compile arbitrary prose into reliable code.

You can [export and import signed packs](docs/marketplace.md), or host their files in a repository.
The built-in marketplace is a pinned catalogue bundled with Adage. Browsing and installing from
arbitrary remote marketplaces is still planned. Sharing a pack never shares execution permission.

## Evidence so far

The studies give us concrete places to build from: reusable repository procedures, better
verification and less repeated discovery. They also show where the learner and delivery need
work. We track learning a pattern, delivering it and improving a real task separately, so each
experiment can tell us what to improve next.

- **Claude Code mod prototype: 48% fewer model requests and 22% lower reported API cost** than
  the agent alone, with **all 12 runs passing hidden checks in both arms**. The mod supplied approved
  steps before the model needed to request them. This was development run 4 on six sandbox
  tasks, each repeated twice, using Claude Sonnet 5.5; earlier runs informed the fixes. It used
  the deeper mod integration, separate from the portable command-hook path.
  [Study and comparison](docs/benchmarks.md#claude-code-mod-prototype).
- In one held-out comparison on Marshmallow with Claude Haiku 4.5, advisory mode used about
  **22% fewer model requests**, with **19 of 20 runs passing hidden checks in each arm**. This covered
  four tasks repeated five times, not twenty independent tasks.
- In the same study, optional execution used fewer requests but passed hidden checks in
  **17/20 runs versus 19/20** for the agent alone.
  It failed the study's quality rule. An earlier five-repository study also saved cost while
  reducing the pass rate. We retain both results.
- Recent history audits recovered previously missed subagent commands and a custom verification
  candidate. Those are improvements in learning coverage; the new candidate remains inactive,
  with no demonstrated live-agent benefit.

The [evidence page](docs/benchmarks.md) includes the methods, limitations, negative results and
links to the records. Model-request counts and API-list costs are not measurements of energy
use or a promise about a subscription bill. Help us test the specialization hypothesis with
representative histories and new tasks from your own repository.

## Your data and control

- **History stays local.** Normal learning and matching do not send transcripts to a model or
  service. Hooks make no network requests. A background version check runs at most daily;
  `adage config set update.check off` disables it. Explicit installs, updates and optional live
  evaluations can use the network.
- **Guidance is the starting point.** The agent chooses how to use it. On supported Claude Code
  calls, Adage can also rewrite the Python interpreter to a verified project environment;
  `adage delegate rewrites off` disables that behavior.
- **Execution is a separate choice.** Running commands for the agent requires `adage-exec`,
  machine policy and local approval for each action. It uses an OS sandbox on macOS or Linux.
  Windows execution is unsupported.
- **You can stop it.** `ADAGE_DISABLE=1` or a repository's `.adage/disabled` file disables
  interventions. `adage delegate off --repo .` turns off automatic execution. Setup changes are
  shown before writing and backed up. [Removal instructions](docs/usage.md#removing-adage).

## Engineering

Adage is a Rust workspace with a local CLI, terminal UI and embedded React map. Its core design
keeps learning, runtime decisions, storage and execution independently testable. Start with
[components and boundaries](docs/architecture.md) for the current design and
[testing](docs/testing.md) for the checks that enforce it.

| Design choice | How it is checked |
| --- | --- |
| Crates depend only on lower layers; the live hook cannot link the learner | Dependency graph tests in `xtask/src/layers.rs` |
| Harness-specific input becomes one canonical trace model | Byte-level importer tests and native hook fixtures for Claude Code, Codex and OpenCode |
| Learning is deterministic; external reads go through explicit interfaces | Input-order determinism tests, in-memory learning stores and the runtime's `Checkout` port |
| Session state survives interrupted writes; execution journals reject corrupt records | Two-slot state recovery, checksums and strict journal tests in `adage-store` |
| CLI, terminal UI and HTTP API share application operations and reflex representations | `adage-app` plus cross-surface JSON contract tests |
| User approval and sandboxed execution are separate from learning | Approval, confinement, stale-receipt and process-spawn guards |

To inspect the product without connecting an agent, from a checkout:

```sh
cargo run --locked -p adage-cli -- ui --demo
```

To verify the learning-to-delivery path on macOS or Linux (requires Rust, a C compiler and `make`):

```sh
cargo test --locked -p adage-cli --test learning_delivery -- --nocapture
```

That proof imports native traces, learns and reviews a procedure, delivers it through the real
hook, and checks the repaired state of a held-out task in a disposable repository. It uses no
model API and measures no token savings. [The proof's scope](docs/learning-infrastructure.md#run-the-end-to-end-proof)
and [security policy](SECURITY.md) describe the boundaries. Execution remains experimental,
and live evaluation coverage is limited.

## Documentation

Start with the [documentation index](docs/README.md), or go directly to:

| | |
| --- | --- |
| [First session](docs/first-reflex.md) | Connect an agent, learn from history and inspect the result |
| [How it works](docs/how-it-works.md) | Behavior, permissions and current limits |
| [Usage](docs/usage.md) | Commands, settings, updating and removal |
| [Packs and the marketplace](docs/marketplace.md) | Built-in instincts, sharing and what remains planned |
| [Evidence](docs/benchmarks.md) | Results and their limitations |
| [Architecture](docs/architecture.md) | Components and boundaries for contributors |

## Contributing

**Help teach Adage what useful work looks like.** Find a recurring workflow it misses, capture
representative examples, improve automatic detection, and measure the resulting reflex on new
tasks. High-value reflexes can become shareable packs; improvements to the learner help other
repositories discover their own. [CONTRIBUTING.md](CONTRIBUTING.md) has the starting points.
Please remove private data from shared traces and sign off commits with `git commit -s`.

Licensed under [Apache-2.0](LICENSE). Third-party notices are in [NOTICE](NOTICE).
