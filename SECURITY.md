# Security policy

## Reporting a vulnerability

Please report vulnerabilities privately through GitHub's private vulnerability reporting:
**Security → Report a vulnerability** on
[stevenvaughanbooth/adage](https://github.com/stevenvaughanbooth/adage/security/advisories/new).
Do not open a public issue, pull request or discussion for a suspected vulnerability.

Include what you found, how to reproduce it (a track, transcript or hook payload that triggers it is
ideal), the version (`adage version`) and platform, and the impact you expect. Fixes are released
as a patch version with a GitHub security advisory, and reporters are credited unless they prefer
not to be.

Adage is pre-1.0. Only the latest release receives security fixes.

## Signing dependencies

The workspace pins `ssh-key =0.7.0-rc.11` and locks `crypto-bigint 0.7.10`, replacing the old
SSH dependency chain affected by
[RUSTSEC-2026-0387](https://rustsec.org/advisories/RUSTSEC-2026-0387.html). SSH-agent IPC is a
small internal adapter; key/signature decoding and all cryptography remain in RustCrypto.
Requests use bounded frames and one 30-second deadline per exchange. RSA agent requests require
SHA-512, and every returned signature is verified before it is accepted.

The SSH library and its RSA backend are currently prerelease dependencies. Their resolved
versions are committed in `Cargo.lock`; upgrades require the signing, encrypted-key and OpenSSH
interoperability suites, the platform transport tests, `cargo deny check`, and refreshed
third-party notices. These checks cover Ed25519, RSA and P-256/P-384/P-521 with temporary keys
and isolated agents. Native macOS/Windows signing jobs run on every pull request in addition to
the Linux product suite.

The pre-existing exception for
[RUSTSEC-2023-0071](https://rustsec.org/advisories/RUSTSEC-2023-0071.html) remains: RSA private-key
operations are local and are not exposed through a network signing service. The bigint upgrade
does not repair this separate RSA timing advisory. Any change that makes private-key operations
remotely observable must revisit that exception and the threat model. No new advisory suppression
was added for this migration.

## Threat model and scope

Adage runs inside coding agents' hooks. Advisory routes deliver text for the agent to act on under
its harness permissions. The optional `adage-exec` runtime can also execute a locally approved
reflex directly on the CPU: E1 provides bounded read-only results; E2 verifies or regenerates
declared outputs through private staging. Learning, installing a pack, and approving its evidence
do not grant execution permission. See [execution and approval behavior](docs/execution.md).
These boundaries are in scope:

- **Tracks are untrusted input.** A track from `adage add`, a teammate, or a repository must not be
  able to: make Adage execute anything without a matching local execution grant; access files
  outside the approved sandbox profile;
  escape its directory through symlinks or `..`; get a directive rendered with text or slot values
  that its contracts do not allow; exceed the effect class Adage re-derives for a command (for
  example, turn a destructive command fire); or get past signature verification and the
  forming rules of `adage add`.
- **Hook payloads and transcripts are untrusted input.** Prompts, tool outputs and file contents must
  not reach a directive except through validated slots (typed contracts, safe repository-relative
  paths that exist).
- **Execution grants are local authority.** Commands, bound inputs, script/dependency identities,
  output contracts, repository scope and sandbox profile must match the reviewed grant. Agent
  sessions cannot mint grants. Changed identities must lapse approval; pack signatures and learning
  evidence must never substitute for it. Bypasses of these checks are in scope.
- **Sandboxing and staged application.** E1/E2 fail closed when confinement is unavailable.
  Network access is denied. Undeclared writes, access beyond the enforced profile, unsafe staging
  paths, and applying E2 changes without validation are in scope. Staged execution must not mutate
  the original source directly. Journal corruption or an interrupted application must not silently
  become a successful receipt.
- **Hook failures preserve the agent session.** Execution failures must use the documented fallback
  or refuse the action, rather than report success. Hooks must respect their time/output bounds and
  emit only the harness's expected structured response. Unbounded hangs, execution after refusal,
  and raw command output escaping validation are in scope.
- **Sharing must not leak.** `adage share` must refuse tracks containing absolute or home-directory
  paths and secret-looking strings, and must sign exactly the bytes it writes.
- **Supply chain.** Release artifacts that do not match their published checksums, attestations or
  SBOM; the Claude Code plugin running a bundled binary that fails its `SHA256SUMS` check. The plugin
  never downloads anything at hook time.

Out of scope: what an agent does with a directive (it runs under its harness's own permissions);
attacks that need control of the user's account, `PATH`, or Adage's data directory; denial of
service by a user against their own machine; and the `demo/` and `spikes/` directories, which are
research code.

## Privacy

Learning and hook execution run locally; Adage has no telemetry and does not upload session traces.
Requested model setup (`adage models fetch` or the corresponding init flow) can download model
artifacts with SHA-256 verification. CPU execution uses a network-denying sandbox. Exported packs
and evaluation reports are deliberate sharing operations: review their contents before publishing.
