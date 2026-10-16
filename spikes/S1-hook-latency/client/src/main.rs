// S1 hook-latency spike: minimal hook client.
//
// Modes (argv[1]):
//   noop    -- do not touch stdin at all, exit(0) immediately. Measures pure
//              process spawn + exit overhead (scenario A floor).
//   stdin   -- drain stdin fully, do a cheap byte-scan for a couple of
//              "fields" (no JSON parse), exit(0). Scenarios B/C/G.
//   socket  -- drain stdin, then connect to a Unix socket (argv[2], default
//              /tmp/adage-s1.sock), send a small request, read a small
//              reply, exit(0) regardless of outcome (fail-open). Scenarios
//              D/E/F.
//
// No external crates: keeps the build fast and the binary small, and avoids
// depending on network access to crates.io on the spike machine.

use std::env;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::exit;
use std::time::Duration;

const CHUNK: usize = 64 * 1024;

/// Drain stdin to EOF without doing any structured parsing. Returns the
/// bytes read (kept only long enough to scan two marker substrings, mimicking
/// "extract a couple of JSON fields" without a real JSON deserialize).
fn drain_stdin() -> Vec<u8> {
    let mut buf = Vec::with_capacity(CHUNK);
    let mut chunk = [0u8; CHUNK];
    let mut stdin = io::stdin().lock();
    loop {
        match stdin.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(_) => break,
        }
    }
    buf
}

/// Naive substring search -- good enough at these payload sizes and avoids
/// pulling in a regex/JSON crate for a two-field scan.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    haystack.windows(needle.len()).any(|w| w == needle)
}

fn extract_fields(buf: &[u8]) -> (bool, bool) {
    let has_event = contains(buf, b"\"hook_event_name\"");
    let has_session = contains(buf, b"\"session_id\"");
    (has_event, has_session)
}

fn socket_roundtrip(sock_path: &str) {
    let stream = match UnixStream::connect(sock_path) {
        Ok(s) => s,
        Err(_) => return, // fail-open: no socket, no directive, exit 0 below
    };
    let _ = stream.set_write_timeout(Some(Duration::from_millis(100)));
    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
    let mut stream = stream;
    if stream.write_all(b"{\"t0\":\"hit\"}\n").is_err() {
        return;
    }
    let mut reply = [0u8; 256];
    if let Ok(n) = stream.read(&mut reply) {
        if n > 0 {
            let _ = io::stdout().write_all(&reply[..n]);
        }
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map(|s| s.as_str()).unwrap_or("stdin");

    match mode {
        "noop" => {
            exit(0);
        }
        "stdin" => {
            let buf = drain_stdin();
            let _ = extract_fields(&buf);
            exit(0);
        }
        "socket" => {
            let buf = drain_stdin();
            let _ = extract_fields(&buf);
            let sock_path = args
                .get(2)
                .map(|s| s.as_str())
                .unwrap_or("/tmp/adage-s1.sock");
            socket_roundtrip(sock_path);
            exit(0);
        }
        _ => {
            exit(0);
        }
    }
}
