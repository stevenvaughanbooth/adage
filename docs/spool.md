# The session spool

Every hook event Adage acts on leaves a record in the session's spool,
`$ADAGE_HOME/sessions/<session>.spool`. The spool is what Adage knows happened: `adage watch`, the
status card, `adage review`, `adage graph`, `adage savings`, `adage doctor` and the Claude Code mod
all read it, and a session whose state file can no longer be read is rebuilt from it. This page
lists every kind of record, its fields, and which reader uses them.

## Format

A spool is append-only. Each record is one frame, `magic | len | crc | payload`, written with one
`O_APPEND` write (`adage_store::spool`); a frame torn by a crash is skipped. The payload is one
JSON object. Its keys serialise in sorted order, so a record's bytes do not depend on the order in
which its fields were set.

Records never hold prompt text, file contents or tool output. Beside the reflexes' own text, the
commands a record names are the covering run of a call answered by a receipt (`pretool`,
`command`) and each advisory obligation's bound command, with the repository-relative path whose
edit last armed it (`decision.obligation_state`).

One writer builds every record: `adage_hook::record::Record`, one `RecordKind` per kind below
(`crates/adage-hook/src/record.rs`). A record's `kind` is never renamed, because every reader
matches on it. `crates/adage-hook/src/record/snapshots/` pins the bytes of every kind; a change
there is a change to this page.

## Fields every record has

| Field | What | Notes |
| --- | --- | --- |
| `ts` | When it was written, in milliseconds since the Unix epoch | `press_spent`: when the token was spent |
| `event` | The harness's native event name (`PostToolBatch`, `tool.execute.after`, `Press`, ...) | |
| `kind` | One of the kinds below | Records written before `kind` existed carry only `event`; `adage_runtime::rebuild::kind_of` maps it through the event table (`adage_trace::Harness::events`) |
| `duration_us` | The hook's own time until the record was written, in microseconds (the process's spawn is not in it) | `press_spent`: the spend's own time (a person-only verb writes it, not a hook). Records written before the field lack it |

Fields several kinds share:

| Field | What | Kinds |
| --- | --- | --- |
| `harness` | The harness's hook name (`codex`, `opencode`). Absent for Claude Code: records from before there were other harnesses named none, and every reader takes a record without one as Claude Code's | the decided and seen kinds, `session_start` |
| `arm`, `propensity` | The session's arm (`treated` or `control`) and the probability it was drawn with (1 when `ADAGE_ARM` forced it; `null` on records from before propensities) | decided kinds, `session_start`, `pretool`, `skill_prompt` |
| `repo` | The repository key the index routes the checkout by | all but `stop_gate` and the press kinds |
| `generation` | The index generation the decision was made under | decided kinds, `session_start` |
| `tool_use_id` | The harness's id of the one call the record is about | `pretool` |
| `tool_use_ids` | The harness's ids of the calls an event judged, in order | decided `batch` and `failure` |
| `agent` | The subagent that made the call(s) (Claude Code's `agent_id`, the canonical `Call::agent`), so a delegation's calls can be counted on their own. Absent for the main loop's calls, and on records from before it | decided `batch` and `failure`, `pretool` |

## Kinds

### Decided events: `prompt`, `batch`, `failure`, `stop`, `turn_step`

Written by `adage_hook::respond` after `adage_runtime::decide` ran under the session's state lock,
in a repository with reflexes or candidates, for an event the fast path did not skip. The kind is
the runtime event's name (`Event::name`): a `batch` is a tool batch or one completed call; a
Codex or OpenCode call that reports a failure is a `failure`.

| Field | What |
| --- | --- |
| `decision` | The `adage_runtime::Decision`: `directives`, `would_have` (both arms), `judgements` (`followed`, `ignored`, `misfired`, `obligation_met`, `predicted`, `substituted`, and `refused`: a step Adage made in the agent's place that the harness refused before it ran, which is never `predicted` and meets no obligation), `observed` call keys (a failed call's key ends ` !failed`, a call the harness refused ` !refused`), `continue_reason` / `would_continue` (a Stop request), `predicted` / `would_predict` and `predicted_at` (`turn_step`), `withheld: "permission"` and `permission` (`{ decision: "ask" \| "deny" \| "none", tool?, reason? }`: a `turn_step` whose `would_predict` the person's permissions held back; no `predicted_at`), `routine` / `would_route`, `noticed`, the context states `ctx` / `cand_ctx`, and `obligation_state` (the session's advisory obligations and Stop-request count after the event, when they changed or an edit was observed, and after an extension's action met one; never a claim that a check passed) |
| `exec` | adage-exec's entry per attempted route (`route`, `level`, `exit`, `bytes`, `sha256`, `truncated`, `duration_ms`, `auto`; or `withheld` and why) |
| `recovery_required` | `true` when an action's outcome could not be confirmed |

A `turn_step` record is written only for a step that predicted or routed (or would have, in the
control arm). The Claude Code mod asks a step in two phases: a dry run (`dry_run: true`) that would
yield a prediction writes nothing, and the verdict that follows writes the record, as a
predicted step on `allow` and as a withheld one (`withheld`, `permission`, `would_predict`, no
`predicted_at`) otherwise. A dry run that would yield nothing (nothing to predict, the cap
reached, the control arm, observe mode) is recorded at once, as a step with no phase is. A session end is never decided (the fast path skips it), so a decided `session_end`
does not occur; one is only seen (below).

### Seen events: `session_start`, `prompt`, `batch`, `failure`, `stop`, `session_end` with `unrouted`

Written by `session_start::record_seen` for an event in a repository with no reflexes yet:
`unrouted: true`, `repo`, `calls` (how many tool calls, when any), `harness`, and `bin` (the
serving binary's file name) at a start. No arm, no propensity, no decision: the savings estimate
counts only sessions exposed to reflexes, and a rebuild replays only decisions. A step boundary is
never recorded here.

### `session_start`

Written at a session start in a repository with reflexes (or one being bootstrapped), after the
session's arm is assigned: `arm`, `propensity`, `repo`, `generation`, `harness`, and `bin` (`adage`
or `adage-exec`: which binary answers the session's hooks).

### `stop_gate`

Written when adage-exec's finish-line gate ran at a `Stop`: `blocked` (it asked the agent to
continue). No `repo` or `harness`.

### `pretool`

Written once per Bash call about to run that Adage decided (`PreToolUse`: the command hook, or the
mod's verdict; the mod's dry run writes nothing). `arm`, `propensity`, `repo`, `tool_use_id`, and
the fields of whichever decision it was:

| Decision | Fields |
| --- | --- |
| The interpreter rewrite or advice | `pretool: {rewrite: "test" \| "repro", interpreter}` or `pretool: {advise: <kind>, interpreter}`, `receipt` (a fresh receipt was delivered beside it), `checked_by` (`planner` or `engine`), `withheld` in the control arm |
| A receipt delivered alone | `receipt: true` |
| A learned dead end replaced | `checked_by: engine`, `decision` (judged `substituted`), and `substituted: true`, or `would_substitute: true` and `withheld: true` in the control arm or observe mode |
| A call answered by a receipt | `answered: "receipt"`, `receipt: true`, `checked_by: engine`, `command` (the covering run), `card` (what the mod's receipt card shows), `decision` (its `obligation_met` judgements), or `withheld: true` |

### `skill_prompt`

Written when the mod asked about a skill's text and Adage had verified steps to add:
`would_annotate: {skill, verified, stale, bytes}` (both arms), `annotated` (the same, when it was
added), `observe: true` in observe mode, `arm`, `propensity`, `repo`.

### `press_issued`, `press_spent`, `tripwire`

The press (`adage_hook::press`). Facts, never the token:

| Kind | `event` | Fields |
| --- | --- | --- |
| `press_issued` | `Press` | `source` (the start's `startup`, `clear` or `resume`), `chain` (0) |
| `press_spent` | `Press` | `chain` (the link the spend moved to), `verb` (the person-only verb) |
| `tripwire` | `PreToolUse` | `warned` (this was the session's one warning) |

### `injection`, `injection_label`

The world (`adage_hook::world`; the situational awareness layer, `docs/usage.md` "The world").
Written only while `world.enabled` is set and `world/paused` is absent.

| Kind | `event` | Fields |
| --- | --- | --- |
| `injection` | `UserPromptSubmit` | `injection` (`adage_trace::Injection`: `id`, `session`, `at`, `cls` = `hint` or `disambiguation`, `artifacts`, `tokens`, `confidence`), `scored` (the top three candidates' scores and terms), `arm`, `repo`; `withheld: true` and `would_hint: true` in the control arm |
| `injection_label` | the batch's or prompt's event | `injection_id`; `used` (the agent opened a named artifact within three tool batches, or did not); `overridden: true` (the next prompt named its own artifact) |

Neither holds the prompt, the hint's text or an artifact's content: the ids, the scores and the
label are what the resolver learns from.

## Readers

| Reader | Kinds | Fields it uses |
| --- | --- | --- |
| `adage_runtime::rebuild` (a session state that cannot be read) | decided kinds, `stop_gate` | `arm`, `propensity`, `repo`, `kind` (or `event`), `decision` (`observed`, `would_have`, `judgements`, `would_continue`, `predicted_at`, `would_predict`, `withheld` (the held routes), `ctx`, `cand_ctx`, `noticed`, `obligation_state`: the obligations as recorded, in place of those the emitted routes and judgements imply; an older batch record with an edit and no `obligation_state` un-meets every met obligation) |
| `adage watch` and `--follow --json` (`adage-cli` `ui/feed.rs`, `ui/follow.rs`); the mod's feed reads that JSON | every kind | `kind`, `event`, `ts`, `arm`, `repo`, `harness`, `unrouted`, `calls`, `decision`, `pretool`, `answered`, `card`, `command`, `exec`, `recovery_required`, `tool_use_id(s)`, `withheld`; the mod counts `press_issued` rows against the tokens it holds |
| `adage doctor`, hook latency (`doctor/latency.rs`) | `batch` | `duration_us`, `decision.would_have` (only batches with nothing to say count) |
| `adage savings` (`adage_app::savings`) | every kind with an arm | `ts`, `arm`, `propensity`, `kind` (`session_start`, `pretool` rewrites, `stop` / `session_end`, predicted `turn_step`), `decision.directives`, `decision.judgements` |
| `adage graph`, the map and its whole-session trace (`adage_app::graph`, `graph/measurements.rs`) | every kind (the trace inventory); counts from decided kinds and `pretool` | `ts`, `event`, `kind`, `arm`, `repo`, `harness`, `generation`, `unrouted`, `calls`, `decision` (`directives` and `would_have` with `route`, `track`, `version`; `judgements`, `observed`, `continue_reason`, `would_continue`, `noticed`, `predicted`, `would_predict`, `substituted`, `would_substitute`), `pretool`, `answered`, `withheld` |
| `adage review` (`adage_app::review`, through `graph/measurements.rs`) | decided kinds | `arm`, `repo`, `decision.directives` and `decision.would_have` (the exposure an outcome is tied to), `decision.judgements` |
| Learned values (`adage_learn::instincts`) | decided kinds | `arm`, `repo`, `decision.judgements` |
| `adage report` (`adage-cli` `main.rs`) | decided kinds | `arm`, `decision.directives`, `decision.would_have` |
| The harness ledger (`adage_app::harness_ledger`) | `session_start` | `kind`, `repo`: a usage note is accepted only for a session that started in that repository |
| `adage-exec doctor`, `adage review` (`adage_exec::last_withheld`) | decided kinds | `ts`, `exec[].withheld`, `route`, `level` |
| `adage world status`, `adage world eval` (`adage-cli` `world/`) | `injection`, `injection_label` | `injection.artifacts`, `injection.cls`, `scored`, `used`, `overridden`, `would_hint` |

## Adding a record kind

1. Add a variant to `RecordKind` (`crates/adage-hook/src/record.rs`) with its `kind` in `as_str`.
2. Build the record with `Record::new(kind, event)`, its own fields with `with`, then `harness`,
   `tool_use_id` or `calls` where they apply, `timed(started)` last, and `append` (a decision's
   record, whose failure is reported) or `append_or_log`.
3. Add it to the snapshot in `record/snapshot_tests.rs` and accept the new lines.
4. Add it to this page, and to every reader that should know it (the feed's `from_record` first).
   If a rebuilt state needs it, `adage_runtime::rebuild`.
