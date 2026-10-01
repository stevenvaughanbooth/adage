# Adage from first principles: an audit of the everyday loop

Status: audit, 2026-10-01. Slice 1 (§4) is implemented in the change that adds this document; the
other slices are proposals for the design phase. Method in §2; every number here was measured on
the machine named there, not estimated.

## 1. What Adage has to do, and what each step should cost

A person installs Adage once. After that, five things happen, and each answers a question the
person will ask sooner or later.

| Step | What happens | The question it answers | What it should cost |
| --- | --- | --- | --- |
| 1. Set up | Hooks are registered; the repository's files bind instincts | "Is it on?" | Seconds; reads this repository only |
| 2. A session runs | Each hook reads the prompt or tool call, answers from the index, records the event | "Is it doing anything?" | Milliseconds per hook; never blocks the agent |
| 3. A session ends | A background learner reads this repository's new traces and updates its reflexes | "Is it learning?" | Seconds, in the background, once per session at most |
| 4. Watching | `adage watch`, `adage ui`, `adage map` show events, reflexes and health | "What did it do, and why?" | Near zero while idle |
| 5. Health | `adage doctor` (and the health lamp everywhere) | "Is anything wrong, and what do I do?" | One plain sentence and one fix per problem |

Four rules follow from this, and three of them are already in `docs/zero-config.md` §0:

1. **Never wait on anything slow or remote.** No hook and no learner touches the network.
2. **Nothing fails silently.** A step that cannot do its job says so, once, where the person is
   looking.
3. **Say what things do.** A reflex is shown by what it tells the agent, not by its id, and a
   theme word (fire, prime, forming) comes with its meaning. Zero-config §0 adds "only where a
   decision needs them"; the lexicon says "everywhere" (F7).
4. **Work in proportion.** Read what the question needs (this repository, its tracked files, its
   new traces), and do nothing while nothing changes.

## 2. Method

- **Machine**: the author's Mac (macOS 26). The repository lives at `~/Documents/skill-compile`,
  which iCloud Drive syncs ("Desktop & Documents Folders", with "Optimize Mac Storage" on). There
  are 24 worktrees under `.claude/worktrees/`, and a paid study ran during the audit (its binaries
  and data are isolated under `/private/tmp`, and nothing here touched them).
- **Baseline**: a fresh clone of the same repository outside iCloud
  (`git clone`, 11 s, 535 commits, 166 MiB, every object packed), with an empty `ADAGE_HOME`.
- **Tools**: the installed release binary, a symbolized debug build for stacks (`sample`), and
  hook calls replayed against a copy of the data directory.

## 3. Findings

### F1. Adage waited on iCloud, so learning stopped and nothing said so (fixed: slice 1)

- **What happened**: iCloud had offloaded 4,187 of the repository's 4,974 loose git objects and
  some working files, and `cloudd` sat at 99% CPU, so downloads stalled. A read of one offloaded
  object (a few hundred bytes) was still blocked after 60 s. Every learner run after 10:04 hung:
  one for about 30 minutes, then each new one as sessions stopped. The last learn request handled
  was from 09:50, and by the end of the audit no learner had finished in over an hour.
- **Where**: with symbols, the stack was `instincts::learn_only → plan_repo → bind_with →
  declared::extract → files::RepoFiles::head → read()` on `.github/workflows/diagnose.yml`, an
  offloaded file. `adage condition --instincts-only` took over 120 s in the main checkout, against
  0.19 s on the clean clone.
- **Principle broken**: rules 1 and 2. Triggering a download is touching the network.
- **Fix**: `adage_core::cloud::never_download()` sets macOS's per-process policy
  (`IOPOL_MATERIALIZE_DATALESS_FILES_OFF`), so a read of an offloaded file fails at once and every
  reader skips it like any unreadable file. `adage train`, `adage update` and
  `adage review delegate` keep the system default, because the policy passes to the programs they
  start (agent sessions, an installer, and the reflex's command staged in the sandbox). `adage doctor` gains a
  `cloud files` warning, shown only when files are offloaded:
  `a cloud drive (iCloud Drive or another sync app) keeps at least 1690 git objects and 63 files of
  ~/Documents/skill-compile only in the cloud (such as spikes/README.md); Adage skips those
  instead of waiting for a download, so it sees less of this repository`, with the fix
  `move the repository out of the synced folder (for example to ~/code), or in Finder
  Control-click its folder and choose Keep Downloaded`.
- **Unread is unknown, not absent.** Skipping an offloaded file must not change what Adage
  believes about it. The first version of this fix let a learner run from an older worktree,
  whose `crates/adage-term/src/tokens.rs` was offloaded, extinguish the "don't hand-edit
  generated files" instinct for that file. The file reader now records a file that is there but
  unread (`only in the cloud, not downloaded`, or `could not be read`), and a re-bind with an
  incomplete read keeps the instincts it could not confirm. Only a complete read removes one; a
  learned route taking it over, a person's rejection, or turning it off still remove it at once.
- **Result on the same machine**: the same command finishes in 0.16 s with the release binary.
  The background learner, which had not finished a run in over an hour, finished in 11.0 s
  (outcome `ok`, 6 changes, every queued request handled) and has run on its own after each
  session since. `doctor` reports 20 passed, 1 to look at (the cloud warning), 0 failing.

### F2. The file walk spent its budget on output git ignores (fixed: slice 1)

- **What happened**: `RepoFiles::walk` (5,000 entries at most) went into
  `demo/results/*/raw/`. That directory is gitignored and holds more than 5,000 run files, so the
  walk ran out before it reached `crates/`. A working checkout and a clean clone bound different
  instincts.
- **Fix**: the walk skips what git ignores, through the existing `adage_core::gitignore`, with a
  new `Ignore::ignored_dir` so that directory-only rules (`raw/`) apply before descending.
  Instincts read "the repository's files", and ignored output is not part of the repository; the
  walk already skipped `dist`, `build` and `target` for the same reason.

### F3. The live view is empty until a repository has reflexes

- **What happened**: since 45c1285 (2026-09-28), a hook returns before recording anything when
  the index holds no reflexes for the repository (`respond` in `crates/adage-hook/src/lib.rs`:
  "nothing installed for this repository: the common, cheapest path"). The repository had none,
  because learning was stalled (F1) and the index was unreadable (F5). From the upgrade at 10:08
  until the index was rebuilt at 11:23, no hook recorded an event for any session (activity is
  written to `sessions/<id>.spool`; older binaries wrote `.live`), and `adage watch` showed
  nothing. A replayed hook returned in 10 ms and wrote nothing.
- **After the repair**: once the repository had reflexes, every session's tool batches and
  prompts appeared in `adage watch` ("tools · 23 calls, nothing to say"), with each conditioning
  run under them, and the hooks delivered learned reflexes to the session that wrote this
  document: after an edit to `crates/**/*.rs`, `cargo fmt --all` and
  `cargo clippy --workspace --all-targets`.
- **Principle broken**: rule 2. "Not recorded because there is nothing to deliver yet" looks
  exactly like "broken".
- **Proposal**: keep the fast path for delivery, but record one small event per session in a
  repository (session started, session ended) so that `watch` can say
  `skill-compile · session running · no reflexes here yet · learning starts when it ends`. The
  record costs one append per session, not one per tool call.

### F4. "Is it learning?" cannot be answered, and `doctor` says all is well

- **What happened**: while learning was stalled for over an hour, `doctor` printed
  `PASS auto-condition lock · no learner stuck` and `PASS auto-condition queue · none waiting`.
  A learner counts as stuck only after 15 minutes (`STUCK_MS`, `doctor.rs`), each new session
  replaced the hung learner with another, and the queue check skips requests while a learner holds
  the lock. The learner's own state file said `"outcome":"running"` the whole time.
- **Proposal**: show the running learner the way people think about it:
  `learning · running for 4 min (usually 0.3 s) · 6 sessions waiting`, in `watch`, the status card
  and `doctor`. Call a run stuck at the larger of 60 s and 20 times its usual duration, and count
  requests waiting behind a held lock.

### F5. The upgrade made existing data unreadable, and the message was a parser error

- **What happened**: the lexicon change (e9f23f2) renamed stored values (`active` to `firm`,
  `aspect: green` to `strength: fire`) and deliberately keeps no reader for the old words: Adage is
  unreleased. Data in the author's own data directory (demo tracks from 2026-09-23 and the
  compiled index) stopped loading. `doctor` said `unknown variant 'active', expected one of …`,
  and hooks served an empty index.
- **Done in this audit**: the four demo route files were migrated by hand with the lexicon's
  mapping (a backup is in `~/Library/Application Support/Adage-backup-2026-10-01`), and the index
  is rebuilt by `adage condition`. No compatibility aliases were added; that decision stands.
- **Proposal**: when a stored file fails to parse, say what it means and what to do:
  `written by an older Adage (before 2026-09-28): run adage condition to rebuild it`.
  The demo no longer writes into the real data directory (`demo/run.py` and `adage ui --demo`
  both use their own `ADAGE_HOME`), so this cannot recur from the demo.

### F6. Hook wiring went out of date with no warning

- **What happened**: `adage init` started registering `PreToolUse` on 2026-09-29 (9056bc8).
  Settings written before then have 6 of the 7 events, and `doctor` still printed
  `PASS hooks: claude-code`.
- **Proposal**: `doctor` compares the registered events with the current set and names the
  missing ones, with the fix `adage install claude-code --write`.
- **Related**: during the audit, `adage init` registered user-scope hooks (all 7 events) in
  `~/.claude/settings.json` and `~/.codex/hooks.json`, while the repository's own
  `.claude/settings.local.json` still registers 6. `doctor` warns about running twice only for
  the plugin plus a settings file. The live feed showed one record per event, consistent with
  Claude Code merging identical hook commands, so no harm was seen; `doctor` should still say
  which registration is redundant.

### F7. Two approved documents disagree about words, and output says what a reflex is called, not what it does

- **The conflict.** `docs/lexicon.md` (approved 2026-09-28) says every user-facing word follows
  the theme ("trace" for session, "conditioning" for learning, fire/prime/inhibit). Zero-config
  §0 rule 3 says those words stay out of default surfaces until a decision needs them. Both are
  approved; the code follows the lexicon (`crates/adage-cli/src/words.rs::prose` rewrites
  `doctor` lines into it). This is a decision for the product owner, not a bug (question 3, §6).
- **A real bug either way.** `prose()` rewrites by blanket regex: any "red" or "green" in a
  sentence becomes "inhibit" or "fire", whatever it meant.
- `adage condition --instincts-only` prints `PRIME ◉ instinct-u4-4baff2 — no traces yet ·
  forming` and `instincts 0 bound · 3 advice`. The reflex's meaning, "Don't hand-edit generated
  files: crates/adage-term/src/tokens.rs", appears nowhere: a reflex is shown by its id.
- `next adage review · 3 reflexes wait for a decision` counts reflexes that are already delivered
  and have never been reviewed (`review.rs`, instincts and new conditioned reflexes). "Waits"
  suggests they are blocked.
- **Proposal**: whichever words win, every surface names a reflex by what it does (its summary)
  and a lexicon word appears with its meaning the first time on a screen (`prime: a suggestion`),
  as the site already does with hover definitions. Strength and lifecycle words go through the
  lexicon's own types, not regexes over finished sentences.

### F8. Work out of proportion

| What | Measured | Why it is too much | Proposal |
| --- | --- | --- | --- |
| `adage watch`, idle | 482 CPU-minutes in 7 days (about 5% of a core, all the time) | It reruns every health check every 3 s (`HEALTH_EVERY`), 70 ms of CPU each, including checksum checks of files that have not changed | Recheck when the data directory or index changes, or every 30 s; cache checksum results by size and modification time |
| Default history for `adage condition`, `adage sense` and `init`'s sensing step | 556 Claude transcripts (418 MB, 425 project directories) and 203 Codex rollouts (232 MB) | The automatic learner already reads only the repository's own traces; the manual commands read everything | Default to the current repository, as the learner does (zero-config §1 already promises "other repos' files are not opened") |
| A learner pass after each session | 11.0 s cold, 8.1 to 8.6 s warm, reading 97 to 98 of the repository's traces | It costs in proportion to the repository's whole history, not to what is new: about a third re-parses transcripts that grew since the last pass (live sessions' transcripts always miss the cache), and about a third recompiles the whole graph from every trace (profiled with a debug build) | The persistent corpus in the learning-model design phase, so a pass reads only new traces |
| Repository identities | 211 cached, one per worktree or temporary directory seen | Expected, but it shows the scale `init` sensing walks | See slice 3 |
| `bind::local_keys` | Walks first-parent history to the root commit on every bind, even when `origin` names the repository | A legacy key form, needed only when a pack is scoped by it | Compute it only when a pack's key has that form |
| `gitobj::Repo::object` | Opens the loose file before searching packs | git searches packs first; the loose-first order costs a failed open per packed object | Search packs first, as git does |

### F9. `init` reads every harness's history across every repository

`init` step 4 ("sense your traces") reads all configured Claude, Codex and OpenCode history. On
this machine it was also the step that hung (F1). The author proposed the direction for slice 3:
read Claude Code traces by default, and ask which repositories to learn from.

### F10. The map is hard to find, and empty when it is found

`adage watch` is the live pane alone, with no tabs; `m` opens the map in the browser, but only
the footer says so. The map is tab 3 of `adage ui` (1 Live, 2 Reflexes, 3 Map, 4 Health,
5 Settings), and `adage map` opens it in the browser. With no reflexes it is empty, with no hint
why. `adage map --demo` shows a sample.

### F12. Instincts follow whichever checkout's session ended last

The learner re-binds instincts from the root of the session that triggered it. With 24 worktrees
on commits up to 8 days apart (the main checkout is on ce5e311), the repository's instincts
change with whichever session stopped last: at 11:26 a run from a worktree 21 hours old re-bound
them. This is the "one bound location" question already open in `docs/marketplace.md`; slice 1
only stops an unread file from removing one.

### F11. The environment works against git as well as Adage

These are not Adage's problems to fix, but Adage should name them:

- The repository is in iCloud with storage optimization on. Any `git` command that needs one of
  the 4,187 offloaded objects can hang too. The condition comes and goes: by 12:15 the loose
  objects had been packed (317 left, none offloaded), `cloudd` was down to 8% CPU, and
  `doctor`'s warning had gone, as it should. It returns whenever iCloud offloads files again.
- Each worktree's Cargo `target/` directory (gigabytes of build output) is inside the synced
  folder, which is the likely reason `cloudd` sat at 99% CPU.
- The main checkout is on a `main` 8 days old (ce5e311), so instincts bound there describe old
  code.

### F13. A learned reflex told the agent to run a command that cannot work

After an edit to `crates/adage-cli/src/main.rs` in the session that wrote this document, the hook
delivered (route `run-4abe85`, prime, learned from 9 of 43 traces):

```text
In this repository, changes to files matching crates/adage-cli/src/*.rs must be followed by running:
  cargo test adage-cli crates/adage-cli/tests/share.rs crates/adage-exec/tests/escape/main.rs crates/adage-instincts/tests/fixtures/pnpm/src/main.ts
```

`cargo test` takes one filter, so this fails before it runs anything; the "arguments" are file
paths from the sessions it learned from. Later the same session was told, on a prompt, that
"requests like this one usually start by running: `echo ---`": the separator agents print between
command outputs, learned as a first step. Two problems: a learned command is never checked to be a
command that can run, and prime-level advice ("consider this") is worded as an obligation
("must be followed by"). The same session also got two sound reflexes (`cargo fmt --all`,
`cargo clippy --workspace --all-targets`, `cargo build -p adage-cli`), so this is about the
learner's guard, not the idea.

- **Proposal**: before a learned command is delivered, parse it with the command table and reject
  arguments that are file paths a test runner does not accept in that position; word prime
  directives as advice ("agents here usually run …") and keep "must" for firm obligations.

## 4. Slices

Ordered by value: what makes the loop work comes first, then what makes it legible, then what
makes it cheap.

| Slice | Covers | Size | State |
| --- | --- | --- | --- |
| 1. Never wait on the cloud; unread is unknown; walk only tracked output | F1, F2 | Small | Implemented, with tests; verified on the author's machine |
| 2. Learning you can see | F3, F4, F6 | Medium | Implemented, with tests |
| 3. `init` scoped to what you choose | F9, F8 (history) | Medium | Implemented, with tests: `learn.repos`, the repository step, manual commands scoped (`--all`) |
| 4. Say what things do | F7, F10, F5 (message) | Medium | Implemented, with tests (lexicon words kept; every surface says what a reflex does) |
| 5. Work in proportion | F8 (watch, identities, `local_keys`, pack order, learner passes) | Small to medium | Proposal (the manual-history part landed with slice 3) |
| 6. Learned commands that can run | F13 | Small | Implemented, with tests |

### Slice 3 design sketch: `init` scoped to what you choose

1. **Find repositories without reading transcripts.** Claude Code's project directories are
   named after the working directory, and each transcript's first line records its `cwd`. Group
   them by repository root and key: worktrees under `.claude/worktrees/` belong to their main
   checkout, and temporary and deleted directories are dropped. Show each with its session count
   and when it was last used. Nothing is parsed yet.
2. **Ask, with a default.** In the wizard, the current repository is preselected, and others are
   listed from most to least recently used. With `--yes`, take the current repository only. With
   `--print`, list the repositories and do nothing.
3. **Remember the choice** in `config.toml` (`learn.repos`), so the background learner and the
   manual commands honour it. A repository not chosen still gets hooks if they are registered
   with user scope, but no history is read for it until the person adds it.
4. **Claude Code by default.** Codex and OpenCode history are offered only when their hooks are
   registered (as `learn.codex_sessions = auto` already does for the learner).
5. **Instinct packs at the same moment.** The packs-and-marketplace work
   (`docs/marketplace.md`) has `init` turn on base instinct packs; the repository choice and the
   pack choice belong on the same screen, and slice 3 should land after it.

## 5. Decisions closed

- Adage never triggers a cloud download, interactive commands included. Reporting an offloaded
  file is better than a hidden download the person did not ask for.
- No compatibility readers for pre-lexicon data. Local data is migrated, not aliased.
- Instinct walks list tracked and trackable files only; what git ignores is not the repository.
- Missing information never removes a reflex. A file that is there but unread leaves the
  binding as it was; only a complete read, a learned route, a person, or configuration removes an
  instinct.

## 6. Questions for the next phase

1. Slice 2: should session start and end be recorded for repositories with no reflexes (one
   append each), or should `watch` derive "session running" from the learner's queue alone?
2. Slice 3: does the repository choice apply to hooks too (user-scope hooks that stay silent in
   unchosen repositories), or only to which history is read? *Decided: history only. Hooks record
   sessions in every repository they are registered for; `learn.repos` decides which repositories'
   history is read, and reflexes already learned keep firing.*
3. Slice 4: where do the lexicon's words appear? The lexicon (approved 2026-09-28) says on every
   user-facing surface; zero-config §0 rule 3 says only where a decision needs them. One of the
   two should be amended so the code has one rule.
4. Should `doctor` also warn when the repository sits in a synced folder but nothing is offloaded
   yet, since that is when moving it is cheapest?
