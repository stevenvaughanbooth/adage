//! Requests and responses for `adage map`: just enough HTTP/1.1 for a browser on this machine.
//! One request per connection (`Connection: close`), GET/HEAD reads and a bounded, same-origin review POST.

use super::assets::{self, Asset};
use super::events::Next;
use super::Shared;
use serde_json::{json, Value};
use std::borrow::Cow;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::{Duration, Instant};

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
    pub(super) query: String,
    pub(crate) host: Option<String>,
    pub(crate) accept_encoding: Option<String>,
    pub(crate) last_event_id: Option<u64>,
    pub(super) origin: Option<String>,
    pub(super) csrf: Option<String>,
    pub(super) content_type: Option<String>,
    content_length: Option<usize>,
    pub(super) body: Vec<u8>,
}

/// A request rejected before it reaches an application route.
#[derive(Debug, PartialEq, Eq)]
enum RequestError {
    Malformed,
    TooLarge,
    Timeout,
}

impl RequestError {
    fn response(&self) -> Response {
        match self {
            Self::Malformed => text("400 Bad Request", "bad request"),
            Self::TooLarge => text("431 Request Header Fields Too Large", "request headers exceed 8192 bytes"),
            Self::Timeout => text("408 Request Timeout", "request took too long"),
        }
    }
}

/// HTTP tokens (methods and field names) exclude whitespace, delimiters and control bytes.
fn token(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(&b))
}

impl Request {
    fn parse(head: &str) -> Result<Request, RequestError> {
        let mut lines = head.strip_suffix("\r\n\r\n").ok_or(RequestError::Malformed)?.split("\r\n");
        let mut first = lines.next().ok_or(RequestError::Malformed)?.split(' ');
        let method = first.next().ok_or(RequestError::Malformed)?;
        let target = first.next().ok_or(RequestError::Malformed)?;
        let version = first.next().ok_or(RequestError::Malformed)?;
        if !token(method)
            || !target.starts_with('/')
            || target.bytes().any(|b| b.is_ascii_control() || b == b' ' || b == b'#')
            || !matches!(version, "HTTP/1.0" | "HTTP/1.1")
            || first.next().is_some()
        {
            return Err(RequestError::Malformed);
        }
        let (path, query) = target.split_once('?').unwrap_or((target, ""));
        let mut r = Request { method: method.into(), path: path.into(), query: query.into(), ..Default::default() };
        let mut seen = std::collections::BTreeSet::new();
        for line in lines {
            let (k, v) = line.split_once(':').ok_or(RequestError::Malformed)?;
            if !token(k) || v.bytes().any(|b| b.is_ascii_control() && b != b'\t') {
                return Err(RequestError::Malformed);
            }
            let v = v.trim_matches([' ', '\t']);
            let key = k.to_ascii_lowercase();
            // Never allow a duplicate field to replace the host, origin, capability or framing
            // checked for a review mutation. Accept-Encoding is explicitly list-valued.
            if key != "accept-encoding" && !seen.insert(key.clone()) {
                return Err(RequestError::Malformed);
            }
            match key.as_str() {
                "origin" => r.origin = Some(v.into()),
                "x-adage-csrf" => r.csrf = Some(v.into()),
                "content-type" => r.content_type = Some(v.into()),
                "content-length" => {
                    if v.is_empty() || !v.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(RequestError::Malformed);
                    }
                    r.content_length = Some(v.parse().map_err(|_| RequestError::Malformed)?);
                }
                "transfer-encoding" => return Err(RequestError::Malformed),
                "host" => {
                    if v.is_empty() {
                        return Err(RequestError::Malformed);
                    }
                    r.host = Some(v.to_ascii_lowercase());
                }
                "accept-encoding" => {
                    // List-valued fields may be repeated; retain every preference in wire order.
                    let encodings = r.accept_encoding.get_or_insert_with(String::new);
                    if !encodings.is_empty() {
                        encodings.push(',');
                    }
                    encodings.push_str(&v.to_ascii_lowercase());
                }
                "last-event-id" => r.last_event_id = v.parse().ok(),
                _ => {}
            }
        }
        if r.host.is_none() {
            return Err(RequestError::Malformed);
        }
        Ok(r)
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
    pub(super) fn new(
        status: &'static str,
        content_type: &'static str,
        body: impl Into<Cow<'static, [u8]>>,
    ) -> Response {
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

pub(super) fn text(status: &'static str, body: &str) -> Response {
    Response::new(status, "text/plain; charset=utf-8", format!("{body}\n").into_bytes())
        .header("Cache-Control", "no-store")
}

/// Beside the doctor's checks: what the hooks do on this machine that is a setting, not a
/// problem. Rewrites to the project's environment (on unless `runtime.rewrite` is false or the
/// `exec/rewrite-off` marker is there), and what intent reflexes embed with (the model bundled in
/// the binary, an installed one, or the hashed fallback).
fn runtime(paths: &adage_store::Paths) -> Value {
    let config = adage_store::config::load(paths).config;
    let marker = paths.rewrite_off().exists();
    let (model, fallback) = crate::status_card::embeddings(paths);
    json!({
        "rewrite": { "on": config.runtime_rewrite && !marker, "config": config.runtime_rewrite, "marker": marker },
        "embeddings": { "model": model, "bundled": model.ends_with("(bundled)"), "fallback": fallback },
    })
}

pub(super) fn json_response(v: &Value) -> Response {
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
    if req.path == "/api/v1/review" {
        return Reply::Full(super::review::respond(req, shared));
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
            Ok(mut v) => {
                v["demo"] = json!(shared.demo);
                json_response(&v)
            }
            // The page shows "offline" and retries.
            Err(e) => text("503 Service Unavailable", &format!("{e:#}")),
        },
        "/api/v1/trace" => {
            let value = |key: &str| {
                req.query
                    .split('&')
                    .filter_map(|p| p.split_once('='))
                    .find(|(k, _)| *k == key)
                    .map(|(_, v)| decode_component(&v.replace('+', " ")))
            };
            match (value("session"), value("repo")) {
                (Some(session), Some(repo)) if !session.is_empty() && !repo.is_empty() => {
                    match adage_app::graph::trace(&shared.paths, &session, &repo) {
                        Ok(Some(trace)) => json_response(&trace),
                        Ok(None) => text("404 Not Found", "this recorded session is not available"),
                        Err(e) => text("400 Bad Request", &format!("{e:#}")),
                    }
                }
                _ => text("400 Bad Request", "session and repo are required"),
            }
        }
        "/api/v1/health" => {
            let checks = crate::doctor::checks(&shared.paths, shared.repo_root.as_deref());
            let count = |s: &str| checks.iter().filter(|c| json!(c.status) == s).count();
            json_response(&json!({
                "checks": checks,
                "summary": { "pass": count("pass"), "warn": count("warn"), "fail": count("fail") },
                "runtime": runtime(&shared.paths),
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

/// Read the bounded request head and retain already received body bytes separately.
/// Resetting a socket timeout after each byte would let a slow client hold a worker forever.
fn read_head(s: &mut TcpStream, timeout: Duration) -> Result<(String, Vec<u8>), RequestError> {
    let deadline = Instant::now() + timeout;
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    loop {
        if let Some(end) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            // A body or pipelined request received in the same read is never interpreted as headers.
            let body = buf.split_off(end + 4);
            let head = String::from_utf8(buf).map_err(|_| RequestError::Malformed)?;
            return Ok((head, body));
        }
        let available = MAX_HEAD - buf.len();
        if available == 0 {
            return Err(RequestError::TooLarge);
        }
        let limit = available.min(chunk.len());
        let count = read_before(s, &mut chunk[..limit], deadline)?;
        buf.extend_from_slice(&chunk[..count]);
    }
}

/// Read some bytes within one absolute deadline shared by every read in a head or body.
fn read_before(s: &mut TcpStream, buffer: &mut [u8], deadline: Instant) -> Result<usize, RequestError> {
    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            return Err(RequestError::Timeout);
        }
        s.set_read_timeout(Some(remaining)).map_err(|_| RequestError::Malformed)?;
        match s.read(buffer) {
            Ok(0) => return Err(RequestError::Malformed),
            Ok(n) => return Ok(n),
            Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {
                return Err(RequestError::Timeout);
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err(RequestError::Malformed),
        }
    }
}

fn read_body(s: &mut TcpStream, mut remaining: &mut [u8], timeout: Duration) -> Result<(), RequestError> {
    let deadline = Instant::now() + timeout;
    while !remaining.is_empty() {
        let count = read_before(s, remaining, deadline)?;
        remaining = &mut remaining[count..];
    }
    Ok(())
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
    s.set_write_timeout(Some(WRITE_TIMEOUT))?;
    let (head, mut body) = match read_head(&mut s, READ_TIMEOUT) {
        Ok(parts) => parts,
        Err(error) => return write(&mut s, &error.response(), false),
    };
    let mut req = match Request::parse(&head) {
        Ok(req) => req,
        Err(error) => return write(&mut s, &error.response(), false),
    };
    if req.method == "POST" && req.path == "/api/v1/review" {
        let Some(length) = req.content_length.filter(|n| *n > 0 && *n <= 16 << 10) else {
            return write(&mut s, &text("400 Bad Request", "review requires a bounded Content-Length"), false);
        };
        if body.len() > length {
            return write(&mut s, &text("400 Bad Request", "invalid request body"), false);
        }
        let received = body.len();
        body.resize(length, 0);
        if let Err(error) = read_body(&mut s, &mut body[received..], READ_TIMEOUT) {
            return write(&mut s, &error.response(), false);
        }
        req.body = body;
    }
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

pub(super) fn decode_component(s: &str) -> String {
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
    fn refuses_ambiguous_or_malformed_request_heads() {
        for head in [
            "GET / HTTP/1.1\r\nHost: localhost:8765\r\nHost: attacker.example\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: attacker.example\r\nHOST: localhost:8765\r\n\r\n",
            "GET / HTTP/1.1\r\nHost : localhost:8765\r\n\r\n",
            "GET / HTTP/1.1\r\n Host: localhost:8765\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost:8765\r\nnot-a-field\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost:8765\r\nX-Test: a\0b\r\n\r\n",
            "GET / HTTP/1.1\r\nHost: localhost:8765\r\nX-Test: a\nb\r\n\r\n",
            "GET  / HTTP/1.1\r\nHost: localhost:8765\r\n\r\n",
            "GET / HTTP/1.1 extra\r\nHost: localhost:8765\r\n\r\n",
            "GET / HTTP/9.0\r\nHost: localhost:8765\r\n\r\n",
            "GET http://localhost:8765/ HTTP/1.1\r\nHost: localhost:8765\r\n\r\n",
            "GET /#fragment HTTP/1.1\r\nHost: localhost:8765\r\n\r\n",
            "GET / HTTP/1.1\r\nHost:\r\n\r\n",
            "GET / HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(Request::parse(head).unwrap_err(), RequestError::Malformed, "{head:?}");
        }
        let request = Request::parse(
            "HEAD /?q=a%20b HTTP/1.1\r\nhOsT:\tLOCALHOST:8765 \r\nAccept-Encoding: gzip\r\nAccept-Encoding: br\r\n\r\n",
        )
        .unwrap();
        assert!(request.head_only());
        assert_eq!(request.host.as_deref(), Some("localhost:8765"));
        assert_eq!(request.accept_encoding.as_deref(), Some("gzip,br"));
    }

    #[test]
    fn review_headers_remain_strict_and_preserve_the_capability() {
        let prefix = "POST /api/v1/review HTTP/1.1\r\nHost: localhost:8765\r\n";
        let fields = "Origin: http://localhost:8765\r\nX-Adage-CSRF: token\r\nContent-Type: application/json\r\nContent-Length: 42\r\n";
        let request = Request::parse(&format!("{prefix}{fields}\r\n")).unwrap();
        assert_eq!(request.origin.as_deref(), Some("http://localhost:8765"));
        assert_eq!(request.csrf.as_deref(), Some("token"));
        assert_eq!(request.content_type.as_deref(), Some("application/json"));
        assert_eq!(request.content_length, Some(42));
        for duplicate in fields.split("\r\n").filter(|line| !line.is_empty()) {
            assert_eq!(
                Request::parse(&format!("{prefix}{fields}{duplicate}\r\n\r\n")).unwrap_err(),
                RequestError::Malformed,
                "{duplicate}"
            );
        }
        for invalid in [
            "Content-Length: +42",
            "Content-Length: -1",
            "Content-Length: 4, 4",
            "Content-Length: 18446744073709551616",
            "Transfer-Encoding: chunked",
        ] {
            assert_eq!(
                Request::parse(&format!("{prefix}{invalid}\r\n\r\n")).unwrap_err(),
                RequestError::Malformed,
                "{invalid}"
            );
        }
    }

    fn sockets() -> (TcpStream, TcpStream) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let client = TcpStream::connect(listener.local_addr().unwrap()).unwrap();
        let (server, _) = listener.accept().unwrap();
        (client, server)
    }

    #[test]
    fn request_head_limit_includes_the_terminator_and_never_parses_a_body() {
        let prefix = "GET / HTTP/1.1\r\nHost: localhost:8765\r\nX-Padding: ";
        for extra in [0, 1] {
            let request = format!("{prefix}{}\r\n\r\n", "a".repeat(MAX_HEAD - prefix.len() - 4 + extra));
            let (mut client, mut server) = sockets();
            client.write_all(request.as_bytes()).unwrap();
            let result = read_head(&mut server, Duration::from_secs(1));
            if extra == 0 {
                assert_eq!(result.unwrap().0, request);
            } else {
                assert_eq!(result.unwrap_err(), RequestError::TooLarge);
            }
        }

        let head = "GET / HTTP/1.1\r\nHost: attacker.example\r\n\r\n";
        let (mut client, mut server) = sockets();
        client.write_all(format!("{head}Host: localhost:8765\r\n\r\n").as_bytes()).unwrap();
        client.write_all(&[0xff]).unwrap();
        assert_eq!(read_head(&mut server, Duration::from_secs(1)).unwrap().0, head);
    }

    #[test]
    fn a_review_body_is_preserved_across_the_header_boundary() {
        let expected = vec![0xff; 16 << 10];
        let head = format!(
            "POST /api/v1/review HTTP/1.1\r\nHost: localhost:8765\r\nContent-Length: {}\r\n\r\n",
            expected.len()
        );
        let (mut client, mut server) = sockets();
        let mut request = head.as_bytes().to_vec();
        request.extend_from_slice(&expected);
        client.write_all(&request).unwrap();
        let (parsed_head, mut body) = read_head(&mut server, Duration::from_secs(1)).unwrap();
        assert_eq!(parsed_head, head);
        let length = Request::parse(&parsed_head).unwrap().content_length.unwrap();
        let received = body.len();
        body.resize(length, 0);
        read_body(&mut server, &mut body[received..], Duration::from_secs(1)).unwrap();
        assert_eq!(body, expected, "body bytes are retained without interpreting them as UTF-8 headers");
    }

    #[test]
    fn slow_review_bodies_cannot_extend_the_total_body_deadline() {
        let (mut client, mut server) = sockets();
        let sender = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(10));
                if client.write_all(b"a").is_err() {
                    return;
                }
            }
        });
        let result = read_body(&mut server, &mut [0; 40], Duration::from_millis(100));
        drop(server);
        sender.join().unwrap();
        assert_eq!(result.unwrap_err(), RequestError::Timeout);
    }

    #[test]
    fn slow_clients_cannot_extend_the_total_header_deadline() {
        let (mut client, mut server) = sockets();
        client.write_all(b"GET / HTTP/1.1\r\nHost: localhost:8765\r\nX-Slow: ").unwrap();
        let sender = std::thread::spawn(move || {
            for _ in 0..40 {
                std::thread::sleep(Duration::from_millis(10));
                if client.write_all(b"a").is_err() {
                    return;
                }
            }
            let _ = client.write_all(b"\r\n\r\n");
        });
        let result = read_head(&mut server, Duration::from_millis(100));
        drop(server);
        sender.join().unwrap();
        assert_eq!(result.unwrap_err(), RequestError::Timeout);
    }

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
            review: super::super::review::Access::read_only("test"),
            paths: adage_store::Paths::at(dir),
            port: 8765,
            demo: false,
            repo_root: None,
            hub: Hub::new(),
            connections: AtomicUsize::new(0),
        }
    }

    fn req(head: &str, s: &Shared) -> Response {
        let request = match Request::parse(head) {
            Ok(request) => request,
            Err(error) => return error.response(),
        };
        match route(&request, s) {
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
        assert_eq!(v["demo"], false);
        let mut demo = shared(d.path());
        demo.demo = true;
        let v: Value = serde_json::from_slice(&req(&ok("/api/v1/graph"), &demo).body).unwrap();
        assert_eq!(v["demo"], true);
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
        assert_eq!(req("GET / HTTP/1.1\r\n\r\n", &s).status, "400 Bad Request");
    }

    #[test]
    fn trace_endpoint_loads_a_whole_session_outside_the_recent_window() {
        let d = tempfile::tempdir().unwrap();
        let s = shared(d.path());
        for i in 1..=620 {
            adage_store::spool::append(
                &s.paths.sessions().join("older-session.spool"),
                &json!({
                    "ts": i, "repo": "repo/a+b", "arm": "treated", "event": "PostToolBatch",
                    "decision": { "directives": [] },
                }),
            )
            .unwrap();
        }
        let ok = |path: &str| format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n");
        let response = req(&ok("/api/v1/trace?session=older-session&repo=repo%2Fa%2Bb"), &s);
        assert_eq!(response.status, "200 OK");
        assert_eq!(response.get("Cache-Control"), Some("no-store"));
        let v: Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(v["repo"], "repo/a+b");
        assert_eq!(v["complete"], true);
        assert_eq!(v["events_total"], 620);
        assert_eq!(v["events"].as_array().unwrap().len(), 620);
        assert_eq!(v["events"][619]["sequence"], 620);
        assert_eq!(req(&ok("/api/v1/trace?session=older-session&repo=wrong"), &s).status, "404 Not Found");
        assert_eq!(req(&ok("/api/v1/trace?session=older-session"), &s).status, "400 Bad Request");
        assert_ne!(req(&ok("/api/v1/trace?session=..%2Fsecret&repo=repo%2Fa%2Bb"), &s).status, "200 OK");
        assert_eq!(
            req("POST /api/v1/trace HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n", &s).status,
            "405 Method Not Allowed"
        );
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
