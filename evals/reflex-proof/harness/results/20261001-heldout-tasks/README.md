# Held-out Marshmallow tasks (launch-gate-1 revision 9, draft)

Authored, validated offline and frozen before any development canary-v8 result was read. No paid
model ran and no Claude session was launched.

- Freeze commit: `260729ae0723531fd2c40308a98cdad71257f9ae`, committed 2026-10-01T06:09:53Z on
  `codex/heldout-tasks`.
- `manifest.json`: sha256 `02341cc07e54627f60767c39ef22747250e52d388873c715d6242b156646f556`,
  frozen_at_utc 2026-10-01T06:09:16Z.
- Definitions: `evals/reflex-proof/harness/heldout_tasks.py`. It uses the earlier mm-curve schema, the
  `curve_tasks.prompt` template, the lab `mm_hidden` tests and the generated `mm_gold`;
  `curve_tasks.py` is unchanged.

## Tasks

| Task | Group (split) | Prompt | Hidden test | Gold patch |
| --- | --- | --- | --- | --- |
| `mm-curve--has-keys` | mapping-keys (evaluation) | `5daff605…` | `4d11270b…` | `0499b1e9…` |
| `mm-curve--iso-date` | temporal-format (evaluation) | `5f29842f…` | `13ab2ae5…` | `4e2b1de6…` |
| `mm-curve--json-object` | structured-text (evaluation) | `423da26b…` | `1f812a5c…` | `9a19e415…` |
| `mm-curve--lat-long` | geo-coordinate (evaluation) | `346a2951…` | `d28a3dbc…` | `3a046e61…` |
| `mm-curve--max-sum` | size-limit (withdrawn) | `ffb8afa1…` | `c538d978…` | `fb50db01…` |

Full sha256 values for every prompt, hidden test, gold patch, broken patch and gold file are in
`manifest.json`; the files themselves are under `tasks/`.

Each prompt is the earlier neutral template (validator, docstring, tests, changelog; only the first
valid and first invalid example shown) plus one realism line: `Do not create git commits.`. There is
no work order, no test block and no instruction to run tests, and the prompts never mention Adage.

**max-sum is withdrawn.** It has the size-limit group's `(max, *, error=None)` interface and a one-line
measure-at-most-`max` predicate. Its check is 0.73 similar to max-words; the registered
near-duplicate pair max-utf8-bytes/max-words is 0.74. By the rule that groups those two, max-sum
joins size-limit (training), is never run and stays recorded here. lat-long replaces it.

`near_duplicates.py` produced `near-duplicates.json`, a text-similarity screen of the generated
check against every earlier task. The registered pairs score 0.41 to 0.74. Text similarity is only a
screen; concept and interface decide:

| Task | Nearest earlier task (similarity) | Why it is not a near-duplicate |
| --- | --- | --- |
| has-keys | finite (0.38) | Conceptually nearest is training `contains`, but has-keys requires a mapping and every key. Its hidden cases include the list and string inputs a contains-style check wrongly accepts. |
| iso-date | finite (0.33) | It shares a parse-with-the-standard-library shape with training `ip-address`, but not its domain. |
| json-object | finite (0.55) | The score comes from the shared `__import__(...)` idiom of generated checks, not the concept. Its structure is like `ip-address` (parse, then check). |
| lat-long | alphanumeric (0.47) | The score comes from shared `len(...)` and comparison idioms. No training task has a numeric-range concept. |

The parameterless `(*, error=None)` interface is shared by 12 earlier tasks, so it does not
discriminate. The `(max, …)` interface is shared only by the size-limit group.

## Validation (offline, pinned environment)

Command (from `evals/reflex-proof/harness`), output in `validate.log`:

```sh
python3 heldout_tasks.py validate <scratch>/validation-r1 --authority /private/tmp/adage-real-repo-gate-v1
python3 heldout_tasks.py archive <scratch>/validation-r1 --destination results/20261001-heldout-tasks
```

The authority's `runtime/python`, `cache/uv` and `cache/repobench` were copied, not linked, with uv's
absolute minor-version link repointed into the copy, so nothing was written into the authority.
Python 3.12.14 and the authority's pinned dependencies (pytest 9.1.1 and the rest) were installed
offline into a new `.venv` per checkout from the base `7f0792bd`. The existing Seatbelt-confined
scorer ran the whole suite with the hidden test.

| Task | Base (fails hidden only) | Gold (all pass) | Hidden ids | Broken implementations (hidden behaviour failures only) |
| --- | --- | --- | --- | --- |
| has-keys | 1201 tests | 1202 | 11 | no-mapping-check, any-key, truthy-values |
| iso-date | 1203 | 1204 | 13 | fromisoformat-only (`20240105`), strptime (`2024-1-05`), pattern-only |
| json-object | 1205 | 1206 | 15 | any-json, brace-sniffing, no-leading-whitespace |
| lat-long | 1203 | 1204 | 13 | swapped, exclusive-bounds, no-length-check |
| max-sum (withdrawn) | 1202 | 1203 | 12 | strict, per-element, count |

`validation.json` (sha256 `18f5a3fd…`) has every row. All 25 checkouts behaved as required:
- every base failed only hidden tests, with no suite failures;
- every gold passed the hidden tests and the full existing suite, adding exactly its own test;
- each of the 15 broken implementations failed hidden behaviour tests (valid, invalid or custom
  error) and nothing else;
- every gold's hidden-test JUnit identities were unique and passing
  (`real_repo_pairs.prepare_hidden_expectations`).

## Split and isolation

- `split-audit.json`: `split_audit.py` on `cohorts/curve-guided-v2/cumulative-launch-10` with the
  authorities `/private/tmp/adage-learning-curve-v2` and `/private/tmp/adage-real-repo-gate-v1`
  **passes**. The markers include `HasKeys`, `IsoDate`, `JsonObject`, `LatLong` and every evaluation
  task id. No marker appears in any training input transcript or learned file. Ten contributing
  sessions were checked, 117 sessions scanned, 83 of them forbidden.
- `name-scan.json` (`name_scan.py`) checks the transfer-v3 caveat. 671 earlier session files were
  scanned: native transcripts, streams, prompts, final patches and learning-history copies. None
  names any authored family by class name, task id or snake_case form. The only matches were
  marshmallow's own changelog text (`from_iso_date`, `*_iso_datetime`), counted separately. The live
  dev-v8b study is skipped. Unlike the transfer-v3 tasks, none of these families ran in a cold pilot
  or any other session.

## Natural workflow expectation (recorded before any run)

Route run-175d85 fires on an edit to `tests/*.py`, then `CHANGELOG.rst` within eight events.
`workflow_scan.py` produced `workflow-base-rate.json`. It covers the 31 earlier unguided Marshmallow
sessions on the same template (organic training and the no-hook agent arm; launch-gate cohorts
excluded). All 31 edited `validate.py`, then tests, then `CHANGELOG.rst`. 27 of 31 produced the
context within eight tool events; in the other four, more events separated the two edits.

Expected for every held-out family: the context fires in most sessions. Possible risks by family:
- iso-date and json-object may need more iteration on the agent's own tests, which can separate the
  two edits;
- has-keys and lat-long: none known.

The tasks were not tuned to trigger the reflex (`heldout_tasks.EXPECTATION`).

## Disclosure

Before the coordinator's lock (2026-10-01), the task author's read-only scan for the natural
tests-then-CHANGELOG base rate read the edit order (only) in 3 dev-v8b A0 transcripts:
`00-A0-positive-b1`, `07-A0-positive-b2` and `10-A0-positive-b3`. That means the file paths of their
Edit/Write calls. For `07-A0-positive-b2` it also confirmed that one Edit to a nonexistent path had
failed with a tool error. It computed no outcome metrics (no request counts, costs, quality or arm
comparisons), and those sessions are excluded from the base-rate scan.

Other read-only inputs, all already reported and none from dev-v8b:
- the request-count cross-check was looked at on canary-v7 sessions;
- the per-session cost distribution came from 78 earlier sessions;
- a session inventory counted manifests per task under the authorities; it counted dev-v8b's
  manifests by task only.

These inform the revision 9 budget and metric only.
