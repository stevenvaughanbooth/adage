# Learned action elimination: launch mechanism gate

This is the go/no-go experiment for a complete learned lookup. A successful sandboxed subprocess
is insufficient: the agent must receive the complete answer, omit its own lookup action, and
produce the same correct result. The broader launch gates still include real coding outcomes,
cross-platform execution, onboarding usability and transfer; this small trial cannot prove those.

## Frozen protocol

`action_gate.py prepare` generates the existing seed-12 clinic repository and learns from nine
archived real training sessions through production `adage condition`, with default thresholds.
It supplies no scorer outcomes, no held-out transcripts and no hand-authored route. It rebases
recorded working directories for isolated repository identity. The resulting lookup invocation
is `python3 tools/case {item}`; its command and learned typed binding stay unchanged.

The exact same learned index serves both arms:

- **Suggest:** `adage hook` provides the learned guidance; the agent can call the lookup.
- **Automatic:** `adage-exec hook` matches, binds, checks the human grant, and runs the lookup
  in its read-only, network-denied OS sandbox before delivering complete result data.

The developer task is to retrieve and transcribe the work item's title, description, acceptance
criteria and file list into JSON. This is a narrow triage task, not a software implementation
benchmark. CARE-279, CARE-441 and CARE-807 are held out from the training tasks. Both arms receive
the same task prompt, available tools and learned knowledge. Neither task prompt names the command.
The scorer requires all five fields to match exactly, ignoring whitespace only; it is not a model
judge. Each task gets both arms, with arm order randomized using frozen seed 20260930.

Claude Code 2.1.282 runs Haiku 4.5 in restricted mode with explicit session-only settings,
auto-memory disabled, no persistence, no MCP servers, no Chrome, no skills and Bash as its only
tool. The allow rule and a deterministic PreToolUse guard permit only the exact read-only lookup
for that task, rejecting appended commands, other IDs, shell interpolation and background or
sandbox-escape flags. The wrapper records production hook output; it does not implement matching
or execution. Host login is retained. This is a restricted tool surface around a trusted fixture,
not an OS sandbox for the entire agent; it must not be reused for edited executable code without
additional confinement. The actual Adage subprocess always uses its production sandbox.

## Pass criteria

All six assigned sessions must complete, with no exclusions or automatic retries:

1. Both arms return all fields correctly and terminate successfully.
2. Suggest performs exactly one successful invocation of the learned lookup.
3. Automatic performs zero agent tool calls, delivers the complete expected stdout, and records
   exactly one successful E1 production execution for the learned route.
4. Version-specific diagnostics show UserPromptSubmit result context accepted before the first
   observed model API dispatch. Assistant message IDs are not treated as API request counts.
5. Every paid attempt has a settled, valid cost within its reservation and the total cap.

Save the raw stream, hook output, execution receipts, dispatch milestones, costs, token usage,
process result and source/input hashes. Preserve failed preparations and mechanism failures too.
Report any eliminated model request separately from monetary savings or GPU energy: the latter
is not measured. Six sessions cannot establish general efficiency or quality noninferiority.

## Authority and spending

Preparation writes no execution grant. A person runs the generated `approve.sh` in their terminal;
Adage's agent-session refusal remains intact. That approval is confined to the generated
repository, exact script bytes, E1 profile and explicit complete-stdout output contract. The
private experiment policy allows unstructured output, but has no effect without that grant.
This does not turn on execution in any of the developer's installed harness configurations.

The experiment reserves at most **$3**, six sessions at **$0.50** each, against this task's
user-authorized **$60** budget. Its earlier recorded spend is **$0.0152216**. This is a task ledger,
not a statement of remaining provider credit or other experiments' spend. Shared `evals/live/budget.py`
reserves and settles each attempt durably; missing accounting blocks further dispatch.

```sh
cargo build --locked -p adage-cli -p adage-exec
python3 -B evals/reflex-proof/harness/action_gate.py prepare --root /tmp/adage-action-gate
# The human runs the printed approve.sh command in their terminal.
python3 -B evals/reflex-proof/harness/action_gate.py check /tmp/adage-action-gate
python3 -B evals/reflex-proof/harness/action_gate.py live /tmp/adage-action-gate --limit 2
# Inspect the first pair before dispatching the remaining four.
python3 -B evals/reflex-proof/harness/action_gate.py report /tmp/adage-action-gate
```

The preparation freezes binary copies, learned index/tracks, fixture files, expected answers,
approval recipe and evaluation code. A changed input invalidates the prepared comparison.
The [completed pilot](results/20260929-learned-action-gate/FINDINGS.md) passed all six live
sessions: three agent actions and three model requests eliminated, with every answer correct.
The initial preflight timeout was fixed by resolving Apple's system Python launcher directly;
the old preparation and failures remain recorded. No timeout or authority limit was increased.
One hundred subsequent local hook invocations also passed. These repeats do not expand the
live task sample beyond three held-out instances.

## Shipped instincts

The catalogue already enables eight detectors. Fresh-init integration tests exercise six useful
advisory defaults (wrapper recovery, test runner, generation, lockfiles, package manager) and an
actual Python-environment command rewrite followed by execution of the rewritten command. These
are separate proof obligations: suggestions do not count as eliminated agent actions. See the
[instinct catalogue](../../../docs/instincts.md) for trigger and authority boundaries.
