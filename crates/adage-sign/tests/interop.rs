//! Independent OpenSSH compatibility checks. Every key, signature and agent socket belongs to
//! a temporary directory; the user's keys and agent are never used. Production signing remains
//! native: only this integration suite invokes OpenSSH programs.
#![cfg(unix)]

use adage_sign::{allowed_signers, fingerprint, message, sign_with, sign_with_agent, sign_with_key_file, verify};
use adage_sign::{Method, PublicKey, Signed, Zeroizing, PACK_NAMESPACE};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime};

const PRINCIPAL: &str = "interop@example.invalid";
const DIGEST: &str = "7f3b32f5a1c4e7d2b9f0a8c6e4d2b0f8a6c4e2d0b8f6a4c2e0d8b6f4a2c0e8d6";
const PASSPHRASE: &str = "temporary-interop-key-only";

struct Algorithm {
    name: &'static str,
    kind: &'static str,
    bits: Option<&'static str>,
}

const ALGORITHMS: &[Algorithm] = &[
    Algorithm { name: "ed25519", kind: "ed25519", bits: None },
    Algorithm { name: "rsa", kind: "rsa", bits: Some("2048") },
    Algorithm { name: "ecdsa-p256", kind: "ecdsa", bits: Some("256") },
    Algorithm { name: "ecdsa-p384", kind: "ecdsa", bits: Some("384") },
    Algorithm { name: "ecdsa-p521", kind: "ecdsa", bits: Some("521") },
];

/// OpenSSH is optional for local Rust-only development, but Unix CI must actually exercise the
/// interoperability contract. A broken installed tool is always a failure, never a skip.
fn tool(name: &str) -> Option<PathBuf> {
    let found = std::env::var_os("PATH")
        .and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(name)).find(|path| path.is_file()));
    if found.is_none() {
        assert!(std::env::var_os("CI").is_none(), "{name} is required for OpenSSH interoperability tests in CI");
        eprintln!("{name} is not installed; skipping OpenSSH interoperability test");
    }
    found
}

fn command(tool: &Path, home: &Path) -> Command {
    let mut cmd = Command::new(tool);
    cmd.current_dir(home)
        .env("HOME", home)
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("SSH_AGENT_PID")
        .env_remove("SSH_ASKPASS")
        .env_remove("SSH_ASKPASS_REQUIRE")
        .env_remove("DISPLAY")
        .stdin(Stdio::null());
    cmd
}

fn success(out: Output, context: &str) -> Output {
    assert!(out.status.success(), "{context}: {}\n{}", out.status, String::from_utf8_lossy(&out.stderr));
    out
}

fn generate(keygen: &Path, dir: &Path, algorithm: &Algorithm, passphrase: &str) -> PathBuf {
    let path = dir.join(algorithm.name);
    let mut cmd = command(keygen, dir);
    cmd.args(["-q", "-t", algorithm.kind, "-N", passphrase, "-C", PRINCIPAL]);
    if let Some(bits) = algorithm.bits {
        cmd.args(["-b", bits]);
    }
    // These are disposable test keys. A single bcrypt round keeps encrypted-format coverage
    // fast without changing the OpenSSH encryption format being exercised.
    cmd.args(["-a", "1", "-f"]).arg(&path);
    success(cmd.output().unwrap(), &format!("generate {} fixture key", algorithm.name));
    path
}

fn public_key(path: &Path) -> PublicKey {
    let public = path.with_file_name(format!("{}.pub", path.file_name().unwrap().to_string_lossy()));
    PublicKey::from_openssh(&std::fs::read_to_string(public).unwrap()).unwrap()
}

fn trusted_line(key: &PublicKey) -> String {
    format!("{PRINCIPAL} namespaces=\"{PACK_NAMESPACE}\" {}\n", adage_sign::key_line(key))
}

fn verify_with_openssh(keygen: &Path, dir: &Path, signed: &Signed, msg: &[u8]) {
    let allowed = dir.join("allowed_signers");
    let signature = dir.join("native.sig");
    std::fs::write(&allowed, trusted_line(&signed.key)).unwrap();
    std::fs::write(&signature, &signed.pem).unwrap();
    let mut child = command(keygen, dir)
        .args(["-Y", "verify", "-f"])
        .arg(&allowed)
        .args(["-I", PRINCIPAL, "-n", PACK_NAMESPACE, "-s"])
        .arg(&signature)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(msg).unwrap();
    success(child.wait_with_output().unwrap(), "OpenSSH verification of native signature");
}

#[test]
fn every_supported_key_algorithm_interoperates_with_openssh_in_both_directions() {
    let Some(keygen) = tool("ssh-keygen") else { return };
    let temp = tempfile::tempdir().unwrap();
    let msg = message(DIGEST).unwrap();
    for algorithm in ALGORITHMS {
        let dir = temp.path().join(algorithm.name);
        std::fs::create_dir(&dir).unwrap();
        let path = generate(&keygen, &dir, algorithm, "");
        let public = public_key(&path);
        let signed = sign_with_key_file(&msg, PACK_NAMESPACE, &path)
            .unwrap_or_else(|error| panic!("native {} file signing: {error:#}", algorithm.name));
        assert_eq!(signed.method, Method::KeyFile);
        assert_eq!(fingerprint(&signed.key), fingerprint(&public));
        verify_with_openssh(&keygen, &dir, &signed, &msg);

        let input = dir.join("digest");
        std::fs::write(&input, &msg).unwrap();
        success(
            command(&keygen, &dir)
                .args(["-Y", "sign", "-f"])
                .arg(&path)
                .args(["-n", PACK_NAMESPACE, "-O", "hashalg=sha512"])
                .arg(&input)
                .output()
                .unwrap(),
            &format!("OpenSSH {} signing", algorithm.name),
        );
        let pem = std::fs::read_to_string(dir.join("digest.sig")).unwrap();
        let allowed = allowed_signers::parse(&trusted_line(&public)).unwrap();
        let who = verify(&msg, PACK_NAMESPACE, &pem, &allowed, SystemTime::now())
            .unwrap_or_else(|error| panic!("native verification of OpenSSH {}: {error:#}", algorithm.name));
        assert_eq!(who.principals, [PRINCIPAL]);
        let mut changed = msg.clone();
        changed[0] ^= 1;
        assert!(verify(&changed, PACK_NAMESPACE, &pem, &allowed, SystemTime::now()).is_err());
        assert!(adage_sign::verify_embedded(&msg, "git", &pem).is_err(), "namespace binds the signature");
        assert!(verify(&msg, PACK_NAMESPACE, &pem, &[], SystemTime::now()).is_err(), "integrity is not trust");
    }
}

#[test]
fn encrypted_openssh_key_files_remain_usable_for_every_supported_algorithm() {
    let Some(keygen) = tool("ssh-keygen") else { return };
    let temp = tempfile::tempdir().unwrap();
    let msg = message(DIGEST).unwrap();
    for algorithm in ALGORITHMS {
        let dir = temp.path().join(algorithm.name);
        std::fs::create_dir(&dir).unwrap();
        let path = generate(&keygen, &dir, algorithm, PASSPHRASE);
        assert!(sign_with_key_file(&msg, PACK_NAMESPACE, &path).is_err(), "encrypted keys require a passphrase");
        let tries = std::cell::Cell::new(0);
        let signed = sign_with(&msg, PACK_NAMESPACE, None, Some(&path), &|requested| {
            assert_eq!(requested, path);
            tries.set(tries.get() + 1);
            Some(Zeroizing::new(PASSPHRASE.to_string()))
        })
        .unwrap_or_else(|error| panic!("native encrypted {} file signing: {error:#}", algorithm.name));
        assert_eq!(tries.get(), 1);
        assert_eq!(signed.method, Method::KeyFile);
        assert_eq!(fingerprint(&signed.key), fingerprint(&public_key(&path)));
        verify_with_openssh(&keygen, &dir, &signed, &msg);
    }
}

struct PrivateAgent {
    child: Child,
    socket: PathBuf,
}

impl PrivateAgent {
    fn start(agent: &Path, dir: &Path) -> Self {
        let socket = dir.join("agent.sock");
        let child = command(agent, dir)
            .args(["-D", "-a"])
            .arg(&socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut guard = Self { child, socket };
        let deadline = Instant::now() + Duration::from_secs(5);
        while !guard.socket.exists() {
            assert!(
                guard.child.try_wait().unwrap().is_none(),
                "the isolated ssh-agent exited before creating its socket"
            );
            assert!(Instant::now() < deadline, "the isolated ssh-agent did not create its socket");
            std::thread::sleep(Duration::from_millis(10));
        }
        guard
    }
}

impl Drop for PrivateAgent {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn a_private_agent_signs_each_supported_algorithm_without_using_the_users_agent() {
    let (Some(keygen), Some(agent), Some(add)) = (tool("ssh-keygen"), tool("ssh-agent"), tool("ssh-add")) else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let agent = PrivateAgent::start(&agent, temp.path());
    let msg = message(DIGEST).unwrap();
    for algorithm in ALGORITHMS {
        let dir = temp.path().join(algorithm.name);
        std::fs::create_dir(&dir).unwrap();
        let path = generate(&keygen, &dir, algorithm, "");
        let public = public_key(&path);
        success(
            command(&add, &dir).env("SSH_AUTH_SOCK", &agent.socket).arg(&path).output().unwrap(),
            "add a temporary key to the isolated agent",
        );
        let signed = sign_with_agent(&msg, PACK_NAMESPACE, &agent.socket, Some(&public))
            .unwrap_or_else(|error| panic!("native {} agent signing: {error:#}", algorithm.name));
        assert_eq!(signed.method, Method::Agent);
        assert_eq!(fingerprint(&signed.key), fingerprint(&public));
        verify_with_openssh(&keygen, &dir, &signed, &msg);
        if algorithm.kind == "ed25519" {
            let file = sign_with_key_file(&msg, PACK_NAMESPACE, &path).unwrap();
            assert_eq!(signed.pem, file.pem, "Ed25519 file and agent signatures are deterministic");
        }
    }
}
