//! The map's live stream: one tailer thread follows the session spools and `learn.log` (with
//! `ui/tail.rs`, as `adage watch` does) and watches the index, and publishes what it finds to a
//! [`Hub`]; each page's `/api/v1/events` connection replays the hub as server-sent events.
//!
//! Events carry increasing ids, so a page that reconnects with `Last-Event-ID` receives what it
//! missed, as long as the hub still holds it ([`KEEP`] events). Otherwise it gets a `reset` event
//! and reloads the graph. Three kinds are published:
//!
//! - `decision`: one hook decision, shaped as the graph's `events` ([`crate::graph::event`]);
//! - `learn`: one `learn.log` line (a learning run and what it changed);
//! - `graph`: the index was recompiled (a new generation), so the graph changed.

use super::Shared;
use crate::ui::tail::{LineTail, SpoolTail};
use serde_json::{json, Value};
use std::collections::{HashMap, VecDeque};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, SystemTime};

/// Events kept for reconnecting pages.
pub(crate) const KEEP: usize = 1024;
const POLL: Duration = Duration::from_millis(250);

#[derive(Clone, Debug)]
pub(crate) struct Event {
    pub(crate) id: u64,
    pub(crate) kind: &'static str,
    pub(crate) data: String,
}

impl Event {
    /// The event as it goes on the wire.
    pub(crate) fn frame(&self) -> String {
        format!("id: {}\nevent: {}\ndata: {}\n\n", self.id, self.kind, self.data)
    }
}

struct State {
    /// The id the next event gets. Ids start at the clock (in milliseconds) so that an id from an
    /// earlier run of the server is older than anything held, and gets a `reset`.
    next: u64,
    events: VecDeque<Event>,
    pages: usize,
    sent: u64,
    closed: bool,
}

pub(crate) struct Hub {
    state: Mutex<State>,
    changed: Condvar,
}

/// What a page is sent next.
pub(crate) enum Next {
    Events(Vec<Event>),
    /// It missed more than the hub holds: reload.
    Reset(u64),
    /// Nothing within the wait: send a keep-alive.
    Idle,
    Closed,
}

impl Hub {
    pub(crate) fn new() -> Hub {
        let now = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(1);
        Hub {
            state: Mutex::new(State { next: now, events: VecDeque::new(), pages: 0, sent: 0, closed: false }),
            changed: Condvar::new(),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub(crate) fn publish(&self, kind: &'static str, data: &Value) {
        let mut s = self.lock();
        let id = s.next;
        s.next += 1;
        s.events.push_back(Event { id, kind, data: data.to_string() });
        while s.events.len() > KEEP {
            s.events.pop_front();
        }
        self.changed.notify_all();
    }

    /// The id of the newest event (a new page starts after it).
    pub(crate) fn latest(&self) -> u64 {
        self.lock().next - 1
    }

    /// Events after `last`, waiting up to `wait` for one.
    pub(crate) fn after(&self, last: u64, wait: Duration) -> Next {
        let mut s = self.lock();
        let deadline = std::time::Instant::now() + wait;
        loop {
            if s.closed {
                return Next::Closed;
            }
            if last >= s.next {
                // An id this run never issued: from an earlier run.
                return Next::Reset(s.next - 1);
            }
            if last + 1 < s.next {
                let oldest = s.events.front().map_or(s.next, |e| e.id);
                if last + 1 < oldest {
                    return Next::Reset(s.next - 1);
                }
                let out: Vec<Event> = s.events.iter().filter(|e| e.id > last).cloned().collect();
                s.sent += out.len() as u64;
                return Next::Events(out);
            }
            let left = deadline.saturating_duration_since(std::time::Instant::now());
            if left.is_zero() {
                return Next::Idle;
            }
            s = self.changed.wait_timeout(s, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// Count a page in, unless `max` are already connected (then it is refused and retries).
    pub(crate) fn join(&self, max: usize) -> bool {
        let mut s = self.lock();
        if s.pages >= max {
            return false;
        }
        s.pages += 1;
        true
    }

    pub(crate) fn leave(&self) {
        let mut s = self.lock();
        s.pages = s.pages.saturating_sub(1);
    }

    /// Pages connected, and events sent to them.
    pub(crate) fn stats(&self) -> (usize, u64) {
        let s = self.lock();
        (s.pages, s.sent)
    }

    pub(crate) fn closed(&self) -> bool {
        self.lock().closed
    }

    pub(crate) fn close(&self) {
        self.lock().closed = true;
        self.changed.notify_all();
    }
}

/// The repository each session belongs to, from its state file (read once per session).
fn repo_of(shared: &Shared, cache: &mut HashMap<String, String>, session: &str) -> String {
    if let Some(r) = cache.get(session) {
        return r.clone();
    }
    let st: adage_runtime::SessionState = adage_store::state::read(&shared.paths.state(session)).unwrap_or_default();
    let repo = st.repo.unwrap_or_default();
    if !repo.is_empty() {
        cache.insert(session.to_string(), repo.clone());
    }
    repo
}

fn index_mtime(shared: &Shared) -> Option<SystemTime> {
    std::fs::metadata(shared.paths.index()).and_then(|m| m.modified()).ok()
}

/// One poll: new decisions, learning runs and index generations, published.
pub(crate) struct Tailer {
    spools: SpoolTail,
    log: LineTail,
    repos: HashMap<String, String>,
    index_at: Option<SystemTime>,
    generation: String,
}

impl Tailer {
    /// Starts at the end of what exists: history is in the graph snapshot already.
    pub(crate) fn new(shared: &Shared) -> Tailer {
        let mut t = Tailer {
            spools: SpoolTail::new(shared.paths.sessions()),
            log: LineTail::new(shared.paths.learn_log()),
            repos: HashMap::new(),
            index_at: index_mtime(shared),
            generation: adage_tracks::Index::load(&shared.paths.index()).generation,
        };
        let _ = t.spools.poll();
        let _ = t.log.poll();
        t
    }

    pub(crate) fn poll(&mut self, shared: &Shared) {
        for rec in self.spools.poll() {
            let repo = repo_of(shared, &mut self.repos, &rec.session);
            if let Some(e) = crate::graph::event(&repo, &rec.session, &rec.value) {
                shared.hub.publish("decision", &e);
            }
        }
        for line in self.log.poll() {
            if let Ok(v) = serde_json::from_str::<Value>(&line) {
                shared.hub.publish("learn", &v);
            }
        }
        let at = index_mtime(shared);
        if at != self.index_at {
            self.index_at = at;
            let generation = adage_tracks::Index::load(&shared.paths.index()).generation;
            if generation != self.generation {
                self.generation = generation;
                shared.hub.publish("graph", &json!({ "generation": self.generation }));
            }
        }
    }
}

pub(crate) fn spawn_tailer(shared: Arc<Shared>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        let mut t = Tailer::new(&shared);
        while !shared.hub.closed() {
            std::thread::sleep(POLL);
            t.poll(&shared);
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replays_what_a_page_missed_and_resets_when_too_much_was_missed() {
        let hub = Hub::new();
        let start = hub.latest();
        assert!(matches!(hub.after(start, Duration::from_millis(5)), Next::Idle));
        hub.publish("decision", &json!({ "n": 1 }));
        hub.publish("learn", &json!({ "n": 2 }));
        let Next::Events(ev) = hub.after(start, Duration::ZERO) else { panic!("events") };
        assert_eq!(ev.iter().map(|e| e.kind).collect::<Vec<_>>(), ["decision", "learn"]);
        assert_eq!(ev[0].frame(), format!("id: {}\nevent: decision\ndata: {{\"n\":1}}\n\n", start + 1));
        let Next::Events(ev) = hub.after(start + 1, Duration::ZERO) else { panic!("resume") };
        assert_eq!(ev.len(), 1);
        // An id from an earlier run, or one older than the hub holds.
        assert!(matches!(hub.after(start + 100, Duration::ZERO), Next::Reset(_)));
        for i in 0..KEEP + 5 {
            hub.publish("decision", &json!({ "n": i }));
        }
        assert!(matches!(hub.after(start, Duration::ZERO), Next::Reset(_)));
        hub.close();
        assert!(matches!(hub.after(hub.latest(), Duration::from_secs(5)), Next::Closed));
    }
}
