//! `adage graph --serve [--port N]`: the signal box, served by the binary itself (plan §6.3).
//!
//! `/` is the page (`assets/brain.html`, embedded at build time) and `/graph.json` is the live
//! graph, recomputed per request exactly as `adage graph --json --events 400` prints it. Standard
//! library only: one thread, one connection at a time, short timeouts. Bound to 127.0.0.1 only,
//! GET and HEAD only, and read-only. Requests whose `Host` is not this address are refused, so a
//! web page elsewhere cannot read the graph through DNS rebinding. `--port 0` picks a free port.

use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::Duration;

const PAGE: &str = include_str!("../assets/brain.html");
/// What `demo/dashboard/serve.py` prepends: the page file is a fragment.
const PAGE_HEAD: &str = "<!doctype html>\n<html lang=\"en\"><meta charset=\"utf-8\">\n<meta name=\"viewport\" content=\"width=device-width, initial-scale=1, viewport-fit=cover\">\n";
const DEFAULT_PORT: u16 = 8765;
const MAX_HEAD: usize = 8 << 10;
const EVENTS: usize = 400;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// The page's own inline script and styles, its fonts, and nothing else.
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline' https://fonts.googleapis.com; \
                   font-src https://fonts.gstatic.com; connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

pub fn serve(args: &[String], paths: &Paths) -> Result<()> {
    let port: u16 = match crate::flag(args, "--port") {
        Some(p) => p.parse().with_context(|| format!("--port {p:?} is not a port number"))?,
        None if args.iter().any(|a| a == "--port") => bail!("usage: adage graph --serve [--port N]"),
        None => DEFAULT_PORT,
    };
    let listener = TcpListener::bind(("127.0.0.1", port)).with_context(|| format!("cannot listen on 127.0.0.1:{port} (try --port N, or --port 0 for any free port)"))?;
    let addr = listener.local_addr()?;
    println!("adage graph: the signal box at http://{addr}/  (live JSON at http://{addr}/graph.json)");
    println!("Read-only, on this machine only (ADAGE_HOME={}). Ctrl-C stops it.", paths.root.display());
    std::io::stdout().flush()?;
    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        // One bad client must never stop the server.
        let _ = handle(stream, paths, addr.port());
    }
    Ok(())
}

struct Response {
    status: &'static str,
    content_type: &'static str,
    body: Vec<u8>,
    extra: &'static str,
}

fn text(status: &'static str, body: &str) -> Response {
    Response { status, content_type: "text/plain; charset=utf-8", body: format!("{body}\n").into_bytes(), extra: "" }
}

/// The request head (request line and headers), or `None` if it is too large, malformed, or slow.
fn read_head(s: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        if buf.len() > MAX_HEAD {
            return None;
        }
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    String::from_utf8(buf).ok()
}

fn route(head: &str, paths: &Paths, port: u16) -> (Response, bool) {
    let mut lines = head.split("\r\n");
    let mut first = lines.next().unwrap_or("").split(' ');
    let (method, target) = (first.next().unwrap_or(""), first.next().unwrap_or(""));
    let head_only = method == "HEAD";
    let host = lines
        .filter_map(|l| l.split_once(':'))
        .find(|(k, _)| k.trim().eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.trim().to_ascii_lowercase());
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !host.as_ref().is_some_and(|h| allowed.contains(h)) {
        return (text("403 Forbidden", "adage graph --serve answers only requests for 127.0.0.1 or localhost"), head_only);
    }
    if method != "GET" && !head_only {
        return (Response { extra: "Allow: GET, HEAD\r\n", ..text("405 Method Not Allowed", "read-only: GET and HEAD only") }, false);
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    let res = match path {
        "/" | "/index.html" | "/brain" | "/brain.html" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: format!("{PAGE_HEAD}{PAGE}").into_bytes(),
            extra: "",
        },
        "/graph.json" => match crate::graph::build(paths, EVENTS) {
            Ok(v) => Response { status: "200 OK", content_type: "application/json", body: v.to_string().into_bytes(), extra: "" },
            // The page shows "offline" and retries.
            Err(e) => text("503 Service Unavailable", &format!("{e:#}")),
        },
        _ => text("404 Not Found", "not found"),
    };
    (res, head_only)
}

fn handle(mut s: TcpStream, paths: &Paths, port: u16) -> std::io::Result<()> {
    s.set_read_timeout(Some(READ_TIMEOUT))?;
    s.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let (res, head_only) = match read_head(&mut s) {
        Some(head) => route(&head, paths, port),
        None => (text("400 Bad Request", "bad request"), false),
    };
    let head = format!(
        "HTTP/1.1 {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\n\
         Referrer-Policy: no-referrer\r\nContent-Security-Policy: {CSP}\r\n{}Connection: close\r\n\r\n",
        res.status,
        res.content_type,
        res.body.len(),
        res.extra
    );
    s.write_all(head.as_bytes())?;
    if !head_only {
        s.write_all(&res.body)?;
    }
    s.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_and_refusals() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let req = |r: &str| route(r, &paths, 8765).0;
        assert_eq!(req("GET / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n").status, "200 OK");
        assert!(String::from_utf8(req("GET / HTTP/1.1\r\nHost: localhost:8765\r\n\r\n").body).unwrap().starts_with("<!doctype html>"));
        let g = req("GET /graph.json?t=1 HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n");
        assert_eq!((g.status, g.content_type), ("200 OK", "application/json"));
        assert_eq!(req("GET /x HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n").status, "404 Not Found");
        assert_eq!(req("POST / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n").status, "405 Method Not Allowed");
        assert_eq!(req("GET / HTTP/1.1\r\nHost: evil.example:8765\r\n\r\n").status, "403 Forbidden");
        assert_eq!(req("GET / HTTP/1.1\r\n\r\n").status, "403 Forbidden");
        let (_, head_only) = route("HEAD / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n", &paths, 8765);
        assert!(head_only);
    }
}
