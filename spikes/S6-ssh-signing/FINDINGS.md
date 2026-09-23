# S6 · SSH signing of tracks — Findings

**Verdict: Native.** Rust can sign a track digest through both a key file and a live
`ssh-agent` — no `ssh-keygen` subprocess anywhere — and verify interoperably against
`ssh-keygen -Y verify`, in both directions, with correct namespace and validity-window
enforcement. This overturns the premise in the spike brief and Design Spec §15.2/A7's
worry: the *`ssh-key`* crate alone cannot talk to an agent, but pairing it with a second,
independent pure-Rust crate that speaks the agent wire protocol (`ssh-agent-client-rs`)
closes the gap completely. A7 can be implemented with zero subprocess spawns for both
signing and verification.

Reproduce everything below with `./run_spike.sh` (needs `source "$HOME/.cargo/env"` on
`PATH` already, which the script does itself). It builds the release binary, starts a
private `ssh-agent` on its own socket, runs every test, and kills the agent on exit. Raw
output for each step is in `results/`.

## Interoperability matrix (signer × verifier)

Four signers, over the identical 32-byte canonical digest, namespace `adage-track`,
Ed25519, `hash_alg=sha512`:

| Signer | Mechanism | ssh-keygen -Y verify | Rust (`ssh-key` + our allowed_signers) |
| --- | --- | --- | --- |
| S1 | `ssh-keygen -Y sign -f <private key file>` | PASS | PASS |
| S2 | `ssh-keygen -Y sign -f <public key>` (agent) | PASS | PASS |
| S3 | Rust, `ssh_key::SshSig::sign` with a file key | PASS | PASS |
| S4 | Rust, `ssh_key::SshSig::signed_data` + `ssh-agent-client-rs` `Client::sign` + `SshSig::new` (agent) | PASS | PASS |

All 8 cells pass (`results/matrix.csv`). Because Ed25519 signatures are deterministic
(RFC 8032), S1, S2, S3 and S4 are not just cross-verifiable, they are **byte-identical**
PEM blobs (`diff results/S1.sig results/S2.sig results/S3.sig results/S4.sig` — all
empty). File-key vs. agent-key and ssh-keygen vs. Rust are indistinguishable outputs for
the same input.

Key finding on S4: `ssh-agent-client-rs` depends on `ssh-key = "0.6"`; the newest
`ssh-key` is `0.7.0-rc.11`. Pin the whole project to `ssh-key = "=0.6.7"` (which already
has full `sshsig` support) rather than mixing major versions — the two crates' `PublicKey`
/ `Signature` types are not interchangeable across that boundary.

## Canonical-digest spec

Implemented in `src/digest.rs::compute`. In prose:

1. Walk the track directory recursively, no symlink following.
2. Exclude `track.sig` (it signs everything else) and any path component starting with
   `.` (`.git`, `.DS_Store`, editor swap files).
3. For every remaining file, read raw bytes. Files under `scripts/` are hashed
   **verbatim**; everything else has line endings normalised first (`\r\n` and lone `\r`
   → `\n`). Tracks are declared plain text (Design Spec §10.1), so normalising the
   non-script majority is safe and makes the digest immune to a `core.autocrlf`
   difference between machines; `scripts/` is excluded from normalisation because a
   script's bytes may be meaningful as-is (and could be a binary, in principle).
4. SHA-256 each (normalised) file → lowercase hex.
5. Manifest = one line per file, `"<sha256_hex>  <posix/relative/path>\n"`, **sorted by
   path in byte order** — this is what makes the digest independent of directory
   enumeration order or insertion order (§6b below).
6. Canonical digest = SHA-256 of the manifest bytes (32 raw bytes). This is the "message"
   handed to `SshSig::sign` / `SshSig::signed_data`, which apply their own `hash_alg`
   digest on top per the sshsig wire format — that's a second, independent hash, not a
   substitute for the manifest hash.

This matches Architecture §4.9 ("every file in the track, sorted by path, normalised line
endings, hashed... Signatures... refer to this digest, never to archive bytes") exactly,
with the `scripts/`-is-verbatim rule as the one addition the design spec left implicit.

## Namespace enforcement

Tested two ways (`results/namespace_test.txt`):

1. **Synthetic:** the same digest signed once with `-n git`, once with `-n adage-track`.
   Both engines correctly refuse the `git`-namespace signature when asked to verify it as
   `adage-track` (`ssh-keygen`: *"Couldn't verify signature: namespace does not match"*;
   Rust: `namespace invalid`).
2. **Real:** a throwaway git repo (`sandbox/gitrepo`, repo-local config only,
   `gpg.format=ssh`, `user.signingkey=<pubkey>`, signing done through the same private
   agent) produces an actual commit signed by `git commit`, namespace `git` by
   construction. `git log --show-signature` confirms `Good "git" signature for steven`.
   The signature is extracted verbatim from `git cat-file commit HEAD`'s `gpgsig` header
   and handed to both verifiers asking for `adage-track` — both refuse it
   (`results/git_commit.sig`, `results/git_show_signature.txt`). A real git-commit
   signature cannot be replayed as a track signature.

`PublicKey::verify` in `ssh-key` checks the embedded public key, then the namespace,
before touching cryptography at all (`public.rs:213`) — namespace confusion is refused
even before the crypto check runs.

## Tamper tests (`results/tamper_test.txt`)

- **Content tamper:** one byte changed in `routes/work-item-context.yaml` → digest
  changes (`7f3b32f5…` → `b0d51f31…`); the original signature, checked against the
  recomputed (tampered) digest, fails cryptographic verification. Confirms the intended
  workflow: a verifier always recomputes the digest from disk and never trusts a stored
  value.
- **Reorder:** the same six files rebuilt on disk in the reverse creation order produce
  the **identical** digest — the sort step in the manifest neutralises filesystem
  enumeration order, exactly as designed.
- **Line endings:** converting `routes/work-item-context.yaml` to CRLF leaves the digest
  **unchanged** (normalisation working as intended); converting `scripts/wi-lookup` to
  CRLF **changes** the digest (verbatim hashing catches it) — this is the
  `scripts/`-is-verbatim boundary working as designed, not a bug.

## valid-after / valid-before (`results/valid_time_test.txt`)

An expired window (`valid-before=20200101`) and a not-yet-valid window
(`valid-after=20990101`) are both rejected identically by `ssh-keygen -Y verify` and the
Rust verifier at the real current time; an open window (`valid-after` in the past, no
`valid-before`) succeeds on both. Pinning the clock inside the expired window
(`ssh-keygen -Overify-time=20200101120000Z` / our `--verify-time` flag, same format)
flips both to success — confirming the two engines interpret the timestamp format
identically for deterministic testing.

**The `ssh-key` crate has no `allowed_signers` parsing, no namespace-restriction
option, and no valid-after/valid-before support at all** — this is entirely Adage-side
code (`src/allowed_signers.rs`, ~230 lines): a line parser, a quoted-comma-aware options
splitter, and a from-scratch OpenSSH timestamp parser (`YYYYMMDD[Z]` /
`YYYYMMDDHHMM[SS][Z]`, Howard Hinnant's days-from-civil algorithm, no external date
crate needed).

## git config plumbing (`results/git_config_plumbing.txt`)

`git config --get` on a repo-local config cleanly returns `gpg.format`,
`user.signingkey`, `commit.gpgsign`, and `gpg.ssh.allowedSignersFile` — all of which
`libgit2`/shelling out to `git config` (or a config-file parser, to stay subprocess-free)
can read the same way. `user.signingkey` here pointed at a *public* key file, which is
what makes git use the agent transparently; Adage should accept both a private-key path
and a public-key-plus-agent path the same way git does.

## Recommended A7 implementation

- Pin `ssh-key = "=0.6.7"` (matches `ssh-agent-client-rs`'s dependency; has full
  `sshsig`). Re-evaluate the pin when `ssh-agent-client-rs` or `ssh-key` 0.7 stabilise
  together.
- Sign natively: try the developer's key via `ssh-agent-client-rs` first (reading
  `SSH_AUTH_SOCK`, `user.signingkey` from git config to pick the identity); fall back to
  a file-based key (machine keys for auto-built personal tracks, Architecture §4.9) when
  no agent is running. No `ssh-keygen` subprocess needed in either case — Architecture
  §8's "no code path that spawns a process on the hot path" holds for signing too, not
  just verification.
- Verify natively with `ssh_key::PublicKey::verify(namespace, msg, &sig)` after a
  hand-rolled `allowed_signers` parser (code above, ready to lift into the real
  implementation) that also enforces `namespaces=`, `valid-after=`, `valid-before=`.
- Canonical digest exactly as specified above; treat `scripts/` as verbatim.

## What I could not do

- **FIDO / hardware security keys and password-manager agents** (1Password SSH agent,
  etc.) — the spike brief says "ideally"; there was no hardware key or password-manager
  agent available in this sandboxed environment to test against. `SshSig::sign`
  explicitly rejects `sk-ed25519`/`sk-ecdsa` (hardware-backed) key types
  (`sshsig.rs:111-118`) when signing through a raw `SigningKey`, but signing through the
  **agent** path (S4) does not go through that check — the agent does the crypto, so a
  resident/hardware key should work through `ssh-agent-client-rs` unmodified. Unverified
  without real hardware.
- **Principal/namespace glob patterns.** `ssh-keygen`'s allowed_signers principals and
  `namespaces=` fields are pattern-lists (globs, negation) per `ssh_config(5)` PATTERNS;
  our parser does exact string matching only. Fine for the spike; a real implementation
  needs a small glob matcher.
- **RSA/ECDSA/certificate signers.** Out of scope per the brief (Ed25519 throwaway keys
  only); `cert-authority` is parsed in `allowed_signers.rs` but certificate chains are not
  validated.
- Timestamp parsing always treats unsuffixed values as UTC; `ssh-keygen` uses the local
  timezone for unsuffixed values. A documented simplification, called out in
  `allowed_signers.rs`.
