//! `adage map`, through the binary: the map page, its API and headers, from an
//! isolated `ADAGE_HOME`, on a free port on 127.0.0.1.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

mod common;

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
    raw_request(addr, &format!("GET {path} HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n"))
}

fn raw_request(addr: &str, request: &str) -> (String, String) {
    let mut s = TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    // One write for the whole request, as a browser sends it.
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    s.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    (head.to_string(), body.to_string())
}

#[test]
fn serves_the_map_and_its_api_from_the_binary() {
    let d = tempfile::tempdir().unwrap();
    let mut child = common::scrub(&mut Command::new(env!("CARGO_BIN_EXE_adage")))
        .args(["map", "--port", "0", "--no-open"])
        // This empty-home fixture must not inherit the developer checkout's repository identity.
        .current_dir(d.path())
        .env("ADAGE_HOME", d.path().join("data"))
        .env("HOME", d.path())
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

    // The page: the web app, readable by a client that asks for no compression.
    let (head, body) = get(&addr, "/", &addr);
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    assert!(head.contains("text/html") && head.contains("Cache-Control: no-cache"), "{head}");
    assert!(!head.contains("Content-Encoding"), "no Accept-Encoding, no compression: {head}");
    assert!(body.starts_with("<!doctype html>") && body.contains("<title>Adage map</title>"), "{body}");
    // Its own files only: no inline script, no font CDN.
    let csp = head.lines().find(|l| l.starts_with("Content-Security-Policy:")).expect("a CSP");
    assert!(
        csp.contains("script-src 'self'") && !csp.contains("unsafe-inline") && !csp.contains("googleapis"),
        "{csp}"
    );

    for path in ["/api/v1/graph", "/graph.json"] {
        let (head, body) = get(&addr, path, &addr);
        assert!(head.starts_with("HTTP/1.1 200") && head.contains("application/json"), "{path}: {head}");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["sessions"]["total"], 0);
        assert!(v["routes"].as_array().unwrap().is_empty());
        // Nothing to estimate yet, and rewrites to the project's environment are on by default.
        assert_eq!(v["savings"], serde_json::json!([]));
        assert_eq!(v["pretool"], serde_json::json!([]));
        assert_eq!(v["runtime"]["rewrite"]["on"], true);
    }
    let (head, body) = get(&addr, "/api/v1/health", &addr);
    assert!(head.starts_with("HTTP/1.1 200"), "{head}");
    let h: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(h["checks"].is_array(), "{body}");
    // Beside the checks: the rewrite switch and what intent reflexes embed with.
    assert_eq!(h["runtime"]["rewrite"]["on"], true, "{body}");
    assert!(h["runtime"]["embeddings"]["model"].as_str().is_some_and(|m| !m.is_empty()), "{body}");

    let (_, access) = get(&addr, "/api/v1/review", &addr);
    let access: serde_json::Value = serde_json::from_str(&access).unwrap();
    assert_eq!(access["enabled"], false, "noninteractive startup remains read-only");
    assert!(access["token"].is_null());
    let (head, _) = get(&addr, "/graph.json", "attacker.example");
    assert!(head.starts_with("HTTP/1.1 403"), "other hosts are refused: {head}");
    let (head, _) = get(&addr, "/../etc/passwd", &addr);
    assert!(head.starts_with("HTTP/1.1 404"), "{head}");

    // Neither a duplicate header nor bytes after the header boundary can replace the checked host.
    let (head, _) =
        raw_request(&addr, &format!("GET /graph.json HTTP/1.1\r\nHost: attacker.example\r\nHost: {addr}\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 400"), "duplicate hosts are malformed: {head}");
    let (head, _) =
        raw_request(&addr, &format!("GET /graph.json HTTP/1.1\r\nHost: attacker.example\r\n\r\nHost: {addr}\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 403"), "body bytes are never parsed as headers: {head}");
    let (head, _) = raw_request(&addr, &format!("GET / HTTP/1.1 extra\r\nHost: {addr}\r\n\r\n"));
    assert!(head.starts_with("HTTP/1.1 400"), "malformed request line: {head}");
    let prefix = format!("GET / HTTP/1.1\r\nHost: {addr}\r\nX-Padding: ");
    let (head, _) = raw_request(&addr, &format!("{prefix}{}", "a".repeat(8192 - prefix.len())));
    assert!(head.starts_with("HTTP/1.1 431"), "an unterminated header at the limit is refused: {head}");

    // HEAD preserves the representation metadata without sending a body or opening a live stream.
    for path in ["/", "/api/v1/graph", "/api/v1/events"] {
        let (head, body) = raw_request(&addr, &format!("HEAD {path} HTTP/1.1\r\nHost: {addr}\r\n\r\n"));
        assert!(head.starts_with("HTTP/1.1 200"), "{path}: {head}");
        assert!(body.is_empty(), "HEAD {path} must not return a body");
    }
    assert!(!d.path().join("data").exists(), "serving is read-only");
}

#[test]
fn keeps_serving_after_its_stdout_is_closed() {
    let d = tempfile::tempdir().unwrap();
    let mut child = common::scrub(&mut Command::new(env!("CARGO_BIN_EXE_adage")))
        .args(["map", "--port", "0", "--no-open"])
        // This empty-home fixture must not inherit the developer checkout's repository identity.
        .current_dir(d.path())
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

fn start_review_server(root: &std::path::Path, agent: bool) -> (Server, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_adage"));
    common::scrub(&mut cmd);
    cmd.args(["map", "--port", "0", "--no-open"])
        .current_dir(root)
        .env("ADAGE_HOME", root.join("data"))
        .env("HOME", root)
        // Test-only lift of the terminal check; agent markers must still refuse review.
        .env("ADAGE_ALLOW_NONINTERACTIVE", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if agent {
        cmd.env("CLAUDECODE", "1");
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut first = String::new();
    stdout.read_line(&mut first).unwrap();
    let addr = first.split("http://").nth(1).and_then(|s| s.split('/').next()).expect(&first).to_string();
    (Server(child, Some(stdout)), addr)
}

fn request(addr: &str, method: &str, host: &str, headers: &str, body: &str) -> (String, String) {
    exchange(addr, &format!("{method} /api/v1/review HTTP/1.1\r\nHost: {host}\r\nContent-Length: {}\r\n{headers}Connection: close\r\n\r\n{body}", body.len()))
}

fn exchange(addr: &str, request: &str) -> (String, String) {
    let mut stream = TcpStream::connect(addr).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    stream.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    stream.read_to_string(&mut out).unwrap();
    let (head, body) = out.split_once("\r\n\r\n").unwrap();
    let length: usize = head.lines().find_map(|line| line.strip_prefix("Content-Length: ")).unwrap().parse().unwrap();
    assert_eq!(body.len(), length);
    (head.into(), body.into())
}

#[test]
fn review_requires_local_capability_and_the_exact_inspected_revision() {
    use adage_tracks::{testkit, RouteState, Step, Strength, Trigger};
    use serde_json::{json, Value};
    let root = tempfile::tempdir().unwrap();
    let home = testkit::LearnedHome::at(root.path().join("data"));
    // Zero evidence is deliberate: accepting an unchanged 0/0 reflex must remain accepted.
    let route = testkit::route("verify", Trigger::SessionStart, Step::Run { command: "make test".into() })
        .state(RouteState::Nascent)
        .strength(Strength::Prime)
        .support(0, 0)
        .build();
    let track = testkit::track("acme/checks", "acme").routes(vec![route]).build();
    home.install("acme/checks", &track);
    home.recompile();
    let (_server, addr) = start_review_server(root.path(), false);
    let (head, body) = get(&addr, "/api/v1/review", &addr);
    assert!(head.contains("Cache-Control: no-store"));
    let access: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(access["enabled"], true);
    let token = access["token"].as_str().unwrap();
    assert_eq!(token.len(), 64);
    let (_, body) = get(&addr, "/api/v1/review?reference=acme%2Fchecks&reflex=verify", &addr);
    let preview: Value = serde_json::from_str(&body).unwrap();
    assert!(preview["current_directive"].as_str().unwrap().contains("make test"));
    let mut decision =
        json!({"reference":"acme/checks", "reflex":"verify", "verb":"approve", "revision":preview["revision"]});
    let valid = format!("Origin: http://{addr}\r\nX-Adage-CSRF: {token}\r\nContent-Type: application/json\r\n");
    for headers in [
        format!("X-Adage-CSRF: {token}\r\nContent-Type: application/json\r\n"),
        format!("Origin: https://attacker.example\r\nX-Adage-CSRF: {token}\r\nContent-Type: application/json\r\n"),
        format!("Origin: http://{addr}\r\nX-Adage-CSRF: wrong\r\nContent-Type: application/json\r\n"),
        format!("Origin: http://{addr}\r\nContent-Type: application/json\r\n"),
    ] {
        assert!(request(&addr, "POST", &addr, &headers, &decision.to_string()).0.starts_with("HTTP/1.1 403"));
    }
    assert!(request(&addr, "POST", "127.0.0.1:1", &valid, &decision.to_string()).0.starts_with("HTTP/1.1 403"));
    assert!(request(&addr, "POST", &addr, &format!("{valid}Origin: http://{addr}\r\n"), &decision.to_string())
        .0
        .starts_with("HTTP/1.1 400"));
    assert!(request(&addr, "POST", &addr, &valid.replace("application/json", "text/plain"), &decision.to_string())
        .0
        .starts_with("HTTP/1.1 415"));
    assert!(request(&addr, "PUT", &addr, &valid, &decision.to_string()).0.starts_with("HTTP/1.1 405"));
    assert!(request(&addr, "POST", &addr, &valid, "{}").0.starts_with("HTTP/1.1 400"));
    // Reject an oversized declared body before reading it; no need to send unbounded bytes.
    assert!(exchange(
        &addr,
        &format!("POST /api/v1/review HTTP/1.1\r\nHost: {addr}\r\nContent-Length: 16385\r\n{valid}\r\n")
    )
    .0
    .starts_with("HTTP/1.1 400"));
    decision["verb"] = json!("grant");
    assert!(request(&addr, "POST", &addr, &valid, &decision.to_string()).0.starts_with("HTTP/1.1 400"));
    decision["verb"] = json!("approve");
    decision["revision"] = json!("0".repeat(64));
    let (head, body) = request(&addr, "POST", &addr, &valid, &decision.to_string());
    assert!(head.starts_with("HTTP/1.1 409") && body.contains("Nothing was written"), "{head} {body}");
    decision["revision"] = preview["revision"].clone();
    let (head, body) = request(&addr, "POST", &addr, &valid, &decision.to_string());
    assert!(head.starts_with("HTTP/1.1 200"), "{head} {body}");
    let saved: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(saved["saved"], true);
    assert_eq!(saved["state"], "firm", "a learned nascent reflex enters service on approval");
    assert_eq!(saved["strength"], "prime");
    assert_eq!(saved["reference"], "acme/checks");
    // A browser refresh reads persisted approval and live advice; it is not merely optimistic UI.
    let (_, body) = get(&addr, "/api/v1/graph", &addr);
    let graph: Value = serde_json::from_str(&body).unwrap();
    let route = graph["routes"].as_array().unwrap().iter().find(|r| r["id"] == "verify").unwrap();
    assert_ne!(route["version"], track.manifest.track.version, "approval bumped the manifest version");
    assert_eq!(route["reference"], saved["reference"], "installed identity survives the version bump");
    assert_eq!(route["last_review"]["decision"], "approved");
    assert_eq!(route["last_review"]["current"], true);
    assert_eq!(route["in_index"], true);
    assert_eq!(route["strength"], "prime");
    assert!(!route["review"]["reasons"].as_array().unwrap().iter().any(|r| r["kind"] == "approval"));
    // The stale confirmation cannot be replayed, and review never created execution grants.
    assert!(request(&addr, "POST", &addr, &valid, &decision.to_string()).0.starts_with("HTTP/1.1 409"));
    assert!(!home.root().join("exec").exists());
}

#[test]
fn an_agent_launched_map_keeps_the_review_api_read_only() {
    let root = tempfile::tempdir().unwrap();
    let (_server, addr) = start_review_server(root.path(), true);
    let (_, body) = get(&addr, "/api/v1/review", &addr);
    let access: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(access["enabled"], false);
    assert!(access["token"].is_null());
    let headers = format!("Origin: http://{addr}\r\nContent-Type: application/json\r\n");
    let (head, _) = request(&addr, "POST", &addr, &headers, "{}");
    assert!(head.starts_with("HTTP/1.1 403"));
    assert!(!root.path().join("data").exists());
}
