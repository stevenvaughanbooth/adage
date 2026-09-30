# Prospective launch screen — not run

Registered 2026-09-29 before new paid task sessions. Earlier published benchmark outcomes are
known; this is a prospective regression screen, not an independent blind replication. No paid
task result is claimed here. Source revision, working diff, binary hashes, harness version,
model ID, dataset hashes and frozen learned-artifact hashes must accompany the run.

## Question and arms

Does the **shipped default initialization and learning path** produce useful guidance and improve
held-out developer tasks? Primary pair: `adage` versus `control`, identical session hooks and
frozen learned artifact, with guidance delivered only in adage. Automatic CPU execution is a
separate experiment: compare `adage` versus `e1` only after a useful bounded output contract and a
real human grant exist. Do not attribute advice-arm changes to CPU execution or energy savings.

Default bootstrap must read staged training history through the shipped discovery/auto-learning
path, with default thresholds and no hand-authored route or hidden-score outcomes. Capture
`adage init --print`, actual hook/bootstrap or `condition --auto` invocation, history input hashes,
learner report/log, and final index hashes. An init printout alone is not an executed bootstrap.
Freeze the learned artifact before the first held-out session; do not learn from any eval run.

**Do not run the existing runner's `learn` phase for this claim.** That phase supplies hidden
scorer labels through `--outcomes` and historically defaults to `--demo`; it is outcome-assisted
learning. The earlier `harness/prepare_pilot.py` artifact used default thresholds but also supplied
those labels. Its results are preserved as outcome-assisted evidence, not relabelled as shipped
default evidence. The shipped bootstrap fixture preparation is tracked separately.

## Fixed task matrix

| Leg | Training | Held-out tasks | Repeats | Arms | Paid sessions |
| --- | --- | --- | --- | --- | --- |
| Generated clinic, seed 12 | Archived nine train sessions: CARE-971, CARE-297, CARE-672, three each | CARE-441, CARE-807, CARE-279 | 2 | control, adage | 12 |
| Real repository: Click | Ten chronological train tasks from click-slice, archived training traces only | click-0cd28d65, click-2468b709, click-19fd4d6e, click-c943271a, click-3bb230dc, click-c3535905 | 2 | control, adage | 24 |

Click training task IDs: click-b7cf0697, click-a1235aac, click-eaf33a95, click-0be2bd38,
click-4f936ac1, click-701b3131, click-27aaed3f, click-8533c966, click-8d4051e9, click-ebcd548d.
Click's raw training archive and repository mirror/environment are currently missing on this
host; that leg stays blocked until supplied and hashed. No replacement tasks selected after
observing outcomes. Do not pool clinic and Click into a generic developer-value headline.

Pinned input SHA-256:

- `evals/datasets/live-20260928-py-clinic-s12/manifest.json`:
  `5468814cd88ec296c0840d86e7e07c2b57bccc15d4d5935e2a28a35ede5d7d00`
- `evals/gen/specs/smoke.json`:
  `2febfe03446b4160171f79ae06c636e89573f10e4335034bb42c9198bd8fc7b4`
- `evals/repobench/configs/v2/click-slice.json`:
  `38198605768399d183cf0f7f630ba3be3a9794d87540abcc99060ef2e4e153e7`

Use `claude-haiku-4-5-20251001`, pinned installed Claude version recorded before dispatch, seed
20260929, concurrency 1, 40-turn cap, existing fresh sandbox/scorer framework. Arm order is seeded
per task/repeat. Every run gets fresh session history and a fresh task checkout. The model must
not read the scorer, solution, training archive, another run, or private index/ledger.

## Budget and stop rules

Remaining shared authorization at registration: $59.9847784 (earlier substrate call $0.0152216).
Proposed task reservations: $0.75/session × 36 = $27. At most two readiness screens at $0.50 each
add $1, so the prospective reservation is **$28**, leaving $31.9847784 headroom. This is a plan,
not a new spend authorization or actual expenditure. No model call has been made for this screen.

Allocate per-root totals explicitly: clinic $9, Click $18; readiness root $1. The runner passes
each cap to Claude and persists a full reservation before dispatch. Unknown/invalid cost,
interruption, persistence failure, or reported cap overrun blocks further dispatch. Never charge
an unknown session as zero. Already-running calls can complete under their reservations. CLI
budget limits may overshoot during a request; the large unallocated remainder is intentional.

Do not increase turns, change prompts, rebuild routes, change thresholds, add training data or
rerun a failing task opportunistically. Count cap-limited runs as censored and report them. An
infra failure is reported separately; reconcile its cost before any explicitly documented retry.
Stop the readiness stage on any failure of authentication, confinement, scorer, or hook delivery.

## Measurements and permitted conclusions

Record task-level hidden-test pass, cost per session, total cost per resolved task, elapsed wall
time, tool calls, turns, directive exposure/following, and errors/censoring/missing values by arm.
Report all scheduled tasks, including unfinished sessions. Preserve terminal `total_cost_usd`,
usage and result status; message IDs or `num_turns` are not API request counts. CPU execution
claims require actual production receipts plus removed redundant agent work in a separate pair.

Report per-task paired deltas and task-clustered 95% intervals with the existing seeded bootstrap;
repeat runs are not independent tasks. Three or six task clusters are a **mechanism/regression
screen**, not a powered quality or savings study. A zero learned route, zero exposure, or no
benefit is a valid result. Report generated-fixture findings as generated-fixture findings.
Do not claim general cost savings, improved capability, or reduced GPU energy from this screen.

The existing runner comparison table excludes infrastructure/scorer failures from its task-metric
denominators. Label it **completed valid runs**, not intention-to-treat. Beside it, report a full
assignment ledger: scheduled, dispatched, completed, infrastructure/scorer errors, censored,
budget-stopped, and unknown-cost counts for each arm and task. Provide assigned-task completion
and success counts separately; missing/unfinished outcomes remain explicitly unknown rather than
silently becoming successes or ordinary failures. Compare cost only where accounting is known;
if any assigned cost is unknown, total-cost and cost-per-resolution claims are incomplete. The
protocol alone is not publication-ready benchmark evidence.

## Execution readiness on this host

The runner's full-process confinement requires Linux bubblewrap. Docker Desktop is installed,
but an approved unsandboxed `docker info` found its engine stopped. No Linux execution resource
was created or started. Existing GitHub evals workflow is offline and contains no paid runner.

A read-only `claude auth status` probe reported authenticated `claude.ai` with normal config,
and unauthenticated with a fresh CLAUDE_CONFIG_DIR while retaining the same HOME. No credential
values were retrieved. Thus the current isolated runner needs a supported provider token supplied
securely to its environment or an authenticated Linux evaluation host; `host_auth` is not a
Keychain bridge. Do not put credentials in a config, artifact or chat.

Claude's documented macOS native sandbox is session-configurable, but confines Bash subprocesses;
Read/Edit/Write use separate permission checks. It has not been validated here as an equivalent
replacement for this benchmark's full-process/read-isolation boundary. No confinement was disabled
to obtain results. Official contract: https://code.claude.com/docs/en/sandboxing.

After readiness and a verified frozen shipped-bootstrap index, invoke the existing runner's
**`eval report` only**, with `confine=true`, `learn_args=[]`, the matrix above, and explicit session
and root budgets. Do not execute the outcome-assisted learn phase or request execution grants
just to make the advisory benchmark run.
