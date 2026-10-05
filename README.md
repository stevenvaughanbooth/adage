# Adage

**Train reflexes. Skip rediscovery.**

Adage learns repeated patterns in your coding agents' work and turns them into **reflexes**:
local guidance about what to do next, when a familiar situation appears. Each reflex has a
trigger, a response, and evidence you can inspect. The agent runs every command through its own
tools and permissions; Adage supplies text and never executes the response itself.

**Fire** directs a response, **prime** suggests it, and **inhibit** warns against a learned dead end.
Reflexes are learned; **instincts** would be built-in responses and are **not available in this build**.
The file format and advanced CLI still call a reflex a `route` and its collection a `track`.

```text
agent traces → repeated patterns → learned reflexes → guidance → observed outcomes
      ↑                                                              │
      └──────────────────── learn from later work ────────────────────┘
```

## Try this checkout

Build from the repository root with stable Rust, then launch Adage in your terminal:

```bash
cargo build --release --locked -p adage-cli
target/release/adage
```

On Windows, the binary is `target/release/adage.exe`.

Start on **Home**. Press **Ctrl+K** or **:** anywhere to search the command palette, or use
**0–4** and **Tab / Shift+Tab** to switch screens. Everyday setup, learning, review, settings,
reports, and signed collection import/export are available in the app.

To explore first, press **d** on Home or choose **Explore demo trace graph**. The browser opens
three scripted guided/control examples with navigable trace graphs. The hooks made the recorded
decisions; the demo's learning evidence is simulated and labeled. This demo does not use or
change your repository's agent history.

## Connect your repository

Use the command palette for each step; no terminal commands are needed after launching Adage.

1. **Open a repository** and choose the Git checkout you want Adage to learn about.
2. Choose **Connect Claude Code**, **Connect Codex**, or **Connect OpenCode**. Pick repository
   or user scope and inspect the exact configuration change before confirming. For Codex,
   open **`/hooks` in Codex** and accept the new hooks; registration in Adage does not replace
   Codex's approval. Adage keeps this next step visible until it receives a hook.
3. Open **History sources** to see the folders Survey and Learn use. Claude Code and Codex
   history are detected from their configured homes. Use **Configure Claude Code history**,
   **Configure Codex history**, or **Configure OpenCode history** to choose another folder.
   For OpenCode, choose **Import OpenCode history** to copy local session exports for the
   selected repository into the configured exports folder, or Adage's local history folder. Run it again after later OpenCode sessions:
   hook registration does not export new history automatically. An existing exports folder
   can be selected instead.
4. Choose **Learn from this repository**. Its preview shows the source folders, readable
   sessions for this repository, sessions with actions, and previously guided sessions before
   you confirm. **Survey session history** lets you inspect candidates first. Survey, manual
   learning, and automatic learning use the same sources. If there is no usable history, Adage
   explains how to add it. Learning may finish without finding a pattern strong enough to become a reflex.
5. Open **Review learned reflexes**. Inspect each trigger, response, and evidence. For a pinned
   reflex with a proposed change, the review shows the current/proposed field differences and
   both directive previews. **Approve proposed change** applies exactly the version you
   reviewed; if it changes before confirmation, Adage asks you to reload and inspect it again.
6. Start or resume your coding agent in that repository, then check **Health** and **Live**.
   **Hooks registered** means configuration exists. **Hooks received** means Adage has actually
   received a hook for this repository. A receipt verifies delivery, not that a reflex was
   applicable or useful; inspect the resulting trace for that.

Press **w**, or choose **Open the web map**, for your live workspace. Adage starts a local
server, chooses an available port, and opens the browser. Keep the terminal app open to keep
that server available. The web app is read-only: it shows learned reflexes, evidence, recorded
traces, and incoming activity; its **Set up my repository** path guides you back to the terminal
app for changes. Its navigation palette helps you move between those views.

The live workspace and the scripted demo are labeled separately. Opening
[`brain.html`](crates/adage-cli/assets/brain.html) directly loads the embedded demo, without a
server; it does not show your own live sessions. In either surface, a trace contains recorded
actions, guidance, and outcomes.

## What Adage learns

From real sessions in a repository, Adage learns five kinds of reflex:

| Reflex | Learned from | Response |
| --- | --- | --- |
| **Prompt → lookup** | Agents passing an ID from the prompt to the same command | **Fire / prime:** “Work item details are looked up with: `python3 scripts/wi-lookup W-2042`” |
| **Linked files** | Agents reading, one turn at a time, the files a lookup listed | **Prime:** “That result lists these files … read them together in one turn” |
| **Dead ends** | A verify command that fails, followed by one that works | **Inhibit:** “Running `python3 -m unittest` fails (…). Use this instead: `make test`” |
| **Finish line** | A regenerate step that follows edits to a path pattern | **Fire:** after the edit, and once at Stop if still unmet: “changes to `schema/*.json` must be followed by `make gen`” |
| **Intent** | Prompts with similar meaning whose sessions ran the same command first | **Fire / prime:** “Requests like this one usually start by running: `make test`” (optional; choose **Download embedding model**) |

Every reflex carries evidence in distinct sessions, including a Wilson lower bound, and an
effect class that caps how strongly it can direct the agent. Typed slot contracts check prompt
values before they reach a directive (`^W-\d{4}$`, for example). File paths must be safe,
repository-relative, and exist.

## Terminal and web reference

### Watch it live

`adage watch` (or `adage doctor --watch`) is a compact live pane for a split terminal. It works in
60×20 and grows with the pane:

```text
 adage watch · harbor-dispatch · gen 44876069                         LIVE
+ adage 0.1.0  + hooks cc  + 4 reflexes  + auto-learn on  - no model
+ fail-open 0
 activity · harbor-dispatch ────────────────────────────────────────────
12:04:00 ●d76f0f cc prompt  → lookup-incident [fire]
12:04:01 ◌19b2cf cc prompt  withheld lookup-incident [fire]
12:04:02 ●d76f0f cc tools   → read-linked-files [prime]  ✓ followed lookup-i
12:04:05 ●bc0c7d cc tools   ✗ not observed lookup-incident
12:04:06 ●bc0c7d cc failure → dead-end-npm-test [inhibit]
12:04:07 ●bc0c7d cc stop    ■ stop-request
12:04:08 ●bc0c7d cc tools   ✓ met codegen-after-registry
 learning ──────────────────────────────────────────────────────────────
12:04 auto-learn: 27/28 traces used, 730ms, 2 changes, gen 44876069
12:04   learned incidents: read-linked-files prime→fire (13 of 17 sess
 q quit · p pause · f all repos · v hide quiet · ? help
```

- **Top:** health lamps, re-checked every few seconds: the binary, hook registration and
  receipt for this repository, its reflexes in the index, auto-learn, the embedding model, recent fail-open errors.
- **Middle:** every hook call as it happens, from the session spools: time, session, harness, event,
  and what Adage did: a directive (→, coloured by strength), withheld in a control trace (◌),
  followed ✓ or not observed within the follow window ✗, a finish line met, or a stop request.
- **Bottom:** what learning changed, from `learn.log`, manual or automatic; its title says
  *learning now…* while a background learner holds this repository's lock.

For prompt/intent guidance, the follow window is the next **3 observed tool batches**.
Codex and OpenCode report each tool call separately. “Not observed” means the directed action
was absent in that window; it does not establish deliberate ignoring or the final outcome of
the session.

Keys: `q` quits, `p` pauses, `f` switches between this repository and all of them, `v` hides events
where Adage said nothing, `↑`/`↓` scroll, `?` shows help. `NO_COLOR` is honoured: strengths are then
spelled out (`[fire]`, `[prime]`, `[inhibit]`), as above.

`adage ui` (or plain `adage` at a terminal) is the full-screen app, with five tabs:

| Tab | What it does |
| --- | --- |
| **Home** | Explains the learning loop, shows readiness and next steps, and opens the command palette |
| **Live** | the watch view, larger |
| **Reflexes** | Learned reflexes grouped by collection: fire/prime/inhibit strength, trigger and response, trace evidence, live outcomes, and directive previews with example values. Proposed changes show the field diff and proposed response. Approve (`a`), extinguish (`r`), weaken (`d`), strengthen (`p`), pin (`n`), or dismiss (`x`), with a confirmation before each decision |
| **Health** | every doctor check, live, including live learning's. `Enter` fixes the selected one where a key can: register hooks (confirming the exact file and diff), fetch the model (with progress), learn this repository now (what a background learner runs: `adage learn --auto --repo-root`), turn auto-learn on, open the settings, or open the fail-open log |
| **Settings** | every `adage config` key with its value and where it came from (default, file, or environment); `Enter` edits, `Space` toggles, and `adage config set`'s validation and agent-session refusal apply |

Use **0–4** or **Tab / Shift+Tab** to navigate. The command palette also includes trace surveys,
outcome reports, collection status, reflex trace graphs, learning and error logs, evaluation datasets,
offline model installation, and signed pack import/export. Path forms accept spaces and `~` without
shell quoting. Import trust is explicit, and exports use your SSH signing agent.

`adage watch --demo` and `adage ui --demo` replay a small scripted scenario (three sessions of the
harbor-dispatch demo, through real hook calls) into a temporary data directory, so you can see it
all without an agent running. In the terminal app, choose **Explore demo trace graph** (or press
**d** from Home) to open the three guided/control examples in the map. The map uses the launch
site’s day/night themes and lets you inspect each recorded hook, response, and outcome.
`adage graph --serve --demo` also serves an isolated demo; opening `brain.html` directly loads
the embedded example without a server. Demo learning evidence is simulated and labeled.
`adage watch --once [--size 80x24]` prints a single frame as text.
Decisions, installs and settings refuse inside an agent session, as their commands do.

### Advanced CLI: Claude Code

```bash
# 1. Build the binary and stage it into the plugin
plugin/build.sh

# 2. Learn from your existing Claude Code history (reads ~/.claude/projects). After this,
#    Adage keeps learning by itself as sessions end (see "Live learning" below).
target/release/adage learn
target/release/adage status

# 3. Run Claude Code with the Adage plugin
claude --plugin-dir plugin/claude-code
```

### Advanced CLI: OpenCode and Codex

```bash
cargo build --release -p adage-cli
target/release/adage install opencode            # print the plugin; add --write to install it
target/release/adage install codex               # print a hooks.json; add --write to merge it
opencode export <session-id> > s.json && target/release/adage learn --transcripts s.json
target/release/adage learn --codex-sessions ~/.codex/sessions   # Codex rollouts
```

`--write` installs into the repository (`.opencode/plugins/adage.js`, `.codex/hooks.json`) or, with
`--scope user`, the harness's documented user location; it never runs inside an agent session.
`adage install claude-code` does the same for Claude Code settings. What each harness supports, and
what it cannot do yet (Codex reports no tool failures to hooks, so only routes that react to a
failed command stay silent there; learned dead ends are announced at the first prompt), is in
[docs/harnesses.md](docs/harnesses.md).

`adage status` shows every track and route with its aspect and evidence; `adage report` summarises
directives emitted, followed, and derailed. Data lives in `~/Library/Application Support/Adage`
(macOS), `~/.local/share/adage` (Linux) or `%APPDATA%\Adage` (Windows); `ADAGE_HOME` overrides it.

**Kill switches:** `ADAGE_DISABLE=1`, or a `.adage/disabled` file in a repository.

**Live learning:** available history is learned automatically. When an agent's session stops or
ends, the hook starts a background learner for that repository (detached; the hook never waits,
and never says anything different). It learns at most once per `learn.min_interval_secs` per
repository, one learner at a time, and appends each route that changed (new, retired, yellow →
green, evidence, awaiting review) to `$ADAGE_HOME/learn.log`. `adage doctor` shows the last run
per repository. OpenCode exports are snapshots: use **Import OpenCode history** again to make
new sessions available; hooks do not refresh those exports. The file contract for tools that follow it live is
[docs/live-learning.md](docs/live-learning.md).

**Settings:** `$ADAGE_HOME/config.toml`, shown and changed with `adage config`:

```bash
adage config                                  # every key, its value, and where it came from
adage config set learn.auto false             # stop learning automatically (ADAGE_AUTO_LEARN=0 also works)
adage config set learn.thresholds demo        # lower evidence bars, for small histories
adage config set learn.min_interval_secs 300  # learn each repository at most every 5 minutes
```

| Key | Default | |
| --- | --- | --- |
| `learn.auto` | `true` | Learn automatically when sessions stop or end (`ADAGE_AUTO_LEARN` wins) |
| `learn.min_interval_secs` | `120` | At most one automatic learn per repository per interval |
| `learn.thresholds` | `default` | `default` or `demo`, for `adage learn` too (`--demo` wins) |
| `learn.claude_projects` | `""` | Claude Code history (`""` = `$CLAUDE_CONFIG_DIR/projects`, or `~/.claude/projects`) |
| `learn.codex_sessions` | `""` | Codex rollouts (`""` = `$CODEX_HOME/sessions`, or `~/.codex/sessions`) |
| `learn.opencode_exports` | `""` | A directory of `opencode export` files automatic learning reads |
| `learn.parse_cache` | `true` | Keep parsed transcripts in `cache/sessions` (owner-only), so re-learning parses only what changed |
| `runtime.holdout` | `0.1` | Holdout share (`ADAGE_HOLDOUT` wins) |

`config set` refuses inside an agent session.

**Intent families (optional):** `adage models fetch` downloads the pinned static embedding model
(potion-base-8M, 31 MB, SHA-256 verified; `adage models install <dir>` for offline machines) into
the data directory. `adage learn` then clusters each repository's prompts into intent families
(exact kNN + deterministic Louvain, split and merged by what the sessions did next — spike S7) and
learns an intent route per usable family. Without the model everything else works unchanged: learning
skips intent families and hooks never load a model.

**Sharing:** `adage share <track> --out <dir> [--key <path>]` checks every file for absolute paths,
home-directory paths and secret-looking strings, re-validates the track, and signs its digest
(`sshsig`, namespace `adage-track`; ssh-agent first, else the key file, whose passphrase is asked for at
the terminal). `adage add <dir>` verifies the
signature against `$ADAGE_HOME/trusted-signers` or the repository's `.adage/trusted-signers` (approved
once with `--trust` and pinned by hash in `$ADAGE_HOME/trust.json`) and installs the routes at
probation, never green, under `tracks/@imported/` where `adage learn` never writes; if you later learn
the same route yourself, your first-hand route wins. Adding a newer version keeps your own review
decisions for every route that survives. An unknown signer needs `--trust`. Both commands refuse to run inside an agent
session (`CLAUDECODE`) or without a terminal; `ADAGE_ALLOW_NONINTERACTIVE=1` lifts only the terminal check.

**Review:** `adage review [--json]` lists the routes that need a person's decision: new routes from
`adage learn`, routes whose aspect or evidence changed materially since the last decision (shape,
aspect, a Wilson lower bound moving by 0.10, or support doubling or halving), imports at probation,
rejected routes that came back stronger, and routes whose directed response was repeatedly not observed within the follow window in guided sessions. Each
entry shows its evidence, effect and ceiling, and the exact directive text with example slot values.
A pinned route is never overwritten: when learning (or a re-import) finds a materially different
version, it waits as *proposed*, with a diff; `approve` applies it (still pinned) and `dismiss` keeps
yours. `adage review <approve|reject|demote|promote|pin|dismiss> <track> <route> [--by <name>]` records the decision
in the track's `review.yaml`, re-validates the track, and recompiles the index. Promotion never exceeds
the effect ceiling, and an import cannot be promoted to green without first-hand evidence learned
locally. `adage learn` keeps rejected routes retired until they strengthen materially, and holds
demotions and promotions. A rejected route also keeps a teammate's import of the same route out of
the index. Decisions refuse to run inside an agent session. Listing is allowed there.

**Experiment:** `runtime.holdout = 0.1` (default; `ADAGE_HOLDOUT` overrides) holds out 10% of sessions — Adage computes what it would
have said and logs it, but says nothing — so every claimed effect is measured against a control.

## Results

Real **Claude Haiku 4.5** agents, headless, each in a fresh copy of a sandbox repository whose
conventions they cannot guess. Adage learned from 18 training sessions, then two learning-loop rounds
of held-out work items ran 18 sessions with Adage and 18 without (hooks active, directives withheld).
Hidden tests scored every run. Round 2 learned from every earlier session.

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **2** | 20 | 0.0002 |
| Seconds to first edit (median) | **12.0** | 42.5 | 0.0001 |
| Total turns (median) | **9** | 27.5 | < 0.001 |
| Cost per run (mean, USD) | **0.053** | 0.135 | 0.0001 |
| Hidden checks passed | **100%** | 78% | 0.11 (n.s. at n = 18) |

Round 1 showed the loop at work: removing exploration exposed a dead end (agents that no longer read
the Makefile tried `pytest` first: 1.44 failed runs per session). Round 2 learned it as an inhibit reflex
and it disappeared (0.00). Without Adage, 4 of 18 round-1 agents gave up asking what the ticket was.
Full data: [demo/results](demo/results/); interactive results page published as an artifact.

### A second demo: a stronger model, a different stack

Next, **Claude Sonnet 5** on **harbor-dispatch**, a TypeScript service on Node 26 whose conventions
differ from the first sandbox:

- incidents like `INC-94` resolve only through `bin/fetch-incident`;
- `npm test` fails and `npm run check` works;
- `registry/*.csv` edits need `npm run codegen`.

There were 84 sessions: 12 for training, then 36 with Adage and 36 without over two rounds. The run
cost $6.55 in total.

| Round 2, held-out | With Adage | Without | p (permutation) |
| --- | ---: | ---: | ---: |
| Turns before first edit (median) | **3** | 6 | 0.02 |
| Seconds to first edit (median) | **6.7** | 11.0 | < 0.001 |
| Exploration calls before first edit (median) | **2** | 4 | < 0.001 |
| Total turns (median) | **7** | 10 | 0.002 |
| Cost per run (mean, USD) | **0.065** | 0.086 | < 0.001 |
| Hidden checks passed | 100% | 100% | — |

Sonnet found every convention on its own:

- it ran the lookup unaided in every training session;
- it met the finish-line obligation 100% of the time in both arms;
- it never tried `npm test`, so no dead end was learned.

Adage's effect therefore moved from avoiding failures (Haiku: 78% → 100% hidden checks) to
**skipping exploration**: agents went to the lookup and the linked files directly. That halved the
turns before the first edit and cut cost by 24%, with quality unchanged. The weaker the agent is at
finding conventions, the more Adage adds.

Results are in [demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5](demo/results/20260923-1249-harbor-dispatch-claude-sonnet-5/).

## Reproduce the experiments

`demo/` runs real headless Claude Code agents on a sandbox repository whose conventions an agent
cannot guess (work items only via a tracker CLI; tests only via `make test`; `make gen` required
after schema edits), with twelve work items and hidden checks.

```bash
python3 demo/run.py all                  # train (no Adage) → learn → held-out eval, two arms
python3 demo/dashboard/serve.py          # live dashboard at http://127.0.0.1:8765
```

The map page lives with the binary (`crates/adage-cli/assets/brain.html`); `adage graph
--serve` serves it without the Python server.

See [demo/SANDBOX.md](demo/SANDBOX.md). `python3 demo/sandbox/selfcheck.py` proves every work item
fails untouched and passes with its reference solution.

## Repository

| Path | Contents |
| --- | --- |
| `crates/adage-core` | Canonical actions, shell normaliser (tree-sitter-bash + command table as data), effect classes, spans, repo identity |
| `crates/adage-store` | Per-session two-slot state files and CRC-framed spools — no daemon |
| `crates/adage-tracks` | Track format, validation, canonical digest, compiled index |
| `crates/adage-runtime` | Hook-time decisions and directive rendering from fixed templates |
| `crates/adage-learn` | Transcript import and the route miners |
| `crates/adage-sign` | Native `sshsig` signing (ssh-agent or key file) and `allowed_signers` verification |
| `crates/adage-intent` | Prompt normalisation, the static embedding model in plain Rust (WordPiece + safetensors rows read on demand), deterministic kNN + Louvain clustering, behavioural split/merge |
| `crates/adage-eval` | `adage eval`: offline replay of recorded sessions through the runtime, scored against what agents did; safety oracle; property tests |
| `crates/adage-cli` | The `adage` binary; harness adapters in `src/harness/` |
| `crates/adage-sim` | Harness simulator: seeded mock histories (`sim/fixtures/`) and native hook payloads for Claude Code, OpenCode, and Codex |
| `plugin/claude-code` | The Claude Code plugin |
| `plugin/opencode` | The OpenCode plugin (installed by `adage install opencode`) |
| `demo/` | Sandbox, runner, live dashboard |
| `evals/` | Eval datasets (90 real demo sessions, synthetic, adversarial) and the live proving-ground runner — see [docs/evals.md](docs/evals.md) |
| `docs/` | [Design spec](docs/Adage%20—%20Design%20Spec.md) · [Architecture](docs/Adage%20—%20Architecture.md) · [Implementation plan](docs/Adage%20—%20Implementation%20Plan.md) |
| `spikes/` | The experiments that settled the architecture — see [the board](spikes/README.md) |

`cargo test --workspace` runs the suite, including the normaliser's 315 golden fixtures.

## Status

v0.1: Claude Code, plus OpenCode and Codex adapters and importers, built against their
documentation and source and verified end to end by the harness simulator (learn from each
harness's mock history, replay a held-out session through its native hooks in both arms), but not
yet run against a live installation — see [docs/harnesses.md](docs/harnesses.md#verification). Tracks are shared
directly (`adage share` / `adage add`) and reviewed with `adage review`. Intent families use static
embeddings (`adage models fetch`; everything else works without the model). Hardened against an
adversarial review (tracks are untrusted input; effects re-derived; bounded hot path; symlink-safe
paths). Built-in instincts are not available in this build. Team depots and lockfiles are designed but not yet built. See the implementation plan.

## License

Adage is licensed under the [Apache License 2.0](LICENSE). Third-party licenses for everything
compiled into the binary are in [NOTICE](NOTICE). Contributions are welcome under the same
license with a DCO sign-off (`git commit -s`); see [CONTRIBUTING.md](CONTRIBUTING.md).
