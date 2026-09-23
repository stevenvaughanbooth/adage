# S6 · SSH signing of tracks

**Informs:** A7 (signing), O5 · **Wave:** 1 · **Budget:** half a day, no tokens

## Question

Can tracks be signed with the keys developers already use for git — including keys held in
**ssh-agent**, and ideally FIDO or password-manager agents — and verified **natively in Rust**
against an `allowed_signers` file with a namespace, producing signatures interoperable with
`ssh-keygen -Y` and git?

## Why it matters

The review found the `ssh-key` crate cannot sign through ssh-agent. Most developers' signing keys
live in an agent. If native signing is impossible, Adage must shell out to `ssh-keygen -Y sign` for
signing (verification can still be native).

## Method

1. Create throwaway Ed25519 keys **inside this spike folder only** (never touch `~/.ssh`). Start a
   private `ssh-agent` for the test with its own socket, add a key to it.
2. Define the **canonical digest** of a track directory (sorted paths, normalised line endings,
   SHA-256 of a manifest of file digests) — implement in Rust.
3. Sign the digest with:
   - `ssh-keygen -Y sign -n adage-track` using a key file
   - `ssh-keygen -Y sign` using the key **via the agent** (`-f` pointing at the public key)
   - Rust `ssh-key` crate (`SshSig::sign`) with a key file
   - Rust signing through the agent protocol (evaluate the `ssh-agent-client-rs` crate or similar)
4. Verify every signature with both `ssh-keygen -Y verify` and Rust (`ssh-key` verification) against
   an `allowed_signers` file, including **namespace enforcement** (a git-commit-namespace signature
   must not verify as `adage-track`) and `valid-after`/`valid-before` options if supported.
5. Tamper tests: modify a file, reorder files, change line endings — confirm verification behaviour
   matches the canonical-digest design.
6. Check how git config (`gpg.format=ssh`, `user.signingkey`, `gpg.ssh.allowedSignersFile`) can be
   reused to find the developer's key and trusted signers.

## Pass / fail

- **Native path works** if Rust can sign via the agent and verify interoperably.
- **Hybrid** if Rust verifies natively but signing must shell out to `ssh-keygen`.
- Report which, with code.

## Deliverables

`FINDINGS.md` with the interoperability matrix (signer × verifier), the canonical-digest spec,
the recommended implementation for A7, and code in this folder.
