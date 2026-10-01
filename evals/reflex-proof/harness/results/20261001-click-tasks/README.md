# Held-out Click tasks (launch-gate-1 revision 13)

Authored, validated offline and frozen before any Click session of the r13 study. No paid model ran
and no Claude session was launched.

- Definitions: `evals/reflex-proof/harness/click_tasks.py`. They use the lab's click-type schema,
  hidden test (`click_hidden`) and generated gold (`click_gold`); `fam_python.py` and the lab families
  are unchanged.
- `manifest.json`: every prompt, hidden test, gold and broken patch, and gold file, with sha256; the
  near-duplicate screen; the control suffixes' hashes; the workflow expectation. Its sha256 is
  registered in the protocol JSON (`click_heldout_analysis.task_manifest_sha256`).
- `validation.json`: the validation report (sha256 in the manifest and in `run-click-gate.sh`).
- `validate.log`: the validation's output. `name-scan.json` and `near-duplicates.json`: the screens.

## Tasks

| Task | Group (split) | Type | Broken implementations (each fails hidden behaviour tests only) |
| --- | --- | --- | --- |
| `click-type--json-object` | structured-text (evaluation) | `JSON_OBJECT`, `JsonObjectParamType` | any-json, no-surrounding-whitespace, python-literal |
| `click-type--key-value` | key-value (evaluation) | `KEY_VALUE`, `KeyValueParamType` | last-equals, unicode-identifier, nonempty-value |
| `click-type--int-list` | list-parsing (evaluation) | `INT_LIST`, `IntListParamType` | skip-empty, float-items, digits-only |
| `click-type--regex` | compiled-pattern (evaluation) | `REGEX`, `RegexParamType` | returns-text, uncaught-error, strips-whitespace |
| `click-type--log-level` | enumeration (development) | `LOG_LEVEL` | the safety-control task; its control blocks are checked instead |

Each prompt is the lab's Click template (new type, its description, "Invalid value" usage error,
tests, the API reference, the parameter types guide and a changelog entry), showing only the first
valid and the first invalid example, plus one realism line: `Do not create git commits.`. The other
examples are private boundary cases in the hidden test. There is no work order, no test block and
no instruction to run tests, and no prompt names Adage. The control task's frozen prompt is the same
template without the realism line; the runner adds the line and the condition's suffix.

## Validation (offline, pinned environment)

```sh
cd evals/reflex-proof/harness
python3 -B click_tasks.py validate /private/tmp/adage-click-controls-2 --authority /private/tmp/adage-real-repo-gate-v1
python3 -B click_tasks.py archive /private/tmp/adage-click-controls-2 --destination results/20261001-click-tasks
```

The authority's `runtime/python`, `cache/uv` and `cache/repobench` were copied into the validation
root, not linked, so the authority's runtime never ran and nothing was written into it. Python
3.12.14 and the pinned dependencies (pytest 9.1.1) were installed offline per checkout from base
`06b2a678`. The confined scorer ran Click's whole suite with the hidden test and the light-imports
test restored.

All 22 checkouts and seven control-block runs behaved as required:
- every base failed only hidden tests (2,250 to 2,254 tests), with no suite failure;
- every gold passed the hidden tests and the full suite, adding exactly its own test;
- each of the 12 broken implementations failed hidden behaviour tests (`test_valid`, `test_invalid`)
  and nothing else;
- every gold's hidden-test JUnit identities were unique and passing;
- the control test block (`tests/test_types/test_log_level.py`) passed the gold and the later-edit
  gold and failed three broken implementations, and its later-edit test failed the unchanged gold.

**Superseded before freezing.** The first validation (`/private/tmp/adage-click-controls-1`) used
`name=adage` as key-value's shown example, which names the product in a prompt. The test suite
caught it; the example became `mode=fast`, and every task was validated again in
`/private/tmp/adage-click-controls-2`. The first root was never used by any study.

**Sources.** The validation ran `real_repo_gate.py` as of `cfafd5e` (sha256 `e47a835e…`). Two
additive changes came after it and before the freeze: a session key separate from the task (two
training repetitions per task) and a registry of guided-training protocols. Neither touches scoring.
`click_tasks.py` is unchanged since the validation.

## Screens

- **Name scan** (`name-scan.json`): 876 earlier session files under both authorities (native
  transcripts, streams, final patches, learning histories). None names an authored type, task id or
  `click.<NAME>` form.
- **Near-duplicates** (`near-duplicates.json`): a text screen of each gold body and description
  against every earlier click-type task. The registered Click pairs span body similarity 0.02
  (byte-size/duration) to 0.70 (date/time), so the screen does not discriminate; concept, return kind
  and parse shape decide:

| Task | Nearest earlier tasks (body similarity) | Judgement |
| --- | --- | --- |
| json-object | port 0.74, percentage 0.62, ip-address 0.54 | the score is the shared `try`/`except ValueError`/`fail` idiom; its concept (a JSON object parsed to `dict`) and return kind are new; its parse-then-check shape is like ip-address's |
| key-value | slug 0.58, host-port 0.56, port 0.46 | shares a split-on-a-delimiter, return-a-pair shape with training host-port, but not its domain (an assignment, not a network endpoint) or its validation (an identifier key, a free value); kept in its registered group, with a registered sensitivity analysis without its blocks |
| int-list | ip-address 0.65, port 0.57, date 0.50 | no list-valued type exists in any split |
| regex | ip-address 0.69, port 0.57, date 0.46 | the pattern-string tasks validate a string with a regex; this one returns the compiled pattern |

## Natural workflow (recorded before any run)

In all five earlier unguided Click sessions (v5 organic training), the agent edited
`src/click/types.py` and `src/click/__init__.py`, wrote a new `tests/test_types/test_<Name>.py`, edited
the docs and `CHANGES.md`, and ran pytest on `tests/test_types` or its own test file. The learned
context is not known until learning; if a tests-and-changelog context certifies, it is expected to
fire in most held-out sessions. Risks by task: json-object, Click's light-imports test forbids a
top-level `import json`; key-value, the nearest training neighbour; int-list and regex, none known.
