# Your first useful reflex

Adage is a developer preview. Start with ordinary work in one repository: available instincts
help immediately, while repeated agent sessions supply evidence for learned reflexes. A learned
candidate is not automatically permission to run its command.

## Connect and learn

Until packages are published, build the CLI from this repository:

```sh
cargo install --locked --path crates/adage-cli
cd /path/to/your/repository
adage init
```

Run `init` in your own terminal. Its interactive wizard shows the hook configuration before
installing it and offers to learn this repository's existing sessions. A run inside an agent or
with `--print` is a preview, not completed setup. If you already installed the Claude Code plugin,
its session-start hook bootstraps the repository; avoid installing a second copy of the same hooks.

```sh
adage doctor
adage condition --repo-root .
adage review
```

`doctor` identifies connection or model problems and gives their fixes. `condition` imports
supported existing history and binds applicable instincts. `review` explains candidates and their
evidence. It is fine to have no candidates: a fresh repository can have instincts without learned
behavior, and inconsistent or insufficient history should not produce an automatic action.

Start a normal coding session. `adage watch` shows activity alongside it; `adage ui` brings review,
health and settings together. After additional sessions, let background learning run or condition
again. There is no universal number of sessions that guarantees a reflex. Our guided ten-session
example is an evaluation result, not an onboarding requirement.

## Understand what is active

| What you see | What happens | What to check |
|---|---|---|
| Instinct or learned guidance | The agent receives context and still chooses its action | Relevant guidance appears in a normal session |
| Learned execution candidate | A supported routine has evidence; it stays silent without a matching execution grant | Review its trigger, command and supporting histories |
| Approved CPU action | The execution companion can run it when the trigger and all checks pass | A successful execution result appears and the agent uses it |

`adage review approve` accepts evidence or a proposed reflex change. It does **not** grant script
execution. `adage delegate on` connects the execution companion. It does **not** approve every reflex.

## Enable one verification reflex

Automatic execution currently requires macOS or Linux, including WSL. Build the companion beside
the CLI if your installation does not supply it:

```sh
# Run from the Adage source checkout.
cargo install --locked --path crates/adage-exec
```

Use your own terminal for the following decisions. Review the candidate in its target repository.
Verification actions require an explicit E2 opt-in under `[exec]` in your personal
`$ADAGE_HOME/policy.toml` (or the default Adage data directory if no override is set):

```toml
[exec]
max_level = "E2"
auto_approve = "off"
require_sandbox = true
```

Merge these settings into an existing file; do not overwrite other policy. A managed policy can
still prohibit E2. This opt-in enables review of actions; each reflex still needs its own grant.
See [execution policy](execution.md#policy) for precedence and default locations.

For a learned pytest verification candidate, substitute the actual track and reflex shown by
`adage review` into this example. Declare only caches that command needs:

```sh
adage review delegate TRACK REFLEX --level e2 --effect verify \
  --cache '.pytest_cache/**' --cache '.pytest_cache/.gitignore' \
  --cache 'tests/__pycache__/**' \
  --cache 'src/**/__pycache__/**' --timeout-ms 30000 --repo .
adage delegate on --repo .
adage delegate status --repo .
```

The approval command stages the action and validates its behavior before saving a grant. A failed
stage or policy check saves no approval. Verify actions cannot apply source changes. Use explicit
repository directories for cache globs; `**/__pycache__/**` at the root is intentionally rejected.
Pytest's named cache is a narrow exception for caches, not permission for arbitrary hidden outputs.
Its hidden `.gitignore` needs the explicit declaration shown above because wildcard matching
does not implicitly include hidden files. The repository's own `.gitignore` remains protected.

An E1 lookup has a different output contract; do not use these E2 flags for it. The full
[execution guide](execution.md) covers bounded complete results, scripts, policy and revocation.

## Recognize success—and recover

Perform a normal task that fits the learned context. A successful CPU execution is one observation;
the useful outcome is that the agent consumes the result and avoids repeating the action. If it
runs the same check again, do not count that as saved work. Model-request counts and API costs
also do not measure physical GPU utilization.

If nothing executes, check `adage delegate status`, `adage review`, and `adage doctor`: a missing
grant, policy ceiling, unmatched context, changed command or dependency, stale grant, sandbox
failure or duplicate suppression can all correctly withhold an action. Inspect the reported reason
before changing thresholds. Continue ordinary work while evidence accumulates.

To stop automatic actions, run `adage delegate off --repo .`. To withdraw one permission, run
`adage review revoke TRACK REFLEX --repo .`. Importing or sharing a reflex never transfers your
machine's execution grants.

For measured results and remaining release gates, see [launch status](../evals/reflex-proof/LAUNCH-STATUS.md).
