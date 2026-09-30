# Real coding action gate: prospective preparation

This screen asks whether a learned CPU action replaces an agent action while preserving the
result of a complete coding task. It uses authored feature requests on real upstream repositories,
not historical upstream issues and not a generated imitation repository. Prior benchmark outcomes
are known; this is a prospective mechanism/regression screen, not a blind replication.

## Frozen initial matrix

| Repository | Base | Training variants | Held out |
| --- | --- | --- | --- |
| Marshmallow | `7f0792bd` | starts-with, multiple-of, unique, hexadecimal, ends-with | ip-address, not-blank |
| Click | `06b2a67` | hex-color, port, duration, ip-address, semver | percentage, byte-size |

Existing showcase prompts, gold patches and scorers are reused unchanged. Each held-out task
gets suggest and automatic execution, one repeat, seeded arm order `20260930`: eight assigned
sessions. Both arms get identical prompts, tools, dependencies and frozen learned knowledge.
The only treatment is the learned action's exact human execution grant. Neither a family-specific
detector, a manually authored route, nor a treatment-only instruction to skip tests is permitted.

The archived parsed sessions contain real tool inputs and outputs. Some sessions were steered by
prior advice; their influence markers must remain intact. The production learner determines
eligibility. They are audit-only, never reconstructed as new training. Collect five fresh organic
training sessions per repository, with intervention disabled. This prospective revision from four
to five training variants occurred before any new paid dispatch or held-out observation, to satisfy
the production default `min_support=5`. Training uses fixed offline tool permissions and no Adage
guidance; it is not unrestricted natural-use history. Preserve original native Claude JSONL
transcripts with init session identity, cwd and SHA-256; no synthetic calls or reconstructed outcomes.
Freeze and record the candidate before any
held-out call. A missing learned action is a failed preparation, never grounds to fabricate one.
Do not use newer showcase source reads as training for older historical bug-fix tasks: they may
contain the held-out fixes.

## Preparation and confinement

`real_repo_gate.py prepare --root ROOT` clones public pinned upstream repositories, materializes
their base trees, creates local Python environments through uv using a requested Python version
(default 3.12), records actual versions/dependencies, and audits training archive provenance.
It creates no grant and dispatches no model. All setup subprocess attempts are durably appended
to `attempts.jsonl`, including failures. Failed preparation roots are retained.

`real_repo_gate.py validate ROOT` requires each unchanged base to fail the existing hidden scorer
with tests actually collected and a hidden assertion failing, and its gold solution to pass.
The scorer's test process runs under macOS Seatbelt with no network,
read access only to system/runtime dependencies and its checkout, and writes only within its own
checkout/scratch. Hidden tests and gold patches stay in private validation directories. No rescue
package installation executes an agent-modified build backend outside confinement; dependencies
must already be prepared. A damaged environment is a failure.

Before paid work, an outside control verifies a disposable local canary and loopback listener;
the confined Python subprocess must fail to read/write outside its checkout and fail to connect
to that listener. This establishes the test-process boundary, not yet a complete live-agent
permission proof. The coding agent additionally needs restricted file tools, a narrow Bash allowlist,
native sandbox enabled with unavailable/unsandboxed fallback disabled, and protected dependencies,
configuration and Git metadata. The independent scorer must never run modified code unconfined.

## Candidate attachment and live admission

Preparation writes `candidate: null` and `live_dispatch_enabled: false`. The next stage must attach
the actual production learner's route, supporting eligible sessions, exact command/output contract,
repository and dependency identity, binary/route hashes, action timing and input-generation checks.
Prove production staging can resolve the actual runtime before requesting a human grant. The human
approves only that concrete E2 scope through the existing CLI. This preparation is not approval.

The existing shared paid-run budget and live orchestration remain the admission authority: reserve
each session before dispatch, preserve every attempt, stop on unknown accounting, and reconcile
all prior task spend. This document creates no additional spending authorization. Publish the
proposed session/total caps before live dispatch. The fixed reservation is $1.50 per session,
10 training sessions plus 8 held-out sessions, $27 maximum, plus one native confinement probe capped
at $0.50: $27.50 total allocation. Prior cumulative
spend must still be reconciled against the user's $60 task cap. No failed outcome is retried.

`prepare-native-probe ROOT --model claude-haiku-4-5-20251001` prepares a disposable probe with five
turns maximum. `prepare-training ROOT --task TASK --model claude-haiku-4-5-20251001` prepares one
assigned task with 60 turns maximum. Neither command dispatches. `run-session MANIFEST` is the
explicit paid entry point using the shared budget and probe runner, CLI 2.1.282, a 300-second default
wall limit, disabled hooks/skills/memory, host login retained, and native session persistence.
It blocks altered helper/settings hashes, repeated assignments and unresolved accounting. Before
training, a reviewer must save `native-confinement-reviewed.json` with `passed: true` after inspecting
actual tool results: denied outside Read plus all native Bash canary checks, with outside controls.
That record is an evaluation admission decision and grants no product execution authority.

Native settings use `--restricted`, no setting sources, `dontAsk`, no permission prompts,
`sandbox.enabled`, `failIfUnavailable`, and no unsandboxed fallback. Home and evaluation-parent reads
are denied with narrower checkout/runtime exceptions; dependencies and Git/config metadata are
protected from writes. Tool subprocess credential variables are denied by name while agent boundary
markers remain. The parent keeps its ordinary login. These settings follow the current
[Claude sandbox reference](https://code.claude.com/docs/en/sandboxing); the paid native probe must
prove the installed CLI accepted them, separately from the direct Seatbelt scorer proof.

### Preparation failures retained

The first offline validation collected no Marshmallow tests because macOS timezone data resolves
outside `/usr` and failed one existing Click test because it checks `/tmp` directory readability.
Those are infrastructure failures, not model outcomes or valid base sensitivity. The correction
permits only canonical system timezone data and literal `/tmp` and `/private/tmp` directory nodes.
Automatic review initially rejected that patch as broad temporary-file access; a disposable native
proof established readable directory nodes with a known child file's content still denied, after
which the narrow change was approved. No temporary-tree subpath read was added, no test excluded,
and failed validation reports and every subprocess attempt remain recorded.

## Required evidence and go/no-go

Every assignment must preserve its raw transcript, process status, independent hidden-test result,
production receipts, repository diff, matcher decision, grant identity, input/output hashes,
agent action count, observed model requests, wall time and actual cost. Record missed matches,
redundant actions, failures, censored sessions and setup errors; do not silently retry failures.

A pair passes only when both coding outcomes pass independent checks, the treatment has a successful
production CPU action tied to the final relevant input generation, and the corresponding agent
action is eliminated. A test started before the final edit is not evidence that the final code was
verified. Count repeated automatic work and its latency. No pooled general savings claim follows
from four task pairs. Report task-level effects and preserve failures in the shareable ledger.
