# Live learning

Adage learns while agents work. When a session stops or ends, the hook asks for the repository to
be learned, and a background learner does it, usually within seconds. There is no `adage condition`
to remember, and no daemon. This page is the contract for everything that watches it happen
(`adage ui`, `adage watch`, scripts): the settings, the files, and the two streams to follow.
The design is in [Architecture §4.7.2](Adage%20—%20Architecture.md#472-live-learning).

## Settings: `$ADAGE_HOME/config.toml`

Read by hooks, learners and every command through one reader (`adage_store::config`). Every key
has a default, so the file is optional.

```toml
[learn]
auto = true               # learn automatically when agent sessions stop or end
min_interval_secs = 120   # at most one automatic learn per repository per interval (0–86400)
thresholds = "default"    # evidence bars: "default", or "demo" (lower); `adage condition --demo` still overrides
claude_projects = ""      # Claude Code history ("" = ~/.claude/projects)
codex_sessions = "auto"   # Codex rollouts: "auto" ($CODEX_HOME/sessions once Codex hooks are registered), a directory, or "" (none)
opencode_exports = ""     # a directory of `opencode export` files ("" = none)
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
| `learn.parse_cache` | `true` | — (`adage condition --no-cache` skips it for one run) |
| `ui.notices` | `daily` | — |
| `runtime.control` | `0.1` | `ADAGE_CONTROL` (any number; 1 or more is shadow mode, as before) |

Environment variables win over the file, and the file over the defaults. An unparseable file, an
unknown key, or an invalid value is reported by `adage config` and `adage doctor`, and that key
keeps its default: a hook never fails because of the configuration.

```text
adage config [--json]            every key: value, source (default | file | env VAR), and what it does
adage config get <key>           one effective value
adage config set <key> <value>   validated, written atomically
```

Keys that change what Adage learns from or how much evidence it needs (`learn.thresholds`,
`learn.claude_projects`, `learn.codex_sessions`, `learn.opencode_exports`, `instincts.packs`) need
a person: a terminal (or `ADAGE_ALLOW_NONINTERACTIVE=1`), never inside an agent session. Every
other key only tunes Adage or turns something down, and can be set from anywhere.

`config set` keeps every other key but not comments. Paths are absolute or start with `~/`.

## How it runs

1. **Trigger.** On Claude Code and Codex `Stop` and `SessionEnd`, and OpenCode `session.idle` and
   `session.deleted`, after its normal decision, the hook (if `learn.auto` is on and the
   repository has no `.adage/disabled`) writes the repository's request marker. Then, unless a
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

Automatic learning of a repository produces exactly the tracks a full `adage condition` over the same
sources produces for it (tested), so the two never fight. Set `learn.codex_sessions` if you also
learn Codex history by hand, or evidence from it will come and go between manual and automatic
runs.

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

A request is **pending** while `requested_at > processed`. A learner is **running** while its lock
is held (test with a shared lock, released at once: `adage_store::lock::is_held`), and **waiting**
while `after` is in the future. `adage doctor` reports a learner holding its lock for over 15
minutes, and a request pending over 5 minutes past its interval.

Other files: `$ADAGE_HOME/tracks.lock` (one writer of tracks and index at a time: learners,
`adage condition`, review decisions, `adage add`), and `$ADAGE_HOME/cache/sessions/` (the parse cache:
`manifest.json` maps each transcript to its size, modification time and repository root, and
`<hash>.json` holds its parsed session; owner-only, rebuilt on a new build, safe to delete).

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
| `changes[].change` | `new` (in service, was not), `nascent` (waits for approval in `adage review`), `extinguished`, `dormant` (kept on its last evidence because it steered its own evidence away; prime at most), `strength`, `evidence` (sessions of support or opportunity changed), `proposal` (a pinned route has a proposed change) |
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
