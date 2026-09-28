//! Requests and responses for `adage map`: just enough HTTP/1.1 for a browser on this machine.
//! One request per connection (`Connection: close`), GET and HEAD only.

use super::assets::{self, Asset};
use super::events::Next;
use super::Shared;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

const MAX_HEAD: usize = 8 << 10;
const EVENTS: usize = 400;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// How long an idle event stream waits before a keep-alive comment.
const KEEP_ALIVE: Duration = Duration::from_secs(15);
/// How often an idle event stream checks that its page is still there. A closed page's stream
/// would otherwise hold its connection until the next write fails, up to [`KEEP_ALIVE`] later.
const LIVENESS: Duration = Duration::from_secs(1);

/// This origin's own files and nothing else: no inline script or style element, no font CDN.
/// Styles set from script (the graph's positions) are not governed by `style-src`.
pub(crate) const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self'; font-src 'self'; \
                              img-src 'self' data:; connect-src 'self'; worker-src 'self'; manifest-src 'self'; \
                              base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

#[derive(Debug, Default)]
pub(crate) struct Request {
    pub(crate) method: String,
    pub(crate) path: String,
    pub(crate) host: Option<String>,
    pub(crate) accept_encoding: Option<String>,
    pub(crate) last_event_id: Option<u64>,
}

impl Request {
    pub(crate) fn parse(head: &str) -> Request {
        let mut lines = head.split("\r\n");
        let mut first = lines.next().unwrap_or("").split(' ');
        let method = first.next().unwrap_or("").to_string();
        let target = first.next().unwrap_or("");
        let path = target.split(['?', '#']).next().unwrap_or("").to_string();
        let mut r = Request { method, path, ..Default::default() };
        for (k, v) in lines.filter_map(|l| l.split_once(':')) {
            let v = v.trim();
            match k.trim().to_ascii_lowercase().as_str() {
                "host" => r.host = Some(v.to_ascii_lowercase()),
                "accept-encoding" => r.accept_encoding = Some(v.to_ascii_lowercase()),
                "last-event-id" => r.last_event_id = v.parse().ok(),
                _ => {}
            }
        }
        r
    }

    fn head_only(&self) -> bool {
        self.method == "HEAD"
    }
}

pub(crate) struct Response {
    pub(crate) status: &'static str,
    pub(crate) content_type: &'static str,
    pub(crate) headers: Vec<(&'static str, String)>,
    pub(crate) body: Cow<'static, [u8]>,
}

impl Response {
    fn new(status: &'static str, content_type: &'static str, body: impl Into<Cow<'static, [u8]>>) -> Response {
        Response { status, content_type, headers: vec![], body: body.into() }
    }

    fn header(mut self, k: &'static str, v: impl Into<String>) -> Response {
        self.headers.push((k, v.into()));
        self
    }

    #[cfg(test)]
    pub(crate) fn get(&self, k: &str) -> Option<&str> {
        self.headers.iter().find(|(h, _)| h.eq_ignore_ascii_case(k)).map(|(_, v)| v.as_str())
    }
}

fn text(status: &'static str, body: &str) -> Response {
    Response::new(status, "text/plain; charset=utf-8", format!("{body}\n").into_bytes())
        .header("Cache-Control", "no-store")
}

fn json_response(v: &Value) -> Response {
    Response::new("200 OK", "application/json", v.to_string().into_bytes()).header("Cache-Control", "no-store")
}

/// What a request gets: a whole response, or the event stream.
pub(crate) enum Reply {
    Full(Response),
    Events,
}

/// The encodings a client accepts, by RFC 9110 §12.5.3: without the header, any; `q=0` refuses.
fn accepts(header: Option<&str>, coding: &str) -> bool {
    let Some(h) = header else { return true };
    let mut star = None;
    for part in h.split(',') {
        let mut it = part.trim().split(';');
        let name = it.next().unwrap_or("").trim();
        let q = it.find_map(|p| p.trim().strip_prefix("q=").and_then(|q| q.trim().parse::<f32>().ok())).unwrap_or(1.0);
        if name == coding {
            return q > 0.0;
        }
        if name == "*" {
            star = Some(q > 0.0);
        }
    }
    // Identity is acceptable unless refused outright.
    star.unwrap_or(coding == "identity")
}

/// The best representation of an asset for this client: brotli, then gzip, then the file as is.
/// A client that sends no `Accept-Encoding` (curl, scripts) gets the file as is when there is
/// one, else gzip: RFC 9110 allows any coding then, but such clients rarely decode brotli.
fn asset_response(a: &Asset, accept: Option<&str>) -> Response {
    if accept.is_none() {
        let (body, encoding) = match (a.raw, a.gz) {
            (Some(raw), _) => (raw, None),
            (None, Some(gz)) => (gz, Some("gzip")),
            (None, None) => (a.br.unwrap_or_default(), Some("br")),
        };
        let cache = if a.immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
        let mut r = Response::new("200 OK", a.content_type, body)
            .header("Cache-Control", cache)
            .header("Vary", "Accept-Encoding");
        if let Some(e) = encoding {
            r = r.header("Content-Encoding", e);
        }
        return r;
    }
    let pick: Option<(&'static [u8], Option<&str>)> = if let (Some(br), true) = (a.br, accepts(accept, "br")) {
        Some((br, Some("br")))
    } else if let (Some(gz), true) = (a.gz, accepts(accept, "gzip")) {
        Some((gz, Some("gzip")))
    } else if let (Some(raw), true) = (a.raw, accepts(accept, "identity")) {
        Some((raw, None))
    } else {
        None
    };
    let Some((body, encoding)) = pick else {
        return text("406 Not Acceptable", "this file is served compressed only (gzip or br)")
            .header("Vary", "Accept-Encoding");
    };
    let cache = if a.immutable { "public, max-age=31536000, immutable" } else { "no-cache" };
    let mut r = Response::new("200 OK", a.content_type, body).header("Cache-Control", cache);
    if a.br.is_some() || a.gz.is_some() {
        r = r.header("Vary", "Accept-Encoding");
    }
    if let Some(e) = encoding {
        r = r.header("Content-Encoding", e);
    }
    r
}

pub(crate) fn route(req: &Request, shared: &Shared) -> Reply {
    let port = shared.port;
    let allowed = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    if !req.host.as_ref().is_some_and(|h| allowed.contains(h)) {
        return Reply::Full(text("403 Forbidden", "adage map answers only requests for 127.0.0.1 or localhost"));
    }
    if req.method != "GET" && !req.head_only() {
        return Reply::Full(
            text("405 Method Not Allowed", "read-only: GET and HEAD only").header("Allow", "GET, HEAD"),
        );
    }
    let accept = req.accept_encoding.as_deref();
    let path = req.path.as_str();
    let res = match path {
        "/api/v1/events" => return Reply::Events,
        "/api/v1/graph" | "/graph.json" => match crate::graph::build(&shared.paths, EVENTS) {
            Ok(v) => json_response(&v),
            // The page shows "offline" and retries.
            Err(e) => text("503 Service Unavailable", &format!("{e:#}")),
        },
        "/api/v1/health" => {
            let checks = crate::doctor::checks(&shared.paths, shared.repo_root.as_deref());
            let count = |s: &str| checks.iter().filter(|c| json!(c.status) == s).count();
            json_response(&json!({
                "checks": checks,
                "summary": { "pass": count("pass"), "warn": count("warn"), "fail": count("fail") },
            }))
        }
        p if p.starts_with("/api/v1/reflex/") => {
            let id = decode_component(&p["/api/v1/reflex/".len()..]);
            match crate::graph::build(&shared.paths, 0) {
                Ok(g) => {
                    let found: Vec<Value> = g["routes"]
                        .as_array()
                        .into_iter()
                        .flatten()
                        .filter(|r| r["id"].as_str() == Some(id.as_str()))
                        .cloned()
                        .collect();
                    if found.is_empty() {
                        text("404 Not Found", "no reflex with that id")
                    } else {
                        json_response(&json!({ "reflexes": found }))
                    }
                }
                Err(e) => text("503 Service Unavailable", &format!("{e:#}")),
            }
        }
        "/" | "/index.html" => match assets::find("index.html") {
            Some(a) => asset_response(a, accept),
            None => text("404 Not Found", "not found"),
        },
        p => match p.strip_prefix('/').and_then(assets::find) {
            Some(a) => asset_response(a, accept),
            None => text("404 Not Found", "not found"),
        },
    };
    Reply::Full(res)
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

fn common_headers(content_type: &str) -> String {
    format!(
        "Content-Type: {content_type}\r\nX-Content-Type-Options: nosniff\r\nReferrer-Policy: no-referrer\r\n\
         Cross-Origin-Opener-Policy: same-origin\r\nCross-Origin-Resource-Policy: same-origin\r\n\
         Content-Security-Policy: {CSP}\r\n"
    )
}

fn write(s: &mut TcpStream, res: &Response, head_only: bool) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {}\r\n{}Content-Length: {}\r\n",
        res.status,
        common_headers(res.content_type),
        res.body.len()
    );
    for (k, v) in &res.headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("Connection: close\r\n\r\n");
    s.write_all(head.as_bytes())?;
    if !head_only {
        s.write_all(&res.body)?;
    }
    s.flush()
}

/// Too many connections: answered at once, on the accepting thread.
pub(crate) fn busy(mut s: TcpStream) {
    let _ = s.set_write_timeout(Some(Duration::from_secs(1)));
    let res = text("503 Service Unavailable", "adage map is serving too many connections").header("Retry-After", "2");
    let _ = write(&mut s, &res, false);
}

pub(crate) fn handle(mut s: TcpStream, shared: &Shared) -> std::io::Result<()> {
    s.set_read_timeout(Some(READ_TIMEOUT))?;
    s.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let Some(head) = read_head(&mut s) else {
        return write(&mut s, &text("400 Bad Request", "bad request"), false);
    };
    let req = Request::parse(&head);
    match route(&req, shared) {
        Reply::Full(res) => write(&mut s, &res, req.head_only()),
        Reply::Events => stream(s, &req, shared),
    }
}

/// `/api/v1/events`: server-sent events until the page goes away or the server stops.
fn stream(mut s: TcpStream, req: &Request, shared: &Shared) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 200 OK\r\n{}Cache-Control: no-store\r\nX-Accel-Buffering: no\r\nConnection: close\r\n\r\n",
        common_headers("text/event-stream; charset=utf-8")
    );
    s.write_all(head.as_bytes())?;
    if req.head_only() {
        return s.flush();
    }
    let hub = &shared.hub;
    if !hub.join(super::MAX_STREAMS) {
        // Too many pages: the browser's EventSource retries after `retry`.
        return s.write_all(b"retry: 2000\n\n").and_then(|()| s.flush());
    }
    let mut last = req.last_event_id.unwrap_or_else(|| hub.latest());
    let mut idle = Duration::ZERO;
    let result = (|| {
        s.write_all(format!("retry: 2000\nid: {last}\nevent: hello\ndata: {{}}\n\n").as_bytes())?;
        s.flush()?;
        loop {
            match hub.after(last, LIVENESS) {
                Next::Events(ev) => {
                    let mut out = String::new();
                    for e in &ev {
                        out.push_str(&e.frame());
                        last = e.id;
                    }
                    s.write_all(out.as_bytes())?;
                }
                Next::Reset(latest) => {
                    last = latest;
                    s.write_all(format!("id: {latest}\nevent: reset\ndata: {{}}\n\n").as_bytes())?;
                }
                Next::Idle => {
                    if peer_closed(&s) {
                        return Ok(());
                    }
                    idle += LIVENESS;
                    if idle < KEEP_ALIVE {
                        continue;
                    }
                    s.write_all(b": keep-alive\n\n")?;
                }
                Next::Closed => return Ok(()),
            }
            idle = Duration::ZERO;
            s.flush()?;
        }
    })();
    hub.leave();
    result
}

/// Whether the page at the other end has gone: a closed socket reads as end of file at once,
/// without blocking and without consuming anything (an event stream's page never sends more).
fn peer_closed(s: &TcpStream) -> bool {
    if s.set_nonblocking(true).is_err() {
        return false;
    }
    let mut b = [0u8; 1];
    let gone = match s.peek(&mut b) {
        Ok(0) => true,
        Ok(_) => false,
        Err(e) => e.kind() != std::io::ErrorKind::WouldBlock,
    };
    let _ = s.set_nonblocking(false);
    gone
}

/// Percent-encoding for a URL query value.
pub(crate) fn encode_component(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode_component(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 2 < b.len() {
            if let Some(v) = std::str::from_utf8(&b[i + 1..i + 3]).ok().and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(v);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::map::events::Hub;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn a_closed_page_is_noticed_without_a_write() {
        // An event stream's slot is freed as soon as its page goes, not at the next keep-alive.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(l.local_addr().unwrap()).unwrap();
        let (server, _) = l.accept().unwrap();
        assert!(!peer_closed(&server), "an open page is not gone");
        drop(client);
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while !peer_closed(&server) {
            assert!(std::time::Instant::now() < deadline, "a closed page was not noticed");
            std::thread::sleep(Duration::from_millis(10));
        }
        // The socket is blocking again for the writes that follow.
        let mut b = [0u8; 1];
        server.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
        assert_eq!(server.peek(&mut b).unwrap(), 0);
    }

    #[test]
    fn event_streams_are_capped_below_connections() {
        let hub = Hub::new();
        for _ in 0..3 {
            assert!(hub.join(3));
        }
        assert!(!hub.join(3), "a fourth page waits");
        hub.leave();
        assert!(hub.join(3));
    }

    fn shared(dir: &std::path::Path) -> Shared {
        Shared {
            paths: adage_store::Paths::at(dir),
            port: 8765,
            repo_root: None,
            hub: Hub::new(),
            connections: AtomicUsize::new(0),
        }
    }

    fn req(head: &str, s: &Shared) -> Response {
        match route(&Request::parse(head), s) {
            Reply::Full(r) => r,
            Reply::Events => Response::new("SSE", "text/event-stream", Vec::new()),
        }
    }

    #[test]
    fn routes_and_refusals() {
        let d = tempfile::tempdir().unwrap();
        let s = shared(d.path());
        let ok = |p: &str| format!("GET {p} HTTP/1.1\r\nHost: 127.0.0.1:8765\r\nAccept-Encoding: gzip, br\r\n\r\n");
        let page = req(&ok("/"), &s);
        assert_eq!((page.status, page.content_type), ("200 OK", "text/html; charset=utf-8"));
        assert_eq!(page.get("Cache-Control"), Some("no-cache"));
        let g = req(&ok("/graph.json?t=1"), &s);
        assert_eq!((g.status, g.content_type), ("200 OK", "application/json"));
        let v: Value = serde_json::from_slice(&req(&ok("/api/v1/graph"), &s).body).unwrap();
        assert_eq!(v["sessions"]["total"], 0);
        let h: Value = serde_json::from_slice(&req(&ok("/api/v1/health"), &s).body).unwrap();
        assert!(h["checks"].as_array().is_some_and(|c| !c.is_empty()) && h["summary"]["pass"].is_u64());
        assert_eq!(req(&ok("/api/v1/reflex/nope"), &s).status, "404 Not Found");
        assert_eq!(req(&ok("/api/v1/events"), &s).status, "SSE");
        assert_eq!(req(&ok("/x"), &s).status, "404 Not Found");
        assert_eq!(req(&ok("/../Cargo.toml"), &s).status, "404 Not Found");
        assert_eq!(req("POST / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n", &s).status, "405 Method Not Allowed");
        assert_eq!(req("GET / HTTP/1.1\r\nHost: evil.example:8765\r\n\r\n", &s).status, "403 Forbidden");
        assert_eq!(req("GET / HTTP/1.1\r\nHost: localhost:8765\r\n\r\n", &s).status, "200 OK");
        assert_eq!(req("GET / HTTP/1.1\r\nHost: 127.0.0.1:9999\r\n\r\n", &s).status, "403 Forbidden");
        assert_eq!(req("GET / HTTP/1.1\r\n\r\n", &s).status, "403 Forbidden");
    }

    #[test]
    fn negotiates_encodings() {
        let a =
            assets::ASSETS.iter().find(|a| a.immutable && a.br.is_some() && a.gz.is_some()).expect("a hashed script");
        let enc = |h: Option<&str>| {
            let r = asset_response(a, h);
            (r.status, r.get("Content-Encoding").map(String::from))
        };
        assert_eq!(enc(Some("gzip, deflate, br")), ("200 OK", Some("br".into())));
        assert_eq!(enc(Some("gzip, deflate")), ("200 OK", Some("gzip".into())));
        assert_eq!(enc(Some("br;q=0, gzip")), ("200 OK", Some("gzip".into())));
        assert_eq!(enc(None), ("200 OK", Some("gzip".into())), "no header: never brotli, which curl cannot read");
        assert_eq!(asset_response(assets::find("index.html").unwrap(), None).get("Content-Encoding"), None);
        let identity = asset_response(a, Some("identity"));
        assert!(identity.status == "406 Not Acceptable" || identity.get("Content-Encoding").is_none());
        assert_eq!(asset_response(a, Some("br")).get("Cache-Control"), Some("public, max-age=31536000, immutable"));
        // The page itself is always available as is.
        let index = assets::find("index.html").unwrap();
        assert!(index.raw.is_some() && !index.immutable);
        assert_eq!(asset_response(index, Some("identity")).get("Content-Encoding"), None);
    }

    #[test]
    fn percent_encoding_round_trips() {
        assert_eq!(encode_component("finish-line/regenerate a"), "finish-line%2Fregenerate%20a");
        assert_eq!(decode_component("finish-line%2Fregenerate%20a"), "finish-line/regenerate a");
        assert_eq!(decode_component("%zz%2"), "%zz%2");
    }
}
