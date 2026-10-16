# adage-sign

Native `sshsig` signing and verification of pack digests: through ssh-agent when one runs, else
from an OpenSSH private key file, and verified against an `allowed_signers` file. No `ssh-keygen`
process is ever spawned.

## Where it sits

- Layer 0 (kernel primitives) in `xtask/src/layers.rs`: no workspace dependencies.
- External: `ssh-key` (pinned `=0.6.7`), `ssh-agent-client-rs`, `zeroize`, `anyhow`.
- Linked by: `adage-pack` (`digest::sign_dir`, `verify_dir`, `NAMESPACE`), `adage-cli` (`add`
  verifies, `train` signs). `adage-train` lists it in its `Cargo.toml` as well.
- As a layer-0 crate it must link nothing in the workspace; a sign-related type that needs the
  pack model belongs in `adage-pack`, not here.

## What's inside

- `lib.rs`: `PACK_NAMESPACE` (`"adage-pack"`); `message(digest_hex)` (the 32 raw digest bytes
  that are signed); `sign`, `sign_with` (with a `Passphrase` callback), `sign_with_key_file`,
  `sign_with_agent`; `Signed { pem, key, method }`, `Method::{Agent, KeyFile}`;
  `parse_signature`, `verify_embedded` (integrity only), `verify` (against allowed signers,
  enforcing `namespaces=`, `valid-after=`, `valid-before=` at a given time), `lists_key`,
  `fingerprint`, `key_line`. Re-exports `ssh_key::PublicKey` and `zeroize::Zeroizing`.
- `allowed_signers.rs`: `AllowedSigner { principals, namespaces, valid_after, valid_before,
  key }` with `allows_namespace` and `allows_time`; `parse` (the OpenSSH format; `cert-authority`
  entries are skipped); `parse_timestamp`.

## How it connects

- `adage-pack` computes `pack_digest`, turns it into bytes with `message`, and calls `sign` or
  `verify` in `PACK_NAMESPACE`; the armoured `pem` is written as `pack.sig`.
- `adage-cli`'s `add` decides trust: `verify` against `$ADAGE_HOME/trusted-signers` (the
  person's file) and `<repo>/.adage/trusted-signers`, `lists_key` to tell a refused known key
  from an unknown one,
  `verify_embedded` to name the signer of an unverified pack. `train` signs a compiled pack with
  `sign_with`, asking for a passphrase at the terminal.
- The signed message and namespace match `ssh-keygen -Y verify -n adage-pack` over the same
  bytes, so signatures interoperate with OpenSSH tooling.

## Changing it

Invariants to keep:

- The signed message is exactly the digest's 32 raw bytes, hashed with SHA-512 inside sshsig,
  in `PACK_NAMESPACE`. A git commit signature (namespace `git`) must never verify as a pack
  signature, and an entry restricted to other namespaces must refuse one
  (`namespace_is_enforced`).
- `verify` proves trust (a listed key, within its window, in the namespace); `verify_embedded`
  proves integrity only. Callers decide trust; keep that split.
- `finish` verifies every signature this crate produces before returning it, so an agent that
  hands back an unusable signature fails here, not at a recipient.
- Passphrases are `Zeroizing` and asked for at most `PASSPHRASE_TRIES` times; a key file that is
  encrypted with no way to ask is refused, pointing at `ssh-add`.
- `cert-authority` entries never become plain signers.

Tests: `cargo test -p adage-sign`. `signs_through_a_private_agent` starts its own `ssh-agent`
on a socket in a temporary directory and never touches the user's agent; it is skipped when no
`ssh-agent` binary is installed. Then `cargo test -p adage-pack -p adage-cli`. The crate reads
nothing outside itself.

Pitfalls: the `ssh-key = "=0.6.7"` pin exists because `ssh-agent-client-rs` depends on
`ssh-key` 0.6 and the key and signature types are not interchangeable across the 0.6/0.7
boundary (comment in `Cargo.toml`); `adage-pack`'s dev-dependency carries the same pin, so a
version bump must change both. Ed25519 signatures are deterministic, which a test relies on
(agent and file signatures byte-identical).
