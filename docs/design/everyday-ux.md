# Adage, understood: the everyday experience

> Status: design, 2026-10-01, for review. It sits beside two approved documents and changes
> neither: [redesign-plan.md](redesign-plan.md) decides how Adage looks (tokens, lamps, the output
> grammar), and [the lexicon](../lexicon.md) decides its words. This document decides **what each
> screen must tell the person, in which state, and how it proves it is true**. The evidence is in
> [first-principles-audit.md](../first-principles-audit.md) (F1 to F13), measured on a real
> machine. Where a choice belongs to the product owner, it is listed in §9 rather than made here.

## 1. The model a person needs

One paragraph, which every surface must agree with:

> Adage reads the sessions your coding agent has already had in a repository, notices the steps
> that repository keeps needing, and reminds the agent at the right moment. Each reminder is a
> **reflex**. Adage never runs anything unless you delegate it. You can always see what it
> learned, what it told your agent, and whether that helped.

Three nouns carry the whole model: **repository**, **session** (the lexicon's *trace*), and
**reflex**. Everything else (instincts, packs, strength, lifecycle, control sessions) is detail a
person meets when a screen needs a decision about it.

## 2. Six questions, one answer each

Every surface exists to answer one of these. If two surfaces answer the same question, they must
give the same answer from the same data.

| # | The question | Where it is answered | What goes wrong today (audit) |
| --- | --- | --- | --- |
| Q1 | Is it on, here? | `adage` (the card's header) | In a worktree the card names the worktree's folder, not the repository |
| Q2 | Is it doing anything right now? | `adage watch` | Nothing is recorded in a repository with no reflexes, so the pane is empty (F3) |
| Q3 | Is it learning? | The card's `learning` row, `watch`'s conditioning lines | A learner stuck for over an hour showed as passing (F4) |
| Q4 | What did it tell my agent, and why? | `watch` (enter on a line), `adage review` | Reflexes are shown by id (`run-7c62ab`), not by what they say (F7); one told the agent to run a command that cannot work (F13) |
| Q5 | Is anything wrong, and what do I do? | The card's lamp and one fix line; `doctor` for detail | The fix line is a paragraph; hooks missing an event passed (F6) |
| Q6 | Did it help? | The card's `savings` row | `activity: 3 sessions in 30 days` when dozens ran, because unrecorded sessions are not counted |

## 3. Rules every surface follows

These add to style.md's principles; they are about truth and comprehension, not looks.

1. **Healthy is proven, not assumed.** A green lamp needs positive evidence: for hooks, an event
   received from a session in the last day, not only a settings file that names the binary; for
   learning, a run that finished, not the absence of an error. With no evidence either way, the
   lamp is neutral and says why ("no session here since the upgrade").
2. **Running things show how long and how long they usually take.** `learning · running for
   4 min (usually 8 s)`. A run is stuck at the larger of 60 s and 20 times its usual time.
3. **Counts say what they count.** A number that leaves something out says so:
   `3 sessions recorded (recording began 11:23)`, never a bare `3 sessions`.
4. **A reflex is shown by what it tells the agent.** Its summary, with the files it is about:
   `After editing crates/**/*.rs, run cargo fmt --all`. The id appears only in `--json`, in the
   map's detail panel, and where a command needs it as an argument.
5. **A theme word comes with its meaning** the first time on a screen (`prime · a suggestion`),
   as the site does with hover definitions. Which surfaces use theme words at all is §9, Q-A.
6. **One problem, one line, one command.** The card shows at most one fix, in one line of at most
   100 columns, ending in the command that fixes it or the screen that explains it.
   `doctor` carries the full sentence.
7. **Nothing waits on the network or the cloud.** A file Adage cannot read without a download is
   skipped and named, never waited for (audit slice 1, done).

## 4. The states a repository can be in

Each surface must have a defined rendering for each state, tested from shared fixtures
(redesign-plan §9).

| State | Means | Card lamp and line | `watch` header |
| --- | --- | --- | --- |
| S0 Not connected | No harness has Adage's hooks | ○ `not connected · adage init` | `not connected: sessions here are not seen` |
| S1 Watching, nothing learned yet | Hooks work; fewer sessions than learning needs | ● `learning: 3 of about 5 sessions so far` | `watching · learning starts after a few sessions` |
| S2 Working | Reflexes exist and hooks deliver them | ● `8 reflexes in use` | `live` |
| S3 Learning stuck | A run exceeded the stuck rule, or failed 3 times | ● amber `learning stuck for 12 min (usually 8 s) · adage doctor` | `learning stuck` |
| S4 Seeing less | Files or git objects are only in the cloud | ● amber `some files are only in iCloud · adage doctor` | `seeing less: some files are only in the cloud` |
| S5 Out of date | Hooks lack an event this version uses, or a stored file is from an older Adage | ● amber `hooks need updating · adage install claude-code --write` | `hooks out of date` |
| S6 Broken | The index or data directory cannot be used | ● red `cannot read reflexes · adage doctor` | `not delivering` |

S1 needs one change of behaviour (audit slice 2): record session start and end, and nothing more,
in a repository with no reflexes, so `watch` can show that sessions are seen.

## 5. Surfaces

### 5.1 `adage` (the card): Q1, Q3, Q5, Q6 at a glance

At most 12 lines at 80 columns. In a worktree it names the repository and says which checkout.

```text
adage 0.1.0 · github.com/stevenvaughanbooth/adage (worktree local-adage-install) · ● seeing less

reflexes    8 in use: 3 instincts, 5 learned from your sessions
learning    after each session · last 12 min ago, 1 change · usually 8 s
activity    since 11:23: 3 sessions recorded · 2 reminders · 0 followed
savings     measuring: 0 of 10 comparison sessions so far
waiting     1 reflex agents already follow · approve it to deliver it → adage review

fix         some files are only in iCloud, so Adage sees less of this repository → adage doctor

next  adage watch · adage review · adage map
```

### 5.2 `adage watch`: Q2 and Q3, live

Three bands, unchanged in layout (redesign-plan §6); what changes is what a line says.

```text
 adage watch · github.com/stevenvaughanbooth/adage · live · 8 reflexes

─ now ───────────────────────────────────────────────────────────────────────
 11:41  session 0f55ee  edited crates/adage-cli/src/main.rs
          → reminded: after editing crates/**/*.rs, run cargo fmt --all   (suggestion · 18 of 36 sessions)
 11:42  session 0f55ee  ran cargo fmt --all
          ✓ followed
 11:44  session 1731b8  23 tool calls · nothing applied

─ learning ──────────────────────────────────────────────────────────────────
 11:26  read 98 sessions in 8.6 s · 1 change: new suggestion "after editing crates/**/*.rs, run cargo clippy …"
 next   after the next session ends

─ health ────────────────────────────────────────────────────────────────────
 ✓ hooks: an event 1 min ago   ✓ learning: finished 15 min ago   ! some files only in iCloud
 m map · enter details · / filter · q quit
```

### 5.3 `adage review`: Q4, and the decisions

Sorted by what needs a person first; every row is a sentence.

```text
adage review · github.com/stevenvaughanbooth/adage

needs you
  1  On requests like these, start with git status
     agents already do this unaided (13 of 13 sessions), so it is held back until you approve it
     approve · extinguish

new, already in use (look them over)
  2  After editing crates/**/*.rs, run cargo fmt --all            suggestion · 18 of 36 sessions
  3  After editing crates/**/*.rs, run cargo clippy --workspace…  suggestion · 26 of 36 sessions
  4  Don't hand-edit Cargo.lock: change Cargo.toml, let cargo update it   instinct · from your files

next  adage review 1 approve · adage review 3 weaken · adage map
```

### 5.4 `adage doctor`: Q5 in full

Problems first, passes folded into one line unless `--all`:

```text
adage doctor · 1 to look at · 19 passed

! cloud files   iCloud keeps 1368 git objects and 63 files of ~/Documents/skill-compile only in
                the cloud; Adage skips them instead of waiting for a download.
  fix           move the repository out of iCloud Drive (for example to ~/code), or in Finder
                Control-click its folder and choose Keep Downloaded

✓ 19 checks passed (adage doctor --all lists them)
```

### 5.5 `adage init`: setting up, scoped to what you choose

Per audit slice 3: find repositories from Claude Code's own history without reading transcripts;
the current repository preselected; base instinct packs chosen on the same screen (the packs work
already has `init` turn them on).

```text
adage init · 1 of 3 · which repositories should Adage learn from?

  [x] github.com/stevenvaughanbooth/adage   98 sessions · last today      (this repository)
  [ ] github.com/acme/billing                12 sessions · last 3 days ago
  [ ] ~/scratch/notes                         2 sessions · last 2 weeks ago

  Adage reads only the repositories you tick, and only Claude Code sessions.
  space toggle · enter continue · a all · n none
```

### 5.6 Commands: the everyday set first

24 commands today, 6 of them under "Every day". Help groups by the question a person has, with
nothing removed:

```text
Every day:            adage (the card), watch, review, map
When something is off: doctor
Set up:               init, install, packs, config, update
Going further:        ui, status, savings, sense, condition, report, graph, train,
                      marketplace, share, add, delegate, models, completions, eval
```

## 6. What changes in the code, by slice

These are the audit's slices 2 to 6, restated as what each surface gains.

| Slice | Gains | Mainly |
| --- | --- | --- |
| 2. Learning you can see | S1 and S3 exist; healthy is proven (rule 1); the card's and `watch`'s learning lines (rule 2); hook-event drift (S5) | `adage-hook` (session start and end in unrouted repositories), `doctor.rs`, `autolearn.rs` state, `status_card.rs`, `ui/` |
| 3. `init` scoped to what you choose | §5.5; `learn.repos`; Claude Code by default | `init.rs`, `ui/wizard`, `autolearn.rs`, config |
| 4. Say what things do | Rules 4 to 6; §5.1 to 5.4; the help groups | `words.rs`, `condition_out.rs`, `review.rs`, `status_card.rs`, `cli.rs` |
| 5. Work in proportion | `watch` idle cost; manual history scoped to the repository | `ui/jobs.rs`, `sense`, `condition` |
| 6. Learned commands that can run | F13 | `adage-learn` (testmap), `adage-core` command table |

## 7. Acceptance

- **Six-question test.** For each state fixture S0 to S6, a snapshot of the card, `watch`
  and `doctor` answers Q1 to Q6 consistently (the same counts and the same lamp).
- **No ids on default surfaces.** A test fails if a route id (`run-…`, `instinct-…`) appears in the
  card, `watch` or `review` text output.
- **Card budget.** At most 12 lines and 100 columns per line at an 80-column terminal, in every
  state.
- **Healthy needs evidence.** The S1 fixture with no recorded hook event renders a neutral hooks
  lamp, not a pass.
- **A newcomer test.** Before shipping slice 4, three people who have not used Adage each answer
  Q1 to Q6 from the card and `watch` alone within two minutes.

## 8. Decisions closed by this document

- Six questions, with one surface answering each (§2).
- Seven truth and comprehension rules (§3), including "healthy is proven, not assumed".
- Seven repository states, each with a defined rendering (§4).

## 9. Questions for the product owner

- **Q-A. Theme words.** The lexicon says every user-facing surface uses them; zero-config §0
  rule 3 says only where a decision needs them. This document's mockups follow a middle course:
  plain words on the card and in `watch`, theme words with their meaning in `review`, `ui` and the
  map. Which should hold?
- **Q-B. Recording without reflexes.** Slice 2 records session start and end in repositories with
  no reflexes (one append each). Acceptable, or should S1 be inferred from the learner's queue?
- **Q-C. Help groups.** Is regrouping `--help` as in §5.6 acceptable, given every command stays?
- **Q-D. The order of work.** Slices 2 and 4 make the product legible; slice 3 depends on the
  packs work landing (it has). Proposed order: 2, 4, 3, 6, 5.
