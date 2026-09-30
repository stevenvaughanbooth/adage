# Production-path pilot: prepared, not dispatched

2026-09-29, source revision acdb0a4. Additional paid spend **$0**; release the unused $15
reservation. Earlier fixture hook-boundary spend remains $0.0152216.

The lowest-cost useful candidate is a two-session pair on the generated clinic fixture's held-out
CARE-279 task, using the existing live runner, Haiku 4.5, one repeat, sequential adage and e1 arms.
Both arms get the same learned knowledge; the treatment adds execution. A no-advice control is
an optional later third arm, because it would confound knowledge delivery with automatic execution.
The e1 arm executes through production adage-exec; its name also covers E2 actions. This is a
mechanism pilot, not a statistical savings result or evidence about ordinary repositories.

## Offline result

`prepare_pilot.py` generates the existing seed-12 fixture, reads only nine archived training
sessions (CARE-971, CARE-297, CARE-672), and invokes the built production `adage condition` at
default thresholds **with hidden scorer pass/fail labels supplied through `--outcomes`**. This
is outcome-assisted preparation, not evidence for the shipped default init/bootstrap learner.
No held-out session or solution is fed to the learner. It records source and
binary checksums, each input checksum and session ID, the generated script checksum, and exact
condition argv in `preparation.json`. The generated index is copied to the treatment home with
**no grants**. No global settings or hook installations change.

Observed learned routes:

| Route | Trigger | Result |
| --- | --- | --- |
| card-157e34741c | intent | context card |
| card-b62470ea5c | intent | context card |
| lookup-work-item | prompt | `python3 tools/case {item}`, firm learned observe step |

The learner's pattern contract accepts CARE followed by a hyphen and three digits. Current
production E1 planning supports this learned PromptValue slot; the launch report's old rejection
is fixed. The current strict training subset produces **no after-edit make-gen route**, including
in a separate diagnostic run with `--demo`. The previous launch E2 demonstration therefore cannot
be reproduced by granting an E2 route learned from this subset. We did not manufacture a route.

## Why no paid pair or grant yet

1. **Output sufficiency:** this learned command emits an unlabelled title, multiline description,
   and an `Acceptance:` heading whose value starts on the following line. The current typed
   projection at preparation time delivered file paths and same-line labelled text. The subsequent
   multiline fix now retains labelled Acceptance sections, but still omits the unlabelled title
   and description. It cannot preserve the whole
   work item from this output. JSON output exists behind `--json`, but the learned command does
   not include that flag. Hand-editing it would break this proof's learning requirement. A files-only
   grant repeats the launch failure: the agent must perform the lookup again to learn the task.
   Raw output would require a separate explicit policy/grant choice; none was activated or requested.
2. **Runner confinement:** `evals/live/runner.py` confinement requires Linux bubblewrap. It is
   absent on this macOS host. Setting `confine=false` does not supply a macOS agent sandbox;
   adage-exec's own sandbox confines only its subprocess, not the surrounding coding agent. We
   did not weaken confinement to force a paid run.
3. **Isolated authentication:** the runner explicitly supports auth environment variables or a
   credentials file. Its `host_auth` mode supports host-managed OAuth/token-file auth, not macOS
   Keychain. No supported credential environment variable is present here. The earlier substrate
   probe's successful host login does not establish isolated runner auth.
4. **Real human grant:** `crates/adage-cli/src/share.rs::require_person` refuses review delegation
   in an agent environment before checking terminal/CI mode. We preserved every protection marker.
   `ADAGE_ALLOW_NONINTERACTIVE=1` is not a way around that refusal.
5. **Production sandbox startup fixed:** root reproduced a replay subprocess SIGABRT in macOS
   dyld and fixed it with read access to the literal root directory node. Three replay tests now
   pass, including blocked external file reads/writes and blocked loopback networking with a
   successful outside-sandbox control. This does not resolve the separate coding-agent runner
   confinement/authentication limits above. The final full executor rerun also exposed an E2
   process-budget failure: legitimate regeneration exited 128 with `fork: Resource temporarily
   unavailable`. That separate fix is in progress; consult the current executor test results before
   claiming E2 functional here. The process limit was not removed to force this pilot through.
6. **Budget gate corrected:** the runner now passes `--max-budget-usd`, reserves full session caps
   atomically, and blocks further dispatch on unknown cost. Its persistent attempt ledger preserves
   rerun costs and a process lock prevents concurrent writers to the same root. This removes the
   earlier accounting blocker, not confinement or output-contract blockers. No paid pair occurred.

## Reproduction and proposed next matrix

From the worktree (build only if the binaries are missing or stale):

```sh
cargo build --offline -p adage-cli -p adage-exec
python3 -B evals/reflex-proof/harness/prepare_pilot.py
```

The script prints a fresh private ROOT. It contains `pilot.json`, `preparation.json`, the generated
fixture under `private/bundle`, the condition report, and the two identical learned homes. Preview:

```sh
python3 -B evals/live/runner.py ROOT/pilot.json eval report --dry-run --root ROOT --adage target/debug/adage
```

Once output sufficiency, confinement, authentication, and the per-call spending cap are established,
the existing runner's live form is the same command with `--execute` in place of `--dry-run`.
Do not run it yet. No current grant command is recommended because the useful output contract is
not ready. The eventual approval scope is only `wardline-clinic/learned lookup-work-item`, E1,
the generated fixture's hash-bound `tools/case`, bounded typed task data, and the pilot's private
e1-home. It is not global permission for arbitrary scripts.

Start with one adage/e1 pair at a $1/session cap ($2 proposed reservation), then inspect: actual production
receipt, injected sufficient task data, absence/presence of redundant agent lookup, hidden-test
success, request IDs, tool calls, actual cost and latency. Only add two more pairs if both arms are
valid and the mechanism is working. A pair whose typed result omits task instructions is a failed
mechanism test, not grounds to count the hook invocation itself as saved work.

`target/debug/adage` must sit beside `target/debug/adage-exec`; building only the latter caused the
SessionEnd parity prerequisite failure. No production code was changed for this preparation.
