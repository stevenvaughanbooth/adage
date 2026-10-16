// S1 hook-latency spike: trivial echo server standing in for `adaged`.
//
// Binds a Unix socket at argv[1] (default /tmp/adage-s1.sock), removing any
// stale socket file first, then accepts connections in a loop, spawning one
// thread per connection. Each connection: read until a newline (or EOF),
// write a fixed small reply, close. This is deliberately as cheap as
// possible -- the point of scenario D is to measure the client + transport
// cost with the server warm and doing near-zero work.

use std::env;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::thread;

const REPLY: &[u8] = b"{\"directive\":null}\n";

fn handle(mut stream: UnixStream) {
    let mut buf = [0u8; 4096];
    // Read whatever the client sends (small request); ignore content.
    let _ = stream.read(&mut buf);
    let _ = stream.write_all(REPLY);
    let _ = stream.flush();
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let sock_path = args
        .get(1)
        .map(|s| s.as_str())
        .unwrap_or("/tmp/adage-s1.sock")
        .to_string();

    let _ = fs::remove_file(&sock_path);
    let listener = UnixListener::bind(&sock_path).expect("bind unix socket");
    // 0600: only current user can connect, matches the real design intent.
    let _ = fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o600));

    eprintln!("echoserver: listening on {sock_path}");

    for conn in listener.incoming() {
        match conn {
            Ok(stream) => {
                thread::spawn(move || handle(stream));
            }
            Err(_) => continue,
        }
    }
}
