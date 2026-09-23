//! Track signatures (Architecture §4.9, spike S6): `sshsig` over a track's canonical digest, in
//! namespace `adage-track`, created natively — through ssh-agent when one is running, else from a
//! key file — and verified natively against an `allowed_signers` file. No `ssh-keygen` process.
//!
//! The signed message is the 32 raw bytes of the canonical digest (`adage_tracks::io::digest`),
//! exactly as in S6, so `ssh-keygen -Y verify -n adage-track` over those bytes agrees.

pub mod allowed_signers;

pub use allowed_signers::AllowedSigner;
use anyhow::{bail, Context, Result};
pub use ssh_key::PublicKey;
use ssh_key::{HashAlg, LineEnding, PrivateKey, SshSig};
use std::path::Path;
use std::time::SystemTime;

/// The sshsig namespace for track signatures. A git commit signature (namespace `git`) can never
/// be replayed as a track signature.
pub const NAMESPACE: &str = "adage-track";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Agent,
    KeyFile,
}

#[derive(Debug, Clone)]
pub struct Signed {
    /// The `-----BEGIN SSH SIGNATURE-----` armoured signature, as written to `track.sig`.
    pub pem: String,
    pub key: PublicKey,
    pub method: Method,
}

/// The message a track signature covers: the digest's raw bytes.
pub fn message(digest_hex: &str) -> Result<Vec<u8>> {
    if digest_hex.len() != 64 || !digest_hex.bytes().all(|c| c.is_ascii_hexdigit()) {
        bail!("not a sha256 digest: {digest_hex:?}");
    }
    Ok((0..32).map(|i| u8::from_str_radix(&digest_hex[2 * i..2 * i + 2], 16).unwrap()).collect())
}

pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// The public key as `keytype base64` (no comment).
pub fn key_line(key: &PublicKey) -> String {
    let mut k = key.clone();
    k.set_comment("");
    k.to_openssh().unwrap_or_default().trim().to_string()
}

/// Sign `msg`: ssh-agent first (the identity matching `key` if one is given, else the agent's
/// first key, Ed25519 preferred), falling back to `key` as an unencrypted private key file.
pub fn sign(msg: &[u8], namespace: &str, agent_sock: Option<&Path>, key: Option<&Path>) -> Result<Signed> {
    let wanted = key.map(public_key_for).transpose()?;
    let agent_err = match agent_sock {
        Some(sock) => match sign_with_agent(msg, namespace, sock, wanted.as_ref()) {
            Ok(s) => return Ok(s),
            Err(e) => format!("ssh-agent: {e:#}"),
        },
        None => "no ssh-agent (SSH_AUTH_SOCK is not set)".to_string(),
    };
    match key {
        Some(k) if !is_public(k) => sign_with_key_file(msg, namespace, k),
        Some(k) => bail!("{agent_err}; {} is a public key, so its private half must be in ssh-agent", k.display()),
        None => bail!("{agent_err}; pass --key <private key file>"),
    }
}

pub fn sign_with_key_file(msg: &[u8], namespace: &str, path: &Path) -> Result<Signed> {
    let key = PrivateKey::read_openssh_file(path).with_context(|| format!("reading private key {}", path.display()))?;
    if key.is_encrypted() {
        bail!("{} is passphrase-protected; add it to ssh-agent (ssh-add) and retry", path.display());
    }
    let sig = SshSig::sign(&key, namespace, HashAlg::Sha512, msg).context("signing with key file")?;
    finish(sig, key.public_key().clone(), Method::KeyFile, namespace, msg)
}

pub fn sign_with_agent(msg: &[u8], namespace: &str, sock: &Path, wanted: Option<&PublicKey>) -> Result<Signed> {
    let mut client = ssh_agent_client_rs::Client::connect(sock).context("connecting")?;
    let ids: Vec<PublicKey> = client
        .list_all_identities()
        .context("listing identities")?
        .into_iter()
        .filter_map(|i| match i {
            ssh_agent_client_rs::Identity::PublicKey(k) => Some((*k).into_owned()),
            ssh_agent_client_rs::Identity::Certificate(_) => None,
        })
        .collect();
    let key = match wanted {
        Some(w) => {
            ids.into_iter().find(|k| k.key_data() == w.key_data()).context("the agent does not hold this key")?
        }
        None => {
            let mut ids = ids;
            ids.sort_by_key(|k| k.algorithm() != ssh_key::Algorithm::Ed25519);
            ids.into_iter().next().context("the agent holds no keys")?
        }
    };
    let data = SshSig::signed_data(namespace, HashAlg::Sha512, msg)?;
    let raw = client.sign(&key, &data).context("agent signing request")?;
    let sig = SshSig::new(key.key_data().clone(), namespace, HashAlg::Sha512, raw)?;
    finish(sig, key, Method::Agent, namespace, msg)
}

/// Check our own output before anyone else sees it (an agent may hand back an unusable signature,
/// e.g. SHA-1 RSA).
fn finish(sig: SshSig, key: PublicKey, method: Method, namespace: &str, msg: &[u8]) -> Result<Signed> {
    key.verify(namespace, msg, &sig).context("the new signature does not verify")?;
    Ok(Signed { pem: sig.to_pem(LineEnding::LF)?, key, method })
}

fn is_public(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "pub")
}

/// The public half of a key path: the file itself if `.pub`, else `<path>.pub`, else the private
/// key's embedded public key (readable even when the private key is encrypted).
fn public_key_for(p: &Path) -> Result<PublicKey> {
    if is_public(p) {
        return PublicKey::read_openssh_file(p).with_context(|| format!("reading {}", p.display()));
    }
    let pubp = p.with_file_name(format!("{}.pub", p.file_name().unwrap_or_default().to_string_lossy()));
    if let Ok(k) = PublicKey::read_openssh_file(&pubp) {
        return Ok(k);
    }
    Ok(PrivateKey::read_openssh_file(p).with_context(|| format!("reading {}", p.display()))?.public_key().clone())
}

pub fn parse_signature(pem: &str) -> Result<SshSig> {
    pem.parse::<SshSig>().context("track.sig is not an SSH signature")
}

/// The key embedded in a signature, once the signature is checked against it in `namespace`. This
/// proves only integrity, not trust: callers decide whether the key is trusted.
pub fn verify_embedded(msg: &[u8], namespace: &str, pem: &str) -> Result<PublicKey> {
    let sig = parse_signature(pem)?;
    let key = PublicKey::new(sig.public_key().clone(), "");
    key.verify(namespace, msg, &sig).context("signature does not verify")?;
    Ok(key)
}

/// Verify against allowed signers, enforcing each entry's `namespaces=`, `valid-after=` and
/// `valid-before=` at `now`. Returns the entry that accepted the signature.
pub fn verify(
    msg: &[u8],
    namespace: &str,
    pem: &str,
    signers: &[AllowedSigner],
    now: SystemTime,
) -> Result<AllowedSigner> {
    let sig = parse_signature(pem)?;
    let mut reasons = Vec::new();
    for s in signers.iter().filter(|s| s.key.key_data() == sig.public_key()) {
        if !s.allows_namespace(namespace) {
            reasons.push(format!("entry restricts namespaces to {:?}", s.namespaces.as_deref().unwrap_or("")));
        } else if !s.allows_time(now) {
            reasons.push("outside the entry's valid-after/valid-before window".to_string());
        } else {
            match s.key.verify(namespace, msg, &sig) {
                Ok(()) => return Ok(s.clone()),
                Err(e) => reasons.push(format!("signature does not verify: {e}")),
            }
        }
    }
    let fp = sig.public_key().fingerprint(HashAlg::Sha256);
    if reasons.is_empty() {
        bail!("no allowed signer has key {fp}");
    }
    bail!("key {fp} is listed but refused: {}", reasons.join("; "))
}

/// True if any entry lists this key, whatever its options.
pub fn lists_key(signers: &[AllowedSigner], key: &PublicKey) -> bool {
    signers.iter().any(|s| s.key.key_data() == key.key_data())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    fn keypair(dir: &Path) -> (std::path::PathBuf, PublicKey) {
        let mut k = PrivateKey::random(&mut rand_core::OsRng, ssh_key::Algorithm::Ed25519).unwrap();
        k.set_comment("tester@example");
        let p = dir.join("id_test");
        std::fs::write(&p, k.to_openssh(LineEnding::LF).unwrap().as_bytes()).unwrap();
        (p, k.public_key().clone())
    }

    fn signers(line: &str) -> Vec<AllowedSigner> {
        allowed_signers::parse(line).unwrap()
    }

    const DIGEST: &str = "7f3b32f5a1c4e7d2b9f0a8c6e4d2b0f8a6c4e2d0b8f6a4c2e0d8b6f4a2c0e8d6";

    #[test]
    fn round_trip_with_a_key_file() {
        let d = tempfile::tempdir().unwrap();
        let (key, pk) = keypair(d.path());
        let msg = message(DIGEST).unwrap();
        let s = sign(&msg, NAMESPACE, None, Some(&key)).unwrap();
        assert_eq!(s.method, Method::KeyFile);
        assert!(s.pem.starts_with("-----BEGIN SSH SIGNATURE-----"));
        let allowed = signers(&format!("tester@example {}", key_line(&pk)));
        let who = verify(&msg, NAMESPACE, &s.pem, &allowed, SystemTime::now()).unwrap();
        assert_eq!(who.principals, vec!["tester@example".to_string()]);
        assert_eq!(fingerprint(&verify_embedded(&msg, NAMESPACE, &s.pem).unwrap()), fingerprint(&pk));
    }

    #[test]
    fn tampered_digest_is_rejected() {
        let d = tempfile::tempdir().unwrap();
        let (key, pk) = keypair(d.path());
        let s = sign(&message(DIGEST).unwrap(), NAMESPACE, None, Some(&key)).unwrap();
        let other = message(&DIGEST.replace('7', "8")).unwrap();
        let allowed = signers(&format!("t {}", key_line(&pk)));
        assert!(verify(&other, NAMESPACE, &s.pem, &allowed, SystemTime::now()).is_err());
        assert!(verify_embedded(&other, NAMESPACE, &s.pem).is_err());
    }

    #[test]
    fn namespace_is_enforced() {
        let d = tempfile::tempdir().unwrap();
        let (key, pk) = keypair(d.path());
        let msg = message(DIGEST).unwrap();
        // A git-namespace signature is never a track signature.
        let git = sign(&msg, "git", None, Some(&key)).unwrap();
        let open = signers(&format!("t {}", key_line(&pk)));
        assert!(verify(&msg, NAMESPACE, &git.pem, &open, SystemTime::now()).is_err());
        assert!(verify_embedded(&msg, NAMESPACE, &git.pem).is_err());
        // An entry restricted to other namespaces refuses a genuine track signature.
        let s = sign(&msg, NAMESPACE, None, Some(&key)).unwrap();
        let git_only = signers(&format!("t namespaces=\"git\" {}", key_line(&pk)));
        let err = verify(&msg, NAMESPACE, &s.pem, &git_only, SystemTime::now()).unwrap_err();
        assert!(err.to_string().contains("namespaces"), "{err}");
        let globbed = signers(&format!("t namespaces=\"git,adage-*\" {}", key_line(&pk)));
        assert!(verify(&msg, NAMESPACE, &s.pem, &globbed, SystemTime::now()).is_ok());
    }

    #[test]
    fn validity_window_is_enforced() {
        let d = tempfile::tempdir().unwrap();
        let (key, pk) = keypair(d.path());
        let msg = message(DIGEST).unwrap();
        let s = sign(&msg, NAMESPACE, None, Some(&key)).unwrap();
        let now = SystemTime::now();
        let expired = signers(&format!("t valid-before=20200101 {}", key_line(&pk)));
        assert!(verify(&msg, NAMESPACE, &s.pem, &expired, now).is_err());
        let future = signers(&format!("t valid-after=20990101 {}", key_line(&pk)));
        assert!(verify(&msg, NAMESPACE, &s.pem, &future, now).is_err());
        let inside = allowed_signers::parse_timestamp("20191231").unwrap() + Duration::from_secs(60);
        assert!(verify(&msg, NAMESPACE, &s.pem, &expired, inside).is_ok());
    }

    #[test]
    fn unknown_signer_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let (key, _) = keypair(d.path());
        let e = tempfile::tempdir().unwrap();
        let (_, stranger) = keypair(e.path());
        let msg = message(DIGEST).unwrap();
        let s = sign(&msg, NAMESPACE, None, Some(&key)).unwrap();
        let allowed = signers(&format!("t {}", key_line(&stranger)));
        let err = verify(&msg, NAMESPACE, &s.pem, &allowed, SystemTime::now()).unwrap_err();
        assert!(err.to_string().contains("no allowed signer"), "{err}");
        // A cert-authority entry is never a plain signer.
        let pk = verify_embedded(&msg, NAMESPACE, &s.pem).unwrap();
        assert!(signers(&format!("t cert-authority {}", key_line(&pk))).is_empty());
    }

    /// Agent signing against a private `ssh-agent` on its own socket in a tempdir — never the
    /// user's agent. Skipped where `ssh-agent` is not installed.
    #[test]
    fn signs_through_a_private_agent() {
        let Some(agent) = ["/usr/bin/ssh-agent", "/usr/local/bin/ssh-agent", "/opt/homebrew/bin/ssh-agent"]
            .into_iter()
            .find(|p| Path::new(p).exists())
        else {
            eprintln!("ssh-agent not installed; skipping");
            return;
        };
        let d = tempfile::tempdir().unwrap();
        let sock = d.path().join("a.sock");
        let child = std::process::Command::new(agent)
            .arg("-D")
            .arg("-a")
            .arg(&sock)
            .env_remove("SSH_AUTH_SOCK")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        struct Kill(std::process::Child);
        impl Drop for Kill {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        let _guard = Kill(child);
        for _ in 0..100 {
            if sock.exists() {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let (key, pk) = keypair(d.path());
        let private = PrivateKey::read_openssh_file(&key).unwrap();
        ssh_agent_client_rs::Client::connect(&sock).unwrap().add_identity(&private).unwrap();
        let msg = message(DIGEST).unwrap();
        // With --key naming the key, the agent is preferred over the file.
        let via_agent = sign(&msg, NAMESPACE, Some(&sock), Some(&key)).unwrap();
        assert_eq!(via_agent.method, Method::Agent);
        let allowed = signers(&format!("t {}", key_line(&pk)));
        assert!(verify(&msg, NAMESPACE, &via_agent.pem, &allowed, SystemTime::now()).is_ok());
        // Ed25519 is deterministic: agent and file signatures are byte-identical.
        let via_file = sign(&msg, NAMESPACE, None, Some(&key)).unwrap();
        assert_eq!(via_agent.pem, via_file.pem);
        // No key named: the agent's key is used.
        assert_eq!(sign(&msg, NAMESPACE, Some(&sock), None).unwrap().method, Method::Agent);
        // An agent without the named key falls back to the key file.
        ssh_agent_client_rs::Client::connect(&sock).unwrap().remove_all_identities().unwrap();
        assert_eq!(sign(&msg, NAMESPACE, Some(&sock), Some(&key)).unwrap().method, Method::KeyFile);
    }
}
