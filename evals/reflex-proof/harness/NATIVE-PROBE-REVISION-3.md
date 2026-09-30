# Native startup probe: revision 3, pending accounting clearance

This is a diagnostic proposal, not evidence that the startup failure is fixed. Preparation is
offline and invokes no Claude process. The original censored attempt, runner and manifests remain
unchanged; its missing terminal cost remains unknown and blocks the shared budget ledger.

## Observed evidence

The installed native CLI is version 2.1.282. The failed attempt produced no stream records, no
stderr, and no native session directory before the 120-second watchdog cancelled it. Its debug
log ends during startup, after settings rules parsed and built-in plugin discovery began. There
is no recorded sandbox initialization, model request dispatch, tool call or canary result.
The earlier successful triage run passed the corresponding startup region within milliseconds.
None of this establishes an exact bill of zero.

Read-only inspection of the locally installed binary provides a more precise diagnostic boundary:
skill discovery runs concurrently with setup. Therefore the final skill-directory log does not
prove that skill discovery caused the stall. The setup routine can await startup of the local
cross-session inbox before logging that it is listening. Startup also has roster/policy waits
around that routine. The successful run logged an inbox; the failed run did not. This makes an
inbox/storage initialization wait plausible, but no blocked stack or resolved promise was captured.
There is no evidence that Bash sandbox restrictions caused this pre-tool stall.

## Bounded proposed changes

`prepare_native_probe_v3.py ORIGINAL_MANIFEST NEW_SIBLING_DIRECTORY` prepares a new assignment:

1. Reuse the untouched disposable canary checkout, after checking its script hash and absence of
   its inside-write marker. This keeps test inputs constant; it does not reuse any model output.
2. Use a fresh, short, mode-0700 local inbox directory through `--messaging-socket-path`, which is
   present in the installed CLI parser. This avoids default-inbox path conflicts without disabling
   messaging authorization, changing host credentials, deleting locks, or altering existing sockets.
3. Add the locally implemented diagnostic flag `CLAUDE_CODE_PROFILE_STARTUP=1`, preserving all other
   inherited environment values and execution-boundary markers. Profiling may only flush once
   startup or shutdown progresses; it is not guaranteed to diagnose another hang.
4. Keep model, five-turn limit, $0.50 session cap, no hooks, tool permissions, native sandbox,
   original canary script and network-denial checks identical. Allocate new logs/settings/manifest,
   and record the original manifest hash plus the new preparer hash.

This proposal deliberately does not switch authentication, disable plugins or managed policy,
unset agent markers, use `--bare`, disable confinement, or reset user configuration. A longer timeout
alone is not treated as a fix. No paid dispatch may occur while the prior ledger entry is unresolved.
The original `real_repo_gate.py run-session NEW_MANIFEST --wall-seconds 120` remains the bounded
execution entry point after the user resolves the accounting question and the new recipe is reviewed.

## Evidence required on the next authorized attempt

Preserve process PID and timing, raw stdout/stderr, debug logs, any startup profile, and native
transcript identity. Inspect startup progress separately from model dispatch. If it stalls again,
capture a short stack sample of this exact process while it is alive, without reading its environment
or credentials; do not launch another Claude process to test hypotheses. Preserve the full reserved
attempt and stop again on missing terminal cost. No outcome retry is implicit in this proposal.

A passing native canary still requires denied outside Read, all exact Bash read/write/dependency/Git/
network checks, a successful inside-write control, actual sandbox activation evidence, and known
terminal accounting. It is a platform preflight result, not a learned-reflex efficacy result.

## Prepared recipe and offline admission check

Prepared manifest: `sessions/native-confinement-v3/manifest.json` in the existing private experiment.
Its SHA-256 is `abfefebaf837e848bab4adf48a278c2bceabeb437b9d15ee890838640ca1fc8b`.
An offline call to the original dispatcher passed source/settings/canary hash checks and stopped at
the existing unresolved-accounting guard. Model execution was replaced with a fail-if-called test
sentinel, which was never reached; the paid ledger bytes were unchanged. The environment wrapper
retains the original Claude argv and budget argument, and native transcript provenance continues
to use the unchanged canary checkout cwd. `offline-admission-check.json` records these assertions.
Two preparer regression tests pass. This validates preparation and admission only, not CLI startup.
