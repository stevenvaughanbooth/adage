# The wire to the mod: shared answer files

What the `adage` binary answers the Claude Code mod is pinned by one JSON file per answer shape,
`crates/adage-hook/tests/wire/*.json`. The Rust side writes each file from a typed answer and
compares (`crates/adage-hook/tests/wire.rs`); the mod's tests read the same files, so a change on
either side that the other does not follow fails a test on the side that did not change. This
page is the files' contract. MAINT-RUST item W4: both halves are built (the TypeScript side is
[below](#the-typescript-side)).

## The answers

Every answer the binary sends the mod is one `adage_hook::answer::ModAnswer`, printed by one
function, `adage_hook::answer::answer` (`crates/adage-hook/src/answer.rs`). A command hook's answer
is Claude Code's own format (`hookSpecificOutput`, `systemMessage`, `decision`) and is not on this
wire: the adapter's `render` writes it. These are the answers only the mod reads:

| Key | Asked by (the mod's call) | Binary event | Rust variant | The mod's decoder |
| --- | --- | --- | --- | --- |
| `plan` | the dry run of a Bash call (`dry_run: true`) | `PreToolUse` | `ModAnswer::Plan` | `call.ts` `planOf` (its parts by `receiptPlanOf`, `substituteOf`, `rewritePlanOf`) |
| `substitute` | the verdict naming a dead end (`verdict.substitute`, `engine: same`) | `PreToolUse` | `ModAnswer::Substitute` | `substitute.ts` `substituteAnswerOf` |
| `receipt` | the verdict `answered: receipt` | `PreToolUse` | `ModAnswer::Receipt` | `receipt.ts` `receiptAnswerOf` |
| `standing` | `prompt.compose`, `agent.spawn` (with `agent`) | `Standing` | `ModAnswer::Standing` | `attach.ts` `standingOf`, `subagentOf` |
| `text` | `skill.prompt` | `SkillPrompt` | `ModAnswer::Text` | `skill.ts` `skillTextOf` |
| `press` | the session's start | `Press` | `ModAnswer::Press` | `token.ts` `pressOf` |
| `predict` | `turn.step` | `TurnStep` | `ModAnswer::Predict` | `predict.ts` `predictionOf` |
| `routine` | `turn.step` with `routine_model` | `TurnStep` | `ModAnswer::Routine` | `step.ts` `isRoutine` |

Every decoder is pure and takes the parsed answer as `unknown`: anything malformed is nothing
(undefined, `''` or `false`), never a throw, and the call goes on as if the binary had said nothing.

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

## The TypeScript side

`claude plugin test` reads nothing outside the plugin's folder: a test file cannot import
`node:fs`, a JSON file, or a path above `plugin/claude-code` (2.1.288 refuses each, and the test
engine's `$` has no `fs`). So the files reach the mod's tests through a mirror:

- **The mirror:** `plugin/claude-code/hooks/fixtures/wire.ts`, one `WIRE` object from file name
  (without `.json`) to the file's text, byte for byte. `node scripts/check-plugin.mjs --wire`
  writes it; every other run of the script (CI's `plugin` job) fails while it differs from the
  files, naming the command to run. It is never edited by hand.
- **The loader:** `plugin/claude-code/hooks/fixtures/kit.ts` exports `wire(name)` (the answer,
  parsed afresh, so a test may change its copy), `wireLine(name)` (the bytes the binary prints:
  the answer on one line with its keys sorted, and a newline) and `wireWith(name, over)` (a case
  the files do not hold, made from one that they do: `over` is laid on the answer's body, the
  value under its key or the whole answer for `standing`, field by field; an `undefined` field is
  removed, and a value that is not an object replaces the body, so `wireWith('predict', null)` is
  `{ "predict": null }`). `name` is typed by the mirror's keys, and a name with no file throws,
  listing the files there are. `WIRE_NAMES` lists them all.
- **The tests:** one per file reads it through the decoder in the table above and asserts every
  field the decoder parses out of it (`call.test.ts`: `plan`, `plan-advice`;
  `substitute.test.ts`: `substitute`; `receipt.test.ts`: `receipt`, `receipt-without-card`;
  `attach.test.ts`: `standing`, `standing-subagent`; `skill.test.ts`: `text`; `token.test.ts`:
  `press`; `predict.test.ts`: `predict`, `predict-chain`; `step.test.ts`: `routine`), and
  `bridge.test.ts` reads every file's `wireLine` back through `ask` as its `wire`. Every test that
  stubs the binary's answer (the kit's world by `hook` or `answer`, the fake by `run`, a step's
  or a call's `ask`) takes it from `wire`, `wireLine` or `wireWith`, and so do the kit's own
  `TOKEN`, `PREDICT`, `PREDICTED_ID`, `VENV`, `SKILL_ANNOTATED` and `SKILL_TEXT`.
- **The rule:** `scripts/check-plugin.mjs` (`checkWire`) fails when a file is read by no plugin
  test (`wire('<name>')`, `wireLine(…)` or `wireWith(…)` in a `*.test.ts`), or when a test or
  `kit.ts` holds an answer literal of a shape that has a file: an object opening with `plan`,
  `substitute`, `receipt`, `predict` or `routine` with an object, a list or `true` under it; any
  answer key first in an object printed (`JSON.stringify({ … })`) or handed back
  (`answer: { … }`); `standing` and `environment` together; or the binary's JSON text itself
  (`'{"predict":…}'`). What a test lays over a wire answer inside `wireWith(…)` is not read as an
  answer. A new key's files are covered by the rule as soon as they exist.

## Changing an answer

1. Change the type in `answer.rs` and the code that builds it.
2. Change or add the case in `crates/adage-hook/tests/wire.rs` (`cases()`); a new variant must
   also be named in `key()`, whose `match` is exhaustive.
3. `ADAGE_BLESS=1 cargo test -p adage-hook --test wire` rewrites the files; review the diff.
4. `node scripts/check-plugin.mjs --wire` mirrors them into the plugin; `claude plugin test
   plugin/claude-code` then fails in the decoder's test (and any test whose stub no longer reads)
   until the mod reads the new shape. A new key also needs its decoder and its test
   ([mod-structure.md](mod-structure.md), "A mod answer"), which check-plugin asks for by name.

Until step 4, check-plugin fails on the mirror; after it, a decoder that does not follow fails the
plugin's tests. Either way the change cannot land on one side alone.

The Rust test also checks that every file has a case and every variant a file, that each file reads
back (`serde_json::from_str::<ModAnswer>`) as its case, and that `answer` prints what the
untyped `json!(…).to_string()` printed before (one line, keys sorted).
