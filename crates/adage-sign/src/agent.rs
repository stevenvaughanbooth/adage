//! The two SSH-agent operations needed for signing, using [RFC 9987] framing.
//!
//! Key, certificate and signature decoding stays in `ssh-key`; this module only transports
//! length-prefixed SSH messages ([RFC 4251, section 5]). Every exchange has one deadline across
//! writes and reads, and advertised lengths are checked before allocating. The 30-second budget
//! allows an agent to request confirmation or a security-key touch without waiting indefinitely.
//!
//! [RFC 9987]: https://www.rfc-editor.org/rfc/rfc9987.html
//! [RFC 4251, section 5]: https://www.rfc-editor.org/rfc/rfc4251.html#section-5

use anyhow::{bail, Context, Result};
use ssh_key::{Algorithm, Certificate, HashAlg, PublicKey, Signature};
use std::io::{self, Read, Write};
use std::path::Path;
use std::time::{Duration, Instant};

const FAILURE: u8 = 5;
const REQUEST_IDENTITIES: u8 = 11;
const IDENTITIES_ANSWER: u8 = 12;
const SIGN_REQUEST: u8 = 13;
const SIGN_RESPONSE: u8 = 14;
const RSA_SHA2_512: u32 = 4;
const MAX_PACKET: usize = 1024 * 1024;
const MAX_IDENTITIES: u32 = 1024;
const TIMEOUT: Duration = Duration::from_secs(30);
const RETRY_INTERVAL: Duration = Duration::from_millis(5);

trait Transport: Read + Write {}
impl<T: Read + Write> Transport for T {}

pub(crate) struct Client {
    stream: Box<dyn Transport>,
    timeout: Duration,
}

impl Client {
    pub(crate) fn connect(path: &Path) -> Result<Self> {
        Ok(Self { stream: connect(path)?, timeout: TIMEOUT })
    }

    pub(crate) fn list_identities(&mut self) -> Result<Vec<PublicKey>> {
        let response = self.exchange(&[REQUEST_IDENTITIES], IDENTITIES_ANSWER)?;
        let mut reader = Fields(&response);
        let count = reader.u32()?;
        if count > MAX_IDENTITIES {
            bail!("ssh-agent returned too many identities ({count}, maximum {MAX_IDENTITIES})");
        }
        let mut keys = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let blob = reader.string()?;
            let comment = reader.string()?;
            let mut key_fields = Fields(blob);
            let algorithm = std::str::from_utf8(key_fields.string()?).context("invalid ssh-agent key algorithm")?;
            if algorithm.ends_with("-cert-v01@openssh.com") {
                // Certificates cannot be used as plain pack signers. Still validate the complete
                // encoding, rather than silently accepting malformed or unknown identity blobs.
                Certificate::from_bytes(blob).context("invalid ssh-agent certificate")?;
            } else {
                let mut key = PublicKey::from_bytes(blob).context("invalid ssh-agent public key")?;
                key.set_comment(comment);
                keys.push(key);
            }
        }
        reader.finish()?;
        Ok(keys)
    }

    pub(crate) fn sign(&mut self, key: &PublicKey, data: &[u8]) -> Result<Signature> {
        let mut request = vec![SIGN_REQUEST];
        put_string(&mut request, &key.to_bytes()?)?;
        put_string(&mut request, data)?;
        let rsa = matches!(key.algorithm(), Algorithm::Rsa { .. });
        request.extend_from_slice(&(if rsa { RSA_SHA2_512 } else { 0 }).to_be_bytes());
        let response = self.exchange(&request, SIGN_RESPONSE)?;
        let mut reader = Fields(&response);
        let blob = reader.string()?;
        reader.finish()?;
        let signature = Signature::try_from(blob).context("invalid ssh-agent signature")?;
        // Signature::try_from does not enforce exhaustion. Re-encoding also preserves the
        // flags/counter trailers used by OpenSSH security-key signatures.
        if Vec::<u8>::try_from(signature.clone())? != blob {
            bail!("non-canonical or trailing data in ssh-agent signature");
        }
        if rsa && signature.algorithm() != (Algorithm::Rsa { hash: Some(HashAlg::Sha512) }) {
            bail!("ssh-agent did not return the requested RSA SHA-512 signature");
        }
        Ok(signature)
    }

    fn exchange(&mut self, request: &[u8], expected: u8) -> Result<Vec<u8>> {
        if request.is_empty() || request.len() > MAX_PACKET {
            bail!("ssh-agent request exceeds the packet limit");
        }
        let deadline = Instant::now() + self.timeout;
        let mut packet = Vec::with_capacity(4 + request.len());
        packet.extend_from_slice(&(request.len() as u32).to_be_bytes());
        packet.extend_from_slice(request);
        write_all(&mut *self.stream, &packet, deadline).context("writing ssh-agent request")?;
        let mut length = [0; 4];
        read_exact(&mut *self.stream, &mut length, deadline).context("reading ssh-agent response length")?;
        let length = u32::from_be_bytes(length) as usize;
        if length == 0 || length > MAX_PACKET {
            bail!("invalid ssh-agent response length ({length}, maximum {MAX_PACKET})");
        }
        let mut response = vec![0; length];
        read_exact(&mut *self.stream, &mut response, deadline).context("reading ssh-agent response")?;
        match response[0] {
            FAILURE if response.len() == 1 => bail!("ssh-agent refused the request"),
            kind if kind == expected => Ok(response[1..].to_vec()),
            kind => bail!("unexpected ssh-agent response type {kind}"),
        }
    }
}

fn put_string(output: &mut Vec<u8>, value: &[u8]) -> Result<()> {
    let length = output.len().checked_add(4).and_then(|n| n.checked_add(value.len()));
    if !length.is_some_and(|n| n <= MAX_PACKET) {
        bail!("ssh-agent request exceeds the packet limit");
    }
    output.extend_from_slice(&(value.len() as u32).to_be_bytes());
    output.extend_from_slice(value);
    Ok(())
}

struct Fields<'a>(&'a [u8]);

impl<'a> Fields<'a> {
    fn take(&mut self, length: usize) -> Result<&'a [u8]> {
        if length > self.0.len() {
            bail!("truncated ssh-agent response field");
        }
        let (value, rest) = self.0.split_at(length);
        self.0 = rest;
        Ok(value)
    }

    fn u32(&mut self) -> Result<u32> {
        Ok(u32::from_be_bytes(self.take(4)?.try_into().expect("four-byte field")))
    }

    fn string(&mut self) -> Result<&'a [u8]> {
        let length = self.u32()? as usize;
        self.take(length)
    }

    fn finish(&self) -> Result<()> {
        if !self.0.is_empty() {
            bail!("trailing data in ssh-agent response");
        }
        Ok(())
    }
}

fn remaining(deadline: Instant) -> io::Result<Duration> {
    match deadline.checked_duration_since(Instant::now()) {
        Some(remaining) if !remaining.is_zero() => Ok(remaining),
        _ => Err(io::Error::new(io::ErrorKind::TimedOut, "ssh-agent request timed out")),
    }
}

fn retry(error: io::Error, deadline: Instant) -> io::Result<()> {
    match error.kind() {
        io::ErrorKind::Interrupted => Ok(()),
        io::ErrorKind::WouldBlock => {
            std::thread::sleep(remaining(deadline)?.min(RETRY_INTERVAL));
            Ok(())
        }
        _ => Err(error),
    }
}

fn read_exact(stream: &mut dyn Transport, mut buffer: &mut [u8], deadline: Instant) -> io::Result<()> {
    while !buffer.is_empty() {
        remaining(deadline)?;
        match stream.read(buffer) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "ssh-agent closed the connection")),
            Ok(count) => buffer = &mut buffer[count..],
            Err(error) => retry(error, deadline)?,
        }
    }
    Ok(())
}

fn write_all(stream: &mut dyn Transport, mut buffer: &[u8], deadline: Instant) -> io::Result<()> {
    while !buffer.is_empty() {
        remaining(deadline)?;
        match stream.write(buffer) {
            Ok(0) => return Err(io::Error::new(io::ErrorKind::WriteZero, "ssh-agent stopped accepting data")),
            Ok(count) => buffer = &buffer[count..],
            Err(error) => retry(error, deadline)?,
        }
    }
    // No flush: sockets and named pipes write immediately. Windows FlushFileBuffers waits for
    // the remote process to read, which would defeat the deadline. Its response is our receipt.
    Ok(())
}

#[cfg(unix)]
fn connect(path: &Path) -> io::Result<Box<dyn Transport>> {
    use socket2::{Domain, SockAddr, Socket, Type};
    let socket = Socket::new(Domain::UNIX, Type::STREAM, None)?;
    socket.connect_timeout(&SockAddr::unix(path)?, TIMEOUT)?;
    socket.set_nonblocking(true)?;
    let descriptor: std::os::fd::OwnedFd = socket.into();
    let stream = std::os::unix::net::UnixStream::from(descriptor);
    Ok(Box::new(stream))
}

#[cfg(windows)]
fn connect(path: &Path) -> io::Result<Box<dyn Transport>> {
    use interprocess::os::windows::named_pipe::{pipe_mode, DuplexPipeStream};
    use interprocess::ConnectWaitMode;
    let stream =
        DuplexPipeStream::<pipe_mode::Bytes>::connect_by_path_with_wait_mode(path, ConnectWaitMode::Timeout(TIMEOUT))?;
    stream.set_nonblocking(true)?;
    Ok(Box::new(WindowsPipe(stream)))
}

#[cfg(windows)]
struct WindowsPipe(
    interprocess::os::windows::named_pipe::DuplexPipeStream<interprocess::os::windows::named_pipe::pipe_mode::Bytes>,
);

#[cfg(windows)]
impl Read for WindowsPipe {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        match self.0.read(buffer) {
            // interprocess maps both idle PIPE_NOWAIT reads (ERROR_NO_DATA) and a closed pipe
            // to zero. Treat either as pending, so a delayed agent response is not mistaken for
            // EOF. A closed pipe remains bounded by the same request deadline.
            Ok(0) if !buffer.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            result => result,
        }
    }
}

#[cfg(windows)]
impl Write for WindowsPipe {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        match self.0.write(buffer) {
            // A full nonblocking byte-mode pipe may accept zero bytes without an error.
            Ok(0) if !buffer.is_empty() => Err(io::ErrorKind::WouldBlock.into()),
            result => result,
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(windows)]
impl Drop for WindowsPipe {
    fn drop(&mut self) {
        // Never spawn interprocess's background flush/limbo thread when the agent times out.
        self.0.assume_flushed();
    }
}

#[cfg(not(any(unix, windows)))]
fn connect(_path: &Path) -> io::Result<Box<dyn Transport>> {
    Err(io::Error::new(io::ErrorKind::Unsupported, "ssh-agent transport is unavailable on this platform"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::io::Cursor;
    use std::rc::Rc;

    struct Scripted {
        input: Cursor<Vec<u8>>,
        output: Rc<RefCell<Vec<u8>>>,
        read_delay: Duration,
        pending: bool,
    }

    impl Read for Scripted {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            if self.pending {
                return Err(io::ErrorKind::WouldBlock.into());
            }
            std::thread::sleep(self.read_delay);
            let length = buffer.len().min(2);
            self.input.read(&mut buffer[..length])
        }
    }

    impl Write for Scripted {
        fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
            let length = buffer.len().min(3);
            self.output.borrow_mut().extend_from_slice(&buffer[..length]);
            Ok(length)
        }

        fn flush(&mut self) -> io::Result<()> {
            panic!("agent exchanges must not flush")
        }
    }

    fn scripted(input: Vec<u8>) -> (Client, Rc<RefCell<Vec<u8>>>) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let stream =
            Scripted { input: Cursor::new(input), output: output.clone(), read_delay: Duration::ZERO, pending: false };
        (Client { stream: Box::new(stream), timeout: Duration::from_secs(1) }, output)
    }

    fn frame(payload: &[u8]) -> Vec<u8> {
        let mut result = (payload.len() as u32).to_be_bytes().to_vec();
        result.extend_from_slice(payload);
        result
    }

    fn ed25519_key() -> PublicKey {
        PublicKey::new(ssh_key::public::Ed25519PublicKey([7; 32]).into(), "")
    }

    fn identities(entries: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut payload = vec![IDENTITIES_ANSWER];
        payload.extend_from_slice(&(entries.len() as u32).to_be_bytes());
        for (key, comment) in entries {
            put_string(&mut payload, key).unwrap();
            put_string(&mut payload, comment).unwrap();
        }
        payload
    }

    fn signature_response(signature: Signature) -> Vec<u8> {
        let mut response = vec![SIGN_RESPONSE];
        put_string(&mut response, &Vec::<u8>::try_from(signature).unwrap()).unwrap();
        frame(&response)
    }

    #[test]
    fn lists_identities_with_fragmented_io_and_preserves_comments() {
        let key = ed25519_key();
        let (mut client, output) = scripted(frame(&identities(&[(&key.to_bytes().unwrap(), b"test identity")])));
        let keys = client.list_identities().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_data(), key.key_data());
        assert_eq!(keys[0].comment().as_str().unwrap(), "test identity");
        assert_eq!(*output.borrow(), frame(&[REQUEST_IDENTITIES]));
    }

    #[test]
    fn preserves_binary_identity_comments_without_blocking_other_keys() {
        let key = ed25519_key().to_bytes().unwrap();
        let binary_comment = b"identity\0\xff";
        let payload = identities(&[(&key, binary_comment), (&key, b"text comment")]);
        let keys = scripted(frame(&payload)).0.list_identities().unwrap();
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].comment().as_bytes(), binary_comment);
        assert_eq!(keys[1].comment().as_bytes(), b"text comment");
    }

    #[test]
    fn rejects_invalid_frames_and_agent_refusal() {
        for (bytes, expected) in [
            (vec![0, 0], "response length"),
            (vec![0, 0, 0, 0], "invalid ssh-agent response length"),
            (((MAX_PACKET + 1) as u32).to_be_bytes().to_vec(), "invalid ssh-agent response length"),
            (vec![0, 0, 0, 2, IDENTITIES_ANSWER], "reading ssh-agent response"),
            (frame(&[FAILURE]), "refused"),
            (frame(&[FAILURE, 0]), "unexpected"),
            (frame(&[SIGN_RESPONSE]), "unexpected"),
        ] {
            let error = scripted(bytes).0.list_identities().unwrap_err().to_string();
            assert!(error.contains(expected), "{error:?} did not contain {expected:?}");
        }
    }

    #[test]
    fn rejects_invalid_identities_and_trailing_data() {
        let key = ed25519_key().to_bytes().unwrap();
        let mut trailing_key = key.clone();
        trailing_key.push(0);
        let mut trailing_response = identities(&[]);
        trailing_response.push(0);
        let mut too_many = vec![IDENTITIES_ANSWER];
        too_many.extend_from_slice(&(MAX_IDENTITIES + 1).to_be_bytes());
        let mut invalid_certificate = Vec::new();
        put_string(&mut invalid_certificate, b"ssh-ed25519-cert-v01@openssh.com").unwrap();
        let mut invalid_algorithm = Vec::new();
        put_string(&mut invalid_algorithm, &[0xff]).unwrap();
        put_string(&mut invalid_algorithm, &[7; 32]).unwrap();
        for payload in [
            trailing_response,
            too_many,
            identities(&[(&invalid_algorithm, b"")]),
            identities(&[(&trailing_key, b"")]),
            identities(&[(&invalid_certificate, b"")]),
            identities(&[(&[0, 0, 0, 99], b"")]),
        ] {
            assert!(scripted(frame(&payload)).0.list_identities().is_err());
        }
    }

    #[test]
    fn validates_and_skips_certificates_while_retaining_plain_keys() {
        use ssh_key::{certificate, getrandom::SysRng, rand_core::UnwrapErr, PrivateKey};
        let signer = PrivateKey::random(&mut UnwrapErr(SysRng), Algorithm::Ed25519).unwrap();
        let mut builder =
            certificate::Builder::new(vec![1; 16], signer.public_key().key_data().clone(), 0, u64::MAX).unwrap();
        builder.all_principals_valid().unwrap();
        let cert = builder.sign(&signer).unwrap().to_bytes().unwrap();
        let key = signer.public_key().to_bytes().unwrap();
        let payload = identities(&[(&cert, b"certificate"), (&key, b"plain key")]);
        let keys = scripted(frame(&payload)).0.list_identities().unwrap();
        assert_eq!(keys.len(), 1);
        assert_eq!(keys[0].key_data(), signer.public_key().key_data());
    }

    #[test]
    fn signs_with_exact_key_data_and_no_flags_for_ed25519() {
        let key = ed25519_key();
        let signature = Signature::new(Algorithm::Ed25519, vec![8; 64]).unwrap();
        let (mut client, output) = scripted(signature_response(signature.clone()));
        assert_eq!(client.sign(&key, b"message").unwrap(), signature);
        let output = output.borrow();
        let mut reader = Fields(&output);
        assert_eq!(reader.u32().unwrap() as usize, output.len() - 4);
        assert_eq!(reader.take(1).unwrap(), &[SIGN_REQUEST]);
        assert_eq!(reader.string().unwrap(), key.to_bytes().unwrap());
        assert_eq!(reader.string().unwrap(), b"message");
        assert_eq!(reader.u32().unwrap(), 0);
        reader.finish().unwrap();
    }

    #[test]
    fn rsa_requests_sha512_and_rejects_a_downgrade() {
        let mut key_blob = Vec::new();
        put_string(&mut key_blob, b"ssh-rsa").unwrap();
        put_string(&mut key_blob, &[1, 0, 1]).unwrap();
        put_string(&mut key_blob, &[0x11; 256]).unwrap();
        let key = PublicKey::from_bytes(&key_blob).unwrap();
        for hash in [None, Some(HashAlg::Sha256), Some(HashAlg::Sha512)] {
            let signature = Signature::new(Algorithm::Rsa { hash }, vec![8; 256]).unwrap();
            let (mut client, output) = scripted(signature_response(signature));
            assert_eq!(client.sign(&key, b"message").is_ok(), hash == Some(HashAlg::Sha512));
            let output = output.borrow();
            assert_eq!(&output[output.len() - 4..], &RSA_SHA2_512.to_be_bytes());
        }
    }

    #[test]
    fn signatures_preserve_security_key_trailers_and_reject_trailing_bytes() {
        let mut sk_ed25519 = vec![8; 64];
        sk_ed25519.extend_from_slice(&[1, 0, 0, 0, 42]);
        let mut sk_ecdsa = Vec::new();
        put_string(&mut sk_ecdsa, &[1]).unwrap();
        put_string(&mut sk_ecdsa, &[2]).unwrap();
        sk_ecdsa.extend_from_slice(&[1, 0, 0, 0, 42]);
        for signature in [
            Signature::new(Algorithm::Ed25519, vec![8; 64]).unwrap(),
            Signature::new(Algorithm::SkEd25519, sk_ed25519).unwrap(),
            Signature::new(Algorithm::SkEcdsaSha2NistP256, sk_ecdsa).unwrap(),
        ] {
            let key = ed25519_key(); // Cryptographic verification is performed by the caller.
            assert_eq!(scripted(signature_response(signature.clone())).0.sign(&key, b"x").unwrap(), signature);
            let mut blob = Vec::<u8>::try_from(signature).unwrap();
            blob.push(0);
            let mut payload = vec![SIGN_RESPONSE];
            put_string(&mut payload, &blob).unwrap();
            assert!(scripted(frame(&payload)).0.sign(&key, b"x").is_err());
        }
        let mut payload = vec![SIGN_RESPONSE];
        put_string(&mut payload, &[0, 0, 0, 99]).unwrap();
        assert!(scripted(frame(&payload)).0.sign(&ed25519_key(), b"x").is_err());
        let mut response = signature_response(Signature::new(Algorithm::Ed25519, vec![8; 64]).unwrap());
        response.push(0);
        let length = (response.len() - 4) as u32;
        response[..4].copy_from_slice(&length.to_be_bytes());
        assert!(scripted(response).0.sign(&ed25519_key(), b"x").is_err());
    }

    #[test]
    fn rejects_oversized_requests_before_writing() {
        let (mut client, output) = scripted(Vec::new());
        assert!(client.sign(&ed25519_key(), &vec![0; MAX_PACKET]).is_err());
        assert!(output.borrow().is_empty());
    }

    #[test]
    fn deadline_bounds_pending_and_slow_partial_responses() {
        for pending in [true, false] {
            let stream = Scripted {
                input: Cursor::new(frame(&identities(&[]))),
                output: Rc::new(RefCell::new(Vec::new())),
                read_delay: Duration::from_millis(10),
                pending,
            };
            let mut client = Client { stream: Box::new(stream), timeout: Duration::from_millis(20) };
            let error = client.list_identities().unwrap_err();
            assert_eq!(error.downcast_ref::<io::Error>().unwrap().kind(), io::ErrorKind::TimedOut);
        }
    }

    #[cfg(windows)]
    fn named_pipe_test(disconnect: bool) {
        use interprocess::os::windows::named_pipe::{pipe_mode, PipeListenerOptions};
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::mpsc;
        static NEXT_PIPE: AtomicUsize = AtomicUsize::new(0);
        let path =
            format!(r"\\.\pipe\adage-sign-test-{}-{}", std::process::id(), NEXT_PIPE.fetch_add(1, Ordering::Relaxed));
        let listener = PipeListenerOptions::new().path(path.as_str()).create_duplex::<pipe_mode::Bytes>().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let server = std::thread::spawn(move || {
            let stream = listener.accept().unwrap();
            stream.set_nonblocking(true).unwrap();
            let mut stream = WindowsPipe(stream);
            let deadline = Instant::now() + Duration::from_secs(2);
            let mut request = [0; 5];
            read_exact(&mut stream, &mut request, deadline).unwrap();
            assert_eq!(request, [0, 0, 0, 1, REQUEST_IDENTITIES]);
            if !disconnect {
                std::thread::sleep(Duration::from_millis(50));
                write_all(&mut stream, &frame(&identities(&[])), deadline).unwrap();
                // Keep the pipe open until the client has consumed its response.
                done_rx.recv_timeout(Duration::from_secs(2)).unwrap();
            }
        });
        let mut client = Client::connect(Path::new(&path)).unwrap();
        client.timeout = if disconnect { Duration::from_millis(100) } else { Duration::from_secs(1) };
        let result = client.list_identities();
        if disconnect {
            assert_eq!(result.unwrap_err().downcast_ref::<io::Error>().unwrap().kind(), io::ErrorKind::TimedOut);
        } else {
            assert!(result.unwrap().is_empty());
            done_tx.send(()).unwrap();
        }
        server.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_waits_for_delayed_agent_response() {
        named_pipe_test(false);
    }

    #[cfg(windows)]
    #[test]
    fn named_pipe_disconnect_has_a_deadline() {
        named_pipe_test(true);
    }
}
