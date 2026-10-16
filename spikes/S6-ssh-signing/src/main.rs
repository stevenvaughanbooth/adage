mod allowed_signers;
mod digest;

use anyhow::{bail, Context, Result};
use clap::{Parser, Subcommand};
use rand_core::OsRng;
use ssh_key::{Algorithm, HashAlg, PrivateKey, PublicKey, SshSig};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Parser)]
#[command(name = "s6", about = "S6 spike: SSH signing of Adage tracks")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Generate a throwaway Ed25519 keypair (unencrypted, spike use only).
    Keygen {
        #[arg(long)]
        out_prefix: PathBuf,
        #[arg(long, default_value = "adage-spike")]
        comment: String,
    },
    /// Compute the canonical digest of a track directory.
    Digest {
        #[arg(long)]
        track: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        manifest_out: Option<PathBuf>,
    },
    /// Sign a digest file natively, using a private key file (no agent, no subprocess).
    SignFile {
        #[arg(long)]
        key: PathBuf,
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        digest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "sha512")]
        hash_alg: String,
    },
    /// Sign a digest file natively through the ssh-agent protocol (no subprocess).
    SignAgent {
        #[arg(long)]
        pubkey: PathBuf,
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        digest: PathBuf,
        #[arg(long)]
        out: PathBuf,
        #[arg(long, default_value = "sha512")]
        hash_alg: String,
        /// Path to the agent's Unix socket (must NOT be the user's real ssh-agent).
        #[arg(long)]
        sock: PathBuf,
    },
    /// List identities held by an agent (debugging aid).
    ListAgent {
        #[arg(long)]
        sock: PathBuf,
    },
    /// Verify a signature natively against an allowed_signers file, with namespace
    /// and valid-after/valid-before enforcement, entirely with the `ssh-key` crate
    /// plus our own allowed_signers parser (the crate has no allowed_signers support).
    Verify {
        #[arg(long)]
        sig: PathBuf,
        #[arg(long)]
        namespace: String,
        #[arg(long)]
        digest: PathBuf,
        #[arg(long)]
        allowed_signers: PathBuf,
        /// Optional principal to require a match for (else any principal's key that
        /// verifies is accepted, matching `ssh-keygen -Y verify -I <principal>` when
        /// omitted would NOT be valid ssh-keygen usage -- ssh-keygen always requires
        /// -I; we default to "any" to make sweeping the matrix easier).
        #[arg(long)]
        principal: Option<String>,
        /// Verify as of this OpenSSH-format timestamp (YYYYMMDD[Z] / YYYYMMDDHHMM[SS][Z])
        /// instead of the current time -- mirrors `ssh-keygen -Y verify -Overify-time=`.
        #[arg(long)]
        verify_time: Option<String>,
    },
}

fn hash_alg_from_str(s: &str) -> Result<HashAlg> {
    match s {
        "sha256" => Ok(HashAlg::Sha256),
        "sha512" => Ok(HashAlg::Sha512),
        other => bail!("unsupported hash algorithm {other:?} (use sha256 or sha512)"),
    }
}

fn read_digest(path: &Path) -> Result<Vec<u8>> {
    let bytes = std::fs::read(path).with_context(|| format!("reading digest file {path:?}"))?;
    Ok(bytes)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Keygen { out_prefix, comment } => cmd_keygen(&out_prefix, &comment),
        Cmd::Digest {
            track,
            out,
            manifest_out,
        } => cmd_digest(&track, &out, manifest_out.as_deref()),
        Cmd::SignFile {
            key,
            namespace,
            digest,
            out,
            hash_alg,
        } => cmd_sign_file(&key, &namespace, &digest, &out, &hash_alg),
        Cmd::SignAgent {
            pubkey,
            namespace,
            digest,
            out,
            hash_alg,
            sock,
        } => cmd_sign_agent(&pubkey, &namespace, &digest, &out, &hash_alg, &sock),
        Cmd::ListAgent { sock } => cmd_list_agent(&sock),
        Cmd::Verify {
            sig,
            namespace,
            digest,
            allowed_signers,
            principal,
            verify_time,
        } => cmd_verify(
            &sig,
            &namespace,
            &digest,
            &allowed_signers,
            principal.as_deref(),
            verify_time.as_deref(),
        ),
    }
}

fn cmd_keygen(out_prefix: &Path, comment: &str) -> Result<()> {
    let key = PrivateKey::random(&mut OsRng, Algorithm::Ed25519)
        .context("generating Ed25519 keypair")?;
    let mut key = key;
    key.set_comment(comment);

    let priv_path = out_prefix.with_extension("");
    let pub_path = {
        let mut p = priv_path.clone();
        p.set_file_name(format!(
            "{}.pub",
            priv_path.file_name().unwrap().to_string_lossy()
        ));
        p
    };

    let priv_pem = key
        .to_openssh(ssh_key::LineEnding::LF)
        .context("encoding private key")?;
    std::fs::write(&priv_path, priv_pem.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&priv_path, std::fs::Permissions::from_mode(0o600))?;
    }

    let pub_line = key.public_key().to_openssh()?;
    std::fs::write(&pub_path, format!("{pub_line}\n"))?;

    println!("wrote {priv_path:?} and {pub_path:?}");
    println!("fingerprint: {}", key.public_key().fingerprint(HashAlg::Sha256));
    Ok(())
}

fn cmd_digest(track: &Path, out: &Path, manifest_out: Option<&Path>) -> Result<()> {
    let d = digest::compute(track)?;
    std::fs::write(out, d.digest)?;
    if let Some(mpath) = manifest_out {
        std::fs::write(mpath, &d.manifest_text)?;
    }
    println!("files hashed: {}", d.files.len());
    for f in &d.files {
        println!("  {}  {}", f.sha256_hex, f.rel_path);
    }
    println!("canonical digest (sha256 of manifest): {}", hex::encode(d.digest));
    Ok(())
}

fn cmd_sign_file(
    key_path: &Path,
    namespace: &str,
    digest_path: &Path,
    out: &Path,
    hash_alg: &str,
) -> Result<()> {
    let hash_alg = hash_alg_from_str(hash_alg)?;
    let key = PrivateKey::read_openssh_file(key_path)
        .with_context(|| format!("reading private key {key_path:?}"))?;
    let msg = read_digest(digest_path)?;
    let sig = SshSig::sign(&key, namespace, hash_alg, &msg).context("signing (file key)")?;
    let pem = sig.to_pem(ssh_key::LineEnding::LF)?;
    std::fs::write(out, pem)?;
    println!("signed {digest_path:?} -> {out:?} (namespace={namespace:?}, key file)");
    Ok(())
}

fn cmd_sign_agent(
    pubkey_path: &Path,
    namespace: &str,
    digest_path: &Path,
    out: &Path,
    hash_alg: &str,
    sock: &Path,
) -> Result<()> {
    let hash_alg = hash_alg_from_str(hash_alg)?;
    let pubkey = PublicKey::read_openssh_file(pubkey_path)
        .with_context(|| format!("reading public key {pubkey_path:?}"))?;
    let msg = read_digest(digest_path)?;

    let signed_data = SshSig::signed_data(namespace, hash_alg, &msg)
        .context("building sshsig signed-data preimage")?;

    let mut client =
        ssh_agent_client_rs::Client::connect(sock).context("connecting to ssh-agent socket")?;
    let signature = client
        .sign(&pubkey, &signed_data)
        .context("agent SIGN_REQUEST failed")?;

    let sig = SshSig::new(pubkey.key_data().clone(), namespace, hash_alg, signature)
        .context("assembling SshSig from agent signature")?;
    let pem = sig.to_pem(ssh_key::LineEnding::LF)?;
    std::fs::write(out, pem)?;
    println!("signed {digest_path:?} -> {out:?} (namespace={namespace:?}, via agent {sock:?})");
    Ok(())
}

fn cmd_list_agent(sock: &Path) -> Result<()> {
    let mut client =
        ssh_agent_client_rs::Client::connect(sock).context("connecting to ssh-agent socket")?;
    let ids = client.list_all_identities().context("SSH2_AGENTC_REQUEST_IDENTITIES")?;
    println!("{} identities in agent at {:?}:", ids.len(), sock);
    for id in ids {
        match id {
            ssh_agent_client_rs::Identity::PublicKey(pk) => {
                println!("  {} {}", pk.fingerprint(HashAlg::Sha256), pk.comment());
            }
            ssh_agent_client_rs::Identity::Certificate(_) => println!("  <certificate>"),
        }
    }
    Ok(())
}

fn cmd_verify(
    sig_path: &Path,
    namespace: &str,
    digest_path: &Path,
    allowed_signers_path: &Path,
    principal: Option<&str>,
    verify_time: Option<&str>,
) -> Result<()> {
    let sig_pem = std::fs::read_to_string(sig_path)
        .with_context(|| format!("reading signature {sig_path:?}"))?;
    let sig: SshSig = sig_pem.parse().context("parsing SSH SIGNATURE pem")?;
    let msg = read_digest(digest_path)?;

    let now = match verify_time {
        Some(t) => allowed_signers::parse_timestamp(t)?,
        None => SystemTime::now(),
    };

    let signers = allowed_signers::parse_file(allowed_signers_path)?;

    let mut tried = 0usize;
    for signer in &signers {
        if let Some(p) = principal {
            if !signer.principals.iter().any(|pr| pr == p) {
                continue;
            }
        }
        // Public key must match the one embedded in the signature (cheap check
        // before we bother with namespace/time/crypto).
        if signer.key.key_data() != sig.public_key() {
            continue;
        }
        tried += 1;

        if !signer.allows_namespace(namespace) {
            println!(
                "REJECT: key matches but allowed_signers restricts it to namespaces {:?}, asked for {namespace:?}",
                signer.namespaces
            );
            continue;
        }
        if !signer.allows_time(now) {
            println!(
                "REJECT: key matches but is outside its valid-after/valid-before window (checked at {now:?})"
            );
            continue;
        }

        match signer.key.verify(namespace, &msg, &sig) {
            Ok(()) => {
                println!(
                    "GOOD signature: namespace={namespace:?} principal(s)={:?} key={} alg={:?}",
                    signer.principals,
                    signer.key.fingerprint(HashAlg::Sha256),
                    sig.algorithm()
                );
                return Ok(());
            }
            Err(e) => {
                println!("REJECT: key matched but cryptographic verification failed: {e}");
            }
        }
    }

    if tried == 0 {
        bail!(
            "no allowed_signers entry matches the signature's public key ({})",
            sig.public_key().fingerprint(HashAlg::Sha256)
        );
    }
    bail!("signature did not verify against any matching allowed_signers entry");
}

