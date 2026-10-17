//! `adage map [--port N] [--no-open] [--focus <reflex>] [--demo]`: the map, served by the binary
//! itself (redesign plan §8).
//!
//! The page is the web app in `web/map/`, built, compressed and committed under
//! `assets/map/` (see [`assets`]), so building Adage never needs Node. Its data comes from three
//! routes: `/api/v1/graph` (the graph as `adage graph --json` prints it; `/graph.json` is an
//! alias), `/api/v1/events` (server-sent events: new decisions from the session spools, learning
//! runs from `learn.log`, and graph changes) and `/api/v1/health` (the doctor's checks).
//!
//! Standard-library HTTP server, with nothing on the hook's path: a thread per connection,
//! at most [`MAX_CONNECTIONS`] at once. Bound to 127.0.0.1 only. Review writes require a
//! terminal-owned Person, the same origin, and a cryptographically random CSRF token.
//! Requests whose `Host` is not this address are refused, so a web page elsewhere cannot read the
//! map through DNS rebinding. The content security policy allows this origin's own files and
//! nothing else: no inline script, no font CDN. `--port 0` picks a free port.

mod assets;
mod events;
mod http;
mod review;

use adage_store::Paths;
use adage_term::{Line, Out, Role};
use anyhow::{bail, Context, Result};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_PORT: u16 = 8765;
/// Connections served at once. Each live page holds one for its event stream, and at most
/// [`MAX_STREAMS`] do, so loading a page always has room (a browser fetches about six files at
/// once).
pub(crate) const MAX_CONNECTIONS: usize = 48;
/// Event streams served at once; a page refused one retries (`retry: 2000`).
pub(crate) const MAX_STREAMS: usize = 24;
// Room for a page load (about six files at once) beside every stream.
const _: () = assert!(MAX_STREAMS + 6 <= MAX_CONNECTIONS);
const USAGE: &str = "usage: adage map [--port N] [--no-open] [--focus <reflex>] [--demo]";

#[derive(Debug, Default, PartialEq)]
struct Options {
    port: Option<u16>,
    no_open: bool,
    focus: Option<String>,
    demo: bool,
}

fn options(args: &[String]) -> Result<Options> {
    let mut o = Options::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--port" => {
                let p = it.next().with_context(|| format!("--port needs a port number\n{USAGE}"))?;
                o.port = Some(p.parse().with_context(|| format!("--port {p:?} is not a port number"))?);
            }
            "--no-open" => o.no_open = true,
            "--focus" => {
                let id = it.next().filter(|v| !v.starts_with("--"));
                o.focus = Some(id.with_context(|| format!("--focus needs a reflex id\n{USAGE}"))?.clone());
            }
            "--demo" => o.demo = true,
            // `adage graph --serve` reaches here with its own flag.
            "--serve" | "map" => {}
            other => bail!("unknown argument {other:?}\n{USAGE}"),
        }
    }
    Ok(o)
}

/// What every connection shares.
pub(crate) struct Shared {
    pub(crate) paths: Paths,
    pub(crate) port: u16,
    pub(crate) demo: bool,
    review: review::Access,
    /// The repository the doctor's per-repository checks look at.
    pub(crate) repo_root: Option<PathBuf>,
    pub(crate) hub: events::Hub,
    pub(crate) connections: AtomicUsize,
}

/// A live map owned by a terminal session. Dropping it closes the listener and event streams.
/// It never changes process signal handlers and never prints into the alternate screen.
pub(crate) struct Server {
    address: std::net::SocketAddr,
    shared: Arc<Shared>,
    acceptor: Option<std::thread::JoinHandle<()>>,
    tailer: Option<std::thread::JoinHandle<()>>,
}
impl Server {
    pub(crate) fn start(paths: Paths, repo_root: Option<PathBuf>, demo: bool) -> Result<Self> {
        let listener = TcpListener::bind(("127.0.0.1", 0)).context("Starting the local web map")?;
        let address = listener.local_addr()?;
        listener.set_nonblocking(true)?;
        let shared = Arc::new(Shared {
            review: review::Access::new(demo),
            paths,
            repo_root,
            demo,
            port: address.port(),
            hub: events::Hub::new(),
            connections: AtomicUsize::new(0),
        });
        let listener_shared = shared.clone();
        let acceptor = std::thread::Builder::new().name("adage-ui-map".into()).spawn(move || {
            while !listener_shared.hub.closed() {
                match listener.accept() {
                    Ok((stream, _)) => accept(stream, &listener_shared),
                    Err(_) => std::thread::sleep(Duration::from_millis(40)),
                }
            }
        })?;
        let tailer = events::spawn_tailer(shared.clone());
        Ok(Self { address, shared, acceptor: Some(acceptor), tailer: Some(tailer) })
    }
    pub(crate) fn url(&self, focus: Option<&str>, traces: bool) -> String {
        let mut url = format!("http://{}/?view={}", self.address, if traces { "traces" } else { "map" });
        if let Some(focus) = focus {
            url.push_str("&reflex=");
            url.push_str(&http::encode_component(focus));
        }
        url
    }
    pub(crate) fn open(&self, url: &str) -> Result<()> {
        try_open_browser(url)
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.shared.hub.close();
        if let Some(thread) = self.acceptor.take() {
            let _ = thread.join();
        }
        if let Some(thread) = self.tailer.take() {
            let _ = thread.join();
        }
    }
}

static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
extern "C" fn on_signal(_: libc::c_int) {
    STOP.store(true, Ordering::SeqCst);
}

/// Ctrl-C and SIGTERM end the accept loop, so `--demo` removes its temporary directory.
fn catch_signals() {
    #[cfg(unix)]
    // SAFETY: the handler only stores to an atomic, which is async-signal-safe.
    unsafe {
        let h = on_signal as extern "C" fn(libc::c_int) as libc::sighandler_t;
        libc::signal(libc::SIGINT, h);
        libc::signal(libc::SIGTERM, h);
    }
}

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    let opts = options(args)?;
    // `--demo` plays a scripted scenario into a temporary home, removed on exit.
    let demo = if opts.demo { Some(Arc::new(crate::ui::demo::setup()?)) } else { None };
    let (paths, repo_root) = match &demo {
        Some(d) => (d.paths.clone(), Some(d.repo.clone())),
        None => (paths.clone(), std::env::current_dir().ok().and_then(|d| adage_core::repo::find_root(&d))),
    };
    let port = opts.port.unwrap_or(DEFAULT_PORT);
    let listener = match TcpListener::bind(("127.0.0.1", port)) {
        Ok(l) => l,
        Err(e) => {
            let mut err = Out::stderr();
            let _ = err.error(
                &format!("cannot listen on 127.0.0.1:{port}"),
                &[e.to_string()],
                Some("use another port, or let Adage pick one: adage map --port 0"),
            );
            bail!("cannot listen on 127.0.0.1:{port}: {e}");
        }
    };
    let addr = listener.local_addr()?;
    let shared = Arc::new(Shared {
        review: review::Access::new(opts.demo),
        paths: paths.clone(),
        port: addr.port(),
        demo: opts.demo,
        repo_root,
        hub: events::Hub::new(),
        connections: AtomicUsize::new(0),
    });
    let url = match &opts.focus {
        Some(id) => format!("http://{addr}/?reflex={}", http::encode_component(id)),
        None => format!("http://{addr}/"),
    };

    // A closed stdout (the launcher went away) must not stop the server: every write is checked.
    let mut out = Out::stdout();
    let mode = if shared.review.enabled() { "review enabled" } else { "read-only" };
    let _ = out.header("map", &[&url, mode, "this machine only"]);
    let _ = out.kv("data", Line::new().quiet(paths.root.display().to_string()));
    let _ = out.kv("api", Line::new().cmd(format!("http://{addr}/api/v1/graph")).sep().quiet("events, health"));
    if demo.is_some() {
        let _ =
            out.kv("demo", Line::new().text("sample traces play in a temporary home").sep().quiet("removed on exit"));
    }
    // An unknown --focus would open a map with nothing selected and no reason why. (The demo's
    // reflexes arrive as it plays, so there is nothing to check against yet.)
    if let (Some(id), None) = (&opts.focus, &demo) {
        let ids: Vec<String> = crate::graph::build(&paths, 0)
            .map(|g| {
                g["routes"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|r| r["id"].as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        if !ids.iter().any(|r| r == id) {
            let known = if ids.is_empty() {
                "there are no reflexes yet".to_string()
            } else {
                format!("known: {}", ids.join(", "))
            };
            let _ = out.warn(&format!("no reflex {id} to focus on; the map opens unfocused ({known})"));
        }
    }
    let _ = out.line(&Line::new().faint("Ctrl-C stops it."));
    let _ = out.flush();

    catch_signals();
    let tailer = events::spawn_tailer(shared.clone());
    let player = demo.as_ref().map(|home| {
        let home = home.clone();
        let exe = std::env::current_exe().unwrap_or_default();
        std::thread::spawn(move || crate::ui::demo::play(&home, &exe, 3, Duration::from_millis(900), &STOP))
    });
    if !opts.no_open {
        open_browser(&url);
    }

    listener.set_nonblocking(true)?;
    let mut status = Status::new();
    while !STOP.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => accept(stream, &shared),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
                status.tick(&shared);
            }
            Err(_) => std::thread::sleep(Duration::from_millis(40)),
        }
    }
    status.clear();
    shared.hub.close();
    let _ = tailer.join();
    if let Some(p) = player {
        let _ = p.join();
    }
    drop(demo);
    Ok(())
}

fn accept(stream: TcpStream, shared: &Arc<Shared>) {
    let _ = stream.set_nonblocking(false);
    if shared.connections.fetch_add(1, Ordering::SeqCst) >= MAX_CONNECTIONS {
        shared.connections.fetch_sub(1, Ordering::SeqCst);
        http::busy(stream);
        return;
    }
    let conn = shared.clone();
    let spawned = std::thread::Builder::new().name("adage-map".into()).spawn(move || {
        // One bad client must never stop the server.
        let _ = http::handle(stream, &conn);
        conn.connections.fetch_sub(1, Ordering::SeqCst);
    });
    if spawned.is_err() {
        // The thread never started, so neither did its count's release.
        shared.connections.fetch_sub(1, Ordering::SeqCst);
    }
}

/// The browser, when asked for; never an error if there is none.
fn open_browser(url: &str) {
    let _ = try_open_browser(url);
}

fn try_open_browser(url: &str) -> Result<()> {
    use std::process::{Command, Stdio};
    let mut c = if cfg!(target_os = "macos") {
        let mut c = Command::new("open");
        c.arg(url);
        c
    } else if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.args(["/c", "start", "", url]);
        c
    } else {
        let mut c = Command::new("xdg-open");
        c.arg(url);
        c
    };
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("Starting the browser opener")?;
    // Reap the opener without holding the UI event loop while a browser starts.
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    Ok(())
}

/// A one-line live status on a terminal (pages connected, events streamed), rewritten in place.
struct Status {
    tty: bool,
    last: String,
    at: std::time::Instant,
}

impl Status {
    fn new() -> Status {
        use std::io::IsTerminal;
        Status { tty: std::io::stdout().is_terminal(), last: String::new(), at: std::time::Instant::now() }
    }

    fn tick(&mut self, shared: &Shared) {
        if !self.tty || self.at.elapsed() < Duration::from_millis(500) {
            return;
        }
        self.at = std::time::Instant::now();
        let (pages, sent) = shared.hub.stats();
        let theme = adage_term::Theme::stdout();
        let (line, _) = Line::new()
            .glyph(if pages > 0 { Role::Fire } else { Role::Ink3 }, adage_term::Glyph::Lamp)
            .text(format!(" {pages} page{} live", if pages == 1 { "" } else { "s" }))
            .sep()
            .quiet(format!("{sent} event{} streamed", if sent == 1 { "" } else { "s" }))
            .render(&theme);
        if line != self.last {
            use std::io::Write;
            let mut o = std::io::stdout();
            let _ = write!(o, "\r\x1b[2K{line}");
            let _ = o.flush();
            self.last = line;
        }
    }

    fn clear(&self) {
        if self.tty && !self.last.is_empty() {
            use std::io::Write;
            let _ = write!(std::io::stdout(), "\r\x1b[2K");
            let _ = std::io::stdout().flush();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_its_own_flags() {
        assert_eq!(options(&[]).unwrap(), Options::default());
        let o = options(&args(&["--port", "0", "--no-open", "--focus", "lookup", "--demo"])).unwrap();
        assert_eq!(o, Options { port: Some(0), no_open: true, focus: Some("lookup".into()), demo: true });
        assert_eq!(options(&args(&["--serve", "--port", "9"])).unwrap().port, Some(9));
        assert!(options(&args(&["--port"])).is_err());
        assert!(options(&args(&["--port", "x"])).is_err());
        assert!(options(&args(&["--focus", "--demo"])).is_err());
        assert!(options(&args(&["--nope"])).is_err());
    }
}

#[cfg(test)]
mod ui_server_tests {
    use super::*;
    use std::io::{Read, Write};
    #[test]
    fn terminal_owned_map_serves_its_context_and_closes_with_the_terminal() {
        let d = tempfile::tempdir().unwrap();
        let server = Server::start(Paths::at(d.path()), None, true).unwrap();
        let address = server.address;
        let url = server.url(Some("[\"repo\",\"pack\",\"reflex id\"]"), true);
        assert!(url.contains("?view=traces&reflex=%5B%22repo%22"));
        let mut stream = TcpStream::connect(address).unwrap();
        stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        write!(stream, "GET /api/v1/graph HTTP/1.1\r\nHost: {address}\r\nConnection: close\r\n\r\n").unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        assert!(text.starts_with("HTTP/1.1 200"), "{text}");
        assert!(text.contains("\"demo\":true"), "the server must retain the isolated demo context: {text}");
        drop(server);
        assert!(
            TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_err(),
            "the listener must not outlive its terminal owner"
        );
    }
}
