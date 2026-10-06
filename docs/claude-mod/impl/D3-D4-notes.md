# D3 · D4 · The `skill` response kind, `verified.json` and `SkillPrompt`

What was built for chunks D3 and D4 of [the implementation plan](../implementation-plan.md)
(§2.2.7, §2.3, §2.4; mod architecture §3.11), the exact contracts D5 (drawing) and the mod's
`skill.prompt` addendum build on, what departs from the plan, and what is still open.

## The contracts

### The skill step in a track (`routes/<id>.yaml`)

```yaml
id: release
summary: …
trigger:
  on: after_edit          # or prompt (span), intent, context; nothing else
  paths: CHANGELOG.md
step:
  do: skill
  skill: release-checklist # 1–64 bytes of [A-Za-z0-9_-] (adage_tracks::safe_skill_name)
  args: minor --dry-run    # optional; plain text (io::safe_text), ≤ 160 bytes, no leading/trailing space
slots: []                  # always none
effect: observe            # always observe
strength: prime            # never fire (SKILL_CEILING), never inhibit
state: firm
exit: [{ when: intent_changes }]
evidence: { … }
```

`adage_tracks`: `Step::Skill { skill, args: Option<String> }`, `shape() == "skill"`,
`SHAPES.len() == 8`, `SKILL_CEILING = Prime`, `step_ceiling(step, effect, by_edit)` (the ceiling
validation and the safety oracle use it), `skill_key(name) == "control Skill <name>"` (the
canonical key of a Skill tool use, `adage_core::tools`), `Route::gist()` "… use the
release-checklist skill". `io::validate` refuses: an unsafe name or arguments, any slot, inhibit
or fire, an effect other than observe, a trigger other than prompt/intent/after-edit/context, and
routine execution evidence. Packs carry no skill step (`convert.rs`: "a skill step has no form in
adage-pack/1"); a pack format revision is the plan's later step.

### The directive

Template (`render::route_text`; the sentence is `adage_trace::wire::skill_sentence`):

```text
In this repository the `release-checklist` skill is used at this point. Ignore this if the user is asking for something else.
In this repository the `release-checklist` skill is used at this point, with the arguments minor. Ignore this if …
```

Wrapped as every directive: `<adage-reflex track="…" route="…" strength="prime" kind="skill">`.
`wire::named` reads the name back **before** its backquote scan: `Named.skills ==
["release-checklist"]`, `commands` empty, so `Directed.skills` (new, `#[serde(default)]`) holds it
and `all` stays false. `adage_safety::check_directive` accepts only this sentence for the route's
own skill and arguments.

### Delivery and judgement (`adage-runtime`)

- **Prompt (span)**: said when the prompt carries a span of the trigger's kind (nothing is bound);
  **intent**: as any intent route; **after edit**: said once per session at the first matching
  edit, never an obligation (no Stop request); **context**: as a context run route.
- Pending entry: `{ route, expect_key: "control Skill <name>", expect_head: "", command: "<name>" }`
  (`pending_skill`). A Skill tool use of that skill within the follow window is `Followed`; one by
  the mod (`adage_` id, `by: adage`) is `Predicted`; anything else lets the window run out to
  `Ignored`.
- **The `skill` prediction kind from a real route** (`predict_step`, `skill_call`): only for a
  pending entry whose route **is** a `Step::Skill`, holds a `predict` grant, and whose skill is in
  `Policy.invoke_skills` (new field, default empty). The hook fills it (`adage_hook::skill::invocable`)
  only at a `TurnStep` with grants: the skills whose accepted `verified.json` for this repository
  says `inert: true`, and only while `skills.invoke` (new `config.toml` key, default `false`,
  guarded) is on. Answer: `{"predict":{"kind":"skill","route","track","calls":[{"tool":"Skill","input":{"skill":"release-checklist","args":"minor"}}]}}`
  (`args` only when the step has them). Control arm: `would_predict` only.

### `verified.json` (`adage_pack::verified`, layer 4)

`$ADAGE_HOME/train/skills/<prefix>/verified.json`, beside `pack/`, never inside it (the pack's
digest and `procedure.json` do not depend on it):

```json
{ "schema": "adage-train-verified/1",
  "skill": { "name": "acme-work-item", "sha256": "<64 hex>", "inert": true },
  "repo": "acme@c1aead9f06e4",
  "pack": "acme/skill-acme-work-item",
  "digest": "<64 hex: the staged pack's>",
  "steps": [
    { "id": "s4", "route": "acme-work-item-s4-verify", "command": "make test",
      "result": "works", "date": "2026-10-03", "sandbox": "Linux, Landlock ABI 7 + …" },
    { "id": "s4+s5", "route": "acme-work-item-s4-s5-sequence", "command": "make test, then make ci",
      "result": "works", "date": "2026-10-03", "sandbox": "…" },
    { "id": "s1", "command": "scripts/wi-lookup {id}", "result": "stale",
      "signature": "exit 1: FileNotFoundError: …", "date": "2026-10-03", "sandbox": "…" } ],
  "accepted_at": "2026-10-03" }
```

- `result` ∈ `works | stale | not-replayed`; `signature` only for `stale` (what the replay saw
  fail, from the stale reason); `route` empty for a stale step (its route left the pack); `id` is
  the candidate's step ids joined with `+`. One step per compiled candidate that runs a command
  (run, after, obligation, sequence shapes), kept or dropped as stale (`Compiled.dropped`, new);
  read, advice and avoid candidates have nothing of their own to replay and are not listed.
- `inert` (`verified::inert(text)`): false when the SKILL.md holds `` !` `` anywhere or its front
  matter says `context: fork`.
- Written by `adage_train::skill::stage` on every `adage train`; `accepted_at` is stamped by
  `adage train accept <name>` (`verified::accept(dir, digest, date)`, only for the record of the
  pack just accepted) and kept across a retrain only when skill sha256, repository and pack digest
  are all unchanged.
- Readers: `verified::read(dir)` (strict schema, ≤ 256 KiB, ≤ 64 steps, text fields ≤ 600
  bytes, dates `YYYY-MM-DD`; text is bounded, not judged: every line is sanitised when rendered),
  `verified::find(skills_dir, name, repo)` and `find_all(skills_dir, repo)` (accepted records
  only, at most 256 staged skills looked through, no symlinks).

### `SkillPrompt` (`adage_hook::skill`)

```text
stdin   {"session_id","cwd","hook_event_name":"SkillPrompt","skill":"acme-work-item","text":"<the skill's text>","observe"?:bool}
stdout  nothing, or {"text":"<text>\n\nAdage verified these steps in this repository on 2026-10-03:\n- make test: works\n- scripts/wi-lookup {id}: stale (exit 1: …)"}
```

- Only `adage hook claude SkillPrompt` (any other harness: nothing). Nothing unless an accepted
  record for that skill name and this repository's key (`adage_core::repo::repo_key(root)`, the
  key `adage train` recorded) exists and it lists at least one `works` or `stale` step.
- One line per `works` / `stale` step in record order; `not-replayed` steps say nothing; command
  and signature through `render::sanitise`; heading date = the latest step date; at most 2,400
  bytes added (the blank line included), whole lines only.
- Assigns the session's arm if it has none (as `PreToolUse` does), then spools one record:
  `{"ts","event":"SkillPrompt","kind":"skill_prompt","arm","propensity","repo","would_annotate":{"skill","verified","stale","bytes"},"annotated"?:{same},"observe"?:true,"duration_us"}`.
  `annotated` only when the text was changed; the control arm and the mod's observe mode print
  nothing. Spool consumers that switch on `kind` ignore `skill_prompt` today (D5 draws it).
- Fails open like every hook path; a bad payload, no record, an unreadable file: nothing.

**Registration edits in `crates/adage-hook/src/lib.rs`** (the harness agent's file): `pub mod
skill;` beside `pub mod press;` (line 28); in `handle()`, right after the kill-switch check and
before the `PreToolUse` dispatch, `if event == skill::EVENT { return skill::prompt(adapter, &p,
&session, &root, paths, started); }` (lines 375–377); in `respond()`, the `Policy` literal gains
`invoke_skills: if predict_grants.is_empty() { Vec::new() } else { skill::invocable(paths, root) },`
(line 1452) placed before `predict_grants,`.

### For D5 and the mod

- A `skill` prediction arrives as any other (`kind: "skill"`); the mod's existing
  `$.command.list()` check (P5) still applies. The row marker reads the `turn_step` record's
  `decision.predicted.kind == "skill"`.
- The `skill.prompt` hook: ask `SkillPrompt` with `{ skill: e.skill, text: e.text }` (and
  `observe: true` in observe mode), 2,000 ms; `{ text }` → return it, nothing → `next(e)`. The
  "verified 3 of 5 steps here · step 2 corrected" marker can be drawn from the spooled
  `skill_prompt` record's `annotated.verified` / `annotated.stale`.

## Files

Owned and new: `crates/adage-tracks/src/{model.rs,io.rs,interlock.rs,index.rs}` (the skill step,
validation, `command_of`, the index `format` / `Unloaded::Newer` / `newer_format`),
`crates/adage-tracks/tests/skill.rs` (new); `crates/adage-runtime/src/{lib.rs,render.rs}`
(skill-only functions: `bind_prompt_route`'s skill branch, the after-edit skill branch,
`fire_contexts`, `skill_call`, `pending_skill`, `Policy.invoke_skills`, the predict skill arm),
`crates/adage-runtime/src/skill_tests.rs` (new, registered with `mod skill_tests;` beside
`mod tests;`); `crates/adage-pack/src/{verified.rs (new),lib.rs,convert.rs}`;
`crates/adage-train/src/{skill.rs,compile.rs,verify.rs}`, `crates/adage-train/tests/verified.rs`
(new); `crates/adage-hook/src/skill.rs` (new).

Small edits in other owners' files: `crates/adage-hook/src/lib.rs` (the three registration lines
above); `crates/adage-runtime/src/tests.rs` (T-C1-06's skill half now uses a real `Step::Skill`
route and `invoke_skills`, the only test of the old stand-in); `crates/adage-trace/src/{wire.rs,
session.rs}` (`skill_sentence`, `Named.skills`, `Directed.skills`); `crates/adage-ingest/src/signals.rs`
(copies `skills`); `crates/adage-store/src/config.rs` (`skills.invoke`); `crates/adage-safety/src/lib.rs`
(`T_SKILL`, `step_ceiling`); exhaustive match arms in `crates/adage-graph/src/compile/ids.rs`,
`crates/adage-app/src/{graph.rs,reflex.rs}`, `crates/adage-cli/src/share.rs`,
`crates/adage-eval/src/{conventions.rs,savings_replay.rs,score.rs,lib.rs}`; reviewed comments at
the wildcard sites `crates/adage-learn/src/{dormant.rs,engine.rs,value.rs}` (comment only);
`crates/adage-cli/src/train.rs` (`accept` stamps `accepted_at`), `crates/adage-cli/src/doctor.rs`
(the newer-index message and its test), `crates/adage-cli/tests/hooks.rs` (a `commit-review`
skill route on a sha prompt, one new `UserPromptSubmit` step at the end of `claude_scenario`),
`crates/adage-cli/tests/golden/claude-code.txt`, `crates/adage-cli/tests/skill_prompt.rs` (new),
`xtask/src/affected.rs` (`evals/lab/` is read by adage-cli now); docs below.

Docs: `docs/how-it-works.md` (a sixth behaviour row, "Six distinct behaviors", and a paragraph),
`docs/training.md` ("Verified in place", the staged files, `accept`'s fourth step),
`docs/live-learning.md` (the `skills.invoke` row; the index format and the minimum-version rule),
`CHANGELOG.md` (two bullets under Runtime).

## Tests (by test-plan id)

| Id | Where | Status |
| --- | --- | --- |
| T-D3-01 | `adage-tracks/tests/skill.rs` (YAML round trip, shape, `SHAPES.len()`, ceiling, accepts `release-checklist`, refuses `../x`, 65 bytes, slots, inhibit, fire, other triggers) | green |
| T-D3-02 | `adage-runtime/src/skill_tests.rs::the_skill_template_names_the_skill_and_no_command` | green |
| T-D3-03 | `skill_tests.rs::a_skill_directive_is_followed_by_using_that_skill` (+ prompt span and context delivery, + the skill prediction gate) | green |
| T-D3-04 | not built (the bridge: graph learner) | open, see below |
| T-D3-05 | not built (learned skill routes) | open, see below |
| T-D3-06 | `cargo clippy` on every crate touched; reviewed comments at `interlock.rs::avoids`, `dormant.rs::probe`, `engine.rs::named_commands`, `value.rs::directed_command` | green for the crates listed below |
| T-D3-07 | `adage-cli/tests/hooks.rs::claude_code_golden` (re-blessed; `git diff` adds only the new step) | green; `adage-exec --test parity` green |
| T-D3-08 | `adage-tracks/src/index.rs::format_tests`, `adage-cli/src/doctor.rs::an_index_from_a_newer_binary_is_reported_as_newer`; the replay-gate whitelist part has no new id (nothing learns a skill route yet) | green |
| T-D4-01 | `adage-train/tests/verified.rs` (acme skill in the acme sandbox: one record per candidate, works/stale/not-replayed, date, inert; pack digest and `procedure.json` independent; dynamic block and `context: fork` → `inert: false`; acceptance kept across a retrain of the same pack, dropped for a changed skill); `adage-pack/src/verified.rs` unit tests | green |
| T-D4-02 | `adage-cli/tests/skill_prompt.rs::an_accepted_skill_is_annotated_sanitised_and_bounded` | green |
| T-D4-03 | `skill_prompt.rs::the_arm_is_assigned_and_the_control_arm_only_records` (+ observe mode, other harnesses) | green |
| T-D4-04 | the mod's `skill.prompt` (lands with D5, per the plan) | open (D5) |
| T-D4-05 | live, S9 Q13 | not run (no live session here) |

The golden diff (`git diff crates/adage-cli/tests/golden/claude-code.txt`, the `PreToolUse`
step is another chunk's, A1):

```
+### PreToolUse
+(nothing)
+
…
+### UserPromptSubmit
+{"hookSpecificOutput":{"additionalContext":"<adage-reflex track=\"acme/work@0.3.0\" route=\"commit-review\" strength=\"prime\" kind=\"skill\">\nIn this repository the `commit-review` skill is used at this point. Ignore this if the user is asking for something else.\nLearned by Adage from this repository's earlier agent sessions; not requested by the user.\n</adage-reflex>","hookEventName":"UserPromptSubmit"}}
+
```

## Departures from the plan, and why

1. **No learned skill routes (§2.1.2; T-D3-04, T-D3-05).** The canonical bridge, `input::Step::Skill`,
   `NodeType::Skill`, `target_label`, the compiler's `skill` shape and the engine's judge live in
   `adage-graph` and `adage-learn`, which the learner agent is rewriting this wave (D1:
   `mine/mod.rs`, `select.rs`, `compile/shapes.rs`, `engine.rs`); this brief kept those crates
   out. Adding Skill events also changes the miner's alphabet (`gap`/`within` count alphabet
   events), so it must land with the route-fidelity gates run, which belongs with the learner.
   Everything downstream is ready for it: a compiled `Step::Skill` validates, indexes, renders,
   delivers, is judged and predicted; `Named.skills`/`Directed.skills` carry the steering; a skill
   directive names no command, so `engine::exposed` treats it as naming nothing but `all`.
2. **`SkillPrompt` is gated by `skill::rewrites_skill_text(harness)` (Claude Code only), not by a
   `Capabilities` field**, and renders its own `{"text": …}` rather than through a new
   `Output` variant: the harness agent is rewriting `Capabilities`, `Output` and
   `harness/claude.rs` this wave. Moving the gate to a `skill_prompt` capability and the render to
   `Output::Text` is two small edits in their files once they land.
3. **The skill gate is a `Policy` field, not a filter on the grants**: `Policy.invoke_skills` is
   what the runtime checks, so `decide()` alone enforces "inert, accepted, `skills.invoke`".
4. **`verified.json` steps**: one per candidate that runs a command (not per candidate of every
   shape: a read batch, advice or a prohibition has nothing it verifies of its own), with a
   `route` and the step ids joined by `+`; plus top-level `pack` and `digest` so that accepting the
   pack (and only that pack) accepts the record.
5. **The index's `format` is 1** (absent/0 before). A newer index this binary cannot parse is
   `Unloaded::Newer(format)`: served empty and **not** rebuilt (the hook's `damaged` flag is only
   for `Damaged`), and `doctor` fails the index check with "newer than this binary". The learner's
   D1 fields (`candidates`, `candidate_contexts`) parse in older binaries already, so they need no
   bump of their own.
6. **A retrain keeps acceptance only for the identical pack** (same skill sha256, repository and
   digest). A replay whose stale reason names a sandbox's temporary path makes `procedure.json`,
   and so the digest, differ between runs (pre-existing in `verify.rs`); such a retrain needs a new
   `accept`.

## Live-only, not verified here

S9 Q13 (`/acme-work-item`, the Skill tool and a subagent preload each read the annotated text),
T-D4-04/05, and whether a Skill call yielded by `turn.step` is recorded by the engine as the
agent's. No API key in this environment.

## Commands run

- `cargo fmt --all` then `cargo fmt --all --check`: clean.
- `cargo clippy -p adage-tracks -p adage-runtime -p adage-pack -p adage-hook -p adage-train -p adage-safety -p adage-store -p adage-trace --all-targets -- -D warnings`: clean.
- `cargo clippy --keep-going -p adage-ingest -p adage-graph -p adage-app -p adage-eval -p adage-cli --all-targets -- -D warnings`:
  one error, not in this change (`crates/adage-cli/tests/zero_config_plugin.rs:922`, a doc list
  indentation lint in another chunk's test).
- `cargo test -p <crate> --no-fail-fast`, final run: adage-tracks 48 + 3 + 4 + 1 + 4 passed;
  adage-runtime 103 + 1; adage-pack 16 + 6 + 4 + 13 + 8 + 7 + 5 + 3; adage-hook 43;
  adage-train 22 + 7 + 2 + 2 (the sandbox is available here, so the replay parts ran);
  adage-safety 6; adage-store 20; adage-trace 3. All green. (An earlier adage-tracks run caught
  the learner's in-progress `candidates` test failing; it passed in the final run.)
- `cargo test -p adage-cli --test hooks` (after `ADAGE_BLESS=1 … claude_code_golden`), `--test
  skill_prompt`, `--test train_accept`, `--bin adage newer`: green.
- `cargo test -p adage-exec --test parity`: 22 passed.
- `cargo test -p xtask`: 18 passed.
- `cargo test -p adage-learn` was not runnable: its lib test target does not compile in the
  learner's in-progress `diff.rs` (not this change).

## Review fixes

| Finding | What changed | Test |
| --- | --- | --- |
| **M5** A skill's inertness and hash were never checked again at use | `verified.json`'s `skill` gains `path` (the absolute file `adage train` compiled; `adage_train::skill::Outcome.skill_file`, canonical; `#[serde(default)]`, written only when set, checked absolute and at most 4,096 bytes). `adage_pack::verified::current(&Verified) -> Option<bool>` reads that file again (a regular file of at most 1 MiB, `MAX_SKILL_BYTES`), and answers `Some(inert)`, recomputed from the file's text, only when its SHA-256 is the record's. `skill::prompt` annotates nothing (and records nothing) unless `current` is `Some`; `skill::invocable` keeps a skill only when `v.skill.inert && current(v) == Some(true)`. A record written before this change has no path and is not used until the skill is trained again (fail closed: the skill is still read as written, unannotated and not invoked). The payload's `text` is not hashed: what the engine hands `skill.prompt` is the expanded prompt, not necessarily the file's bytes (front matter, arguments), so the file on disk is the check | `verified::tests::a_record_is_current_only_while_its_skill_file_is_unchanged`; `skill::tests::nothing_is_invocable_until_…` (an edited skill and a removed one drop out); `adage-cli/tests/skill_prompt.rs::an_accepted_skill_is_annotated_sanitised_and_bounded` (the record names its file; after a `` !`…` `` block is added, nothing is annotated or recorded) |
| **L2** `verified::find_all` took 256 directories in read order, then sorted | It sorts every staged directory name first and then keeps the first `MAX_SKILLS` | `verified::tests::find_all_takes_the_first_skills_in_name_order` (257 directories; only the first by name is found) |

Residual (M5): a skill of the same name elsewhere (a user-level skill shadowing the project's, or
one a plugin ships) is not told apart: the record is keyed by name and repository, and the check
is that the trained file is unchanged, not that the engine expanded that file.
