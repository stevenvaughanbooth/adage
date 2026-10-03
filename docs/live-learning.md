# Live learning

Adage learns while agents work. When a session stops or ends, the hook asks for the repository to
be learned, and a background learner does it, usually within seconds. There is no `adage condition`
to remember, and no daemon. This page is the contract for everything that watches it happen
(`adage ui`, `adage watch`, the Claude Code mod, scripts): the settings, the files, and the
streams to follow.
The design is in [Architecture §4.7.2](Adage%20—%20Architecture.md#472-live-learning).
For current product behavior and execution boundaries, see [How Adage works](how-it-works.md).

## First run and existing history

Run `adage init` at a terminal inside the repository. The wizard shows hook registrations before
writing them, then conditions that repository from its existing history. It uses the same scoped
learner as the hooks (`condition --auto --repo-root <root>`), including linked checkouts, and binds
applicable instincts from repository files even when there are no traces. Skipping a hook install
does not enable that harness's automatic history discovery. `init --print` is a read-only preview.

Manual `adage sense` and `adage condition` use the configured Claude Code history, Codex rollouts
and OpenCode exports, and read only what was recorded in the repository they run in, picked as the
learner picks it (step 4 below); `--all` reads every repository's, and outside a repository they
ask for `--all` rather than read everything. Without source overrides, Claude history is under
`CLAUDE_CONFIG_DIR/projects` (else `~/.claude/projects`); Codex history is under
`CODEX_HOME/sessions` (else `~/.codex/sessions`) and, with `learn.codex_sessions = auto`, is read
once Codex hooks are registered; `learn.codex_sessions = ""` disables it. A configured directory
replaces that harness's default location. OpenCode exports require `learn.opencode_exports` or an
explicit `--transcripts` file.

Explicit `--transcripts`, `--claude-projects` and `--codex-sessions` replace all default sources;
an empty explicit source is never silently replaced by other history, and every repository they
hold is learned. `--cwd-prefix <path>` filters by recorded path components (so
`/work/app` does not include `/work/app-other`). With no default history in a repository, it binds
available file-based instincts; explicit empty sources still report an error. `--no-instincts`
and `instincts.enabled = false` suppress the no-history fallback.

Conditioning reads traces locally and uses their recorded evidence; it needs no hidden test
outcome file and runs none of their commands. Few traces may support no learned reflex yet.
History discovery, a learned route, and hook delivery are distinct from measured developer savings.

## Settings: `$ADAGE_HOME/config.toml`

Read by hooks, learners and every command through one reader (`adage_store::config`). Every key
has a default, so the file is optional.

```toml
[learn]
auto = true               # learn automatically when agent sessions stop or end
min_interval_secs = 120   # at most one automatic learn per repository per interval (0–86400)
thresholds = "default"    # evidence bars: "default", or "demo" (lower); `adage condition --demo` still overrides
claude_projects = ""      # Claude Code history ("" = CLAUDE_CONFIG_DIR/projects, else ~/.claude/projects)
codex_sessions = "auto"   # Codex rollouts: "auto" ($CODEX_HOME/sessions once Codex hooks are registered), a directory, or "" (none)
opencode_exports = ""     # a directory of `opencode export` files ("" = none)
repos = "all"             # repositories whose history is read: "all", or keys, e.g. ["github.com/acme/billing"]
parse_cache = true        # keep parsed transcripts in cache/sessions (owner-only; faster re-learning)

[instincts]
enabled = true            # bind Adage's instincts from each repository's files

[ui]
notices = "daily"         # one-line notices at session start (Claude Code), at most daily; or "off"

[runtime]
control = 0.1             # share of sessions held out as the control arm (0–1)
```

| Key | Default | Environment override |
| --- | --- | --- |
| `learn.auto` | `true` | `ADAGE_AUTO_LEARN` (`0`/`1`) |
| `learn.min_interval_secs` | `120` | — |
| `learn.thresholds` | `default` | — |
| `learn.claude_projects` | `""` | — |
| `learn.codex_sessions` | `auto` | — |
| `learn.opencode_exports` | `""` | — |
| `learn.repos` | `all` | — |
| `learn.parse_cache` | `true` | — (`adage condition --no-cache` skips it for one run) |
| `ui.notices` | `daily` | — |
| `runtime.control` | `0.1` | `ADAGE_CONTROL` (any number; 1 or more is shadow mode, as before) |
| `runtime.rewrite` | `true` | — (`adage delegate rewrites on\|off`; see docs/execution.md) |
| `runtime.routine_model` | `""` | — (the Claude Code mod sends a step that only does what a delivered directive says to this model, at low effort; the hook is told only that one is set) |
| `runtime.predict_cap` | `6` | — (a person's setting: at most this many steps per session Adage takes in the model's place under a `predict` grant, 0 to 50; 0 predicts none) |
| `skills.invoke` | `false` | — (a person's setting: the Claude Code mod may invoke an inert, accepted skill at a `skill` reflex's step, under its `predict` grant; see docs/training.md) |

Environment variables win over the file, and the file over the defaults. An unparseable file, an
unknown key, or an invalid value is reported by `adage config` and `adage doctor`, and that key
keeps its default: a hook never fails because of the configuration.

```text
adage config [--json]            every key: value, source (default | file | env VAR), and what it does
adage config get <key>           one effective value
adage config set <key> <value>   validated, written atomically
```

Keys that change what Adage learns from or how much evidence it needs (`learn.repos`,
`learn.thresholds`, `learn.claude_projects`, `learn.codex_sessions`, `learn.opencode_exports`,
`instincts.packs`), and the two that let Adage act in the model's place (`runtime.predict_cap`,
`skills.invoke`), need
a person: a terminal (or `ADAGE_ALLOW_NONINTERACTIVE=1`), never inside an agent session, and
never a press from the Claude Code session (a guarded key is set at a terminal only). Every
other key only tunes Adage or turns something down, and can be set from anywhere.

`config set` keeps every other key but not comments. Paths are absolute or start with `~/`.

### Turning it off in a repository

```text
adage off    write <root>/.adage/disabled: every hook in this repository prints and records nothing
adage on     remove it (a person's decision: at a terminal, or with a press from the session)
```

`adage off` turns something down, so it runs anywhere, an agent session included (the Claude Code
mod's `/adage off` runs it); it is idempotent. `adage on` refuses inside an agent session with the
person-only message and leaves the file, unless a person answered the mod's ask: then the verb
reads a **press** on stdin, a single-use token the binary issued to the mod before the agent's
first act, and behaves as at a terminal (docs/execution.md, "Deciding from the session"). The
press is asked for (`adage hook claude Press`) before the kill switches, so a repository that is
off can still be asked to turn back on. Outside a repository each prints one line and exits 0.
`ADAGE_DISABLE=1` turns every hook off wherever it is set, whatever the file says. While a
repository is off, the status card's first line says so and its `next` leads with `adage on`;
`adage doctor` reports the switch.

## How it runs

1. **Trigger.** On Claude Code and Codex `Stop` and `SessionEnd`, and OpenCode `session.idle` and
   `session.deleted`, after its normal decision, the hook (if `learn.auto` is on, `learn.repos`
   names the repository, and it has no `.adage/disabled`) writes the repository's request marker. Then, unless a
   learner holds the repository's lock or one is already scheduled, it starts
   `adage condition --auto --repo-root <root>` detached (its own process group on Unix; a detached
   process group on Windows; no standard streams). If the last learn for the repository started
   less than `min_interval_secs` ago, it adds `--after <ms>` and the learner waits until then, so
   the last session is always learned and at most one learn starts per interval. The hook never
   waits for it, and what the hook prints never changes. A failure is written to
   `fail-open.log` (class `condition`).
2. **Learner.** Takes the repository's lock or exits (single flight). Learns that repository
   only, from Claude Code history, the configured Codex and OpenCode sources, and any transcript a
   hook named, through the parse cache; holds `tracks.lock` while it writes tracks and the index.
   Appends a line to `learn.log`. Then re-reads the request marker once: a request that arrived
   while it ran is learned before it exits.
3. **Session start (bootstrap).** At `SessionStart` (OpenCode `session.created`), in a repository
   with no reflexes whose learner has not run since the index last changed, or when the index
   cannot be read, the hook starts the learner at once with `--bootstrap`: it binds the
   repository's instincts and recompiles the index first (tens of milliseconds), then learns.
   In a repository with reflexes, the session start assigns the session's arm and spools one
   `session_start` record. Either way it may print one notice (below).
4. **Scope.** The learner reads only this repository's traces: Claude Code project directories
   named for one of its checkouts (the root, its main checkout, its linked worktrees), and within
   them only transcripts whose recorded working directory is inside a checkout; Codex rollouts and
   OpenCode exports by their recorded directory. Nothing else is opened. `learn.codex_sessions =
   auto` reads `$CODEX_HOME/sessions` once Codex hooks are registered. Sources are additive: a
   transcript found once is remembered (`<slug>.sources.json`, up to 5,000) and forgotten only
   when its directory lists without it, so a learner started from a sandboxed session keeps what
   it cannot see (the parse cache serves it).
5. **After learning** the learner updates the repository's ledger and savings estimate
   (`$ADAGE_HOME/ledger/`, below) and composes the next notice. Every learner also re-binds
   instincts from the repository's files. Three failed runs in a row back the next one off (twice
   the interval per further failure, up to 64×).
6. **Manual learning** (`adage condition`) uses the same thresholds (unless `--demo`), the same cache
   and the same lock, and writes the same `learn.log` lines with `"trigger":"manual"`.

Automatic and manual learning use the same learner and source settings. Automatic Codex discovery
in `auto` mode additionally requires registered Codex hooks or a Codex hook request. Register its
hooks or configure its history directory explicitly to keep its past traces in both paths.

## Files

`$ADAGE_HOME/learn-queue/`, one set per repository; `<slug>` is the repository key with every
character other than `[A-Za-z0-9_-]` replaced by `-` (the same slug as its tracks directory).
Times are milliseconds since the Unix epoch. JSON files are replaced atomically (write, rename).

| File | Writer | Contents |
| --- | --- | --- |
| `<slug>.json` | hook | Latest request: `{"repo", "root", "transcript_path"?, "harness", "requested_at"}` |
| `<slug>.spawn.json` | hook | Last learner started: `{"at", "after"}` (`after` = when it starts learning) |
| `<slug>.lock` | learner | Held while a learner runs or waits (`flock` / `LockFileEx`); released when it exits, however it exits |
| `<slug>.state.json` | learner | `{"pid", "started_at", "finished_at", "outcome": "running"\|"ok"\|"error", "processed"}` |
| `<slug>.sources.json` | learner | Every transcript found for the repository (sources or hooks), forgotten only when verifiably gone |
| `<slug>.notice.json` | learner | The next notice: `{"id", "text", "kind": "health"\|"waiting"\|"value"\|"reflexes", "at", "basis", "baseline"}` |
| `<slug>.notice-shown.json` | hook | The last notice shown: `{"id", "at"}`; a notice is shown only if its id is new and a day has passed |

`<slug>.state.json` also carries `errors`: failed runs in a row.

`$ADAGE_HOME/ledger/` holds, per repository, `<slug>.jsonl` (one line per trace: session id,
harness and version, models, start and end, model turns, tool calls, first edit turn, tokens,
cost `{usd?, source: harness|priced@<date>|none}`, `asked_for_help`; no prompt, command or path),
`<slug>.seen.json` (which transcripts were summarised, by a hash of their path) and
`<slug>.summary.json` (the savings estimate every surface reads; `adage savings --json`).

`$ADAGE_HOME/ledger/.harness.jsonl` holds what a harness reported about its own sessions, one line
per session (a later note replaces an earlier one), written only by `adage ledger note`:

```json
{"session_id":"…","repo":"github.com/acme/billing","harness":"claude-code","harness_version":"2.1.288",
 "started_at":1790000000000,"ended_at":1790000600000,
 "steps":[{"index":0,"model":"claude-sonnet-4-6","input":12,"output":340,"cache_read":51000,"cache_write":900,"predicted":false},
          {"index":1,"model":"claude-sonnet-4-6","input":0,"output":0,"cache_read":0,"cache_write":0,"predicted":true}],
 "cost_usd":0.0412,"hook_ms":[3,2,4],"noted_at":1790000600100}
```

`adage ledger note` reads that object on stdin (without `noted_at`; `repo` may be the session's
working directory, resolved to its key; times may be RFC 3339 strings; `cost_usd`, `hook_ms`,
token counts and `predicted` may be left out). It is accepted only for `harness: "claude-code"`,
a version string, and a session whose spool holds a `session_start` in that repository: exit 0,
nothing printed; otherwise exit 2 with the reason on stderr and nothing written. It is allowed
inside an agent session (it tunes nothing). Whenever the savings estimate is refreshed (the
learner after a learn, `adage savings`, the note itself) the repository's notes are merged into
its `<slug>.jsonl` under `<slug>.lock`: the note's tokens, models, times and `model_turns` (only
the steps the model answered) replace the transcript's, its cost too (`source: harness` when the
engine reported `cost_usd`, else the steps priced from the table), and the transcript's tool
calls, first edit and `asked_for_help` are kept. A later transcript pass never keeps the session
for good: the next merge lays the note over it again. A step with `predicted: true` is one Adage
took in the model's place; the estimate reads it as a model turn not spent, `adage savings
--json` lists the reported sessions under `harness` and counts the spools' predicted steps under
`counts.predicted_steps`. Notes of sessions that ended over 180 days ago are dropped.

A request is **pending** while `requested_at > processed`. A learner is **running** while its lock
is held (test with a shared lock, released at once: `adage_store::lock::is_held`), and **waiting**
while `after` is in the future. `adage doctor` reports a learner holding its lock for over 15
minutes, and a request pending over 5 minutes past its interval.

Other files: `$ADAGE_HOME/tracks.lock` (one writer of tracks and index at a time: learners,
`adage condition`, review decisions, `adage add`), and `$ADAGE_HOME/cache/sessions/` (the parse cache:
`manifest.json` maps each transcript to its size, modification time and repository root, and
`<hash>.json` holds its parsed session; owner-only, rebuilt on a new build, safe to delete).

**Candidates.** `$ADAGE_HOME/tracks/<slug>/learned/candidates.json`, replaced atomically by every
learn of the repository (owner-only), written only through the learner's store
(`adage_learn::store::Store::write_candidates`): the sequences counted below the support a reflex
needs, and the nascent reflexes with a context trigger, which wait for review. Counted, never
delivered.

```json
{"schema":"adage-candidates/1","repo":"github.com/acme/billing","s_min":3,
 "candidates":[{"id":"run-69ef44","trigger":{"on":"context","items":[…],"within":8},
                "step":{"do":"run","command":"make lint"},"support":2,"days":2,
                "last_seen":"2026-09-09","review":false,"sessions":["…","…"]}],
 "formed":[{"id":"run-1a5b55","sessions":["…"]}]}
```

`s_min` is the support a reflex needs in this repository (`learn.min_support`, or 1% of its
traces when that is more); a candidate has at least `s_min − 2` and never fewer than two
(`review: true` ones have formed). `id` is the id the same pattern gets as a reflex. `sessions`
are its supporting traces; `formed` lists last learn's candidates that this learn made reflexes,
with the traces that tipped each. At most 128 per repository. The index compiles each repository's
valid candidates (`candidates`, a list per repository key, without `sessions`; never a route's id)
with their own context net (`candidate_contexts`) and their words in the fast path's triggers;
neither is part of the index's `generation`. `adage status --json` lists them under
`candidates`, each in the reflex shape with `state: "candidate"` (or the nascent reflex's own),
`directive: null`, `candidate: {support, s_min, days, last_seen, review}`, and `next` (the review
command, for one that waits for review).

**The index's format, and two binaries on one machine.** `index.json` carries `format`, the
newest thing it may hold: absent or `0` before skill reflexes, `1` once a reflex may be a `skill`
step (`adage_tracks::index::INDEX_FORMAT`). A binary that cannot parse an index whose `format` is
newer than its own reads it as no reflexes and never rebuilds it (that would undo the newer
binary's work), and `adage doctor` says the index was written by a newer Adage. The rule: every
`adage` that hooks call on a machine (an npm install on `PATH`, the Claude Code plugin's) must be
at least as new as the newest that learned there; update the older one rather than letting the
two take turns. A damaged index (truncated, edited) is still rebuilt at the next session start.

## Streams to follow

### `learn.log`

`$ADAGE_HOME/learn.log`, one JSON object per line, rotated to `learn.log.1` at 256 KiB. Read from
the offset you last reached; if the file shrinks, it was rotated: finish `learn.log.1` from your
offset, then read `learn.log` from 0. Ignore kinds and fields you do not know.

A learn, one line per repository learned:

```json
{"ts":1758620000000,"kind":"condition","trigger":"auto","repo":"github.com/acme/billing",
 "sessions_read":212,"sessions_used":198,"duration_ms":431,"index_generation":"3f1c9a0d4b2e7f61",
 "changes":[
   {"route":"finish-line-6a01b2","track":"github-com-acme-billing/learned","change":"strength",
    "from":"prime","to":"fire","summary":"finish line route prime → fire (9 of 9 sessions, lower bound 0.70)"},
   {"route":"lookup-work-item","track":"github-com-acme-billing/learned","change":"evidence",
    "from":null,"to":null,"summary":"fire work item route evidence 8/9 → 9/10 sessions"}],
 "version":"0.1.0","sessions_steered":3,"thresholds":"min_support=5 fire_lower_bound=0.8 prime_rate=0.5",
 "intent_model":null,"repos":{"github.com/acme/billing":{"sessions":198,"tracks":[…]}}}
```

| Field | Meaning |
| --- | --- |
| `trigger` | `auto` (a learner a hook started) or `manual` (`adage condition`) |
| `repo` | Repository key; `null` only for a manual run that found no usable session |
| `sessions_read` | Transcripts read (an automatic run counts only its repository's) |
| `sessions_used` | This repository's sessions with steps |
| `changes[].change` | `new` (in service, was not), `nascent` (waits for approval in `adage review`), `extinguished`, `dormant` (kept on its last evidence because it steered its own evidence away; prime at most), `strength`, `evidence` (sessions of support or opportunity changed), `proposal` (a pinned route has a proposed change), `candidate` (a sequence counted below a reflex's support, new or seen in more or fewer traces; see Candidates above) |
| `changes[].support`, `s_min` | On a `candidate`: the traces it was seen in, and the support a reflex needs |
| `changes[].sessions` | On a `new` reflex that was a candidate at the last learn: the session ids of the traces that tipped it |
| `changes[].from`, `to` | Strengths (`fire`, `prime`, `inhibit`) or `null`: `new`/`nascent` have only `to`, `extinguished` only `from`, `strength` both |
| `changes[].summary` | Fixed template text |
| `version` … `repos` | The fields every line had before live learning, unchanged |

A failure:

```json
{"ts":1758620000000,"kind":"condition_error","trigger":"auto","repo":"github.com/acme/billing","class":"lock","error":"taking the tracks lock: PermissionDenied"}
```

`class` is `repo` (its directory is gone), `lock`, `condition` or `panic`; `error` is a fixed message
or an I/O error kind. No line of `learn.log` ever holds prompt text, a command, tool output, or a
path.

### Session spools

`$ADAGE_HOME/sessions/<session>.spool`: every hook decision, as CRC-framed records (Architecture
§4.4). Follow them with `adage_store::spool::read_from(path, offset) -> (records, next_offset)`:
keep the offset per file, and call again when the file grows. A record still being appended is
not returned until it is complete; a torn record from a crash is skipped once a later record
proves it; an offset past the end (the spool was replaced) starts again from 0. New sessions are
new files in the directory. Each record:

```text
{ ts, event, kind, arm, propensity, repo, generation, harness?, decision: {
    directives, would_have, continue_reason?, would_continue?, judgements, observed } }
```

`propensity` is the probability of `arm` at assignment: the control share for `control`, one minus
it for `treated`, and 1 when `ADAGE_ARM` forced the arm (absent in records written before it). Each
directive carries its route's delivery shape as `kind` (`run`, `avoid`, `read-batch`, `obligation`,
`sequence` or `advise`; records written before Track G G5 hold the removed track kinds). `observed` holds canonical template keys, not raw commands, and no prompt text is recorded. Events
the no-match fast path skips leave no record.

### The live feed: `adage watch --follow --json`

```text
adage watch --follow --json [--repo <path>] [--session <id>] [--since <cursor>] [--stdin eof|ignore]
```

The live pane's sources (the spools above, `learn.log`, the health lamps, the learner's queue
files) as one JSON object per line on stdout, for a program; nothing is drawn and no escape
sequence is written. Records and log lines are those of the repository (`--repo`, else the
current directory's; outside a repository, every one). A new follower first sends recent history
(the same backfill as `adage watch`), then follows. Every line has `kind` and `cursor`:

```json
{"kind":"decision","cursor":"1~…","ts":1790000000000,"session":"8f3c…","harness":"claude","event":"tools",
 "repo":"github.com/acme/billing","acts":[{"act":"directed","route":"codegen-after-schema","strength":"fire",
 "detail":"make gen","instinct":false},{"act":"followed","route":"lookup-work-item"}],"observed":2,
 "unrouted":false,"texts":[{"route":"codegen-after-schema","text":"<adage-reflex …>…"}]}
{"kind":"learn","cursor":"1~…","ts":…,"repo":"…","line":{…the learn.log object…},
 "ticks":[{"text":"…","short":"…","tone":"run|change|error","strength":"fire"|null,"repo":"…"}]}
{"kind":"health","cursor":"1~…","lights":[{"label":"hooks claude","lamp":"ok","hint":null}],
 "state":{"word":"healthy|check|problem","lamp":"fire|prime|inhibit","check"?:"…"},"off":false}
{"kind":"learner","cursor":"1~…","repo":"…","state":"waiting|running|idle","next_at":1790000038000|null}
```

`event` is the feed's word (`prompt`, `tools`, `failure`, `stop`, `start`, `end`, `pretool`,
`step`). `act` is `directed`, `withheld`, `followed`, `ignored`, `misfired`, `met`, `unaided`,
`not_reached`, `stop_request`, `stop_withheld`, `pretool` (`rewrite`, `what`, `interpreter`,
`withheld`), `predicted` (a step Adage took in the model's place), `would_predict`, `noticed`
(`candidate`, `support`, `s_min`, `review`: a candidate this event's context completed, once per
session, in both arms alike), `answered` (a call answered by a receipt, so the command did not
run: `receipt` the operation, `command` the run that answered it, `summary`, `tests` it ran that
the call selects, `more`, `ran_at`, `ago`), `recovery` (an action was interrupted and actions in
the worktree wait for a person: `operation` and `command`, the `adage-exec recover … --acknowledge`
to run once the worktree is inspected, both null when the journal itself could not be read; kept
in either arm), or `judged` (`what`: another judgement, such as `substituted`). `tool_use_ids`
names the calls a row is about, where the harness gave ids: a `pretool` row's one call, a tool
batch's calls in order. `control: true` appears on another session's
row when that session is in the control arm. A `health` line comes when the lamps change (checked
every 3 s), a `learner` line when the learner's state does: `waiting` while `<slug>.spawn.json`'s
`after` is in the future (`next_at`), `running` while its lock is held, else `idle`.

**`--session <id>`**, the session the reader runs in: its rows never carry `control`, a withheld
act (`withheld`, `unaided`, `not_reached`, `stop_withheld`, `would_predict`, a withheld
`pretool`), nor, in the control arm, any judgement (a `noticed` act stays: both arms record it);
a row left with nothing after that is not
sent, so a control session's stream is what a treated session with nothing fired would send.

**Cursors.** `cursor` is opaque: the position just after its line in every file read. A new
process started with `--since <cursor>` sends exactly the lines after that one (then follows), so
a reader that restarts its follower loses and repeats nothing. A cursor from another format is
refused (exit 2 with a usage line).

The follower ends with exit 0 when its stdin, a pipe, reaches end of file (a follower started with
no stdin follows until stopped), on SIGTERM, when its parent process dies, or when stdout is
closed. With `--stdin ignore` it never reads stdin, for a parent that closes it at spawn (the
Claude Code mod); the other exits stay.
