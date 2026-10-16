# Adage

Adage is an experiment in helping coding agents reuse what worked in past sessions.
It reads local session histories, finds repeated procedures, and turns supported patterns
into **reflexes**: small rules that can suggest a next step when the same situation comes up.

The question behind it is simple: how much routine work can an agent learn to handle
without reasoning through the same skill or workflow again? Adage does not train a model.
Its learner and runtime use deterministic rules, and the learned results keep their evidence.

Our working hypothesis is that specialized repositories and workflows offer clearer patterns
to learn, producing better reflexes with greater benefit. Help explore that opportunity: find
a recurring workflow Adage misses, contribute representative examples, improve automatic
detection, and measure the reflex on new tasks.

One [Claude Code mod experiment](https://github.com/stevenvaughanbooth/adage/blob/main/docs/benchmarks.md#claude-code-mod-prototype)
used **48% fewer model requests** than the agent alone, with **all 12 runs passing hidden checks
in both arms**. This separate mod prototype was tested on six sandbox tasks, each repeated twice,
in development run 4 after fixes informed by earlier runs. It shows a concrete opportunity for
deeper harness integration alongside Adage's portable command hooks.

## Install

```bash
npm install -g @adage-sh/cli
cd your-project
adage init
```

`adage init` detects Claude Code, Codex and OpenCode, shows the proposed integration changes,
and starts learning from available local histories. Your agent may ask you to approve the
new hooks. Use `adage` to see what was learned and `adage review` to inspect it.

Adage advises by default. Letting a reflex run commands is a separate, explicit opt-in.
Learning and hook-time decisions stay local; no model API is needed for those steps.

```bash
adage update    # install a newer release
```

## What npm installs

The `adage` command selects a prebuilt binary for macOS (Apple Silicon or Intel), Linux
(arm64 or x64), or Windows (x64). The package also includes the skill compiler,
`adage-train`, and, on macOS and Linux, the opt-in command runner, `adage-exec`.
Windows supports advice, not command execution.

npm downloads the matching platform package as a dependency. There is no install script
that fetches binaries or builds source, so `--ignore-scripts` works. Keep optional dependencies
enabled; npm uses them to select your platform. You do not need Rust or a Git checkout.

Read the [guide and evidence](https://github.com/stevenvaughanbooth/adage#readme), or
[report what happened in your project](https://github.com/stevenvaughanbooth/adage/issues).
