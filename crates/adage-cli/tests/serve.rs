//! `adage graph --serve`, through the binary: the signal box page and its live JSON, from an
//! isolated `ADAGE_HOME`, on a free port on 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

struct Server(Child);

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn get(addr: &str, path: &str, host: &str) -> (String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    write!(s, "GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n").unwrap();
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
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let _server = Server(child);
    let mut first = String::new();
    BufReader::new(stdout).read_line(&mut first).unwrap();
    let addr = first.split("http://").nth(1).and_then(|s| s.split('/').next()).expect(&first).to_string();
    assert!(addr.starts_with("127.0.0.1:"), "bound to loopback only: {first}");

    let (head, body) = get(&addr, "/", &addr);
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("text/html") && head.contains("Cache-Control: no-store"), "{head}");
    assert!(body.starts_with("<!doctype html>") && body.contains("name=\"viewport\""), "doctype and viewport are prepended");
    assert!(body.contains("<title>Adage Signal Box</title>") && body.contains("graph.json"));

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
