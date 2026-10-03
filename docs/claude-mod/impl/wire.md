# The wire to the mod: shared answer files

What the `adage` binary answers the Claude Code mod is pinned by one JSON file per answer shape,
`crates/adage-hook/tests/wire/*.json`. The Rust side writes each file from a typed answer and
compares (`crates/adage-hook/tests/wire.rs`); the mod's tests read the same files, so a change on
either side that the other does not follow fails a test on the side that did not change. This
page is the files' contract. MAINT-RUST item W4: the Rust half is built; the plugin's tests read
the files in wave B.

## The answers

Every answer the binary sends the mod is one `adage_hook::answer::ModAnswer`, printed by one
function, `adage_hook::answer::answer` (`crates/adage-hook/src/answer.rs`). A command hook's answer
is Claude Code's own format (`hookSpecificOutput`, `systemMessage`, `decision`) and is not on this
wire: the adapter's `render` writes it. These are the answers only the mod reads:

| Key | Asked by (the mod's call) | Binary event | Rust variant | The mod reads it in |
| --- | --- | --- | --- | --- |
| `plan` | the dry run of a Bash call (`dry_run: true`) | `PreToolUse` | `ModAnswer::Plan` | `call.ts` (the plan), `substitute.ts`, `receipt.ts` |
| `substitute` | the verdict naming a dead end (`verdict.substitute`, `engine: same`) | `PreToolUse` | `ModAnswer::Substitute` | `substitute.ts` (`substituteOf`), `call.ts` |
| `receipt` | the verdict `answered: receipt` | `PreToolUse` | `ModAnswer::Receipt` | `receipt.ts` |
| `standing` | `prompt.compose`, `agent.spawn` (with `agent`) | `Standing` | `ModAnswer::Standing` | `attach.ts` |
| `text` | `skill.prompt` | `SkillPrompt` | `ModAnswer::Text` | `skill.ts` |
| `press` | the session's start | `Press` | `ModAnswer::Press` | `token.ts` |
| `predict` | `turn.step` | `TurnStep` | `ModAnswer::Predict` | `predict.ts` (`predictionOf`), `step.ts` |
| `routine` | `turn.step` with `routine_model` | `TurnStep` | `ModAnswer::Routine` | `step.ts` |

"Nothing to say" is no output at all (an empty stdout), never an empty object; it has no file.

## The files

- **Where:** `crates/adage-hook/tests/wire/`. Nothing else is in the directory.
- **Names:** `<key>.json` for the main case of an answer, `<key>-<case>.json` for another case of
  the same shape. `<key>` is the answer's top-level key from the table above, except `standing`,
  whose answer has four top-level keys (`standing`, `environment`, `compact`, and `subagent` for a
  spawn) and whose files are named `standing*.json`. The part before the first `-` is the key; a
  reader can group files by it.
- **Today's files:** `plan.json` (a substitution, the interpreter rewrite and a fresh receipt),
  `plan-advice.json` (advice and a receipt that is not fresh: no `note`, no `result`),
  `substitute.json`, `receipt.json`, `receipt-without-card.json` (`card: null`), `standing.json`,
  `standing-subagent.json` (with `subagent`; the control arm's empty texts), `text.json`,
  `press.json`, `predict.json` (an obligation: one `Bash` call), `predict-chain.json` (a chain: two
  `Read` calls), `routine.json`.
- **Content:** exactly one answer, as JSON: the object the binary prints on one line, here
  pretty-printed with two-space indentation, keys sorted, and a final newline. `JSON.parse` of a
  file gives the value `JSON.parse` of the binary's stdout gives for the same answer. The binary's
  own bytes are that object on one line with sorted keys (`serde_json`'s `Value::to_string`).
- **Optional fields:** a field that may be absent is absent in some file and present in another
  (`plan.substitute`, `plan.rewrite`, `plan.advice`, `plan.receipt`, and in a plan's receipt
  `operation`, `note`, `result`; `standing.subagent`). `receipt.card` is never absent: it is
  `null` when there is no card. `receipt.result` may be `null`.
- **Values:** illustrative but realistic (the shapes the binary's own tests produce). A reader
  asserts what it parses out of a file, not the prose of a text field.

## Changing an answer

1. Change the type in `answer.rs` and the code that builds it.
2. Change or add the case in `crates/adage-hook/tests/wire.rs` (`cases()`); a new variant must
   also be named in `key()`, whose `match` is exhaustive.
3. `ADAGE_BLESS=1 cargo test -p adage-hook --test wire` rewrites the files; review the diff.
4. The plugin's test for that answer then fails until the mod reads the new shape (wave B wires
   the vitest suite to these files).

The test also checks that every file has a case and every variant a file, that each file reads
back (`serde_json::from_str::<ModAnswer>`) as its case, and that `answer` prints what the
untyped `json!(…).to_string()` printed before (one line, keys sorted).
