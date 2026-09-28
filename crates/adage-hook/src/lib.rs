//! The hook pipeline, as a library shared by `adage hook` (adage-cli) and `adage-exec hook`
//! (Track C §3.2): read the harness payload on stdin, translate it into a normalised event through
//! the harness adapter, take the no-match fast path, decide under the session's state lock, append
//! to the spool, and print the adapter's rendering.
//!
//! [`run`] returns `Ok(None)` when there is nothing to say, and `Err` when something went wrong:
//! the caller prints nothing in either case (fail open), and records the error in
//! `fail-open.log` ([`failopen`]). [`main`] is the whole `hook` subcommand for either binary.
//!
//! When an event ends a turn or a session, the hook then asks for the repository to be learned
//! ([`autolearn`]): that never changes what it prints, and a failure there is recorded in
//! `fail-open.log` and ignored.
//!
//! **Extensions.** `adage hook` runs with no [`Extension`] and its behaviour is exactly the
//! pipeline above. `adage-exec hook` passes one: after `decide()`, when there are directives to
//! emit or a finish-line obligation was armed or is being enforced at this event ([`Stimulus`]),
//! the extension may return a replacement rendering for every directive (the E1 data blocks, an
//! E2 action's result), blocks to add, obligations an action met (recorded in the session state,
//! so no Stop request follows), a change to the Stop request, and a value recorded under `exec`
//! in the spool record. An extension that returns nothing, panics, or returns text that would
//! pass the harness's cap leaves the output exactly as `adage hook` prints it.

pub mod autolearn;
pub mod failopen;
pub mod harness;

use adage_core::{repo, CommandTable};
use adage_intent::model::{self, StaticModel};
use adage_intent::normalize::normalize;
use adage_runtime::{assign, decide, fast, rebuild, render, Arm, Ctx, Directive, Event, Obligation, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
use failopen::Failure;
use harness::Output;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_STDIN: u64 = 8 << 20;

/// What one event is about, for an [`Extension`].
pub struct Turn<'a> {
    pub harness: &'a str,
    /// The native event name.
    pub event: &'a str,
    pub ev: &'a Event,
    pub session: &'a str,
    pub index: &'a Index,
    pub repo_key: &'a str,
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub paths: &'a Paths,
    /// The harness's cap on directive text ([`harness::Adapter::max_context`]).
    pub max_context: usize,
    /// The session's arm: nothing is ever run in the control (holdout) arm.
    pub arm: Option<Arm>,
    /// Finish-line obligations an approved E2 action could carry out at this event.
    pub stimuli: &'a [Stimulus],
}

/// A finish-line obligation this event armed (an edit matched its paths) or is enforcing (a Stop
/// request), for an E2 action (Track C §3.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stimulus {
    pub route: String,
    /// The command the obligation expects, bound.
    pub command: String,
    /// The paths glob that armed it.
    pub paths: String,
    /// The repository-relative path whose edit armed it (empty at Stop).
    pub edited: String,
    /// This event is the Stop request for it.
    pub at_stop: bool,
}

/// The obligations `decide()` armed or began enforcing at this event: `before` is each
/// obligation's `(route, stop_requested)` before it ran.
pub fn stimuli(before: &[(String, bool)], after: &[Obligation], is_batch: bool) -> Vec<Stimulus> {
    after
        .iter()
        .filter(|o| !o.met)
        .filter_map(|o| {
            let at_stop = o.stop_requested && !before.iter().any(|(r, s)| r == &o.route && *s);
            let armed = is_batch && o.fresh;
            (armed || at_stop).then(|| Stimulus {
                route: o.route.clone(),
                command: o.command.clone(),
                paths: o.paths.clone(),
                edited: if at_stop { String::new() } else { o.edited.clone() },
                at_stop,
            })
        })
        .collect()
}

/// What an extension adds to a decision's delivery.
#[derive(Debug, Clone, Default)]
pub struct Delivery {
    /// The text for each directive, in order, replacing [`render::wrap_directive`]'s. `None`, or a
    /// list of the wrong length, delivers exactly the E0 text.
    pub texts: Option<Vec<String>>,
    /// Recorded under `exec` in this event's spool record.
    pub spool: Option<Value>,
    /// Blocks delivered after the directives (an action's result when no directive was due).
    pub extra: Vec<String>,
    /// Obligations an applied action met: marked met in the session state.
    pub met: Vec<String>,
    /// The Stop request: `None` as decided; `Some(None)` withdrawn (an action met it);
    /// `Some(Some(reason))` with this reason instead.
    pub stop: Option<Option<String>>,
}

/// A step after `decide()` that may change what directives are delivered as (Track C §3.2).
pub trait Extension: Sync {
    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery;
    /// The exec-only reflexes (`(track, route id)`, vendored scripts) this extension would carry
    /// out in this repository, so `decide()` may direct them. Asked only when the index holds one.
    fn exec_only(&self, _repo_key: &str, _index: &Index, _paths: &Paths) -> Vec<(String, String)> {
        Vec::new()
    }
}

/// How a binary runs the pipeline.
#[derive(Clone, Copy, Default)]
pub struct Options {
    /// The adage binary live learning starts (`<learner> learn --auto …`); `None`: this binary.
    pub learner: Option<fn() -> Option<PathBuf>>,
    pub extension: Option<&'static dyn Extension>,
}

/// The payload, at most [`MAX_STDIN`] bytes. A larger one is refused, but read to the end first
/// and discarded (Design §13.3: the hook always drains its input), so the harness writing it never
/// sees a broken pipe.
fn read_payload(input: &mut impl Read) -> Result<String, Failure> {
    let mut bytes = Vec::new();
    (&mut *input).take(MAX_STDIN + 1).read_to_end(&mut bytes).map_err(|e| Failure::io("io", "reading stdin", &e))?;
    if bytes.len() as u64 > MAX_STDIN {
        let _ = std::io::copy(input, &mut std::io::sink());
        return Err(Failure::new("payload", "stdin exceeds 8 MiB"));
    }
    String::from_utf8(bytes).map_err(|_| Failure::new("payload", "stdin is not UTF-8"))
}

/// The stack every hook runs on. Main-thread stacks differ by platform (8 MiB on Linux and
/// macOS, 1 MiB on Windows); normalising hostile commands recurses to a bounded depth that fits
/// easily in release builds, but should not depend on which platform's default it got.
pub const STACK: usize = 8 << 20;

/// Run `f` on a thread with [`STACK`] bytes of stack. `None` if the thread could not be started
/// or `f` panicked.
pub fn on_big_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> Option<T> {
    std::thread::Builder::new().stack_size(STACK).spawn(f).ok()?.join().ok()
}

/// The whole `hook <harness> <event>` subcommand: fail open, always. No panic, error, or timeout
/// may block or disturb the agent, and nothing but the harness's JSON reaches stdout.
pub fn main(harness: String, event: String, opts: Options) -> ! {
    // Nothing may reach the harness but our JSON: silence panic messages entirely.
    failopen::install_panic_hook();
    let (h, e) = (harness.clone(), event.clone());
    let out = match on_big_stack(move || std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&h, &e, opts)))) {
        Some(Ok(Ok(out))) => out,
        Some(Ok(Err(f))) => {
            failopen::record(&Paths::from_env().root, &harness, &event, &f);
            None
        }
        Some(Err(_)) => {
            failopen::record(&Paths::from_env().root, &harness, &event, &failopen::panic_failure());
            None
        }
        None => {
            let f = Failure::new("thread", "the hook thread could not start");
            failopen::record(&Paths::from_env().root, &harness, &event, &f);
            None
        }
    };
    if let Some(o) = out {
        use std::io::Write;
        let _ = writeln!(std::io::stdout(), "{o}");
    }
    std::process::exit(0);
}

pub fn run(harness: &str, event: &str, opts: Options) -> Result<Option<String>, Failure> {
    // Read (or drain) stdin first, whatever happens next.
    let payload = read_payload(&mut std::io::stdin().lock());
    let Some(adapter) = harness::adapter(harness) else { return Err(Failure::new("config", "unknown harness")) };
    if std::env::var("ADAGE_DISABLE").is_ok_and(|v| v == "1") {
        return Ok(None);
    }
    let buf = payload?;
    #[cfg(debug_assertions)]
    if std::env::var("ADAGE_TEST_FAULT").is_ok_and(|v| v == "panic") {
        // Fault injection for tests (debug builds only). The message quotes the payload, as a real
        // panic message can: it must never reach the log.
        panic!("injected fault while handling {buf}");
    }
    let p: Value = serde_json::from_str(&buf)
        .map_err(|e| Failure::new("payload", format!("stdin is not a JSON payload ({:?})", e.classify())))?;
    let Some(harness::Envelope { session, cwd }) = adapter.envelope(&p) else {
        return Err(Failure::new("payload", "no session id or working directory"));
    };
    let root = repo::find_root(&cwd).unwrap_or_else(|| cwd.clone());
    if root.join(".adage/disabled").exists() {
        return Ok(None);
    }
    let paths = Paths::from_env();
    let out = respond(adapter, harness, event, &p, &session, &cwd, &root, &paths, opts);
    if adapter.is_learn_point(event) {
        let transcript = p.get("transcript_path").and_then(|v| v.as_str());
        let learner = match opts.learner {
            Some(find) => find().ok_or_else(|| Failure::new("learn", "finding the adage binary: not found")),
            None => std::env::current_exe().map_err(|e| Failure::io("learn", "finding the adage binary", &e)),
        };
        let learned =
            learner.and_then(|l| autolearn::on_turn_end(&paths, &root, adapter.name(), transcript, &l).map(drop));
        if let Err(f) = learned {
            failopen::record(&paths.root, harness, event, &f);
        }
    }
    out
}

/// Decide and render: everything after the kill switches.
#[allow(clippy::too_many_arguments)]
fn respond(
    adapter: &dyn harness::Adapter,
    harness: &str,
    event: &str,
    p: &Value,
    session: &str,
    cwd: &Path,
    root: &Path,
    paths: &Paths,
    opts: Options,
) -> Result<Option<String>, Failure> {
    let index = Index::load(&paths.index());
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    let Some(repo_key) = repo::lookup_key(root, Some(&paths.repo_keys()), &keys) else {
        return Ok(None); // nothing installed for this repository: the common, cheapest path
    };
    // An event this adapter does not act on.
    let Some(ev) = adapter.translate(event, p, &cwd.to_string_lossy()) else { return Ok(None) };
    // An event that cannot change any decision stops here, before the command table, the state
    // lock and the spool (Design §13.2; `adage_runtime::fast`).
    let state_path = paths.state(session);
    let read_state = || state::try_read::<SessionState>(&state_path).ok().flatten();
    if fast::skip(&index.triggers(&repo_key), &ev, &cwd.to_string_lossy(), read_state) {
        return Ok(None);
    }
    let table = CommandTable::load();
    // The embedding model is opened only if the runtime asks, which it does only at a prompt in a
    // repository with intent routes. Missing or damaged model: no vector, no intent directive.
    let models = paths.models();
    let embed = move |text: &str| -> Option<(String, Vec<f32>)> {
        let m = StaticModel::open(&model::dir_in(&models)).ok()?;
        let v = m.embed(&normalize(text))?;
        Some((m.id, v))
    };
    let ctx = Ctx {
        index: &index,
        repo: repo_key.clone(),
        root,
        table: &table,
        embed: Some(&embed),
        params: Default::default(),
    };
    let arm_env = std::env::var("ADAGE_ARM").ok();
    // ADAGE_HOLDOUT, else `runtime.holdout` in config.toml, else 0.1.
    let share: f64 = adage_store::config::load(paths).config.runtime_holdout;
    let spool_path = paths.spool(session);
    // Exec-only reflexes come into play only under an extension that carries them out.
    let exec_only: Vec<(String, String)> = match opts.extension {
        Some(x) if index.for_repo(&repo_key).any(|r| r.exec_only()) => {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x.exec_only(&repo_key, &index, paths)))
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    // A state that can no longer be read is rebuilt from the session's spool, if it is small
    // enough to replay quickly (it is read under the state lock).
    let rebuild = || {
        let small = std::fs::metadata(&spool_path).is_ok_and(|m| m.len() <= rebuild::MAX_SPOOL_BYTES);
        let records: Vec<Value> = if small { spool::read_all(&spool_path).ok()? } else { return None };
        rebuild::rebuild(&records, &index, &repo_key, &table)
    };
    let (mut decision, arm, propensity, stims) =
        state::update_or_rebuild::<SessionState, _>(&state_path, rebuild, |st| {
            if st.arm.is_none() {
                let (arm, p) = assign(session, share, arm_env.as_deref());
                st.arm = Some(arm);
                st.propensity = Some(p);
                st.repo = Some(repo_key.clone());
            }
            // A state from before propensities were recorded: the arm was drawn with (as far as can
            // be known) today's share, unless it is forced now.
            let propensity = st.propensity.unwrap_or_else(|| match (st.arm, arm_env.as_deref()) {
                (_, Some("holdout" | "treated")) => 1.0,
                (Some(Arm::Holdout), _) => share.clamp(0.0, 1.0),
                _ => 1.0 - share.clamp(0.0, 1.0),
            });
            let before: Vec<(String, bool)> =
                st.obligations.iter().map(|o| (o.route.clone(), o.stop_requested)).collect();
            let d = adage_runtime::with_exec_only(exec_only.clone(), || decide(&ctx, st, &ev));
            let stims = stimuli(&before, &st.obligations, matches!(ev, Event::Batch { .. }));
            (d, st.arm, propensity, stims)
        })
        .map_err(|e| Failure::io("state", "updating the session state", &e))?;

    // An extension (adage-exec: E1 prefetch, E2 actions) sees the directives about to be
    // delivered and the obligations armed or enforced now. A panic in it delivers E0.
    let mut delivery = Delivery::default();
    if let Some(x) = opts.extension {
        let directives_due = decision.continue_reason.is_none() && !decision.directives.is_empty();
        if directives_due || !stims.is_empty() {
            let turn = Turn {
                harness,
                event,
                ev: &ev,
                session,
                index: &index,
                repo_key: &repo_key,
                root,
                cwd,
                paths,
                max_context: adapter.max_context(),
                arm,
                stimuli: &stims,
            };
            delivery =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x.deliver(&turn, &decision.directives)))
                    .unwrap_or_default();
        }
    }

    // Obligations an action met: no Stop request follows for them.
    if !delivery.met.is_empty() {
        let met = delivery.met.clone();
        let withdrawn = matches!(delivery.stop, Some(None));
        let marked = state::update::<SessionState, _>(&state_path, |st| {
            for o in st.obligations.iter_mut().filter(|o| met.contains(&o.route)) {
                o.met = true;
                if withdrawn && o.stop_requested {
                    o.stop_requested = false;
                    st.stop_requests = st.stop_requests.saturating_sub(1);
                }
            }
        });
        if let Err(e) = marked {
            failopen::record(&paths.root, harness, event, &Failure::io("state", "marking obligations met", &e));
        }
    }
    match delivery.stop.take() {
        Some(None) => decision.continue_reason = None,
        Some(Some(r)) if decision.continue_reason.is_some() => decision.continue_reason = Some(r),
        _ => {}
    }

    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    // `propensity`: the probability of `arm` at assignment (holdout share, or one minus it; 1 when
    // `ADAGE_ARM` forced it), for inverse-propensity weighting (Design §11, §14.1).
    let mut rec = json!({ "ts": ts, "event": event, "kind": ev.name(), "arm": arm, "propensity": propensity, "repo": repo_key, "generation": index.generation, "decision": decision });
    if adapter.name() != "claude" {
        rec["harness"] = json!(adapter.name());
    }
    if let Some(x) = delivery.spool.take() {
        rec["exec"] = x;
    }
    if let Err(e) = spool::append(&paths.spool(session), &rec) {
        // Measurement is lost, but the decision stands: record it and carry on.
        failopen::record(&paths.root, harness, event, &Failure::io("spool", "appending to the session spool", &e));
    }

    if let Some(reason) = decision.continue_reason {
        return Ok(adapter.render(event, Output::Continue(reason)));
    }
    if decision.directives.is_empty() && delivery.extra.is_empty() {
        return Ok(None);
    }
    let max = adapter.max_context();
    let e0 = || decision.directives.iter().map(render::wrap_directive).collect::<Vec<_>>().join("\n\n");
    let texts = match delivery.texts {
        Some(t) if t.len() == decision.directives.len() => t,
        _ => decision.directives.iter().map(render::wrap_directive).collect(),
    };
    let all: Vec<String> = texts.into_iter().chain(delivery.extra).collect();
    // Whatever an extension adds must fit the cap whole, or it is not delivered at all.
    let mut text = Some(all.join("\n\n")).filter(|t| t.len() <= max).unwrap_or_else(e0);
    if text.is_empty() {
        return Ok(None);
    }
    // Design W8 (spill overflow to a file) cannot arise: the longest decision validation allows
    // fits every harness's cap (`the_largest_decision_fits_every_harness_cap`). This is a backstop.
    if text.len() > max {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, _)| i).unwrap_or(0));
    }
    Ok(adapter.render(event, Output::Context(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ob(route: &str, met: bool, stop_requested: bool, fresh: bool) -> Obligation {
        Obligation {
            route: route.into(),
            command: "make gen".into(),
            paths: "schema/*.json".into(),
            edited: "schema/users.json".into(),
            met,
            stop_requested,
            fresh,
            ..Default::default()
        }
    }

    #[test]
    fn actions_follow_fresh_edits_and_new_stop_requests_only() {
        let after = [ob("fresh", false, false, true), ob("stale", false, false, false), ob("met", true, false, true)];
        let s = stimuli(&[], &after, true);
        assert_eq!(s.iter().map(|x| x.route.as_str()).collect::<Vec<_>>(), ["fresh"]);
        assert_eq!(s[0].edited, "schema/users.json");
        assert!(stimuli(&[], &after, false).is_empty(), "only a tool batch arms an action");
        // At Stop: the obligation whose request this event makes, not one requested before.
        let after = [ob("now", false, true, false), ob("earlier", false, true, false)];
        let before = [("now".to_string(), false), ("earlier".to_string(), true)];
        let s = stimuli(&before, &after, false);
        assert_eq!(s.len(), 1);
        assert!(s[0].at_stop && s[0].route == "now" && s[0].edited.is_empty());
    }
}
