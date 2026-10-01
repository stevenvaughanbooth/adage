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

## Names that Claude Code itself gives tool children (revision 4)

The fixed runner's first native session (`launch-gate-v2`, $0.0278896, exit 0) passed every file,
network and sentinel check. Its review failed closed on credential-like names. The user's shell
was excluded as designed: the sentinel was absent and no launched credential name was visible.
The tool child saw 59 names. The two flagged names come from Claude Code 2.1.282 itself. This was
established by read-only inspection of the installed binary's bundled code; no Claude process was
launched to test it.

- **`CLOUDSDK_PROXY_PASSWORD`.** For network-restricted commands the sandbox runtime starts a
  localhost filtering proxy. It exports `HTTP(S)_PROXY`/`ALL_PROXY` as
  `http://<user>:<proxyAuthToken>@localhost:<port>`. A helper then copies the URL's user and
  password into `CLOUDSDK_PROXY_USERNAME`/`PASSWORD`, plus Docker, gRPC, Java and git proxy
  settings. The token authenticates a tool only to the session's own enforcing proxy, which
  applies the session's network policy (here deny all). It is not a host or user secret, and the
  same value is also inside the proxy URLs, whose names are not credential-like. Expected grant:
  nothing beyond the denied policy. Not yet demonstrated live; the next canary tests it.
- **`CLAUDE_CODE_MESSAGING_TOKEN`** (with `CLAUDE_CODE_MESSAGING_SOCKET`). Claude Code starts a
  local cross-session inbox (`uds-messaging`) in every session, at a default path or at
  `--messaging-socket-path`. It exports the socket and a `childToken` so its children can send
  messages into the session. A tool child that could connect would be able to inject messages into
  the agent's own session and contaminate the evaluation. The flag does not create the inbox; it
  only fixes its path. It was added after a startup stall that a default-inbox conflict plausibly
  caused (`NATIVE-PROBE-REVISION-3.md`). Dropping it would not remove the exposure, so it stays.
- The other visible runtime names (`CLAUDECODE`, `CLAUDE_CODE_SESSION_ID`, `CLAUDE_PID`, `AI_AGENT`,
  `SANDBOX_RUNTIME`, `GIT_CONFIG_*`, `GIT_SSH_COMMAND`, `JAVA_TOOL_OPTIONS`, the `*_PROXY` family,
  temporary directories) are markers, the proxy plumbing above, or git/ssh settings that route
  through the proxy. None is credential-like.

Changes:

1. Both names, and `CLAUDE_CODE_MESSAGING_SOCKET`, are added to the fixed denial list. Whether
   `deny` removes names the runtime injects (rather than inherits) is unknown. The canary now
   reports exactly which names survive.
2. New live proofs in the same canary. `inbox`: a tool's `AF_UNIX` connect to the session's inbox
   socket path (fixed at preparation, embedded in the canary) must be denied with EPERM/EACCES.
   This check is required, whatever the environment shows. `proxy_denies_external`: with the
   proxy variables present, an HTTP request to `example.com` through them must fail.
3. A survivor is tolerated only by name, and only when its proof passed in the same run:
   `CLOUDSDK_PROXY_PASSWORD` if `proxy_denies_external`, `CLAUDE_CODE_MESSAGING_TOKEN` if `inbox`
   (`probe.RUNTIME_INJECTED`, `probe.unexplained_credentials`). Every other credential-like name
   still fails the review. The sentinel must be absent and every name on the denial list must be
   invisible (`env_denied_names_absent`). Neither check has any exemption.
4. Tests: exemptions require the matching proof and never cover host credentials, the sentinel or
   look-alike names. The canary program, run outside a sandbox, fails every confinement check
   (including a reachable inbox socket and a visible sentinel) and reports the unexplained name.

`launch-gate-v2` is preserved and excluded (its failed review is immutable); `canary-v3` was never
prepared. The successor is `launch-gate-v3/canary-v4`.
