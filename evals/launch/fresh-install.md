# Fresh install, first-time user pass (launch candidate)

**Date:** 2026-09-29. **Branch:** `claude/repo-maturity-roadmap-l95ohs` at `f0698ff`. (`204f98e`
landed during the pass. It changes only README benchmark wording and adds
`evals/launch/adversarial-review.md`, so every finding below still applies.)
**Method:** I acted as a new user and followed README "Install" and docs/zero-config.md as written.
Every `adage` command ran under `env -i` with a fresh `HOME` and a plain `PATH`, and with
`CLAUDECODE` and the other agent-session variables removed. Terminal-only commands ran under
`script` to give them a TTY. Hooks got JSON payloads shaped like Claude Code's, following
`crates/adage-cli/tests/zero_config_plugin.rs`.

**Repositories:**

- `pallets/click`, cloned from the RepoBench mirror with `origin` set to
  `https://github.com/pallets/click.git`. It has a real `.venv` made by uv, with click installed
  editable and pytest.
- `urfave/cli`, a Go repository with no history and no Python.

**History:** 10 real Claude Code traces of click tasks from `/tmp/adage-rb2`, with their `cwd`
rewritten to the clone, plus 5 marshmallow traces for an unrelated directory.

**Three homes:**

- **A (CLI path):** `adage install claude-code --write --scope user`.
- **B (plugin path):** `plugin/claude-code` staged with this build's binary in
  `dist/<triple>/adage` and `SHA256SUMS`, as the release does. `CLAUDE_PLUGIN_DATA` is under
  `~/.claude/plugins/data/adage-adage`, and `adage` is not on `PATH`.
- **C:** no history, used for the Go repository.

## Verdict

The zero-config path does most of what it promises:

- The first session start shows exactly one line, which the agent never sees.
- A learner bootstraps in the background.
- A verbatim recurring prompt gets its procedure card.
- The `.venv` rewrite is exact.
- Hooks are fast (p95 under 20 ms).
- Hooks make no network calls.
- A corrupt index heals itself.
- A Go repository with no history stays silent.

It does not yet "just work" around the edges. The problems a first-time user will actually hit:

1. **Opening Claude Code outside a repository** (in `~`, which is common) says "learning from this
   repo". It then starts a learner that fails. From then on, `adage doctor` shows a warning whose
   suggested fix does nothing (B2).
2. **Surfaces disagree about the embedding model.** `adage status` and `adage watch` say there is
   no model, and `status` suggests `adage models fetch`. Following that advice silently stops
   procedure cards until the next learn (B3, B4).
3. **`install --write` shows no diff**, although the README says it does (B5).
4. **`delegate off` is a one-way door** for the `.venv` rewrite: turning it back on also turns on
   execution (B6).
5. **The install channels named in the README are not live yet** (B1). This must be checked before
   announcing.

## Step 1: build

`CARGO_TARGET_DIR=/tmp/adage-fresh-target cargo build --release -p adage-cli -p adage-exec -p adage-train`

- Finished in **6 min 07 s** (12 min user time) with **0 warnings**.
- The target directory used 518 MB (`adage` 27.8 MB, `adage-exec` 16.0 MB, `adage-train` 20.4 MB).
- There was no disk trouble with 3.6 GB free.
- The binaries were copied out and the target directory deleted.
- No friction.

## Step 2: clean HOME, first contact

Bare `adage` in the click clone, with Claude Code on `PATH` and nothing installed:

```text
adage 0.1.0
  Not connected to any agent yet.
  Claude Code found → in Claude Code: /plugin marketplace add stevenvaughanbooth/adage, then /plugin install adage@adage
                       or, from here: adage install claude-code --write --scope user
  adage --help · adage doctor
```

This works well. Without `claude` on `PATH` it says `No agent found: install Claude Code, Codex or
OpenCode, then run adage again.` (fine). One nit: the first line is 120 columns wide and wraps in
an 80-column terminal.

## Step 3: follow the README

### `adage install claude-code --write --scope user`

- Piped (no TTY), it refuses clearly: `` adage: `adage install --write` needs a person at a terminal
  (stdin is not a terminal); use the printed snippet instead, or set ADAGE_ALLOW_NONINTERACTIVE=1 ``.
- With a TTY it prints one line, `adage install: wrote ~/.claude/settings.json`, and writes
  immediately.
- It merges into an existing `settings.json` and keeps `model` and `permissions`.
- Re-running is idempotent: one entry per event.
- The hooks use the exec form (`command` plus `args`). I checked that the local Claude Code build
  (2.1.42) accepts that form and has a `PostToolBatch` event.
- **Friction (B5):** no diff is shown and nothing asks for confirmation. README §"What never
  happens without you" says `adage install … --write` "shows the diff", and zero-config §3.5 says
  "with a diff". The diff helper `install::line_diff` is only called from `exec.rs`. No backup of
  `settings.json` is kept.

### Bare `adage` after install (home A, before any session)

```text
adage 0.1.0 · github.com/pallets/click                         ● check: data directory
  no reflexes active yet
  learning: 0 of ~5 sessions needed before Adage forms its own reflexes
  data directory: ~/.local/share/adage does not exist yet → `adage condition` creates it; nothing to do
```

- **Confusing (B10):** `~/.claude/projects` holds **10 past sessions** for this repository, and
  `adage sense` counts them, but the card says "0 of ~5 sessions".
- The health dot is yellow (`check`) for a state whose own fix line reads "nothing to do".

### `adage doctor` (fresh install)

The result was `12 passed, 3 warning(s)`. Two of the warnings tell a new user to run
`adage condition`:

```text
WARN  index   no index yet: hooks have nothing to direct
              fix: run `adage condition`
WARN  tracks  no tracks installed
              fix: `adage sense` shows which reflexes would form; `adage condition` forms them
```

The README says "Then start a session. That is all." A fresh install that shows three warnings
undermines that. Before the first session these should be `pass` (for example "nothing yet: your
first agent session in a repository starts learning"). This is part of B10.

### `adage models status`

This is clear. It reports `potion-base-8m-i8@b9c0e329a2ce (bundled in this binary)` and that the
checksums match.

### `adage sense`

This works and gives useful data (dead-end candidates, the procedure-card family). Nits (L3):

- It says `Read 15 session file(s) … 15 session(s) in 2 repository(ies)`. The lexicon says
  "trace", and "repository(ies)" should be proper plurals.
- The internal `exec` prefix leaks into every command: `exec python -m pytest {path} -xvs`,
  `exec cat`.
- It prints `lower bound -0.00` (negative zero).
- The candidate id is repeated: `learned/card-5c06f78810: a prompt means: card-5c06f78810 → …`.
- It says "prompt intents 0 usable", which is jargon.

### `adage review`

`adage review: nothing awaits a decision.` Fine.

### `adage train .claude/skills/click-bugfix/SKILL.md`

This used the lab's click skill, copied into the clone. It is fast (0.75 s) and the review is very
readable.

1. **Clone in a private directory.** Every ancestor of `$HOME` or of the scratch directory is
   `0700`, which is the default for many Linux homes. The sandbox cannot lend `.venv`:
   `not lent: .venv (the sandbox runs as nobody, who cannot read it: an ancestor directory is
   private)`. The two `.venv` steps are "not replayed". The explanation is clear, but this will be
   common on Linux (B8).
2. **Clone in world-readable `/tmp`, with a `.venv` whose editable install pointed at a path the
   sandbox cannot read.** This is a copied checkout, the same situation as a worktree sharing a
   venv. Replay says:

   ```text
   STALE     `.venv/bin/python -m pytest tests -q -x` fails on a clean checkout (exit 4: ModuleNotFoundError: No module named 'click'): dropped (your skill is stale here)
   ```

   The skill is correct: that command passes in the real checkout (2241 passed). This is a
   **false "stale" verdict** that blames the skill for an environment problem, and the step is
   dropped (B8).
3. **With the `.venv` rebuilt in place,** all 4 reflexes are verified (`exit 0, shows passed`). A
   related note: the editable `.pth` still points at the original working tree, so the "clean
   checkout" replay imports the user's working copy, not the clean one.
4. **Inhibit reflexes marked `verified` whose alternative was not verified.** Two inhibit reflexes
   are marked `verified` while their replacement says
   `instead, .venv/bin/python -m pytest: .venv/bin/python does not exist in a clean checkout`.
   That is confusing.

`adage train accept click-bugfix` refuses without a TTY, the same way install does, and with a TTY
installs 4 reflexes with a clear summary. Fine.

### `adage delegate off`

- Piped, it refuses: `` adage: `adage delegate off` needs a person at a terminal (stdin is not a
  terminal). ``
- With a TTY it prints:

  ```text
  Execution: off. Hooks call adage again (approvals are kept; revoke them with review revoke).
  Rewrites to the project's environment: off (Python test runs keep the interpreter the agent chose).
  No hook registration written by `adage install` needed changing.
  This takes effect in new agent sessions.
  ```

**B6:** turning something *down* should not need a terminal. The README config section says keys
that only turn something down "can be set from a script", and `!adage delegate off` inside Claude
Code cannot work. Worse, the only way to get rewrites back is `adage delegate on`
(`exec.rs::switch`), which also:

- turns execution on;
- requires `adage-exec` beside `adage`;
- rewrites hook registrations to `adage-exec`;
- makes the base plugin go quiet unless the exec plugin is installed.

A user who tried `delegate off` once cannot restore the default without opting into execution.
Separately, "This takes effect in new agent sessions" is wrong for rewrites: `exec/rewrite-off` is
read on every hook call. I deleted the file by hand to continue.

### `adage map --help`, `adage --help`, and every README command

- Every command the README names exists and runs:
  - `init --print`, `condition`, `watch` (including `--once --size`, `--demo`,
    `doctor --watch`), `map`, `share`, `add`, `status`, `report`;
  - `config` and `config set`, `savings --json`, `ui`;
  - `models fetch|use|status`, `install opencode|codex`, `train`, `train accept`,
    `condition --instincts-only`, `hooks-config`.
- Removed verbs fail cleanly: `adage survey`, `adage learn` and `adage exec enable` all return
  ``unknown command … `adage --help` lists them``.
- **But `adage graph --serve` is silently accepted.** Unknown flags are ignored and the text graph
  prints. This is part of L3.
- `adage review promote a b` gives a usage message that says
  `<approve|…|dismiss> <track> <route>`, although `review --help` says `<reflex>` (L3).

## Step 4: zero-config flow (plugin shim plus hook binary)

### First session start, plugin only (home B)

```text
$ scripts/session-start "$PLUGIN_ROOT" "$PLUGIN_DATA" < SessionStart.json
{"systemMessage":"adage: learning from 10 past sessions in this repo; reflexes arrive within a minute. Nothing runs without you."}
```

- Exit code 0 with nothing on stderr.
- The shim verified the bundled binary and staged `bin/adage -> adage-<sha256>`. It then spawned
  `adage condition --auto --repo-root <click> --bootstrap`, which bound 20 instincts and learned
  4 reflexes, including one procedure card, within a few seconds.
- The unrelated marshmallow traces were not used for click.
- The second session start the same day printed nothing: the daily throttle works, and
  `notice-shown.json` was written.
- Shim timing:

  | Case | Time |
  | --- | --- |
  | Cold (first start, copy and hash the binary) | 162–192 ms |
  | Warm | 14–15 ms, and the binary's own `SessionStart` is 2.8 ms p50 |

  The cold start uses `shasum` (Perl, 132 ms here) before `sha256sum` (27 ms). Trying
  `sha256sum` first on Linux would cut that (L9).

### Prompt-time procedure card

- The 4 verbatim training prompts of the shell-completion family all get the card. It is about
  2,000 characters, in `additionalContext` only, with no `systemMessage`.
- A lightly shortened version of one training prompt gets **nothing**, and so does a reworded one.
  This is the documented strict threshold (README: "0 of 6 informally reworded tasks were
  recognised"), but in practice this card will rarely fire on new work.
- Card content (B9):
  - `Commands:` lists `git status (2/4)` and `ls -la (2/4)`, which are noise.
  - The test command that works (`.venv/bin/python -m pytest …`) is **absent**, although the
    README says a card holds "the test command that works". Only the `avoid python -m pytest …`
    line is there.
  - The `shell_completion.py` exemplar is a syntactically broken fragment: an `if … :` line with
    no body, followed by `return`, and no elision marker.

### PreToolUse `.venv` rewrite (verified `.venv`)

| Command the agent sends | Result |
| --- | --- |
| `python -m pytest tests/test_formatting.py -xvs 2>&1 \| head -50` | rewritten to `<repo>/.venv/bin/python -m pytest … 2>&1 \| head -50` |
| `pytest tests -q` | rewritten to `<repo>/.venv/bin/python -m pytest tests -q` |
| `python3 -c "import click; …"` | rewritten ("this Python run") |
| `cd tests && python -m pytest test_basic.py` | rewritten after `cd` |
| `pip install pytest`, `python3 -m pip install -e .` | `permissionDecision: deny`, with the `.venv/bin/python -m pip install …` hint |
| `.venv/bin/python -m pytest tests`, `ls -la`, `python -c "print(1)"` | untouched, empty output |

- This is exactly as documented.
- The person sees `adage: ran this test run with .venv/bin/python (the project's environment).`
- **Copy (L1):** the notice and the agent text say "Adage **ran** this test run". Adage ran
  nothing; the agent's own tool ran the rewritten command. Everywhere else the product says
  "Adage never runs the commands", so this should read "pointed this test run at
  `.venv/bin/python`".
- `pytest …` → `python -m pytest …` also adds the current directory to `sys.path`, so "Nothing
  else in the command changed" is not strictly true. This is minor.

### PostToolBatch, PostToolUse, PostToolUseFailure, Stop, SessionEnd

- **PostToolBatch** (the event the plugin registers) with an edit to `src/click/*.py` returns the
  `run-3e72af` directive:
  `.venv/bin/python -m pytest tests/test_shell_completion.py tests/test_parser.py tests/test_imports.py`.
  PostToolUse, which is not registered, gives the same directive. Within a session it is not
  repeated.
- **PostToolUseFailure** for `python3 -m pytest tests -x` gives the instinct
  "That failed. … run this instead: `.venv/bin/python -m pytest`".
- **Stop** blocks:
  `Adage finish-line check: this session changed files matching src/click/*.py … Run it … This reminder is from Adage, not from the user.`
- **B7:** that finish line is `run-3e72af`, `prime`/`firm`. `adage status` shows it as
  `0/7 traces rate 0.00 lb -0.00`, and `.why.json` says "7 triggers, 0 ran it" (a desire path).
  A reflex that every surface displays as having zero evidence blocks Stop and says "must be
  followed by running". The mechanism may be right; the explanation is missing. `status` also
  shows `🟢 fire dead-end-586db0 1/8 traces rate 0.12`. A new user reading these numbers will
  conclude Adage enforces things it has never seen work.
- SessionEnd is silent and spawns the learner, as expected.

### Nothing printed that shouldn't be

- stderr was empty on every hook call: 350 timed calls, plus the edge cases below.
- Notices go only in `systemMessage`. Directives go only in `additionalContext` or `reason`.
- Garbage JSON, empty stdin and an unknown event (`Notification`) all exit 0 with no output. Each
  is recorded in `fail-open.log`, and doctor reports them.
- Directive text for the agent is plain. The themed words appear only in the wrapper attributes
  (`<adage-reflex … route="…" strength="prime">`).

### No network access from hooks

- I ran `strace -f -e trace=socket,connect` over the shim's first start. That covers staging, the
  version check, the hook, and the detached bootstrap learner the trace follows.
- I ran the same trace over each of UserPromptSubmit, PreToolUse, PostToolBatch,
  PostToolUseFailure, Stop and SessionEnd, including the learners they spawn.
- **Result: 0 `socket()` and 0 `connect()` calls.**

### Hook latency (50 calls each; release build; home B with 24 reflexes and a card)

| Event | p50 | p95 | max |
| --- | ---: | ---: | ---: |
| UserPromptSubmit (card match, embeds) | 14.3 ms | 18.4 ms | 20.3 ms |
| UserPromptSubmit (no match, embeds) | 13.6 ms | 18.0 ms | 24.8 ms |
| PreToolUse (rewrite) | 2.4 ms | 2.8 ms | 3.0 ms |
| PreToolUse (`ls`) | 1.6 ms | 1.8 ms | 1.8 ms |
| PostToolBatch (edit) | 6.1 ms | 10.2 ms | 10.8 ms |
| PostToolUseFailure | 6.2 ms | 7.2 ms | 8.1 ms |
| SessionStart | 2.8 ms | 2.9 ms | 3.7 ms |

No call produced an error.

### Plugin detection

- `doctor` reports `hooks: claude-code registered by the Adage plugin`, and the card shows
  `● healthy`. Both come from `~/.claude/plugins/data/*adage*/bin/adage`.
- **L5:** registering both the plugin and user settings, which `adage init` suggests side by side,
  is not flagged. Doctor passes, and every rewrite `systemMessage` would appear twice. Directives
  are de-duplicated through the session state, which I checked with concurrent calls.

## Step 5: Go repository with no history (urfave/cli)

- The first session start says
  `adage: learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you.`
- The bootstrap binds 1 instinct (`instinct-u6`).
- UserPromptSubmit, PreToolUse, PostToolBatch, PostToolUseFailure (`go test ./...` failing) and
  Stop all exit 0 with **no output**, before and after the bootstrap. There is no noise and
  nothing breaks.
- The card reads
  `1 reflex active: 1 instinct · learning: 0 of ~5 sessions needed …; 1 instinct active meanwhile`.
  Good.

**But outside any repository (B2):** a SessionStart whose `cwd` is `$HOME`, `/`, or a directory
that no longer exists gets the same notice. In every case:

```text
{"systemMessage":"adage: learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you."}
```

A learner is spawned for each. The repository keys are `homeC@path-f5d317b89775`,
`@path-8a5edab28263` and `dir`. Each learner fails, and `learn.log` gets `condition_error` lines,
each written **twice**. After that, `adage doctor` shows:

```text
WARN  auto-learn runs   last automatic conditioning failed for @path-8a5edab28263 (…, condition: conditioning failed; run `adage condition --json` to see why); dir (…, repo: the repository's directory no longer exists); homeC@path-… 
                        fix: run `adage condition` in a terminal to see the error; `adage doctor` passes once conditioning succeeds
```

Following that fix prints `adage: no traces found in ~/.claude/projects …`. The warning never
clears, because no conditioning of `~` will ever succeed. Starting Claude Code in `~` or in a
scratch directory is common, so a new user will see this on day one.

## Step 6: error paths

### Unwritable HOME

I ran as `nobody` with a `HOME` owned by root and set to 0555.

- **Hooks:** all exit 0. SessionStart prints `adage: learning from this repo as you work; …`
  **every session start**, because the daily throttle cannot persist. It promises learning that
  cannot happen (B11).
- **`adage doctor`:** `FAIL data directory ~/.local/share/adage does not exist and cannot be
  created — fix: set ADAGE_HOME to a writable directory`, exit 1. Clear.
- **Bare `adage`:** still shows "Not connected" and gives no hint about the data directory.
- **`adage condition --instincts-only` and `adage install claude-code --write`:**
  `adage: Permission denied (os error 13)`. There is no path and no hint. `adage config set` does
  name the path. This is part of B11.

### Corrupt index (`index.json` truncated mid-string)

- Hooks fail open and stay silent. The card says clearly:
  `● problem: index … index.json is not a valid index (EOF while parsing a string at line 1 column 22); hooks fail open and say nothing → run adage condition to recompile it`.
- The next SessionStart spawned a rebuild, and the index was valid again within seconds with no
  notice. This matches the design. The session that found the corruption gets nothing, which is
  acceptable.

### Corrupt `config.toml`

The bad keys keep their defaults and the hooks work. **But nothing tells the user.** Neither the
card nor `adage config` mentions the bad lines (`learn.auto = maybe`, a broken `[runtime` table),
although zero-config §5 promises `config.toml line 4 ignored: …` on the card (L6).

### Missing or unusable model

| Case | What happens |
| --- | --- |
| No installed model | The bundled potion-base-8m-i8 serves. `models status`, doctor and the card all say `(bundled)`. Good. |
| Installed model with a corrupted `model.safetensors` | Hooks fall back to the bundled model and cards still arrive. `doctor` gives `FAIL embedding model potion-base-8m is installed but unusable … reinstall` and **exits 1**, although a working fallback is in use; this should be WARN (L7). `models status` does not mention the broken install on its first line. |
| Installed model directory deleted | Back to bundled, silently. Fine. |
| `adage models use potion-code-16m-v2` (not installed) | Exits 0, and the machine silently drops to **the hashed fallback** instead of keeping the bundled model. Intent reflexes stop. Doctor then advises `adage models fetch`, which fetches potion-base-8m, not the selected model (L4). |
| `adage models fetch potion-base-8m` | Works. The Hugging Face attempt failed here without saying why, and it fell back to npm (L10). |

**B3:** `adage status` prints
`intent model: not installed, so intent reflexes will not fire (`adage models fetch`)` whenever no
model is *installed*. It ignores the bundled one (`main.rs:485` opens only the installed
directory). The `adage watch` header shows `- no model` for the same reason, including in
`watch --demo`. Both contradict `models status`, doctor and the card, and they send the user to
`models fetch`.

**B4:** after `adage models fetch potion-base-8m`, with the same session setup and a verbatim
prompt:

| Model in use | Directives delivered |
| --- | --- |
| Fetched potion-base-8m | `click-bugfix-s3-avoid click-bugfix-s3-avoid-2` (no card) |
| Bundled | `card-5c06f78810 click-bugfix-s3-avoid click-bugfix-s3-avoid-2` |

The card family was learned with the bundled model, so fetching a model stops cards until the next
conditioning. `models use` warns "run `adage condition`", but `models fetch`, which is what
`status` and the README table ("Start like others did … needs `adage models fetch`") point to,
says nothing.

## Lexicon check (docs/lexicon.md)

Old verbs are gone from the CLI. Remaining stale words:

- **User-facing CLI text with "route":**
  - `adage review` usage error: `<track> <route>`;
  - watch and learn-log lines: `({change}, {n} routes)` (`ui/learnlog.rs:99`);
  - `exec.rs:258–259`: "no routes are installed", "no route X in track";
  - `exec.rs:473`: `route:` label;
  - `review.rs:553`: "pack route(s) are inert";
  - `review.rs:697`: "has no route".
- **On-disk and wire formats:**
  - `learn.log` JSON key `"route"`;
  - `tracks/<t>/routes/*.yaml`;
  - the agent wrapper attribute `<adage-reflex … route="…">`.

  The lexicon's "on-disk formats migrate" rule lists strengths and states, not this key, so these
  are out of scope or at least debatable. They are documented in `docs/live-learning.md` and
  `docs/execution.md`.
- **Docs linked from the README still say "route" for reflex:** `docs/training.md` (for example,
  "Prebuilt route bound from…" in its terminology table), `docs/execution.md` and
  `docs/harnesses.md` ("After-failure routes"), plus `docs/zero-config.md` §3.2 and §6.
- **`adage sense`:** mixes "session(s)" and "traces" in one screen.
- **`adage graph --serve`:** accepted silently instead of being rejected.
- **README line 63** says `npm install -g @adage/cli` with no caveat. Line 278 adds "(once the scope
  is claimed)", and the npm registry returns 404 for `@adage/cli`.

## Bugs by severity

| # | Severity | Bug | Suggested fix |
| --- | --- | --- | --- |
| B1 | **Blocker (verify before launch)** | The README "Install" channels are not live. `origin` has no `marketplace` branch (the `ref` that `.claude-plugin/marketplace.json` uses). `@adage/cli` returns 404 on npm, and `releases/latest/download/adage-cli-installer.sh` returns 404. `main` still carries the old marketplace.json ("put your agent on rails"). | Run the release workflow, then walk `/plugin marketplace add` → `/plugin install` on a clean machine. Drop npm from the Install paragraph until the scope is claimed. |
| B2 | High | SessionStart outside a repository (`~`, `/`, a deleted directory) says "learning from this repo", spawns a learner that fails, and leaves a permanent doctor warning whose fix does nothing. `learn.log` lines are duplicated. | Require a repository root (`.git`) before bootstrap, notice or spool. A non-repo `cwd` should be the silent fast path. Make doctor's auto-learn check ignore or expire keys that are not repositories. |
| B3 | Medium | `adage status` ("intent model: not installed, so intent reflexes will not fire (`adage models fetch`)") and the `adage watch` lamp (`- no model`) ignore the bundled model. | Use `model::resolve` in both places (as `status_card::embeddings` does). |
| B4 | Medium | `adage models fetch` switches the embedder, and procedure cards learned with the bundled model stop matching until the next conditioning. `fetch` does not say so. | Have `fetch` (and `use`) queue a re-learn of the repositories that have intent or card reflexes. At minimum, print the "run `adage condition`" line. Or let the hook match a card with the model recorded in its trigger. |
| B5 | Medium | `adage install … --write` writes with no diff and no confirmation, contrary to the README and zero-config §3.5. No backup is kept. | Print `install::line_diff` and ask `y/N` at a TTY. Write `settings.json.adage-bak` once. |
| B6 | Medium | `adage delegate off` needs a TTY, although it only turns things down. Rewrites can come back only through `delegate on`, which turns execution on. "Takes effect in new agent sessions" is wrong for rewrites. | Allow `off` without a TTY. Add a separate switch (`adage config set runtime.rewrite true|false`, or `adage delegate rewrites on|off`). Fix the message. |
| B7 | Medium | Evidence display contradicts behaviour: a `firm` prime finish line with `0/7 traces, lb -0.00` blocks Stop ("must be followed by running"), and `fire` shows `1/8 traces`. Negative zero is printed. | Show the evidence the learner actually used (advantage or turns saved, "7 triggers, 0 ran it: desire path") in `status`, `sense` and `review`. Clamp −0. Soften "must" for prime. |
| B8 | Medium | `adage train` gives false "STALE … your skill is stale here" verdicts when `.venv` cannot be used in the sandbox (the editable `.pth` target cannot be read). With a private ancestor directory, which is common on Linux, `.venv` steps are "not replayed". | Classify `ModuleNotFoundError` for the project's own package or test dependencies under a lent `.venv` as "environment not usable in the sandbox", not stale. Lend the `.pth` targets read-only. Mention `chmod o+x` or running from a readable path in the review. |
| B9 | Medium | Card content: `git status` and `ls -la` appear as "Commands", the working test command is missing, and the exemplar is a broken fragment. | Filter read-only navigation commands out of card commands. Add the command that replaced each avoided one (the dead-end pair already knows it). Elide gaps in exemplars (`…`). |
| B10 | Medium | A fresh install looks unhealthy: the card shows `● check: data directory … nothing to do`, doctor shows 3 WARN lines telling you to run `adage condition`, and the card says "0 of ~5 sessions" with 10 sessions in history. | Before the first session, report these as pass or info ("starts at your first agent session"). Count the repository's existing traces in the "learning" line. |
| B11 | Medium | Unwritable data directory: the notice repeats every session start, and `condition` and `install` fail with a bare `Permission denied (os error 13)`. | Skip the notice when the notice state cannot be written, or say `adage: can't write ~/.local/share/adage; set ADAGE_HOME` once. Add path context to I/O errors. |
| L1 | Low | Rewrite copy says "Adage ran this test run", which contradicts "Adage never runs commands". | "Adage pointed this test run at .venv/bin/python …". |
| L2 | Low | Adage accepts the old `--serve` flag on `adage graph` (and ignores other unknown flags) without an error. | Reject unknown flags. |
| L3 | Low | Lexicon leftovers (list above); `sense` shows the `exec` prefix, "repository(ies)" and "session(s)" in place of traces. | Sweep the strings; strip the canonical `exec ` prefix in human output. |
| L4 | Low | `models use <not installed>` silently drops to the hashed fallback; doctor's fix fetches a different model. | Refuse, or keep the bundled model and warn; make the fix name the selected model. |
| L5 | Low | Plugin plus user-settings double registration is not detected, so every rewrite notice appears twice. | Doctor WARN when both are present. `init` should not suggest both. |
| L6 | Low | A corrupt `config.toml` is silently ignored. | Show the ignored lines on the card, in doctor and in `adage config`, as §5 of zero-config says. |
| L7 | Low | A corrupt installed model makes doctor FAIL (exit 1) while the bundled fallback works. | WARN. |
| L8 | Low | The README row "Start like others did … (optional; needs `adage models fetch`)" is stale: the model is bundled. | Drop the parenthetical. |
| L9 | Low | The shim's first start takes about 170 ms, mostly Perl `shasum`. | Try `sha256sum` first. |
| L10 | Low | `models fetch` does not say why Hugging Face failed before falling back to npm. | Print the classified reason on one line. |

## What worked well (keep it)

- A clean release build with no warnings. A single first-session line, never sent to the model,
  with a daily throttle.
- Traces are scoped to the repository, and the learner is detached.
- The `.venv` rewrite is exact, with a verified environment only; `pip install` into the system
  Python is refused; the rewrite is counted on the card (`rewrote 54 commands …`).
- Hooks are fast (≤ 20 ms p95 at a prompt, ≤ 3 ms before a tool), silent on stderr and on bad
  input, and made **zero network syscalls**.
- The index heals itself; a Go repository with no history stays silent.
- Clear refusals when a command needs a person at a terminal.
- `adage train`'s review is readable, and doctor's per-check fixes are good.

*Clean-up:* the build target (`/tmp/adage-fresh-target`) and the `/tmp/adage-fresh-*` directories
were removed, along with the scratch homes, repositories and binaries.
