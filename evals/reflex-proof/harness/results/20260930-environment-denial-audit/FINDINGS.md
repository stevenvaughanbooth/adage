# Environment-denial audit (post-hoc, names only)

## The flaw

`real_repo_gate.native_settings` built the sandbox's `credentials.envVars` denial list from the
environment of whichever process **prepared** a session: every name except a keep list and agent
markers. Claude was then launched with the environment of whichever process **ran** it. A name
present only at launch reached Claude, and through it the tool children, without being denied.
The native-confinement canary checked file reads, writes and the network, but never the
environment. So no archived review shows that environment denial worked.

It surfaced when `launch-gate-v1`'s native session was frozen inside a Claude Code agent and run
from the user's terminal. Its review passed on files and network, but the later policy-equality
check failed in a third shell. That session ($0.0258378) is preserved and **excluded as
confinement evidence**. The ledger entry keeps its cost and marks it excluded. `canary-v2`
aborted before preparing anything (no grant, no spend).

## Archived sessions (`audit.py`, output `audit.json`)

Scanned `/private/tmp/adage-real-repo-gate-v1`, `/private/tmp/adage-learning-curve-v1` and `-v2`.
Of 140 native settings files, 101 sessions were launched:

| Denial set | Sessions | Frozen in |
| --- | ---: | --- |
| `fb0ad67f1d03` (32 names) | 95 | the same terminal-derived agent environment (includes `STARSHIP_SESSION_KEY`, `SSH_AUTH_SOCK`) |
| `4f380763d655` (34 names) | 5 | same family, two extra names |
| `a764c9520d89` (50 names) | 1 | Claude Code agent environment; launched from a terminal (the excluded `launch-gate-v1` session) |

- Launch environments were never recorded, so the audit cannot prove that any earlier session's
  launch environment matched its freezing environment. The 100 earlier sessions were prepared and
  dispatched by the same Codex executor, whose environment came from one terminal setup. That
  makes a match plausible but unverified. None of their denial lists names an Anthropic,
  OpenAI or GitHub credential, so such a variable, if present at launch, would not have been denied.
- No agent tool call in any of the 101 sessions dumped the environment: 1,389 Bash calls, no
  `env`/`printenv`/`set`/`export -p`, no Python `os.environ` or `getenv` access. The only variables
  referenced were `$TMPDIR` and `$PYTHONPATH`. The network was denied throughout. So no exposure is
  observed, but environment confinement was never demonstrated.
- Frozen roots, including transfer-v3, are unchanged. Their task, quality, cost and receipt results
  do not depend on environment denial. Their claim of native confinement is limited to filesystem
  and network, and that is how it is now described.

## Prospective fix

- `probe.execute` launches Claude with an allowlisted environment (`probe.launch_env`): plain
  names (`PATH`, `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `LC_CTYPE`, `TERM`, `TMPDIR`), the
  Claude login names, and a fresh random sentinel. Everything else in the launching shell is dropped.
- `native_settings` denies a fixed list (`probe.DENIED_ENV`): every passed credential name, the
  sentinel and common secret names. Its home read-denial comes from the account database, not `$HOME`.
  The policy no longer depends on the freezing shell.
- Before launch, `probe.check_launch` refuses if any passed name outside the plain allowlist is not
  denied by the session's settings. `run.json` records the launched names (never values).
- The native canary now also requires that the sentinel is absent in tool children and that no
  token, secret, password, credential, API-key or auth name is visible. It prints the visible names
  for the review. The review also requires that the sentinel was launched.
- Tests: identical settings from two different shells, still confining under a third launching
  shell; an extra secret in the launching shell never reaches a real child process; settings that
  fail to deny a passed credential make the launch refuse.
