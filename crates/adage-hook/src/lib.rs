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
//! pipeline above. `adage-exec hook` passes one: after `decide()`, and only when there are
//! directives to emit, the extension may return a replacement rendering for every directive (the
//! E1 data blocks) and a value recorded under `exec` in the spool record. An extension that
//! returns nothing, panics, or returns text that would pass the harness's cap leaves the output
//! exactly as `adage hook` prints it.

pub mod autolearn;
pub mod failopen;
pub mod harness;

use adage_core::{repo, CommandTable};
use adage_intent::model::{self, StaticModel};
use adage_intent::normalize::normalize;
use adage_runtime::{assign, decide, fast, rebuild, render, Arm, Ctx, Directive, Event, SessionState};
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
}

/// What an extension adds to a decision's delivery.
#[derive(Debug, Clone, Default)]
pub struct Delivery {
    /// The text for each directive, in order, replacing [`render::wrap_directive`]'s. `None`, or a
    /// list of the wrong length, delivers exactly the E0 text.
    pub texts: Option<Vec<String>>,
    /// Recorded under `exec` in this event's spool record.
    pub spool: Option<Value>,
}

/// A step after `decide()` that may change what directives are delivered as (Track C §3.2).
pub trait Extension: Sync {
    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery;
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
    // A state that can no longer be read is rebuilt from the session's spool, if it is small
    // enough to replay quickly (it is read under the state lock).
    let rebuild = || {
        let small = std::fs::metadata(&spool_path).is_ok_and(|m| m.len() <= rebuild::MAX_SPOOL_BYTES);
        let records: Vec<Value> = if small { spool::read_all(&spool_path).ok()? } else { return None };
        rebuild::rebuild(&records, &index, &repo_key, &table)
    };
    let (decision, arm, propensity) = state::update_or_rebuild::<SessionState, _>(&state_path, rebuild, |st| {
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
        (decide(&ctx, st, &ev), st.arm, propensity)
    })
    .map_err(|e| Failure::io("state", "updating the session state", &e))?;

    // An extension (adage-exec's E1 prefetch) sees the directives about to be delivered. A panic
    // in it delivers E0.
    let mut delivery = Delivery::default();
    if let Some(x) = opts.extension {
        if decision.continue_reason.is_none() && !decision.directives.is_empty() {
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
            };
            delivery =
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x.deliver(&turn, &decision.directives)))
                    .unwrap_or_default();
        }
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
    if decision.directives.is_empty() {
        return Ok(None);
    }
    let max = adapter.max_context();
    let e0 = || decision.directives.iter().map(render::wrap_directive).collect::<Vec<_>>().join("\n\n");
    let mut text = match delivery.texts {
        // Whatever an extension adds must fit the cap whole, or it is not delivered at all.
        Some(t) if t.len() == decision.directives.len() => Some(t.join("\n\n")).filter(|t| t.len() <= max),
        _ => None,
    }
    .unwrap_or_else(e0);
    // Design W8 (spill overflow to a file) cannot arise: the longest decision validation allows
    // fits every harness's cap (`the_largest_decision_fits_every_harness_cap`). This is a backstop.
    if text.len() > max {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, _)| i).unwrap_or(0));
    }
    Ok(adapter.render(event, Output::Context(text)))
}
