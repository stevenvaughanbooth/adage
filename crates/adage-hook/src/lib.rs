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
//! in the spool record. A read-only extension's oversized text falls back to advice. Recovery
//! warnings and potentially applied actions must never fall back to an instruction to repeat.

pub mod autolearn;
pub mod failopen;
pub mod harness;

use adage_core::{repo, CommandTable};
use adage_intent::model;
use adage_intent::normalize::normalize;
use adage_runtime::{assign, decide, fast, rebuild, render, Arm, Ctx, Directive, Event, Obligation, SessionState};
use adage_store::{spool, state, Paths};
use adage_tracks::Index;
use failopen::Failure;
use harness::Output;
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const MAX_STDIN: u64 = 8 << 20;

/// What one event is about, for an [`Extension`].
pub struct Turn<'a> {
    pub harness: &'a str,
    /// The native event name.
    pub event: &'a str,
    /// Opaque native tool-call identity. None means retries cannot be identified reliably.
    pub event_identity: Option<String>,
    pub ev: &'a Event,
    pub session: &'a str,
    pub index: &'a Index,
    pub repo_key: &'a str,
    pub root: &'a Path,
    pub cwd: &'a Path,
    pub paths: &'a Paths,
    /// The harness's cap on directive text ([`harness::Adapter::max_context`]).
    pub max_context: usize,
    /// The session's arm: nothing is ever run in the control (control) arm.
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
    /// Contexts handled as CPU predictions, including abstentions: the agent was not instructed
    /// to perform them, so retire advisory follow judgements without marking obligations met.
    pub cpu_predictions: Vec<String>,
    /// The Stop request: `None` as decided; `Some(None)` withdrawn (an action met it);
    /// `Some(Some(reason))` with this reason instead.
    pub stop: Option<Option<String>>,
    /// Execution may have effects that require inspection. This warning takes precedence over
    /// ordinary directives and must survive context overflow; it never implies a safe retry.
    pub recovery: Option<String>,
    /// An approved action was selected for this Stop's obligation request (whatever its outcome):
    /// the Stop gate does not also run at this event, so one hook never stages two actions within
    /// its timeout.
    pub at_stop_action: bool,
    /// One line for the person: a grant no longer matches what would run, so its action was
    /// withheld (which field changed, and how to approve it again). Shown at most once per
    /// session; the agent never sees it.
    pub notice: Option<String>,
}

/// Whether this session has not yet had a lapsed-grant notice; marks it as having had one.
fn first_lapse_notice(paths: &Paths, session: &str) -> bool {
    state::update::<SessionState, _>(&paths.state(session), |st| !std::mem::replace(&mut st.lapse_noticed, true))
        .unwrap_or(false)
}

/// What [`respond`] decided for one event, before rendering.
#[derive(Debug, Default)]
struct Responded {
    out: Option<Output>,
    /// [`Delivery::at_stop_action`].
    at_stop_action: bool,
}

const RECOVERY_NOTICE: &str = "Adage cannot confirm an execution outcome. Do not retry the action automatically. Inspect the worktree and execution journal, then ask the person to resolve recovery.";
const APPLIED_NOTICE: &str = "Adage applied an approved action. Its detailed result could not fit here; inspect the execution journal before rerunning the action.";

fn bounded_notice(text: String, fallback: &str, max: usize) -> Option<String> {
    if text.len() <= max {
        Some(text)
    } else if fallback.len() <= max {
        Some(fallback.into())
    } else {
        None
    }
}

fn extension_delivery(f: impl FnOnce() -> Delivery) -> Delivery {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|_| Delivery { recovery: Some(RECOVERY_NOTICE.into()), ..Delivery::default() })
}

fn fitted_context(all: String, e0: impl FnOnce() -> String, applied: bool, max: usize) -> String {
    if all.len() <= max {
        all
    } else if applied {
        bounded_notice(APPLIED_NOTICE.into(), APPLIED_NOTICE, max).unwrap_or_default()
    } else {
        e0()
    }
}

/// A step after `decide()` that may change what directives are delivered as (Track C §3.2).
pub trait Extension: Sync {
    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery;
    /// Optional responsibility contract. No action runs here; the caller gates treatment.
    fn session_context(&self, _repo_key: &str, _root: &Path, _index: &Index, _paths: &Paths) -> Option<String> {
        None
    }
    /// The session start's texts: the contract ([`Extension::session_context`]) and one line for
    /// the person (a grant that no longer matches what would run). An extension that can find
    /// both in one pass overrides this.
    fn session_texts(&self, repo_key: &str, root: &Path, index: &Index, paths: &Paths) -> SessionTexts {
        SessionTexts { context: self.session_context(repo_key, root, index, paths), notice: None }
    }
    /// The exec-only reflexes (`(track, route id)`, vendored scripts) this extension would carry
    /// out in this repository, so `decide()` may direct them. Asked only when the index holds one.
    fn exec_only(&self, _repo_key: &str, _index: &Index, _paths: &Paths) -> Vec<(String, String)> {
        Vec::new()
    }
    /// A fact for a tool call about to run (`PreToolUse`): completed work it would repeat, checked
    /// now. Context only, never a refusal; it goes beside any interpreter advice.
    fn pretool_context(&self, _call: &PreCall<'_>) -> Option<String> {
        None
    }
    /// The finish-line verification (Claude Code `Stop`): run an approved learned check whose
    /// result on the current inputs is not yet known, and say what happened.
    fn stop_gate(&self, _call: &StopCall<'_>) -> Option<StopGate> {
        None
    }
}

/// What [`Extension::session_texts`] says at a session start.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SessionTexts {
    /// For the model.
    pub context: Option<String>,
    /// For the person only (Claude Code's `systemMessage`), at most once per session.
    pub notice: Option<String>,
}

/// A `Stop` event, as [`Extension::stop_gate`] sees it.
pub struct StopCall<'a> {
    pub harness: &'a str,
    pub repo_key: &'a str,
    pub root: &'a Path,
    pub session: &'a str,
    pub paths: &'a Paths,
    pub index: &'a Index,
    pub stop_hook_active: bool,
    /// When the gate must be finished, applying included: the Stop hook's registered timeout,
    /// less what this event already spent and a reserve for what follows. A gate that cannot
    /// finish by then does not start.
    pub deadline: Instant,
}

/// What a Stop gate decided: continue the agent with this reason (a failure it must see), or let
/// the turn end with a line for the person. A `Block` reason is a Stop continuation like any
/// other Adage writes ([`render::finish_line`]): the hook and the learners recognise it as
/// Adage's own text.
#[derive(Debug, Clone, PartialEq)]
pub enum StopGate {
    Block(String),
    Notice(String),
}

/// A Bash call about to run, as [`Extension::pretool_context`] sees it.
pub struct PreCall<'a> {
    pub repo_key: &'a str,
    pub root: &'a Path,
    /// The session's working directory at this call (the payload's `cwd`): a relative command
    /// runs from here, which need not be the repository root.
    pub cwd: &'a Path,
    pub session: &'a str,
    pub paths: &'a Paths,
    pub index: &'a Index,
    pub command: &'a str,
}

/// How a binary runs the pipeline.
#[derive(Clone, Copy, Default)]
pub struct Options {
    /// The adage binary live learning starts (`<learner> condition --auto …`); `None`: this binary.
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
    // The harness's timeout for this hook counts from about now.
    let started = Instant::now();
    // Read (or drain) stdin first, whatever happens next.
    let payload = read_payload(&mut std::io::stdin().lock());
    if harness::adapter(harness).is_none() {
        return Err(Failure::new("config", "unknown harness"));
    }
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
    handle(harness, event, &buf, opts, &Paths::from_env(), started)
}

/// One hook event, once its payload is read: everything [`run`] does after stdin and the kill
/// switches, with Adage's data directory given (`paths`). `started` is when the hook process
/// began: the harness's registered timeout counts from then, and the Stop gate's budget is what
/// is left of it.
pub fn handle(
    harness: &str,
    event: &str,
    buf: &str,
    opts: Options,
    paths: &Paths,
    started: Instant,
) -> Result<Option<String>, Failure> {
    let Some(adapter) = harness::adapter(harness) else { return Err(Failure::new("config", "unknown harness")) };
    let p: Value = serde_json::from_str(buf)
        .map_err(|e| Failure::new("payload", format!("stdin is not a JSON payload ({:?})", e.classify())))?;
    let Some(harness::Envelope { session, cwd }) = adapter.envelope(&p) else {
        return Err(Failure::new("payload", "no session id or working directory"));
    };
    // Outside a repository (a session opened in `~`, `/`, or a directory since deleted) there is
    // nothing to learn: no notice, no learner (docs/zero-config.md §3.2). Reflexes a person keyed
    // to such a directory by hand are still delivered.
    let found = repo::find_root(&cwd);
    let in_repo = found.is_some();
    let root = found.unwrap_or_else(|| cwd.clone());
    if root.join(".adage/disabled").exists() {
        return Ok(None);
    }
    if event == "PreToolUse" {
        return pretool(adapter, &p, &session, &cwd, &root, paths, opts.extension);
    }
    // A damaged index (a truncated write, a disk error) is served as empty and rebuilt: the
    // session start asks the learner to recompile it (docs/zero-config.md §5).
    let loaded = Index::try_load(&paths.index());
    let damaged = matches!(loaded, Err(adage_tracks::index::Unloaded::Damaged));
    let index = loaded.unwrap_or_default();
    let learner = || match opts.learner {
        Some(find) => find().ok_or_else(|| Failure::new("condition", "finding the adage binary: not found")),
        None => std::env::current_exe().map_err(|e| Failure::io("condition", "finding the adage binary", &e)),
    };
    let translated = adapter.translate(event, &p, &cwd.to_string_lossy());
    if matches!(translated, Some(Event::SessionStart)) {
        let start = Start { adapter, harness, event, session: &session, root: &root, in_repo, paths, index: &index };
        return session_start(&start, damaged, &learner, opts.extension);
    }
    // Decide first, always: the session's bookkeeping (a learned obligation's Stop request, an
    // at-Stop action) must not depend on what the finish-line gate finds (review F1).
    let responded = respond(adapter, harness, event, &p, &session, &cwd, &root, paths, opts, &index);
    let out = match (translated, opts.extension) {
        (Some(Event::Stop { stop_hook_active }), Some(x)) if in_repo => {
            // One hook never stages two actions: when an approved action already ran for this
            // Stop's obligation, the gate waits for the next Stop (review F4).
            let acted = responded.as_ref().is_ok_and(|r| r.at_stop_action);
            let gate = if acted {
                None
            } else {
                let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
                repo::lookup_key(&root, Some(&paths.repo_keys()), &keys).and_then(|repo_key| {
                    let call = StopCall {
                        harness,
                        repo_key: &repo_key,
                        root: &root,
                        session: &session,
                        paths,
                        index: &index,
                        stop_hook_active,
                        deadline: stop_deadline(started),
                    };
                    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x.stop_gate(&call))).ok().flatten()
                })
            };
            if let Some(g) = &gate {
                let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
                let rec = json!({ "ts": ts, "event": event, "kind": "stop_gate", "blocked": matches!(g, StopGate::Block(_)) });
                if let Err(e) = spool::append(&paths.spool(&session), &rec) {
                    let f = Failure::io("spool", "appending to the session spool", &e);
                    failopen::record(&paths.root, harness, event, &f);
                }
            }
            match (gate, responded) {
                (None, r) => r.map(|r| r.out),
                (Some(g), Err(f)) => {
                    // The decision failed (and ran nothing); the gate's result still stands.
                    failopen::record(&paths.root, harness, event, &f);
                    Ok(Some(gate_output(g)))
                }
                (Some(g), Ok(r)) => Ok(Some(with_gate(g, r.out))),
            }
            .map(|o| o.and_then(|o| adapter.render(event, o)))
        }
        _ => responded.map(|r| r.out.and_then(|o| adapter.render(event, o))),
    };
    if in_repo && adapter.is_learn_point(event) {
        let transcript = p.get("transcript_path").and_then(|v| v.as_str());
        let learned = learner()
            .and_then(|l| autolearn::on_turn_end(paths, &root, adapter.name(), transcript, &l, false).map(drop));
        if let Err(f) = learned {
            failopen::record(&paths.root, harness, event, &f);
        }
    }
    out
}

/// Kept back from the Stop hook's registered timeout for what follows the gate (the spool
/// record, the learner request, the output).
const STOP_RESERVE: Duration = Duration::from_secs(2);

/// When the Stop gate must be done, applying included: the registered timeout of a Stop hook
/// that runs actions ([`harness::ACTION_HOOK_TIMEOUT_S`]) from the hook's start, less
/// [`STOP_RESERVE`]. Whatever the event already spent is deducted.
fn stop_deadline(started: Instant) -> Instant {
    started + Duration::from_secs(u64::from(harness::ACTION_HOOK_TIMEOUT_S)) - STOP_RESERVE
}

/// A gate result alone.
fn gate_output(g: StopGate) -> Output {
    match g {
        StopGate::Block(reason) => Output::Continue(reason),
        StopGate::Notice(n) => Output::Notice(n),
    }
}

/// The gate's result with the decision's output for the same Stop. A failing gate's
/// continuation and a learned obligation's request (or an action's) both reach the agent, in one
/// continuation; a passing gate's line for the person yields to anything for the agent.
fn with_gate(g: StopGate, decided: Option<Output>) -> Output {
    match (g, decided) {
        (StopGate::Block(reason), Some(Output::Continue(other))) => {
            Output::Continue(render::join_finish_lines(&reason, &other))
        }
        // A Stop carries no context: anything else decided here has no channel at a Stop.
        (StopGate::Block(reason), _) => Output::Continue(reason),
        (StopGate::Notice(_), Some(o)) => o,
        (StopGate::Notice(n), None) => Output::Notice(n),
    }
}

/// The marker `adage delegate rewrites off` (and `delegate off`) writes: no `PreToolUse` rewrite on
/// this machine (docs/execution.md, "Rewrites to the project's environment").
pub fn rewrite_off(paths: &Paths) -> PathBuf {
    paths.root.join("exec").join("rewrite-off")
}

/// The environment's Python version, major.minor, from its `pyvenv.cfg` (`version = 3.12.4`, or
/// uv's `version_info = 3.12.4`).
fn venv_version(venv: &Path) -> Option<String> {
    let cfg = std::fs::read_to_string(venv.join("pyvenv.cfg")).ok()?;
    cfg.lines().find_map(|l| {
        let (k, v) = l.split_once('=')?;
        matches!(k.trim(), "version" | "version_info").then(|| {
            let mut parts = v.trim().split('.');
            format!("{}.{}", parts.next().unwrap_or(""), parts.next().unwrap_or(""))
        })
    })
}

/// Whether the environment at `venv` belongs to the checkout at `root`, not to the checkout the
/// instincts were bound in (`bound`): it is not a link into another directory, and no `.pth` file
/// or editable finder in it points into `bound` outside `root` (a worktree whose `.venv` is a link
/// to, or a copy of, the main checkout's would test the main checkout's code).
fn env_of_checkout(venv: &Path, root: &Path, bound: &Path) -> bool {
    let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let (croot, cbound) = (canon(root), canon(bound));
    if !canon(venv).starts_with(&croot) {
        return false;
    }
    let elsewhere = |text: &str| {
        text.split(|c: char| c == '\n' || c == '"' || c == '\'' || c.is_whitespace()).any(|t| {
            let p = Path::new(t.trim_end_matches([',', ')', ']', '}']));
            p.is_absolute()
                && (p.starts_with(bound) || p.starts_with(&cbound))
                && !(p.starts_with(root) || p.starts_with(&croot))
        })
    };
    let libs = std::fs::read_dir(venv.join("lib")).into_iter().flatten().flatten();
    for lib in libs {
        let site = lib.path().join("site-packages");
        for f in std::fs::read_dir(&site).into_iter().flatten().flatten() {
            let name = f.file_name().to_string_lossy().into_owned();
            let finder = name.starts_with("__editable__") && name.ends_with(".py");
            if !(name.ends_with(".pth") || finder) || f.metadata().map_or(true, |m| m.len() > 64 << 10) {
                continue;
            }
            if std::fs::read_to_string(f.path()).is_ok_and(|t| elsewhere(&t)) {
                return false;
            }
        }
    }
    true
}

/// The Bash allow rules of the person's Claude Code settings that apply in `root`: user
/// (`$CLAUDE_CONFIG_DIR` or `~/.claude`), project and local settings. Unreadable files add
/// nothing. Read only when a rewrite is planned.
fn claude_allow_rules(root: &Path) -> Vec<String> {
    let user = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".claude")));
    let files = user
        .map(|u| u.join("settings.json"))
        .into_iter()
        .chain([root.join(".claude/settings.json"), root.join(".claude/settings.local.json")]);
    let mut out = Vec::new();
    for f in files {
        let Ok(text) = std::fs::read_to_string(&f) else { continue };
        let Ok(v) = serde_json::from_str::<Value>(&text) else { continue };
        let rules = v.pointer("/permissions/allow").and_then(Value::as_array).into_iter().flatten();
        out.extend(rules.filter_map(Value::as_str).filter(|r| r.starts_with("Bash")).map(str::to_string));
    }
    out
}

/// `PreToolUse` (Claude Code only; other harnesses keep the post-hoc directive): a Python test run
/// or scratch repro under the system Python runs with the project's verified environment instead
/// (only the interpreter prefix changes), and a `pip install` of the project or its test
/// dependencies into the system Python gets advice ([`adage_runtime::pretool`]). On by default for
/// a repository whose U11 instinct is in service and whose environment was verified when it was
/// bound; off with `adage delegate rewrites off`; never in the control arm, nor in a session
/// started in an activated environment.
fn pretool(
    adapter: &dyn harness::Adapter,
    p: &Value,
    session: &str,
    cwd: &Path,
    root: &Path,
    paths: &Paths,
    extension: Option<&'static dyn Extension>,
) -> Result<Option<String>, Failure> {
    use adage_runtime::pretool::{self, Pre};
    if adapter.name() != "claude" || p.get("tool_name").and_then(Value::as_str) != Some("Bash") {
        return Ok(None);
    }
    let input = p.get("tool_input").cloned().unwrap_or(Value::Null);
    let Some(command) = input.get("command").and_then(Value::as_str) else { return Ok(None) };
    // The cheap no-match path: no Python program named at all (words, not substrings: `pipeline`
    // is not `pip`).
    let python_word = |w: &str| matches!(w, "pip" | "pip3" | "pytest" | "py.test") || w.starts_with("python");
    if !command.split(|c: char| !(c.is_ascii_alphanumeric() || c == '.' || c == '_')).any(python_word) {
        return Ok(None);
    }
    let key = repo::repo_key_cached(root, &paths.repo_keys());
    // A current receipt covering this call, whatever the interpreter advice below decides. Only
    // treated sessions hold receipts; the extension rechecks freshness now.
    let control = || {
        state::try_read::<SessionState>(&paths.state(session))
            .ok()
            .flatten()
            .is_some_and(|s| s.arm == Some(Arm::Control))
    };
    // Like every other extension call: a panic loses only this note, never the call's rewrite.
    let receipt = extension.filter(|_| !control()).and_then(|x| {
        let index = Index::try_load(&paths.index()).unwrap_or_default();
        let call = PreCall { repo_key: &key, root, cwd, session, paths, index: &index, command };
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x.pretool_context(&call))).ok().flatten()
    });
    // A receipt delivered with no rewrite or advice beside it is recorded too (with no `pretool`
    // action), so evaluation sees every delivery.
    let only_receipt = || {
        let text = receipt.clone()?;
        let st = state::try_read::<SessionState>(&paths.state(session)).ok().flatten();
        let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
        let rec = json!({ "ts": ts, "event": "PreToolUse", "kind": "pretool", "arm": st.as_ref().and_then(|s| s.arm),
                          "propensity": st.as_ref().and_then(|s| s.propensity), "repo": key, "receipt": true });
        if let Err(e) = spool::append(&paths.spool(session), &rec) {
            // Measurement is lost, but the note stands.
            failopen::record(
                &paths.root,
                adapter.name(),
                "PreToolUse",
                &Failure::io("spool", "appending to the session spool", &e),
            );
        }
        adapter.render("PreToolUse", Output::Context(text))
    };
    // The person started the session in an environment of their choosing (a virtualenv, a conda
    // environment): `python` and `pip` are that environment's, and Adage leaves them alone.
    if ["VIRTUAL_ENV", "CONDA_PREFIX"].iter().any(|v| std::env::var_os(v).is_some_and(|x| !x.is_empty())) {
        return Ok(only_receipt());
    }
    // Off with `runtime.rewrite = false` (`adage delegate rewrites off`), or the older marker.
    let cfg = adage_store::config::load(paths).config;
    if !cfg.runtime_rewrite || rewrite_off(paths).exists() {
        return Ok(only_receipt());
    }
    let Some(binding) = adage_learn::instincts::binding(&paths.tracks(), &key) else { return Ok(only_receipt()) };
    let Some(mut env) = binding.env else { return Ok(only_receipt()) };
    let venv = root.join(env.interpreter.strip_suffix("/bin/python").unwrap_or(".venv"));
    if Path::new(&binding.root) != root {
        // Another checkout of the repository (a per-task worktree, a fresh clone) until the
        // rebind this session start asked for lands: its own environment, by the presence of its
        // interpreter, `pyvenv.cfg` and pytest (a few `stat`s), and only while that environment
        // is this checkout's (review S3). Only test runs: a repro needs the bind-time check that
        // the project imports from this checkout.
        let pytest = std::fs::read_dir(venv.join("lib"))
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| e.path().join("site-packages/pytest/__init__.py").is_file());
        let usable = root.join(&env.interpreter).is_file() && venv.join("pyvenv.cfg").is_file() && pytest;
        if !(usable && env_of_checkout(&venv, root, Path::new(&binding.root))) {
            return Ok(only_receipt());
        }
        env.pytest = true;
        env.modules.clear();
    }
    // U11 in service here (not extinguished, rejected or turned off).
    let index = Index::try_load(&paths.index()).unwrap_or_default();
    let u11 = index.for_repo(&key).any(|r| {
        matches!(&r.route.evidence.origin, Some(adage_tracks::Origin::Instinct { instinct, .. }) if instinct == "U11")
    });
    if !u11 {
        return Ok(only_receipt());
    }
    let abs = root.join(&env.interpreter);
    let e = pretool::Env {
        interpreter: pretool::shell_word(&abs.to_string_lossy()),
        shown: env.interpreter.clone(),
        // At the root, the command names the interpreter as the person (and their permission
        // rules) do: `.venv/bin/python` (first-run QA item 7).
        relative: Path::new(&env.interpreter).is_relative().then(|| pretool::shell_word(&env.interpreter)),
        pytest: env.pytest,
        modules: env.modules.clone(),
        root: root.to_path_buf(),
        cwd: cwd.to_path_buf(),
        version: venv_version(&venv),
        uv: root.join("uv.lock").is_file(),
    };
    let read = |script: &str| {
        let p = cwd.join(script);
        let small = std::fs::metadata(&p).is_ok_and(|m| m.is_file() && m.len() <= 256 << 10);
        small.then(|| std::fs::read_to_string(&p).ok()).flatten()
    };
    let Some(mut pre) = pretool::plan(command, &e, read) else { return Ok(only_receipt()) };
    // A rewrite the person's permission rules would treat differently from the command as written
    // is advice instead (review S4).
    if let Pre::Rewrite { command: new, kind, segments, .. } = &pre {
        let mode = p.get("permission_mode").and_then(Value::as_str);
        if !pretool::keeps_permissions(segments, &claude_allow_rules(root), mode) {
            pre = Pre::Advise { reason: pretool::permission_advice(&e, kind, new), kind: "permissions" };
        }
    }
    // The arm: assigned now if the session has none yet; the control arm is left as it is.
    let arm_env = std::env::var("ADAGE_ARM").ok();
    let share: f64 = cfg.runtime_control;
    let (arm, propensity) = state::update::<SessionState, _>(&paths.state(session), |st| {
        if st.arm.is_none() {
            let (arm, pr) = assign(session, share, arm_env.as_deref());
            st.arm = Some(arm);
            st.propensity = Some(pr);
            st.repo = Some(key.clone());
        }
        (st.arm, st.propensity)
    })
    .map_err(|e| Failure::io("state", "assigning the session's arm", &e))?;
    let control = arm == Some(Arm::Control);
    // The receipt leads: a covered run is evidence already in hand, the interpreter is secondary.
    let lead = |text: String| match &receipt {
        Some(r) => format!("{r}\n{text}"),
        None => text,
    };
    let (what, out) = match &pre {
        Pre::Rewrite { command: new, kind, interpreter, .. } => {
            let mut updated = input.clone();
            updated["command"] = Value::String(new.clone());
            // Both notes name the interpreter word that was substituted, as it now reads in the command.
            let notice = format!(
                "adage: pointed this {} at {} (the project's environment).",
                if *kind == "test" { "test run" } else { "Python run" },
                interpreter
            );
            (
                json!({ "rewrite": kind, "interpreter": env.interpreter }),
                Output::Rewrite { input: updated, context: lead(pretool::rewrite_note(interpreter, kind)), notice },
            )
        }
        // Advice, never a refusal: the command runs as written, and the person's own permission
        // rules decide it (review B2).
        Pre::Advise { reason, kind } => {
            (json!({ "advise": kind, "interpreter": env.interpreter }), Output::Context(lead(reason.clone())))
        }
    };
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut rec = json!({ "ts": ts, "event": "PreToolUse", "kind": "pretool", "arm": arm, "propensity": propensity, "repo": key, "pretool": what, "receipt": receipt.is_some() });
    if control {
        rec["withheld"] = json!(true);
    }
    spool::append(&paths.spool(session), &rec)
        .map_err(|e| Failure::io("spool", "appending to the session spool", &e))?;
    if control {
        return Ok(None);
    }
    Ok(adapter.render("PreToolUse", out))
}

/// What a session start is about.
struct Start<'a> {
    adapter: &'a dyn harness::Adapter,
    harness: &'a str,
    event: &'a str,
    session: &'a str,
    root: &'a Path,
    /// `root` is a repository (it has `.git`), not merely the session's working directory.
    in_repo: bool,
    paths: &'a Paths,
    index: &'a Index,
}

/// A session start (docs/zero-config.md §3.2, §3.3):
///
/// 1. In a repository with reflexes, the session is assigned its arm now and one `session_start`
///    record is spooled (`arm`, `propensity`, `repo`, `generation`): every exposed session is
///    then counted by the savings estimate, including those where nothing ever matched, and the
///    bootstrap session below, whose reflexes arrive while it runs.
/// 2. **Bootstrap.** With `learn.auto` on, a repository with no reflexes whose learner has not
///    run since the index last changed, or a damaged index, starts a learner at once
///    (`condition --auto --bootstrap`): instincts and the index within about 100 ms, then the
///    repository's own traces.
/// 3. **Notice.** One line for the person (Claude Code's `systemMessage`; the model never sees
///    it): at the first bootstrap, what Adage is doing; afterwards, the learner's latest notice,
///    at most once a day and only when new. Rebuilding a damaged index says nothing.
fn session_start(
    s: &Start<'_>,
    damaged: bool,
    learner: &dyn Fn() -> Result<PathBuf, Failure>,
    extension: Option<&dyn Extension>,
) -> Result<Option<String>, Failure> {
    let keys: std::collections::BTreeSet<&str> = s.index.routes.iter().map(|r| r.repo.as_str()).collect();
    let routed = repo::lookup_key(s.root, Some(&s.paths.repo_keys()), &keys);
    if let Some(repo_key) = &routed {
        if let Err(f) = record_start(s, repo_key) {
            failopen::record(&s.paths.root, s.harness, s.event, &f);
        }
    }
    // Outside a repository: nothing to learn, so no learner and nothing to say. (Not even a
    // damaged index is rebuilt from here: the next session in a repository does that.)
    if !s.in_repo {
        return Ok(None);
    }
    let cfg = adage_store::config::load(s.paths).config;
    let key = routed.clone().unwrap_or_else(|| repo::repo_key_cached(s.root, &s.paths.repo_keys()));
    let q = autolearn::Queue::new(s.paths, &key);
    let run: Option<autolearn::RunState> = autolearn::read_json(&q.state());
    let first = run.is_none() && !q.shown().exists();
    let stale = || {
        let index_at = std::fs::metadata(s.paths.index())
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as u64);
        match (&run, index_at) {
            (None, _) => true,
            (Some(r), Some(at)) => r.finished_at.is_some_and(|f| f < at),
            (Some(_), None) => true,
        }
    };
    let bootstrap = cfg.learn_auto && (damaged || (routed.is_none() && stale()));
    // The bootstrap session is a session in this repository too: its reflexes arrive while it
    // runs, so it is assigned its arm now and counted by the savings estimate like any other
    // (without this record it never was, and the card's session count lagged one behind).
    if bootstrap && routed.is_none() {
        if let Err(f) = record_start(s, &key) {
            failopen::record(&s.paths.root, s.harness, s.event, &f);
        }
    }
    // Instincts bind against the live checkout, this session's (docs/instincts.md): bound against
    // another one (a per-task worktree, a clone the traces name), or before its declared files
    // changed (a `.venv` made since), they are bound again, by the same fast first phase as a
    // bootstrap. A few dozen `stat`s decide it (`adage_instincts::fingerprint`).
    let rebind = !bootstrap
        && !damaged
        && cfg.learn_auto
        && cfg.instincts_enabled
        && adage_learn::instincts::stale_binding(&s.paths.tracks(), &key, s.root);
    if bootstrap || rebind {
        let asked = learner().and_then(|l| autolearn::on_turn_end(s.paths, s.root, s.adapter.name(), None, &l, true));
        if let Err(f) = asked {
            failopen::record(&s.paths.root, s.harness, s.event, &f);
        }
    }
    let context = if !damaged
        && s.adapter.name() != "opencode"
        && state::try_read::<SessionState>(&s.paths.state(s.session))
            .ok()
            .flatten()
            .is_some_and(|st| st.arm == Some(Arm::Treated))
    {
        extension
            .and_then(|ext| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    ext.session_texts(&key, s.root, s.index, s.paths)
                }))
                .ok()
            })
            .unwrap_or_default()
    } else {
        SessionTexts::default()
    };
    // A grant that stopped matching is said once per session, beside any other notice.
    let lapse = context.notice.filter(|_| cfg.ui_notices && first_lapse_notice(s.paths, s.session));
    let context = context.context.filter(|text| text.len() <= s.adapter.max_context());
    if damaged || !cfg.ui_notices {
        return Ok(
            context.and_then(|context| s.adapter.render(s.event, Output::SessionContext { context, notice: None }))
        );
    }
    let text = if bootstrap && first {
        let now = autolearn::now_ms();
        match autolearn::write_json(&q.shown(), &autolearn::Shown { id: "bootstrap".into(), at: now }) {
            Ok(()) => Some(first_notice(s.root, &cfg)),
            // The data directory cannot be written: nothing can be learned, and a notice shown now
            // could not be remembered, so it would repeat at every session start. Say what is
            // wrong instead, once.
            Err(e) => unwritable_notice(s.paths, &e),
        }
    } else {
        autolearn::take_notice(s.paths, &key)
    };
    let text = match (text, lapse) {
        (Some(t), Some(l)) => Some(format!("{t}\n{l}")),
        (t, l) => t.or(l),
    };
    Ok(match context {
        Some(context) => s.adapter.render(s.event, Output::SessionContext { context, notice: text }),
        None => text.and_then(|t| s.adapter.render(s.event, Output::Notice(t))),
    })
}

/// What the first session in a repository says: how many of its earlier traces Adage is learning
/// from (Claude Code project directories, counted by name: nothing is opened here).
fn first_notice(root: &Path, cfg: &adage_store::config::Config) -> String {
    let projects = cfg
        .learn_claude_projects
        .clone()
        .or_else(|| adage_store::home_dir().map(|h| h.join(".claude").join("projects")));
    let n = projects
        .map(|p| adage_learn::scope::count_claude_sessions(&p, &adage_learn::scope::checkouts(root)))
        .unwrap_or(0);
    match n {
        0 => "adage: learning from this repo as you work; its first reflexes form in a few sessions. Nothing runs without you.".into(),
        1 => "adage: learning from 1 past session in this repo; reflexes arrive within a minute. Nothing runs without you.".into(),
        n => format!("adage: learning from {n} past sessions in this repo; reflexes arrive within a minute. Nothing runs without you."),
    }
}

/// The one line said when the data directory cannot be written (at most once per data
/// directory: a marker in the system's temporary directory remembers it; if that cannot be
/// written either, nothing is said rather than the same line at every session start).
fn unwritable_notice(paths: &Paths, e: &std::io::Error) -> Option<String> {
    let dir = paths.root.to_string_lossy();
    let marker = std::env::temp_dir().join(format!("adage-unwritable-{}", repo::short_hash(&dir)));
    std::fs::OpenOptions::new().write(true).create_new(true).open(marker).ok()?;
    let why = e.to_string();
    let why = why.split(" (os error").next().unwrap_or(&why).to_lowercase();
    Some(format!(
        "adage: can't write its data directory {dir} ({why}), so it cannot learn here. Set ADAGE_HOME to a writable directory; `adage doctor` checks it."
    ))
}

/// Assign the session's arm (if it has none yet) and spool one `session_start` record.
fn record_start(s: &Start<'_>, repo_key: &str) -> Result<(), Failure> {
    let arm_env = std::env::var("ADAGE_ARM").ok();
    let share: f64 = adage_store::config::load(s.paths).config.runtime_control;
    let (arm, propensity) = state::update::<SessionState, _>(&s.paths.state(s.session), |st| {
        if st.arm.is_none() {
            let (arm, p) = assign(s.session, share, arm_env.as_deref());
            st.arm = Some(arm);
            st.propensity = Some(p);
            st.repo = Some(repo_key.to_string());
        }
        (st.arm, st.propensity)
    })
    .map_err(|e| Failure::io("state", "assigning the session's arm", &e))?;
    let ts = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    let mut rec = json!({ "ts": ts, "event": s.event, "kind": "session_start", "arm": arm, "propensity": propensity, "repo": repo_key, "generation": s.index.generation });
    if s.adapter.name() != "claude" {
        rec["harness"] = json!(s.adapter.name());
    }
    spool::append(&s.paths.spool(s.session), &rec)
        .map_err(|e| Failure::io("spool", "appending to the session spool", &e))
}

/// Decide: everything after the kill switches, up to rendering.
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
    index: &Index,
) -> Result<Responded, Failure> {
    let keys: std::collections::BTreeSet<&str> = index.routes.iter().map(|r| r.repo.as_str()).collect();
    let Some(repo_key) = repo::lookup_key(root, Some(&paths.repo_keys()), &keys) else {
        return Ok(Responded::default()); // nothing installed for this repository: the common, cheapest path
    };
    // An event this adapter does not act on.
    let Some(ev) = adapter.translate(event, p, &cwd.to_string_lossy()) else { return Ok(Responded::default()) };
    // An event that cannot change any decision stops here, before the command table, the state
    // lock and the spool (Design §13.2; `adage_runtime::fast`).
    let state_path = paths.state(session);
    let read_state = || state::try_read::<SessionState>(&state_path).ok().flatten();
    if fast::skip(&index.triggers(&repo_key), &ev, &cwd.to_string_lossy(), read_state) {
        return Ok(Responded::default());
    }
    let table = CommandTable::load();
    // The embedding model is opened only if the runtime asks, which it does only at a prompt in a
    // repository with intent routes: the installed model, else the one bundled in the binary
    // (`model::resolve`, as the learner chose). No model: no vector, no intent directive.
    let models = paths.models();
    let embed = move |text: &str| -> Option<(String, Vec<f32>)> {
        let m = model::resolve_static(&models, false)?;
        let v = m.embed(&normalize(text))?;
        Some((m.id, v))
    };
    let ctx =
        Ctx { index, repo: repo_key.clone(), root, table: &table, embed: Some(&embed), params: Default::default() };
    let arm_env = std::env::var("ADAGE_ARM").ok();
    // ADAGE_CONTROL, else `runtime.control` in config.toml, else 0.1.
    let share: f64 = adage_store::config::load(paths).config.runtime_control;
    let spool_path = paths.spool(session);
    // Exec-only reflexes come into play only under an extension that carries them out.
    let exec_only: Vec<(String, String)> = match opts.extension {
        Some(x) if index.for_repo(&repo_key).any(|r| r.exec_only()) => {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| x.exec_only(&repo_key, index, paths)))
                .unwrap_or_default()
        }
        _ => Vec::new(),
    };
    // The learned follow judge's weights for this repository (fitted at learn time), if any.
    let weights = adage_learn::read_follow_weights(&paths.tracks(), &repo_key);
    // A state that can no longer be read is rebuilt from the session's spool, if it is small
    // enough to replay quickly (it is read under the state lock).
    let rebuild = || {
        let small = std::fs::metadata(&spool_path).is_ok_and(|m| m.len() <= rebuild::MAX_SPOOL_BYTES);
        let records: Vec<Value> = if small { spool::read_all(&spool_path).ok()? } else { return None };
        rebuild::rebuild(&records, index, &repo_key, &table)
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
                (_, Some("control" | "treated")) => 1.0,
                (Some(Arm::Control), _) => share.clamp(0.0, 1.0),
                _ => 1.0 - share.clamp(0.0, 1.0),
            });
            let before: Vec<(String, bool)> =
                st.obligations.iter().map(|o| (o.route.clone(), o.stop_requested)).collect();
            // ADAGE_CARD_CHECKS=0 (an evaluation knob, like ADAGE_ARM) leaves the card's test-command
            // line out.
            let card_checks = std::env::var("ADAGE_CARD_CHECKS").map_or(true, |v| v != "0");
            // ADAGE_CARD_ORIENTATION=0 (likewise) gives the card its earlier form: the exemplar of the
            // last edit instead of where its files stand in the checkout now.
            let card_orientation = std::env::var("ADAGE_CARD_ORIENTATION").map_or(true, |v| v != "0");
            let d = adage_runtime::with_follow_weights(weights.clone(), || {
                adage_runtime::with_card_checks(card_checks, || {
                    adage_runtime::with_card_orientation(card_orientation, || {
                        adage_runtime::with_exec_only(exec_only.clone(), || decide(&ctx, st, &ev))
                    })
                })
            });
            let stims = stimuli(&before, &st.obligations, matches!(ev, Event::Batch { .. }));
            (d, st.arm, propensity, stims)
        })
        .map_err(|e| Failure::io("state", "updating the session state", &e))?;

    // An extension (adage-exec: E1 prefetch, E2 actions) sees the directives about to be
    // delivered and the obligations armed or enforced now. A panic cannot prove no action ran.
    let mut delivery = Delivery::default();
    if let Some(x) = opts.extension {
        let directives_due = decision.continue_reason.is_none() && !decision.directives.is_empty();
        if directives_due || !stims.is_empty() {
            let turn = Turn {
                harness,
                event,
                event_identity: adapter.event_identity(event, p),
                ev: &ev,
                session,
                index,
                repo_key: &repo_key,
                root,
                cwd,
                paths,
                max_context: adapter.max_context(),
                arm,
                stimuli: &stims,
            };
            delivery = extension_delivery(|| x.deliver(&turn, &decision.directives));
        }
    }

    // Obligations an action met: no Stop request follows for them.
    if !delivery.met.is_empty() || !delivery.cpu_predictions.is_empty() {
        let met = delivery.met.clone();
        let withdrawn = matches!(delivery.stop, Some(None));
        let marked = state::update::<SessionState, _>(&state_path, |st| {
            st.pending.retain(|p| !delivery.cpu_predictions.contains(&p.route));
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
    // `propensity`: the probability of `arm` at assignment (control share, or one minus it; 1 when
    // `ADAGE_ARM` forced it), for inverse-propensity weighting (Design §11, §14.1).
    let mut rec = json!({ "ts": ts, "event": event, "kind": ev.name(), "arm": arm, "propensity": propensity, "repo": repo_key, "generation": index.generation, "decision": decision });
    if adapter.name() != "claude" {
        rec["harness"] = json!(adapter.name());
    }
    if let Some(x) = delivery.spool.take() {
        rec["exec"] = x;
    }
    if delivery.recovery.is_some() {
        rec["recovery_required"] = json!(true);
    }
    if let Err(e) = spool::append(&paths.spool(session), &rec) {
        // Measurement is lost, but the decision stands: record it and carry on.
        failopen::record(&paths.root, harness, event, &Failure::io("spool", "appending to the session spool", &e));
    }

    let at_stop_action = delivery.at_stop_action;
    let done = |out: Option<Output>| Ok(Responded { out, at_stop_action });
    if let Some(warning) = delivery.recovery.take() {
        return done(bounded_notice(warning, RECOVERY_NOTICE, adapter.max_context()).map(|text| {
            // At a Stop the warning is a continuation, marked as Adage's own text like every other.
            if matches!(ev, Event::Stop { .. }) {
                Output::Continue(render::finish_line(&text))
            } else {
                Output::Context(text)
            }
        }));
    }
    if let Some(reason) = decision.continue_reason {
        return done(Some(Output::Continue(reason)));
    }
    // A grant that stopped matching: one line for the person, once per session, beside whatever
    // the agent receives (never at a Stop, which has no such channel).
    let lapse = delivery.notice.take().filter(|_| {
        !matches!(ev, Event::Stop { .. })
            && adage_store::config::load(paths).config.ui_notices
            && first_lapse_notice(paths, session)
    });
    let with_lapse = |text: String| match &lapse {
        Some(n) if !text.is_empty() => Output::SessionContext { context: text, notice: Some(n.clone()) },
        Some(n) => Output::Notice(n.clone()),
        None => Output::Context(text),
    };
    if decision.directives.is_empty() && delivery.extra.is_empty() {
        return done(lapse.map(Output::Notice));
    }
    let max = adapter.max_context();
    let e0 = || decision.directives.iter().map(render::wrap_directive).collect::<Vec<_>>().join("\n\n");
    let texts = match delivery.texts {
        Some(t) if t.len() == decision.directives.len() => t,
        _ if !delivery.met.is_empty() => vec![APPLIED_NOTICE.into()],
        _ => decision.directives.iter().map(render::wrap_directive).collect(),
    };
    let all: Vec<String> = texts.into_iter().chain(delivery.extra).collect();
    // An applied action cannot fall back to its original "run this" directive on overflow.
    let mut text = fitted_context(all.join("\n\n"), e0, !delivery.met.is_empty(), max);
    if text.is_empty() {
        return done(lapse.map(Output::Notice));
    }
    // Design W8 (spill overflow to a file) cannot arise: the longest decision validation allows
    // fits every harness's cap (`the_largest_decision_fits_every_harness_cap`). This is a backstop.
    if text.len() > max {
        text.truncate(text.char_indices().take_while(|(i, _)| *i < max).last().map(|(i, _)| i).unwrap_or(0));
    }
    done(Some(with_lapse(text)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_contract_and_person_notice_use_separate_channels() {
        use crate::harness::Adapter;
        let value = harness::claude::Claude
            .render(
                "SessionStart",
                Output::SessionContext { context: "model contract".into(), notice: Some("person notice".into()) },
            )
            .unwrap();
        let value: Value = serde_json::from_str(&value).unwrap();
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "model contract");
        assert_eq!(value["systemMessage"], "person notice");
    }

    #[test]
    fn pretool_receipt_context_reaches_a_python_test_call_without_any_rewrite() {
        struct Receipts;
        impl Extension for Receipts {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn pretool_context(&self, call: &PreCall<'_>) -> Option<String> {
                call.command.contains("pytest").then(|| format!("receipt for {}", call.session))
            }
        }
        static RECEIPTS: Receipts = Receipts;
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("home"));
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let call = |command: &str| {
            let p = json!({ "tool_name": "Bash", "tool_input": { "command": command } });
            pretool(&harness::claude::Claude, &p, "s-1", &root, &root, &paths, Some(&RECEIPTS)).unwrap()
        };
        let out: Value =
            serde_json::from_str(&call(".venv/bin/python -m pytest tests/test_x.py::t -q").unwrap()).unwrap();
        assert_eq!(out["hookSpecificOutput"]["additionalContext"], "receipt for s-1");
        assert!(out["hookSpecificOutput"].get("updatedInput").is_none(), "context only, never a rewrite or refusal");
        assert!(out["hookSpecificOutput"].get("permissionDecision").is_none());
        assert!(call(".venv/bin/python -c 'print(1)'").is_none());
        assert!(call("git status").is_none());
        // F8: a receipt delivered alone is recorded, with no rewrite or advice in the record.
        let recs: Vec<Value> = spool::read_all(&paths.spool("s-1")).unwrap();
        assert_eq!(recs.len(), 1, "{recs:?}");
        assert_eq!((recs[0]["kind"].as_str(), recs[0]["receipt"].as_bool()), (Some("pretool"), Some(true)));
        assert!(recs[0].get("pretool").is_none(), "{recs:?}");
        state::update::<SessionState, _>(&paths.state("s-1"), |st| st.arm = Some(Arm::Control)).unwrap();
        assert!(call(".venv/bin/python -m pytest tests/test_x.py -q").is_none(), "never in the control arm");
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": "python -m pytest" } });
        assert!(pretool(&harness::claude::Claude, &p, "s-1", &root, &root, &paths, None).unwrap().is_none());
    }

    /// F8: `pretool_context` is wrapped like every other extension call: a panic there loses only
    /// the note, and the hook still answers.
    #[test]
    fn a_panicking_receipt_check_loses_only_the_note() {
        struct Panics;
        impl Extension for Panics {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn pretool_context(&self, _: &PreCall<'_>) -> Option<String> {
                panic!("a receipt check that panics")
            }
        }
        static PANICS: Panics = Panics;
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("home"));
        let root = temp.path().join("repo");
        std::fs::create_dir_all(&root).unwrap();
        let p = json!({ "tool_name": "Bash", "tool_input": { "command": ".venv/bin/python -m pytest tests -q" } });
        let out = pretool(&harness::claude::Claude, &p, "s-1", &root, &root, &paths, Some(&PANICS));
        assert!(matches!(out, Ok(None)), "{out:?}");
    }

    #[test]
    fn session_contract_is_treatment_only_and_independent_of_ui_notices() {
        struct Contract;
        impl Extension for Contract {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery::default()
            }
            fn session_context(&self, _: &str, _: &Path, _: &Index, _: &Paths) -> Option<String> {
                Some("test execution contract".into())
            }
        }
        let temp = tempfile::tempdir().unwrap();
        let paths = Paths::at(temp.path().join("home"));
        std::fs::create_dir_all(&paths.root).unwrap();
        std::fs::write(paths.root.join("config.toml"), "[learn]\nauto = false\n[ui]\nnotices = \"off\"\n").unwrap();
        let index = Index::default();
        let start = Start {
            adapter: &harness::claude::Claude,
            harness: "claude",
            event: "SessionStart",
            session: "contract-test",
            root: temp.path(),
            in_repo: true,
            paths: &paths,
            index: &index,
        };
        let learner = || Err(Failure::new("test", "no learner needed"));
        for (arm, expected) in [(Arm::Control, false), (Arm::Treated, true)] {
            state::update::<SessionState, _>(&paths.state(start.session), |st| st.arm = Some(arm)).unwrap();
            let result = session_start(&start, false, &learner, Some(&Contract)).unwrap();
            assert_eq!(result.is_some(), expected);
            if let Some(result) = result {
                let result: Value = serde_json::from_str(&result).unwrap();
                assert_eq!(result["hookSpecificOutput"]["additionalContext"], "test execution contract");
                assert!(result.get("systemMessage").is_none());
            }
        }
        assert!(session_start(&start, true, &learner, Some(&Contract)).unwrap().is_none());
        assert!(session_start(&start, false, &learner, None).unwrap().is_none());
    }

    #[test]
    fn execution_fallback_never_reintroduces_an_action_after_an_uncertain_result() {
        let panic = extension_delivery(|| panic!("after a possible side effect"));
        assert!(panic.recovery.unwrap().contains("Do not retry"));
        let overlong = format!("{}{}", RECOVERY_NOTICE, "x".repeat(10_000));
        let warning = bounded_notice(overlong, RECOVERY_NOTICE, 512).unwrap();
        assert_eq!(warning, RECOVERY_NOTICE);
        assert!(bounded_notice("x".repeat(1000), RECOVERY_NOTICE, 10).is_none());
        let applied = fitted_context("x".repeat(10_000), || panic!("must not restore run instruction"), true, 512);
        assert_eq!(applied, APPLIED_NOTICE);
        let readonly = fitted_context("x".repeat(10_000), || "read-only advice".into(), false, 512);
        assert_eq!(readonly, "read-only advice");
    }

    /// Review S3: a worktree whose `.venv` is the main checkout's (a link, or a copy with the
    /// main checkout's editable install) would test the main checkout's code: no rewrite there.
    #[test]
    fn a_worktree_environment_must_be_its_own() {
        let d = tempfile::tempdir().unwrap();
        let main = d.path().join("click");
        let wt = main.join(".claude/worktrees/agent-1");
        let site = |root: &Path| root.join(".venv/lib/python3.12/site-packages");
        for r in [&main, &wt] {
            std::fs::create_dir_all(site(r)).unwrap();
            std::fs::write(r.join(".venv/pyvenv.cfg"), "home = /usr/bin\nversion_info = 3.12.4\n").unwrap();
        }
        assert_eq!(venv_version(&wt.join(".venv")).as_deref(), Some("3.12"));
        // Its own editable install: fine.
        std::fs::write(site(&wt).join("click.pth"), format!("{}\n", wt.join("src").display())).unwrap();
        assert!(env_of_checkout(&wt.join(".venv"), &wt, &main));
        // A copy of the main checkout's environment: its `.pth` points at the main checkout.
        std::fs::write(site(&wt).join("click.pth"), format!("{}\n", main.join("src").display())).unwrap();
        assert!(!env_of_checkout(&wt.join(".venv"), &wt, &main));
        // An editable finder (setuptools' `__editable__` mapping) that points there.
        std::fs::write(site(&wt).join("click.pth"), "").unwrap();
        std::fs::write(
            site(&wt).join("__editable___click_8_finder.py"),
            format!("MAPPING = {{'click': '{}'}}\n", main.join("src/click").display()),
        )
        .unwrap();
        assert!(!env_of_checkout(&wt.join(".venv"), &wt, &main));
        // A link to the main checkout's environment.
        #[cfg(unix)]
        {
            let linked = d.path().join("linked");
            std::fs::create_dir_all(&linked).unwrap();
            std::os::unix::fs::symlink(main.join(".venv"), linked.join(".venv")).unwrap();
            assert!(!env_of_checkout(&linked.join(".venv"), &linked, &main));
        }
    }

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

    /// A stand-in for adage-exec: its finish-line gate answers from a script (one answer per Stop,
    /// in order), and it counts how often it was asked.
    struct Scripted {
        answers: std::sync::Mutex<Vec<StopGate>>,
        asked: std::sync::atomic::AtomicUsize,
    }

    impl Extension for Scripted {
        fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
            Delivery::default()
        }
        fn stop_gate(&self, _: &StopCall<'_>) -> Option<StopGate> {
            self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let mut a = self.answers.lock().unwrap();
            (!a.is_empty()).then(|| a.remove(0))
        }
    }

    fn scripted(answers: Vec<StopGate>) -> &'static Scripted {
        Box::leak(Box::new(Scripted { answers: std::sync::Mutex::new(answers), asked: Default::default() }))
    }

    /// A treated session in a repository whose one learned reflex is a finish-line obligation:
    /// after an edit to `registry/*.csv`, run `make gen`.
    struct ObligationWorld {
        _temp: tempfile::TempDir,
        root: PathBuf,
        paths: Paths,
    }

    impl ObligationWorld {
        fn new() -> Self {
            use adage_tracks::{index::IndexedRoute, Evidence, Route, RouteState, Step, Strength, Trigger};
            let temp = tempfile::tempdir().unwrap();
            let root = temp.path().canonicalize().unwrap().join("acme");
            std::fs::create_dir_all(root.join(".git")).unwrap();
            std::fs::create_dir_all(root.join("registry")).unwrap();
            let paths = Paths::at(temp.path().join("home"));
            std::fs::create_dir_all(&paths.root).unwrap();
            std::fs::write(paths.root.join("config.toml"), "[learn]\nauto = false\n").unwrap();
            let route = Route {
                id: "regenerate-registry".into(),
                summary: String::new(),
                trigger: Trigger::AfterEdit { paths: "registry/*.csv".into() },
                step: Step::Run { command: "make gen".into() },
                slots: vec![],
                effect: adage_core::Effect::Regenerate,
                strength: Strength::Fire,
                state: RouteState::Firm,
                exit: vec![],
                evidence: Evidence { support: 5, opportunities: 5, days: 3, ..Default::default() },
                authored: false,
            };
            let index = Index {
                generation: "obligation-world".into(),
                routes: vec![IndexedRoute {
                    repo: "acme".into(),
                    track: "acme/finish-lines".into(),
                    version: "0.1.0".into(),
                    exec: None,
                    route,
                }],
                ..Default::default()
            };
            index.write_atomic(&paths.index()).unwrap();
            state::update::<SessionState, _>(&paths.state("s"), |st| st.arm = Some(Arm::Treated)).unwrap();
            ObligationWorld { _temp: temp, root, paths }
        }

        fn event(&self, ext: &'static dyn Extension, event: &str, extra: Value) -> Option<Value> {
            let mut p = json!({ "session_id": "s", "cwd": self.root });
            for (k, v) in extra.as_object().unwrap() {
                p[k] = v.clone();
            }
            let opts = Options { learner: None, extension: Some(ext) };
            let out = handle("claude", event, &p.to_string(), opts, &self.paths, Instant::now()).unwrap();
            out.map(|o| serde_json::from_str(&o).unwrap())
        }

        fn edit(&self, ext: &'static dyn Extension) -> Option<Value> {
            let call = json!({ "tool_name": "Edit", "tool_use_id": "t1", "tool_response": "ok",
                               "tool_input": { "file_path": self.root.join("registry/currencies.csv") } });
            self.event(ext, "PostToolBatch", json!({ "tool_calls": [call] }))
        }

        fn stop(&self, ext: &'static dyn Extension, active: bool) -> Option<Value> {
            self.event(ext, "Stop", json!({ "stop_hook_active": active }))
        }

        fn state(&self) -> SessionState {
            state::try_read::<SessionState>(&self.paths.state("s")).unwrap().unwrap()
        }
    }

    /// F1: a Stop-gate block no longer hides a learned finish-line obligation. The gate fails at
    /// the first Stop and the obligation is due there too: the agent gets both in one
    /// continuation. At the next Stop the gate passes, and the obligation has been requested.
    #[test]
    fn a_failing_stop_gate_keeps_the_learned_obligation_request() {
        let w = ObligationWorld::new();
        let failed = render::stop_gate_failed(".venv/bin/python -m pytest tests -q", Some(1), None);
        let ext = scripted(vec![
            StopGate::Block(failed.clone()),
            StopGate::Notice(
                "adage: ran `.venv/bin/python -m pytest tests -q` at finish: passed (exit 0, 1.0 s).".into(),
            ),
        ]);
        let directive = w.edit(ext).expect("the edit arms the obligation");
        assert!(directive["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("make gen"));
        let first = w.stop(ext, false).expect("the gate failed: the agent continues");
        assert_eq!(first["decision"], "block");
        let reason = first["reason"].as_str().unwrap();
        assert!(reason.contains("it failed (exit 1)"), "the gate's failure: {reason}");
        assert!(reason.contains("running `make gen`"), "the obligation's request is not lost: {reason}");
        assert!(harness::is_own_text(reason), "{reason}");
        let st = w.state();
        assert!(st.obligations[0].stop_requested && st.stop_requests == 1, "{:?}", st.obligations);
        // The agent fixes the failure and finishes again: the gate passes; nothing is lost.
        let second = w.stop(ext, true).expect("the person's line");
        assert!(second["systemMessage"].as_str().unwrap().contains("passed"), "{second}");
        assert_eq!(ext.asked.load(std::sync::atomic::Ordering::SeqCst), 2);
        assert_eq!(w.state().stop_requests, 1, "requested once");
    }

    /// F4: when an approved action already ran for this Stop's obligation request, the gate does
    /// not also run in the same hook (two staged runs could outlast the Stop hook's timeout); it
    /// runs at the next Stop. The gate's deadline is the registered timeout from the hook's start,
    /// less a reserve, so what the event already spent is deducted.
    #[test]
    fn an_action_at_stop_leaves_the_gate_for_the_next_stop() {
        struct ActsAtStop {
            asked: std::sync::atomic::AtomicUsize,
            deadlines: std::sync::Mutex<Vec<Instant>>,
        }
        impl Extension for ActsAtStop {
            fn deliver(&self, turn: &Turn<'_>, _: &[Directive]) -> Delivery {
                let at_stop: Vec<String> = turn.stimuli.iter().filter(|s| s.at_stop).map(|s| s.route.clone()).collect();
                // As adage-exec does for an applied at-Stop action: met, and the request withdrawn.
                Delivery {
                    at_stop_action: !at_stop.is_empty(),
                    stop: (!at_stop.is_empty()).then_some(None),
                    met: at_stop,
                    ..Delivery::default()
                }
            }
            fn stop_gate(&self, call: &StopCall<'_>) -> Option<StopGate> {
                self.asked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                self.deadlines.lock().unwrap().push(call.deadline);
                Some(StopGate::Notice("adage: ran `make test` at finish: passed (exit 0, 0.1 s).".into()))
            }
        }
        let ext: &'static ActsAtStop =
            Box::leak(Box::new(ActsAtStop { asked: Default::default(), deadlines: Default::default() }));
        let w = ObligationWorld::new();
        w.edit(ext).expect("the obligation is announced");
        assert!(w.stop(ext, false).is_none(), "the action met the obligation: the turn ends");
        assert_eq!(ext.asked.load(std::sync::atomic::Ordering::SeqCst), 0, "no gate beside an action");
        assert!(w.state().obligations[0].met);
        let before = Instant::now();
        let next = w.stop(ext, false).expect("the gate runs at the next Stop");
        assert!(next["systemMessage"].as_str().unwrap().contains("passed"), "{next}");
        assert_eq!(ext.asked.load(std::sync::atomic::Ordering::SeqCst), 1);
        let deadline = ext.deadlines.lock().unwrap()[0];
        let timeout = Duration::from_secs(u64::from(harness::ACTION_HOOK_TIMEOUT_S));
        assert!(deadline <= before + timeout - STOP_RESERVE + Duration::from_secs(1), "within the timeout");
        // Time the event already spent is deducted.
        let started = Instant::now() - Duration::from_secs(10);
        assert_eq!(stop_deadline(started), started + timeout - STOP_RESERVE);
    }

    /// A grant that stopped matching is told to the person (Claude Code's `systemMessage`, never
    /// the model), at most once per session: at the session start, or beside the first event
    /// whose action it withheld.
    #[test]
    fn a_lapsed_grant_notice_is_shown_once_per_session() {
        const LINE: &str = "adage: the E2 approval for acme/finish-lines/x no longer matches (…), so it does not run.";
        struct Lapsed;
        impl Extension for Lapsed {
            fn deliver(&self, _: &Turn<'_>, _: &[Directive]) -> Delivery {
                Delivery { notice: Some(LINE.into()), ..Delivery::default() }
            }
            fn session_texts(&self, _: &str, _: &Path, _: &Index, _: &Paths) -> SessionTexts {
                SessionTexts { context: Some("contract".into()), notice: Some(LINE.into()) }
            }
        }
        static LAPSED: Lapsed = Lapsed;
        let w = ObligationWorld::new();
        let start = |session: &str| {
            let p = json!({ "session_id": session, "cwd": w.root, "source": "startup" });
            let opts = Options { learner: None, extension: Some(&LAPSED) };
            handle("claude", "SessionStart", &p.to_string(), opts, &w.paths, Instant::now())
                .unwrap()
                .map(|o| serde_json::from_str::<Value>(&o).unwrap())
        };
        let first = start("s").expect("contract and notice");
        assert_eq!(first["systemMessage"], LINE, "{first}");
        assert_eq!(first["hookSpecificOutput"]["additionalContext"], "contract");
        let again = start("s").expect("the contract again");
        assert!(again.get("systemMessage").is_none(), "once per session: {again}");
        let edit = w.edit(&LAPSED).expect("the obligation's directive");
        assert!(edit.get("systemMessage").is_none(), "already told this session: {edit}");
        // Another session: told once, beside the directive the event delivers.
        state::update::<SessionState, _>(&w.paths.state("s2"), |st| st.arm = Some(Arm::Treated)).unwrap();
        let event = |session: &str| {
            let call = json!({ "tool_name": "Edit", "tool_use_id": "t2", "tool_response": "ok",
                               "tool_input": { "file_path": w.root.join("registry/currencies.csv") } });
            let p = json!({ "session_id": session, "cwd": w.root, "tool_calls": [call] });
            let opts = Options { learner: None, extension: Some(&LAPSED) };
            handle("claude", "PostToolBatch", &p.to_string(), opts, &w.paths, Instant::now())
                .unwrap()
                .map(|o| serde_json::from_str::<Value>(&o).unwrap())
        };
        let told = event("s2").expect("directive and notice");
        assert_eq!(told["systemMessage"], LINE, "{told}");
        assert!(told["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("make gen"), "{told}");
        assert!(!told["hookSpecificOutput"]["additionalContext"].as_str().unwrap().contains("no longer matches"));
        let quiet = event("s2");
        assert!(quiet.as_ref().is_none_or(|q| q.get("systemMessage").is_none()), "{quiet:?}");
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
