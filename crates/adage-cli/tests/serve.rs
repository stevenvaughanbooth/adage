//! `adage graph --serve`, through the binary: the map page and its live JSON, from an
//! isolated `ADAGE_HOME`, on a free port on 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

/// The server process, and its stdout kept open for its lifetime (a closed pipe must not matter,
/// but the test should not depend on that).
struct Server(Child, #[allow(dead_code)] Option<BufReader<std::process::ChildStdout>>);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(addr: &str, path: &str, host: &str) -> (String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    // One write for the whole request, as a browser sends it.
    s.write_all(format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    (head.to_string(), body.to_string())
}

#[test]
fn serves_the_signal_box_and_live_json_from_the_binary() {
    let d = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["graph", "--serve", "--port", "0"])
        .env("ADAGE_HOME", d.path().join("data"))
        .env("HOME", d.path())
        .env_remove("CLAUDECODE")
        .env_remove("OPENCODE")
        .env_remove("CODEX_SANDBOX")
        .env_remove("CODEX_SANDBOX_NETWORK_DISABLED")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    stdout.read_line(&mut first).unwrap();
    let _server = Server(child, Some(stdout));
    let addr = first.split("http://").nth(1).and_then(|s| s.split('/').next()).expect(&first).to_string();
    assert!(addr.starts_with("127.0.0.1:"), "bound to loopback only: {first}");

    let (head, body) = get(&addr, "/", &addr);
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("text/html") && head.contains("Cache-Control: no-store"), "{head}");
    assert!(
        body.starts_with("<!doctype html>") && body.contains("name=\"viewport\""),
        "the standalone page includes its doctype and viewport"
    );
    assert_eq!(body.matches("<!doctype html>").count(), 1, "the server must not duplicate the document wrapper");
    assert!(body.contains("<title>Adage · The map</title>") && body.contains("graph.json"));

    let (head, body) = get(&addr, "/graph.json", &addr);
    assert!(head.starts_with("HTTP/1.1 200") && head.contains("application/json"), "{head}");
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["sessions"]["total"], 0);
    assert!(v["routes"].as_array().unwrap().is_empty());

    let (head, _) = get(&addr, "/graph.json", "attacker.example");
    assert!(head.starts_with("HTTP/1.1 403"), "other hosts are refused: {head}");
    let (head, _) = get(&addr, "/../etc/passwd", &addr);
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");
    assert!(!d.path().join("data").exists(), "serving is read-only");
}

#[test]
fn keeps_serving_after_its_stdout_is_closed() {
    let d = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["graph", "--serve", "--port", "0"])
        .env("ADAGE_HOME", d.path().join("data"))
        .env("HOME", d.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut first = String::new();
    BufReader::new(child.stdout.take().unwrap()).read_line(&mut first).unwrap(); // pipe closed here
    let _server = Server(child, None);
    let addr = first.split("http://").nth(1).and_then(|s| s.split('/').next()).expect(&first).to_string();
    std::thread::sleep(Duration::from_millis(200)); // let the banner's second line hit the closed pipe
    let (head, _) = get(&addr, "/graph.json", &addr);
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
}

#[test]
#[cfg(unix)]
fn serves_an_isolated_demo_and_cleans_it_up_on_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    let real_home = dir.path().join("real-data");
    let mut child = Command::new(env!("CARGO_BIN_EXE_adage"))
        .args(["graph", "--serve", "--demo", "--port", "0"])
        .env("ADAGE_HOME", &real_home)
        .env("HOME", dir.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    stdout.read_line(&mut first).unwrap();
    let addr = first.split("http://").nth(1).and_then(|text| text.split('/').next()).expect(&first).to_string();
    let mut server = Server(child, Some(stdout));
    let (head, body) = get(&addr, "/graph.json", &addr);
    assert!(head.starts_with("HTTP/1.1 200"));
    let graph: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(graph["source"]["kind"], "demo");
    assert_eq!(graph["trace_sessions"].as_array().unwrap().len(), 3);
    assert!(!graph["trace_events"].as_array().unwrap().is_empty());
    let demo_home = std::path::PathBuf::from(graph["home"].as_str().unwrap());
    assert!(demo_home.exists());
    assert!(!real_home.exists());
    assert_eq!(unsafe { libc::kill(server.0.id() as libc::pid_t, libc::SIGINT) }, 0);
    let until = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = server.0.try_wait().unwrap() {
            assert!(status.success());
            break;
        }
        assert!(std::time::Instant::now() < until, "demo server did not stop on SIGINT");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!demo_home.exists(), "demo state is removed on clean shutdown");
    assert!(!real_home.exists(), "the user's requested data directory remains untouched");
}
