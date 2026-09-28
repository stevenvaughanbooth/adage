# Adage zero-config design: "install it and it just works"

## 0. Principles

1. **Adage should earn its first directive in the first session without the user running any Adage command.** Consent is needed only for: execution, environment-changing steps, network downloads, and team trust anchors.
2. **Nothing fails silently.** Each failure either heals itself or produces one plain sentence in the place the user is already looking. For persona (a) that place is the Claude Code UI. Doctor is not that place.
3. **The user learns a concept only when Adage needs a decision from them.** Words like fire, prime, forming, tracks and control arm stay out of every default surface.
4. **Dollar figures appear only once measured against the control arm, and always with an interval.** Before that, the user sees counts of observed events.

---

## 1. The ideal first 10 minutes

### (a) Claude Code plugin user who never opens a terminal

| t | What happens (automatic) |
|---|---|
| `/plugin install adage@adage` | The plugin ships **verified binaries for all 5 targets** (see P0-1). No network is used. |
| First `SessionStart` | The shim stages the binary. The hook sees the repo has no index entry and no learn-queue state, so it runs the **bootstrap**: it spawns a detached `adage condition --auto --repo-root <root> --bootstrap`. Phase 1 binds instincts from the repo files and compiles the index (tens of ms). Phase 2 conditions from this repo's existing `~/.claude/projects` traces. The hook itself returns in under 50 ms. It also writes a `session_start` spool record (arm, propensity) and emits one `systemMessage` line that the user sees but the model does not: `adage: learning from 23 past sessions in this repo; 4 instincts active now. Nothing runs without you.` |
| First prompt (seconds later) | The index now has instincts, and after phase 2, learned reflexes. The first directive is delivered. |
| Each `Stop` | Auto-learn runs as it does today. |
| Next day's first `SessionStart` | At most one line, and only when something changed, e.g. `adage: 3 new reflexes; 41 directives followed this week. 1 install step waits for you: run adage review in a terminal.` |

Persona (a) cannot approve nascent reflexes, because decisions require a person at a terminal. That is acceptable: environment steps are a small share of reflexes, and everything else just works.

### (b) CLI user (brew, npm or curl)

- **`brew install` / `npm i -g`**: prints nothing Adage-specific. There are no postinstall scripts. That is correct.
- **First bare `adage`** (not `init`) shows the status card (§3.4). With no hooks yet it says: `Not connected to any agent yet. Claude Code found → adage connect` (the wizard with user scope preselected: the diff, then `y`).
- **The wizard's first question**, answered with one keypress, is **user scope** ("every repo") rather than project scope. Per-repo `.claude/settings.json` edits are dirty-worktree friction.
- **Condition step**: the wizard runs bootstrap for the current repo only. It does not run a global `adage condition` over every repo.
- **Model download**: the wizard offers it, and it runs only on `y`.
- **Next agent session**: from here the flow is the same as persona (a).

### (c) Team lead sharing reflexes

- **Lead**: `adage share <repo-track> --into-repo`. This signs the pack, writes `.adage/packs/<name>/` and adds the lead's key line to `.adage/trusted-signers`. The lead reviews and commits these like any other files.
- **Teammate**: `git pull`. On the next `SessionStart`, the bootstrap (or learner) finds repo-committed packs signed by a key in the repo's `trusted-signers`. It binds them under these limits:
  - capped at **forming / prime, E0 text only**;
  - environment steps stay nascent;
  - execution grants are never inherited.

  The teammate sees one line: `adage: using 6 team reflexes from .adage/packs (signed by alice@acme.com).`
- **Rationale**: this is the same trust level as `CLAUDE.md`, which the repo can already use to steer the agent. The signer file's hash is pinned on first use. If that file later changes, imports pause and the teammate sees `trusted signers changed; run adage review`.

### Must never happen silently

- **Execution**: needs `delegate on` plus a per-reflex `review delegate`, at a terminal. This is unchanged.
- **Network**: no hook and no learner ever touches the network. Model fetch happens only in response to a person's explicit action.
- **Other repos' traces**: the bootstrap and learner read only transcripts whose recorded cwd is inside this repo's root or its worktrees. The filter uses the Claude project-dir slug and the first-line cwd (P0-5). Other repos' files are not opened at all. Today the parse-cache pass may open them.
- **Environment-changing reflexes**: always nascent until approved.
- **Trust-anchor changes and pack signer changes**: always require a person.
- **A settings file changing on disk**: only the plugin (the user installed it) or `connect` with a shown diff may do this.

---

## 2. Friction audit of the current flow

### Steps a user must take today

1. Build from source or install the binary. The README Quick start still says `cargo build` and `plugin/build.sh`.
2. `adage init`. The non-interactive path writes no hooks (`crates/adage-cli/src/init.rs:4-6, 113-124`).
3. `adage install claude-code --write`, which refuses without a TTY or inside an agent (`install.rs:184-190`), or `claude --plugin-dir plugin/claude-code`.
4. `adage condition` (README step 2). Otherwise nothing is delivered until the first `Stop`.
5. Optionally `adage models fetch`.
6. `adage review` for nascent reflexes, which are only announced by `adage condition`'s text output (`main.rs` "nascent reflex(es) formed but not delivered").
7. Codex users: `adage condition --codex-sessions ~/.codex/sessions` for their history.

### Concepts exposed before any decision is needed

fire/prime/inhibit, nascent/forming/firm/dormant/extinguished, 6 review verbs plus delegate/revoke, tracks vs packs, instincts vs conditioned, sense vs condition, `learn.thresholds default|demo`, control arm, repo keys, and "intent families".

### Places it silently does nothing

| # | Failure | Where it is decided |
|---|---|---|
| S1 | **The marketplace plugin has no binary.** `plugin/claude-code/dist/` is gitignored (`.gitignore`), and `.claude-plugin/marketplace.json` says "Needs the adage binary on PATH". The shim prints to stderr only (`plugin/claude-code/scripts/session-start:77`), which is invisible in normal Claude Code UI. The other 5 hooks then exec a missing `${CLAUDE_PLUGIN_DATA}/bin/adage`. | shim |
| S2 | **The first session gets nothing, and nothing is recorded.** `respond` returns `None` when the repo is not in the index (`crates/adage-hook/src/lib.rs:241`). `SessionStart` is always fast-skipped (`crates/adage-runtime/src/fast.rs:42`). Learning starts only at `Stop`/`SessionEnd` (`crates/adage-hook/src/harness/mod.rs:68-70`). No spool is written, so `watch` shows no sign of life. | hook |
| S3 | **Worktree key mismatch.** `origin_key` reads `root/.git/config` (`crates/adage-core/src/repo.rs:100-101`). In a linked worktree, `.git` is a *file*, so the key falls back to `name@rootcommit`, while the main checkout is keyed `github.com/owner/repo`. Evidence splits, and reflexes learned in the main checkout never fire in worktrees. Claude Code agent worktrees (`.claude/worktrees/…`, present in this very repo) hit this constantly. `gitobj.rs:60-74` already resolves `commondir`. No test covers it (`tests/repo_keys.rs`). | repo key |
| S4 | **Codex and OpenCode history are ignored by auto-learn.** `learn.codex_sessions` and `learn.opencode_exports` default to none (`crates/adage-cli/src/autolearn.rs:288-301`). Only transcripts that hooks name *after* install are remembered (`:302-307`). A Codex user's existing history does nothing. | config default |
| S5 | **Small histories learn nothing, and nobody says why.** `min_support=5` is the default. The user just sees "0 reflexes". | thresholds |
| S6 | **Nascent reflexes are logged, not surfaced.** Auto-learn writes `nascent` only to `learn.log` (live-learning.md, `changes[].change`). | learnlog |
| S7 | **Missing or blocked model.** Doctor reports *pass* "optional" (`doctor.rs:340-357`). The fetch uses `curl` against huggingface.co (`models.rs:82`; `adage-intent/src/model.rs:57`) with no mirror override. A proxy 403 is shown only in init/wizard. | models |
| S8 | **Fail-open errors are visible only in `doctor`/`watch`.** A hook that fails on every call looks exactly like "Adage has nothing to say". | failopen |
| S9 | **`delegate on` with the exec plugin not installed.** The base plugin goes quiet (`session-start:20`) whenever `exec/enabled` exists. `exec.rs:161-163` only *prints* "enable the adage-exec plugin". If the user forgets, **all** Adage hooks go silent. | exec marker |
| S10 | **Bare `adage` outside a TTY prints usage** (`main.rs:140`). `!adage` inside Claude Code tells you nothing about status. | main |
| S11 | **Auto-learn is off via env.** `ADAGE_AUTO_LEARN=0` in a dotfile shows up only in doctor. | config |

---

## 3. Zero-config design

### 3.1 Defaults

| Key | Default | Note |
|---|---|---|
| `learn.auto` | `true` | Already the default (`adage-store/src/config.rs:363`). |
| `instincts.enabled` | `true` | Already the default (`:371`). |
| Environment-effect reflexes | nascent | Already the case. |
| Execution | off | Already the case. |
| `learn.codex_sessions` | **`auto`** (new) | `~/.codex/sessions` (or `$CODEX_HOME`) is read when Codex hooks are registered, filtered to this repo's cwd. The user installed Adage into Codex, so that harness's history for *this repo* is in scope. |
| `learn.opencode_exports` | none in v0.1 | OpenCode learns forward only. P2: the plugin calls `opencode export` on `session.deleted`. |
| `learn.thresholds` | `default` | New **adaptive floor**: while a repo has fewer than 15 sessions, instincts carry it, and the status card says `learning: 3 of ~5 sessions needed before Adage forms its own reflexes`. Do not silently switch to `demo` thresholds; honesty beats early noise. |
| `runtime.control` | `0.1` | Later: `auto` drops it to 0.05 once the repo-level estimate's CI half-width is under 25% of the effect. |

### 3.2 Bootstrap at `SessionStart` (the core change)

In `adage-hook/src/lib.rs::run`, treat `SessionStart` as a learn point when **either**:

- the repo has no routes in the index, and its learn-queue `state.json` is absent or older than the index; **or**
- the index failed to load (self-heal).

In that case, call `on_turn_end(..)` with a new `Request.bootstrap = true`. The spawned learner:

1. runs `instincts::learn_only` first and compiles the index (the index is live within about 100 ms);
2. then runs the normal `learn_once`.

`SessionStart` also stops being fast-skipped for repos that have reflexes. It appends one small `session_start` spool record (`arm`, `propensity`, `generation`, `repo`, `harness_version`). This gives the savings estimator an unbiased denominator: today, sessions where nothing matched leave no record at all (S2).

### 3.3 Progressive disclosure: the notice channel

Add a **notice** channel: `Output::Notice(String)` rendered as Claude Code's `systemMessage` on `SessionStart`. The user sees it; the model does not. For Codex and OpenCode, notices go only to the status card and `watch`. Rules:

- At most **one line**, at most **once per repo per 24 h**, and only if there is something *new*:
  - first bootstrap;
  - new reflexes;
  - nascent reflexes waiting;
  - the savings estimate crossed its gate;
  - a health problem (§5).
- The hook reads a precomputed `learn-queue/<slug>.notice.json` written by the learner. The hook never computes anything.
- `ui.notices = daily|off` to silence it.
- Wording priority: a health problem, then something waiting for you, then value, then new reflexes.

Vocabulary shown by default:

| Default word | Hidden until |
|---|---|
| "reflexes" | — |
| "waits for you" | nascent (`adage review`) |
| "team reflexes" | packs |
| "measured against sessions where Adage stayed quiet" | "control" |
| — | strengths, lifecycle and delegation appear only inside `adage review`, `ui` and the map |

### 3.4 Bare `adage`: a status card, always

Replace `main.rs:129,140`. Bare `adage` prints the card whether stdout is a TTY, a pipe or `!adage` in Claude Code. `adage ui` opens the app. `--help` shows usage.

```text
adage 0.1.0 · github.com/acme/billing                         ● healthy
  14 reflexes active: 4 instincts, 9 learned, 1 from your team
  learning after each session · last: 2 min ago, 1 change
  last 30 days: 61 sessions · 188 directives · 152 followed · 7 dead ends headed off
  savings: ~$0.034 and 3.1 turns per session (95% CI $0.011–0.058)
           ≈ $1.87 over 55 sessions, vs 6 where Adage stayed quiet
  1 install step waits for you → adage review
  adage ui · adage map · adage doctor
```

- **Outside a repo**: one line per repo, with the same columns.
- **Health dot**: the worst doctor status, with the single fix line for it.
- **Before the gate**, the savings line reads: `measuring savings: 4 of 10 quiet sessions so far (Adage stays quiet in 1 of 10 sessions so its effect can be measured)`.

### 3.5 Consent moments that stay

| Moment | Where | Terminal? |
|---|---|---|
| Hooks registration (CLI path) | `connect`/wizard, with a diff | yes |
| Environment-step approval | `adage review` | yes |
| `delegate on` and `review delegate` | unchanged | yes |
| Model download | wizard, `adage models fetch`, or the ui Health fix | yes |
| Trusting a pack signer outside the repo's pinned file; repo signer file changes | `adage review` | yes |

The plugin install is itself consent to hooks. Nothing else prompts.

---

## 4. Value visibility: an honest savings estimate

### 4.1 Per-harness data that exists

| Harness | Source | Tokens | Cost |
|---|---|---|---|
| Claude Code | `~/.claude/projects/*/<session_id>.jsonl`; assistant records carry `message.usage` {`input_tokens`, `cache_creation_input_tokens`, `cache_read_input_tokens`, `output_tokens`}, `message.model`, and top-level `version`. Several records share one `message.id` (`adage-learn/src/import.rs:4`), so **dedupe by id**. | yes | **none**: price it |
| Codex | `event_msg` `token_count` with `info.total_token_usage` {`input_tokens`, `cached_input_tokens`, `output_tokens`, `reasoning_output_tokens`}. Take the last non-null value. `info` can be null (see the sim fixtures). Model from `turn_context`; `session_meta.cli_version`. | yes | price it |
| OpenCode | exported message `info.cost`, `info.tokens` {`input`, `output`, `reasoning`, `cache.read`, `cache.write`}, `modelID` | yes | **harness-reported** |

**Pricing**: a pinned table (`crates/adage-core/src/pricing.toml`, with its date), per model and token class. An unknown model means tokens and turns only, with no dollars. Always label the figure **"API-list equivalent"**: subscription users do not pay per token, and saying so is part of being honest.

### 4.2 The ledger

The learner already parses every transcript for the repo through the parse cache. Extend the parsed session (`adage-learn`, new `ledger.rs`) with a `SessionSummary`: `{session_id, harness, harness_version, models[], started_at, model_turns (deduped assistant messages), tool_calls, first_edit_turn, tokens{...}, cost{usd, source: harness|priced@<date>|none}, asked_for_help}`. `asked_for_help` reuses the detector at `import.rs:122`. Sidechain/subagent records are excluded in v0.1.

Write `$ADAGE_HOME/ledger/<slug>.jsonl`, one line per session, replaced by `session_id`. It holds no prompts, commands or paths. It is joined to spools by session id: spool filename equals the Claude and OpenCode session id, and the Codex thread id.

### 4.3 The estimator (`crates/adage-cli/src/savings.rs`)

- **Population**: sessions of repo R that have a `session_start` spool record (exposed to an index with at least one reflex for R), are finished (a `SessionEnd`/`Stop` record, or transcript mtime more than 30 min old), and appear in the ledger.
- **Arm and propensity**: from the spool. Arm assignment is already deterministic by session hash (`adage-runtime/src/lib.rs:41-66`).
- **Outcomes Y**: `cost_usd` (API-list), `model_turns`, `turns_to_first_edit`.
- **Primary estimand**: intent-to-treat per session, Δ = E[Y | control] − E[Y | treated].
  - Hajek IPW with the recorded propensities, so a changed `runtime.control` stays valid.
  - Winsorize Y at the pooled 95th percentile, and disclose it in `--json`.
  - This is the clean estimate: the arm is randomized at session start, before anything downstream.
- **CI**: percentile bootstrap, 2,000 resamples stratified by arm. The seed is derived from the sorted session ids, so every surface shows the same numbers.
- **Total saved** = Δ × N_treated, with CI = CI(Δ) × N_treated.
- **Display gate**: `n_control ≥ 10` and `n_treated ≥ 30`, per repo. Below the gate, show counts only.
- **Above the gate**:

  | CI position | Shown |
  |---|---|
  | lower bound > 0 | "saved ~$x (CI a–b)" |
  | CI straddles 0 | "no measurable difference yet (±$h)" |
  | upper bound < 0 | "Adage appears to add ~$x per session. See adage review." Honesty cuts both ways, and this also triggers a notice. |

- **Quality guardrail** shown alongside: the rate of `asked_for_help` per arm, so savings never hide worse outcomes.
- **Per-reflex attribution** (map and `review` detail only, labelled *indicative*):
  - Units are sessions where reflex r applied: *delivered* in treated, *would_have* in control.
  - Outcome is model turns after the first application, located by matching spool `ts` to transcript timestamps.
  - Same gate per reflex, so most reflexes will show counts only. That is correct.
  - Stimuli after a tool call can be post-treatment effects of earlier reflexes, which is why these figures are labelled indicative and never summed into the headline.
- **Counts before the gate**, all directly observed from spools:
  - directives delivered;
  - followed (existing judgements);
  - dead ends headed off (an inhibit was delivered and the avoided command's template key was not observed later in the session);
  - finish lines met.

  These counts are never converted to dollars.

### 4.4 Surfaces

The learner writes `ledger/<slug>.summary.json` (estimate, gate status and counts) after each run. Every surface reads it.

| Surface | Shows |
|---|---|
| Bare `adage` | the card's savings line |
| `adage watch` | the header line |
| Map | `/api/savings` plus per-reflex panels |
| `SessionStart` notice | when the estimate first crosses the gate, then weekly |

---

## 5. Failure modes: self-heal or explain

| Failure | Self-heal | Explanation (one line; CC notice plus card plus doctor) |
|---|---|---|
| No binary (plugin) | Bundled binaries (P0-1). The shim also writes `$DATA/status` if it fails, and every hook command is wrapped as `sh -c '"$bin" … \|\| exit 0'`. | The shim prints a JSON `systemMessage` on stdout: `adage: no binary for <os-arch>; install with brew/npm`. |
| Stale or missing index; tracks/index digest mismatch | The hook records failure class `index` and treats it as a bootstrap request. The learner recompiles from tracks. | Only if 3 recompiles fail: `adage: couldn't rebuild reflexes (…); run adage doctor`. |
| Corrupt session state | Already rebuilt from the spool (`lib.rs` rebuild). | None. |
| Corrupt `config.toml` | The affected key already keeps its default. | Card: `config.toml line 4 ignored: …` |
| Corrupt track or pack | Quarantine it to `tracks/.quarantine/`, re-index without it, and log to learn.log. | `1 reflex set was damaged and set aside`. |
| Unsupported harness version (payload shape) | After 5 `payload`-class fail-opens for a harness+version within 24 h, write `standdown/<harness>.json`. Hooks for that version then exit immediately. The version comes from the Claude transcript `version`, Codex `cli_version`, and so on. The standdown is cleared on a new version. | `adage: paused for Codex 0.160 (hook format changed); your agent is unaffected.` |
| Hook timeouts | Record `duration_us` in every spool record. Enforce a self-budget: skip the embedding if more than 1.5 s has elapsed. Doctor warns when p95 exceeds 500 ms. | Notice only on a sustained p95 > 2 s. |
| Repo with no history | Instincts via bootstrap. | `learning: 0 of ~5 sessions; N instincts active meanwhile`. |
| Conflicting reflexes | Existing interlocking in the learner. Precedence: first-hand over team pack over instinct. Extinguished shadows imports. | Map only; never a notice. |
| Worktrees and subdirectories | Key via the common dir's `origin` (S3). | None. |
| Learner repeatedly erroring | The existing `condition_error` lines. After 3 consecutive errors, back off (double the interval). | `adage: learning failed 3× here (lock); run adage doctor`. |
| `delegate on` without the exec plugin | The base shim checks `CLAUDE_PLUGIN_ROOT/../adage-exec` (or a marker the exec shim writes on each start); if absent, it keeps serving. | `adage: delegation is on but the adage-exec plugin isn't installed; serving without execution.` |
| Model download blocked (e.g. this container's proxy) | **Never retried automatically.** `models fetch` classifies the curl exit code (6/7 no network; 22 with 403/407 is a proxy or policy block; 35/60 is TLS interception, advise `CURL_CA_BUNDLE`/`SSL_CERT_FILE`). It writes `models/.fetch-failed.json {class, host, at}`. New `ADAGE_MODEL_BASE_URL` points at a mirror; this is safe because SHA-256 pins are unchanged. `models install <dir>` stays. Intent reflexes remain disabled; everything else works. | Doctor: *pass-with-note* `intent grouping off: huggingface.co blocked by proxy (403). Use a mirror (ADAGE_MODEL_BASE_URL) or adage models install <dir>.` The card shows `(prompt grouping off)` in grey. It is **never** a notice. |

---

## 6. Implementation plan

### P0: must ship in v0.1

1. **Bundled plugin binaries.**
   - `release.yml` gains a job that, after dist builds, assembles `plugin/claude-code/dist/<5 targets>/adage` plus `SHA256SUMS` and pushes it to a `marketplace` branch, or a separate `adage-marketplace` repo.
   - `marketplace.json` sources that ref, and its description drops "Needs the adage binary on PATH".
   - The shim change is small: target detection already exists. The shim also prints JSON `systemMessage` on stdout on failure (`plugin/claude-code/scripts/session-start:77`), and hook commands tolerate a missing binary.
   - Same for `claude-code-exec`.
2. **Bootstrap at `SessionStart`** (§3.2).
   - `adage-hook/src/lib.rs` (the learn-point branch at `:210`, the early return at `:241`), `harness/mod.rs:68`.
   - `Request.bootstrap` in `adage-hook/src/autolearn.rs`; two-phase learner in `adage-cli/src/autolearn.rs::learn_once`.
   - `fast.rs:42`: `SessionStart` writes the `session_start` record when the repo has routes.
3. **Worktree key fix.** `origin_key` reads the common dir's `config` via `gitobj` (`adage-core/src/repo.rs:100`). Add tests in `tests/repo_keys.rs` for a linked worktree and a subdirectory cwd.
4. **Notice channel.** `Output::Notice` becomes `systemMessage` in `harness/claude.rs::render`. The learner writes `notice.json`, with a 24 h throttle and `ui.notices`. Before shipping, verify `systemMessage` on `SessionStart` against the live Claude Code version.
5. **Repo-scoped reading.** Before parsing, filter `import::discover` by Claude project-dir slug and first-line cwd (reusing the `--cwd-prefix` logic, `main.rs:186-193`). Codex: `session_meta.cwd`.
6. **Bare `adage` status card** (`main.rs:129,140`; new `crates/adage-cli/src/status_card.rs`). The TUI moves to `adage ui`.
7. **Ledger plus savings estimator with the gate** (`adage-learn/src/ledger.rs`, `adage-cli/src/savings.rs`, `adage-core/src/pricing.toml`). Surfaces: card, watch header, notice. The map can wait until P1.
8. **Self-heal set**: index rebuild on load failure; learner backoff; exec-plugin-missing guard (`session-start:20`, `exec.rs:161`); model fetch classification plus `ADAGE_MODEL_BASE_URL` (`models.rs:82`, `adage-intent/src/model.rs:57`).
9. **`learn.codex_sessions = auto`** (`adage-store/src/config.rs`, `autolearn.rs:288`).
10. **README rewrite.** Install is `/plugin install` or brew, then "start a session". Everything else goes under "Going further".

### P1: v0.1 if time allows, otherwise v0.2

- `adage share --into-repo`, plus repo-pack auto-bind with a pinned signer hash (§1c).
- Per-reflex indicative savings in the map.
- Harness-version standdown.
- Hook `duration_us` and the doctor p95 check.
- `adage connect` alias for the wizard's hook step, with user scope preselected.

### Later

- `runtime.control = auto`.
- OpenCode export-on-delete learning.
- Team-level savings aggregation.
- Subagent/sidechain cost accounting.
- A "review" slash command for persona (a), once there is a design for a person-verified approval inside the harness.

### Acceptance tests (in `crates/adage-cli/tests/`, reusing the `first_run.rs` World)

1. **`zero_config_plugin.rs`**:
   - Setup: fresh `HOME`/`ADAGE_HOME`; `~/.claude/projects/<slug>/` seeded with 12 sim Claude sessions for repo W (`sim/fixtures`) and 5 for unrelated repo X; W has a Makefile.
   - Run `plugin/claude-code/scripts/session-start` with a staged `dist/`, piped a `SessionStart` payload for W.
   - Assert: exit 0 within 200 ms; stdout JSON has a `systemMessage` containing "learning from"; a learner was spawned (`ADAGE_TEST_NO_SPAWN=record`).
   - Run the learner, then pipe `UserPromptSubmit` "fix W-2042".
   - Assert: `additionalContext` contains the lookup directive, with no `adage` command ever run.
   - Assert: X's transcripts were never opened (parse-cache manifest has no X entries).
2. **`worktree_keys`**: learn in the main checkout, then a hook in a linked worktree gets the same directives.
3. **`no_history_instincts`**: an empty projects dir plus a Makefile. `SessionStart` followed by a prompt yields an instinct directive in session 1. The card shows "0 of ~5 sessions".
4. **`savings_gate`**:
   - Synthetic ledger and spools with 25 treated and 4 control sessions: the card shows counts and no `$`.
   - With 40 treated and 12 control and a known Δ: the CI covers the true Δ in at least 93 of 100 seeded simulations, and the output is identical across two invocations.
   - With Δ < 0: the "appears to add" wording appears.
5. **`pricing_honesty`**: a session with an unknown model shows tokens and turns, with `cost.source = none` and no dollars.
6. **`model_blocked`**: `PATH` curl stub exits 22 with "403". `models fetch` writes `.fetch-failed.json` with class `blocked`. Doctor passes with the note. Hooks and learning are unaffected. No automatic retry.
7. **`exec_marker_without_plugin`**: `exec/enabled` is present with no exec plugin dir. The base shim still links the real binary and emits the notice.
8. **`notice_throttle`**: two `SessionStart`s within 24 h give one notice; a new nascent reflex produces a new notice the next day.
9. **`index_self_heal`**: truncate `index` and send a `SessionStart`. A rebuild is spawned, the next prompt delivers, and no notice appears.

### Critical Files for Implementation
- /home/user/adage/crates/adage-hook/src/lib.rs
- /home/user/adage/crates/adage-hook/src/autolearn.rs (and /home/user/adage/crates/adage-cli/src/autolearn.rs)
- /home/user/adage/plugin/claude-code/scripts/session-start (plus /home/user/adage/.github/workflows/release.yml, /home/user/adage/.claude-plugin/marketplace.json)
- /home/user/adage/crates/adage-core/src/repo.rs
- /home/user/adage/crates/adage-cli/src/main.rs (bare-command status card; new savings.rs and adage-learn/src/ledger.rs)