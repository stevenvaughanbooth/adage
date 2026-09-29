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

## Threat model and scope

Adage runs inside coding agents' hooks and turns learned or shared **tracks** into text the agent
reads. It never executes the commands it suggests; the agent runs everything through its own tools,
under its harness's permissions. With that in mind, these are in scope:

- **Tracks are untrusted input.** A track from `adage add`, a teammate, or a repository must not be
  able to: make Adage execute anything; read or write outside its data directory and the repository;
  escape its directory through symlinks or `..`; get a directive rendered with text or slot values
  that its contracts do not allow; exceed the effect class Adage re-derives for a command (for
  example, turn a destructive command fire); or get past signature verification and the
  forming rules of `adage add`.
- **Hook payloads and transcripts are untrusted input.** Prompts, tool outputs and file contents must
  not reach a directive except through validated slots (typed contracts, safe repository-relative
  paths that exist).
- **Hooks fail open.** A hook must never block, crash or hang the agent's session: every error, panic
  or timeout ends in exit status 0 with no output. A way to make a hook block the agent, or print
  something other than Adage's own directive JSON, is a vulnerability.
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

Adage keeps everything local: it uploads nothing and has no telemetry. The only network access is
`adage models fetch`, on request, from a pinned URL with SHA-256 verification.
