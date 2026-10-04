<p align="center">
  <img alt="Adage: agent traces become reflexes" src="web/site/public/og.jpg" width="100%">
</p>

<p align="center">
  <a href="LICENSE"><img alt="License: Apache-2.0" src="https://img.shields.io/badge/license-Apache--2.0-blue.svg"></a>
  <img alt="Written in Rust" src="https://img.shields.io/badge/written%20in-Rust-orange.svg">
  <img alt="Works with Claude Code, Codex and OpenCode" src="https://img.shields.io/badge/works%20with-Claude%20Code%20%C2%B7%20Codex%20%C2%B7%20OpenCode-8b7cf6.svg">
  <img alt="Status: unreleased developer preview" src="https://img.shields.io/badge/status-unreleased%20developer%20preview-yellow.svg">
  <img alt="Execution: macOS and Linux" src="https://img.shields.io/badge/execution-macOS%20%C2%B7%20Linux-lightgrey.svg">
</p>

<p align="center">
  <b>Your coding agent keeps relearning the same lessons. Adage remembers them.</b>
</p>

> [!IMPORTANT]
> **Unreleased developer preview.** Nothing is published yet: no release binaries, no installer
> scripts, no Homebrew tap, no npm package and no Claude Code plugin marketplace. Build it from
> source ([Install](#install)); the repository is private for now, so that requires access to it.
> Advisory instincts and reflexes work in Claude Code today. Opt-in CPU execution runs on macOS
> and Linux; Windows execution is unsupported. The launch studies so far are small and used one
> model (Claude Haiku 4.5) on one repository; see [Evidence so far](#evidence-so-far).

---

Every session, your agent rediscovers how your repository works: that tests run with `make test`,
not `pytest`; that work items are looked up with a script; that editing a schema means
regenerating code. You pay for that exploration again and again.

Adage reads the session transcripts your agents already leave behind, finds the procedures they
keep repeating, and compiles them into **reflexes**: small, deterministic responses that give the
agent the right step at the right moment, with the evidence attached. New reasoning stays with the
agent; the repeated part gets cheaper.

```text
  agent traces ──► adage condition ──► reflexes ──► index ──► hooks
  + instincts                        (tracks,               ├─ guidance / supported rewrites
  + adage train                       packs)                └─ approved adage-exec actions
```

## Highlights

- **Learns from history you already have.** Claude Code, Codex and OpenCode transcripts, read
  locally, one repository at a time.
- **Works on day one.** Built-in *instincts* read your Makefile, `package.json`, CI and lockfiles
  before there is any history. They ship as packs from the marketplace in this repository;
  `adage packs` shows which apply and turns any off.
- **Keeps learning.** After each session ends, a background learner updates the reflexes. No
  re-runs needed.
- **Compiles skills.** `adage train SKILL.md` turns a skill's steps into verified reflexes, replayed
  in a sandbox so stale instructions are caught.
- **Measures itself.** 1 session in 10 is a silent control, so savings are measured against a
  baseline, never assumed.
- **Nothing runs without you.** By default Adage only advises. Running commands is a separate,
  opt-in, sandboxed binary for macOS and Linux.
- **Shareable.** Reflexes travel as signed, reviewable packs you can commit or hand to a teammate:
  the same format the built-in instincts ship in.
- **Small and fast.** One ~10 MB Rust binary with a bundled embedding model, deterministic and
  fail-open.

## Install

Adage is an unreleased developer preview: build it from source. Until the repository is public,
this requires access to it. You need Rust and a C compiler.

```bash
cargo install --locked --git https://github.com/stevenvaughanbooth/adage adage-cli
adage init                                   # inside your repository
```

For a first session, expected results and the separate approval path for CPU actions, follow
[Your first useful reflex](docs/first-reflex.md).

To verify learning without a model account, run the
[end-to-end learning proof](docs/learning-infrastructure.md#run-the-end-to-end-proof): native
traces become a reviewed reflex that repairs a fresh task through the real hook. For prospective
token-savings measurement, use the [frozen study workflow](docs/learning-experiments.md).

**Not published yet:** the Claude Code plugin marketplace, Homebrew and npm. Until the first
release is out, none of these exist; once they do:

```text
/plugin marketplace add stevenvaughanbooth/adage      # in Claude Code (not published yet)
/plugin install adage@adage
brew install stevenvaughanbooth/tap/adage             # not published yet
npm install -g @adage-sh/cli                          # not published yet
```

Building from a checkout, the release channels and the separate `adage-exec` and `adage-train`
binaries are in [docs/usage.md](docs/usage.md#other-ways-to-install).

## Quickstart

Start a session in a repository. The first time, you see one line (the agent never sees it):

```text
adage: learning from 23 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you.
```

From the next prompt on, the agent gets its reflexes. Run `adage` anytime to see where things
stand:

```text
adage 0.1.0 · github.com/acme/billing · ● healthy

reflexes    14 active: 4 instincts, 9 learned, 1 from your team
learning    after each session · last: 2 min ago, 1 change
activity    last 30 days: 61 sessions · 188 directives · 152 followed · 7 dead ends headed off
savings     ✓ ~$0.034 and 3.1 turns per session, API-list equivalent (95% CI $0.011–$0.058)

next  adage review · adage ui · adage map · adage doctor
```

<sub>Illustrative numbers. Until a repository has enough sessions for a real comparison, the card
shows counts, not dollars.</sub>

## Evidence so far

**Adage learns your repository's recurring work and gives your agent what it would otherwise go
looking for.** Adage saves model calls by handling learned reflexes and instincts on your CPU; we
count that in model requests, tokens and API cost. GPU use and energy are not measured.

On four held-out tasks in a real repository ([Marshmallow](https://github.com/marshmallow-code/marshmallow)),
frozen before any result was read and never used for tuning, each run five times with and without
Adage (Claude Haiku 4.5, scored by hidden tests the agent never sees):

- **Advisory mode, the default** (instincts, the orientation card and learned advice; nothing runs
  on the agent's behalf): **about 22% fewer model calls** (29.0 → 22.8 per session, 0.78×; one-sided
  95% upper bound 0.83; fewer in 18 of 20 paired blocks) and **a third lower cost** ($0.201 →
  $0.135 per session), **with the same correctness** (hidden tests 19/20 in both). This comparison
  was registered before the study, to be reported beside its gated test.
- **Execution mode, opt-in and experimental** (Adage also runs the person-approved tests on your
  CPU and checks the finish line): more savings in testing (0.67×, upper bound 0.73), but hidden
  tests passed 17/20 against 19/20, which misses the registered quality rule, so the study's verdict
  for execution mode is that it did not pass. It is not yet proven to keep quality and stays opt-in.
- **Safety controls:** all five (a failing test, a later edit, a config change, a broader change, an
  explicit rerun request) ran as designed, and none skipped necessary work.

One repository and one model so far; transfer to cheaper models and more repositories are next.
The launch site's "How we tested" page explains the method in plain words. The full study
history, misses included, is in [docs/benchmarks.md](docs/benchmarks.md#launch-evidence-cpu-execution-and-cross-session-learning),
with every record in the [launch evidence index](evals/reflex-proof/LAUNCH-STATUS.md).

## What it learns

| Reflex | Learned from | What the agent hears |
| --- | --- | --- |
| **Avoid a dead end** | A command that keeps failing, then one that works | "`python3 -m unittest` fails here. Use `make test`." |
| **Look the item up** | Agents passing an ID from the prompt to the same script | "Work items are looked up with `scripts/wi-lookup W-2042`." |
| **Regenerate after an edit** | A step that always follows edits to certain files | "Changes to `schema/*.json` must be followed by `make gen`." |
| **Steps in order** | Commands run in sequence after an edit | "After editing `registry/*.csv`: `make gen`, then `make check`." |
| **Procedure cards** | Several sessions doing the same kind of task | The files to edit, the test that works and the commands that failed, delivered once |

The full list and how each is delivered: [docs/usage.md](docs/usage.md#what-adage-learns).

## Earlier advisory results

Real Claude Code sessions, each paired with a control session on the same task. These measure
advice and procedure cards, not CPU execution. Every number links to its data in
[docs/benchmarks.md](docs/benchmarks.md).

| Setting | Cost per task | Tests passed (control → Adage) |
| --- | ---: | ---: |
| Recurring tasks on 5 open-source repos (shipped product, Haiku 4.5, 80 pairs) | **−28.2%** | 59/80 → 51/80 ⚠️ |
| A compiled skill vs. no help (12 sessions per arm) | **$0.122 → $0.034** | 9/12 → 12/12 |
| Sandbox repo, Sonnet 5, held-out tasks (36 per arm) | **−24%** | 100% → 100% |
| One-off bug fixes on 13 real repos | 0–4% | unchanged |

**Read with care.** The pass-rate drop in the first row is almost entirely one family, where the
learned procedure faithfully repeated sessions that never wrote a test. One-off bug fixes gain
little, because the expensive part is reasoning about new code. Adage helps most with work your
repository repeats. Sample sizes are small; the [benchmarks page](docs/benchmarks.md) has every
interval and caveat.

## Safe by default

- Adage **advises**. It doesn't run commands for the agent unless you turn on delegation at a
  terminal (`adage delegate on`), and then only reflexes you approved, inside an OS sandbox, on
  macOS or Linux. On Windows it advises but executes nothing, and its hook invocation there is
  untested.
- **No network** from any hook or learner. Transcripts are read locally, and background learning
  reads only the current repository's.
- Settings files change only when you install, and `adage install … --write` shows the diff and
  keeps a backup.
- Off switches: `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

[How Adage works](docs/how-it-works.md) has exactly what runs automatically, what needs your
approval, and what the evidence does and doesn't show.

## Documentation

| | |
| --- | --- |
| [How Adage works](docs/how-it-works.md) | The behavior reference and current limits |
| [Usage](docs/usage.md) | Every command, the live views (`adage watch`, `adage ui`), settings, sharing and review |
| [Packs and the marketplace](docs/marketplace.md) | What a packaged reflex looks like, the base instincts, sharing and hosting |
| [Benchmarks and evidence](docs/benchmarks.md) | Every published number, with data, code and caveats |
| [Harnesses](docs/harnesses.md) | What Claude Code, Codex and OpenCode each support |
| [Execution](docs/execution.md) | Opt-in, sandboxed `adage-exec` |
| [Training](docs/training.md) | Compiling skills with `adage train` |
| [Repository layout and status](docs/repository.md) | Crates, plugins and what's built so far |

## Contributing

Contributions are welcome. Please sign off your commits (`git commit -s`); see
[CONTRIBUTING.md](CONTRIBUTING.md). `cargo test --workspace --locked` runs the suite.

## License

Adage is licensed under the [Apache License 2.0](LICENSE). Third-party licenses are in
[NOTICE](NOTICE).
