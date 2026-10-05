//! `adage graph --serve [--port N] [--demo]`: the map, served by the binary itself (plan §6.3).
//!
//! `/` is the page (`assets/brain.html`, embedded at build time) and `/graph.json` is the live
//! graph, recomputed per request exactly as `adage graph --json --events 400` prints it. Standard
//! library only: bounded request workers and short timeouts. Bound to 127.0.0.1 only,
//! GET and HEAD only, and read-only. Requests whose `Host` is not this address are refused, so a
//! web page elsewhere cannot read the graph through DNS rebinding. `--port 0` picks a free port.

use adage_store::Paths;
use anyhow::{bail, Context, Result};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PAGE: &str = include_str!("../assets/brain.html");
const DEFAULT_PORT: u16 = 8765;
const MAX_HEAD: usize = 8 << 10;
const EVENTS: usize = 400;
const READ_TIMEOUT: Duration = Duration::from_secs(5);
const WRITE_TIMEOUT: Duration = Duration::from_secs(10);
/// The page is self-contained, including its styles and icons; no third-party requests.
const CSP: &str = "default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; font-src data:; \
                   connect-src 'self'; img-src 'self' data:; base-uri 'none'; form-action 'none'; frame-ancestors 'none'";

static SERVE_STOP: AtomicBool = AtomicBool::new(false);

/// Let the foreground CLI finish request workers and drop an isolated demo when interrupted.
/// Dashboard instances owned by the terminal app use their own stop flag and do not install
/// process-wide handlers.
#[cfg(unix)]
struct ShutdownSignals {
    interrupt: libc::sighandler_t,
    terminate: libc::sighandler_t,
}

#[cfg(unix)]
extern "C" fn stop_serving(_: libc::c_int) {
    SERVE_STOP.store(true, Ordering::Release);
}

#[cfg(unix)]
impl ShutdownSignals {
    fn install() -> Self {
        SERVE_STOP.store(false, Ordering::Release);
        // The handler only stores a lock-free flag; all I/O and cleanup happen on normal
        // threads after the accept loop observes it. Restore the prior dispositions on exit.
        unsafe {
            Self {
                interrupt: libc::signal(libc::SIGINT, stop_serving as *const () as libc::sighandler_t),
                terminate: libc::signal(libc::SIGTERM, stop_serving as *const () as libc::sighandler_t),
            }
        }
    }
}

#[cfg(unix)]
impl Drop for ShutdownSignals {
    fn drop(&mut self) {
        unsafe {
            libc::signal(libc::SIGINT, self.interrupt);
            libc::signal(libc::SIGTERM, self.terminate);
        }
    }
}

/// A dashboard owned by the terminal app. It starts only when requested and stops when the
/// app closes. Repeated opens reuse the same listener, data directory and URL; a busy default
/// port falls back to an available one instead of asking the person to manage a server.
pub struct Dashboard {
    url: String,
    stop: Arc<AtomicBool>,
    worker: Option<std::thread::JoinHandle<()>>,
}

impl Dashboard {
    pub fn start(paths: &Paths) -> Result<Self> {
        let listener = match TcpListener::bind(("127.0.0.1", DEFAULT_PORT)) {
            Ok(listener) => listener,
            Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => TcpListener::bind(("127.0.0.1", 0))
                .context("Could not start the web dashboard on an available local port")?,
            Err(e) => return Err(e).context("Could not start the local web dashboard"),
        };
        Self::from_listener(paths, listener)
    }

    fn from_listener(paths: &Paths, listener: TcpListener) -> Result<Self> {
        let addr = listener.local_addr()?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let paths = paths.clone();
        listener.set_nonblocking(true)?;
        let worker = std::thread::Builder::new().name("adage-dashboard".into()).spawn(move || {
            let _ = accept_requests(listener, &paths, &stopping);
        })?;
        Ok(Self { url: format!("http://{addr}/"), stop, worker: Some(worker) })
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn open(&self) -> Result<String> {
        open_browser(&self.url)?;
        Ok(self.url.clone())
    }

    /// The dashboard's embedded example needs neither a second server nor access to local
    /// agent histories. The browser labels it as a scripted demo and can switch back to live.
    pub fn open_demo(&self) -> Result<String> {
        let url = format!("{}?demo=1#traces", self.url);
        open_browser(&url)?;
        Ok(url)
    }
}

impl Drop for Dashboard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// Pass the generated loopback URL as a single argument, never through a shell. Keep browser
/// diagnostics out of the alternate screen and reap the short-lived launcher process.
fn open_browser(url: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    #[cfg(target_os = "macos")]
    let mut command = Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = Command::new("rundll32.exe");
        c.arg("url.dll,FileProtocolHandler");
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = Command::new("xdg-open");
    let mut child = command
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Your default browser could not be opened. The dashboard is ready at the address below")?;
    if child.try_wait()?.is_some_and(|status| !status.success()) {
        bail!("Your default browser could not be opened. The dashboard is ready at the address below");
    }
    // Some Linux openers stay attached to the browser. Waiting on the UI thread would freeze
    // every terminal action for the browser's lifetime; reap the launcher independently.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

pub fn serve(args: &[String], paths: &Paths) -> Result<()> {
    #[cfg(unix)]
    let _signals = ShutdownSignals::install();
    let demo = args.iter().any(|arg| arg == "--demo").then(crate::ui::demo::seeded).transpose()?;
    let paths = demo.as_ref().map(|home| &home.paths).unwrap_or(paths);
    let port: u16 = match crate::flag(args, "--port") {
        Some(p) => p.parse().with_context(|| format!("--port {p:?} is not a port number"))?,
        None if args.iter().any(|a| a == "--port") => bail!("usage: adage graph --serve [--port N] [--demo]"),
        None => DEFAULT_PORT,
    };
    let listener = TcpListener::bind(("127.0.0.1", port))
        .with_context(|| format!("cannot listen on 127.0.0.1:{port} (try --port N, or --port 0 for any free port)"))?;
    let addr = listener.local_addr()?;
    // A closed stdout (the launcher went away) must not stop the server: `println!` would panic.
    let mut out = std::io::stdout();
    let _ = writeln!(out, "adage graph: the map at http://{addr}/  (live JSON at http://{addr}/graph.json)");
    let _ = writeln!(out, "Read-only, on this machine only (ADAGE_HOME={}). Ctrl-C stops it.", paths.root.display());
    if demo.is_some() {
        let _ = writeln!(out, "Scripted demo: real hook decisions, simulated learning. Your workspace is unchanged.");
    }
    let _ = out.flush();
    listener.set_nonblocking(true)?;
    accept_requests(listener, paths, &SERVE_STOP)
}

/// Browsers may open speculative idle connections. A bounded set of request workers keeps
/// those from blocking the page or JSON poll, without creating unbounded threads.
fn accept_requests(listener: TcpListener, paths: &Paths, stop: &AtomicBool) -> Result<()> {
    let port = listener.local_addr()?.port();
    let mut workers: Vec<std::thread::JoinHandle<()>> = Vec::new();
    while !stop.load(Ordering::Acquire) {
        let mut i = 0;
        while i < workers.len() {
            if workers[i].is_finished() {
                let _ = workers.swap_remove(i).join();
            } else {
                i += 1;
            }
        }
        match listener.accept() {
            Ok((stream, _)) if workers.len() < 8 => {
                let paths = paths.clone();
                workers.push(std::thread::spawn(move || {
                    let _ = handle(stream, &paths, port);
                }));
            }
            Ok(_) => {} // At capacity: close the new connection; existing clients finish soon.
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    // Closing the listener immediately releases the port; worker reads have a fixed deadline.
    drop(listener);
    for worker in workers {
        let _ = worker.join();
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

/// Decode only the two identifiers this endpoint accepts. Malformed escapes, duplicate
/// parameters and control characters are rejected rather than normalized into another path.
fn trace_query(target: &str) -> Option<(String, String)> {
    fn decode(value: &str) -> Option<String> {
        let bytes = value.as_bytes();
        let mut out = Vec::with_capacity(bytes.len());
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'%' => {
                    let hi = (*bytes.get(i + 1)? as char).to_digit(16)?;
                    let lo = (*bytes.get(i + 2)? as char).to_digit(16)?;
                    out.push((hi * 16 + lo) as u8);
                    i += 3;
                }
                b'+' => {
                    out.push(b' ');
                    i += 1;
                }
                byte => {
                    out.push(byte);
                    i += 1;
                }
            }
        }
        let value = String::from_utf8(out).ok()?;
        (!value.chars().any(char::is_control)).then_some(value)
    }
    let (_, query) = target.split_once('?')?;
    let (mut session, mut repo) = (None, None);
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=')?;
        let slot = match key {
            "session" => &mut session,
            "repo" => &mut repo,
            _ => return None,
        };
        if slot.is_some() {
            return None;
        }
        *slot = Some(decode(value)?);
    }
    let (session, repo) = (session?, repo?);
    (crate::graph::valid_session_id(&session) && repo.len() <= 2048).then_some((session, repo))
}

/// The request head (request line and headers), or `None` if it is too large, malformed, or slow.
fn read_head(s: &mut TcpStream) -> Option<String> {
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 1024];
    let deadline = Instant::now() + READ_TIMEOUT;
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let remaining = deadline.checked_duration_since(Instant::now())?;
        s.set_read_timeout(Some(remaining)).ok()?;
        if buf.len() >= MAX_HEAD {
            return None;
        }
        let n = s.read(&mut chunk).ok()?;
        if n == 0 {
            return None;
        }
        buf.extend_from_slice(&chunk[..n]);
        if buf.len() > MAX_HEAD {
            return None;
        }
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
        return (
            text("403 Forbidden", "adage graph --serve answers only requests for 127.0.0.1 or localhost"),
            head_only,
        );
    }
    if method != "GET" && !head_only {
        return (
            Response {
                extra: "Allow: GET, HEAD\r\n",
                ..text("405 Method Not Allowed", "read-only: GET and HEAD only")
            },
            false,
        );
    }
    let path = target.split(['?', '#']).next().unwrap_or("");
    let res = match path {
        "/" | "/index.html" | "/brain" | "/brain.html" => Response {
            status: "200 OK",
            content_type: "text/html; charset=utf-8",
            body: PAGE.as_bytes().to_vec(),
            extra: "",
        },
        "/graph.json" => match crate::graph::build(paths, EVENTS) {
            Ok(v) => Response {
                status: "200 OK",
                content_type: "application/json",
                body: v.to_string().into_bytes(),
                extra: "",
            },
            // The page shows "offline" and retries.
            Err(e) => text("503 Service Unavailable", &format!("{e:#}")),
        },
        "/trace.json" => match trace_query(target) {
            Some((session, repo)) => match crate::graph::trace(paths, &session, &repo) {
                Ok(Some(value)) => Response {
                    status: "200 OK",
                    content_type: "application/json",
                    body: value.to_string().into_bytes(),
                    extra: "",
                },
                Ok(None) => text("404 Not Found", "Recorded trace not found"),
                Err(error) => text("503 Service Unavailable", &format!("{error:#}")),
            },
            None => text("400 Bad Request", "trace.json requires one valid session and repository identifier"),
        },
        _ => text("404 Not Found", "not found"),
    };
    (res, head_only)
}

fn handle(mut s: TcpStream, paths: &Paths, port: u16) -> std::io::Result<()> {
    // macOS inherits O_NONBLOCK from the listener. A browser often connects before sending
    // its headers; workers must wait within the read deadline instead of rejecting that gap.
    s.set_nonblocking(false)?;
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
    fn managed_dashboard_serves_despite_idle_connection_and_releases_port() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths::at(dir.path().join("data"));
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let addr = listener.local_addr().unwrap();
        let dashboard = Dashboard::from_listener(&paths, listener).unwrap();
        assert_eq!(dashboard.url(), format!("http://{addr}/"));
        let idle = TcpStream::connect(addr).unwrap();
        // An accepted socket must wait for headers; browsers may use a preconnected socket.
        let mut delayed = TcpStream::connect(addr).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        delayed.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        delayed.write_all(format!("GET / HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes()).unwrap();
        let mut page = String::new();
        delayed.read_to_string(&mut page).unwrap();
        assert!(page.starts_with("HTTP/1.1 200 OK"), "preconnected browser request: {page}");
        let mut request = TcpStream::connect(addr).unwrap();
        request.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
        request.write_all(format!("GET /graph.json HTTP/1.1\r\nHost: {addr}\r\n\r\n").as_bytes()).unwrap();
        let mut body = String::new();
        request.read_to_string(&mut body).unwrap();
        assert!(body.starts_with("HTTP/1.1 200 OK"));
        drop(idle);
        drop(request);
        drop(dashboard);
        assert!(TcpListener::bind(addr).is_ok(), "the app must release its server on exit");
        assert!(!paths.root.exists(), "the dashboard does not write application data");
    }

    #[test]
    fn managed_dashboards_use_distinct_ports_and_data_directories() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let first = Dashboard::start(&Paths::at(a.path())).unwrap();
        let second = Dashboard::start(&Paths::at(b.path())).unwrap();
        assert_ne!(first.url(), second.url(), "an occupied port must never reuse another app's data");
    }

    #[test]
    fn routes_and_refusals() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let req = |r: &str| route(r, &paths, 8765).0;
        assert_eq!(req("GET / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n").status, "200 OK");
        assert!(String::from_utf8(req("GET / HTTP/1.1\r\nHost: localhost:8765\r\n\r\n").body)
            .unwrap()
            .starts_with("<!doctype html>"));
        let g = req("GET /graph.json?t=1 HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n");
        assert_eq!((g.status, g.content_type), ("200 OK", "application/json"));
        assert_eq!(req("GET /x HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n").status, "404 Not Found");
        assert_eq!(req("POST / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n").status, "405 Method Not Allowed");
        assert_eq!(req("GET / HTTP/1.1\r\nHost: evil.example:8765\r\n\r\n").status, "403 Forbidden");
        assert_eq!(req("GET / HTTP/1.1\r\n\r\n").status, "403 Forbidden");
        let (_, head_only) = route("HEAD / HTTP/1.1\r\nHost: 127.0.0.1:8765\r\n\r\n", &paths, 8765);
        assert!(head_only);
    }

    #[test]
    fn selected_trace_is_complete_and_rejects_ambiguous_or_unsafe_requests() {
        let directory = tempfile::tempdir().unwrap();
        let paths = Paths::at(directory.path());
        for sequence in 1..=450 {
            adage_store::spool::append(
                &paths.spool("old-session"),
                &serde_json::json!({
                    "ts": sequence, "repo": "acme/repo", "arm": "treated", "event": "PostToolBatch",
                    "decision": { "observed": ["exec ls"] },
                }),
            )
            .unwrap();
        }
        let request =
            |target: &str| route(&format!("GET {target} HTTP/1.1\r\nHost: localhost:8765\r\n\r\n"), &paths, 8765).0;
        let response = request("/trace.json?session=old-session&repo=acme%2Frepo");
        assert_eq!(response.status, "200 OK");
        let trace: serde_json::Value = serde_json::from_slice(&response.body).unwrap();
        assert_eq!(trace["events_total"], 450);
        assert_eq!(trace["events"][0]["sequence"], 1);
        assert_eq!(trace["events"][449]["sequence"], 450);
        assert_eq!(request("/trace.json?session=old-session&repo=other").status, "404 Not Found");
        assert_eq!(request("/trace.json?session=missing&repo=acme").status, "404 Not Found");
        for target in [
            "/trace.json",
            "/trace.json?session=old-session",
            "/trace.json?session=&repo=acme",
            "/trace.json?session=old-session&repo=acme&repo=other",
            "/trace.json?session=old-session&repo=acme&extra=1",
            "/trace.json?session=..%2F..%2Fpasswd&repo=acme",
            "/trace.json?session=old-session&repo=%GG",
            "/trace.json?session=old-session&repo=%FF",
            "/trace.json?session=old-session&repo=%00",
        ] {
            assert_eq!(request(target).status, "400 Bad Request", "{target}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(paths.spool("old-session"), paths.sessions().join("linked.spool")).unwrap();
            assert_eq!(request("/trace.json?session=linked&repo=acme%2Frepo").status, "404 Not Found");
        }
        let denied = route(
            "POST /trace.json?session=old-session&repo=acme HTTP/1.1\r\nHost: localhost:8765\r\n\r\n",
            &paths,
            8765,
        )
        .0;
        assert_eq!(denied.status, "405 Method Not Allowed");
        let foreign =
            route("GET /trace.json?session=old-session&repo=acme HTTP/1.1\r\nHost: evil.example\r\n\r\n", &paths, 8765)
                .0;
        assert_eq!(foreign.status, "403 Forbidden");
    }
}
