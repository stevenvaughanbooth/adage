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

//! **Where things are.** This file is the entry and the dispatch ([`main`], [`run`], [`handle`]);
//! each kind of event is handled in its own module: `pretool` (`PreToolUse`: the interpreter
//! planner and the mod's two phases), `standing` (the mod's `Standing`), `session_start`,
//! `respond` (decide and deliver every other event, a `TurnStep` included), `stop` (the
//! finish-line gate at a `Stop`), [`press`] and [`skill`]. Each takes the event's context
//! (`ctx::HookCtx`), writes its spool records through `record` and answers the mod through
//! [`answer`]. The extension contract is `extension`, re-exported here.
//! docs/claude-mod/impl/structure-hook.md has the module map.

pub mod answer;
pub mod autolearn;
pub mod failopen;
pub mod harness;
pub mod phase;
pub mod press;
pub mod receipt;
pub mod skill;

mod ctx;
mod extension;
mod identity;
mod pretool;
mod record;
mod respond;
mod session_start;
mod site;
mod standing;
mod stop;
#[cfg(test)]
mod testkit;

pub use extension::{
    stimuli, Delivery, Extension, PreCall, PreNote, ReceiptCard, SessionTexts, Site, Stimulus, StopCall, StopGate, Turn,
};
pub use pretool::rewrite_off;

use adage_core::repo;
use adage_runtime::Event;
use adage_store::Paths;
use ctx::{HookCtx, Reads};
use failopen::Failure;
use harness::HookEvent;
use respond::respond;
use session_start::session_start;
use std::io::Read;
use std::path::PathBuf;
use std::time::Instant;

const MAX_STDIN: u64 = 8 << 20;

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
        // The press runs before the kill switches, so a person can still be asked to turn Adage
        // back on, and the agent's first act still closes its window (mod architecture §3.12).
        let paths = Paths::from_env();
        return Ok(payload.ok().and_then(|buf| press::before_kill_switch(harness, event, &buf, &paths, started)));
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
    let p: serde_json::Value = serde_json::from_str(buf)
        .map_err(|e| Failure::new("payload", format!("stdin is not a JSON payload ({:?})", e.classify())))?;
    let Some(envelope) = adapter.envelope(&p) else {
        return Err(Failure::new("payload", "no session id or working directory"));
    };
    let (session, cwd) = (envelope.session.clone(), envelope.cwd.clone());
    // Outside a repository (a session opened in `~`, `/`, or a directory since deleted) there is
    // nothing to learn: no notice, no learner (docs/zero-config.md §3.2). Reflexes a person keyed
    // to such a directory by hand are still delivered.
    let found = repo::find_root(&cwd);
    let in_repo = found.is_some();
    let root = found.unwrap_or_else(|| cwd.clone());
    let off = root.join(".adage/disabled").exists();
    // Before the kill switch and the fast path: the press answers a `Press`, and the agent's first
    // act closes its window whatever else happens to the event (mod architecture §3.12).
    if let press::Gate::Answer(out) = press::gate(harness, event, &p, &envelope, paths, off, started) {
        return Ok(out);
    }
    if off {
        return Ok(None);
    }
    let cwd_text = cwd.to_string_lossy();
    let ctx = HookCtx {
        adapter,
        harness,
        event,
        p: &p,
        session: &session,
        cwd: &cwd,
        root: &root,
        in_repo,
        paths,
        started,
        opts,
        reads: Reads::default(),
    };
    // One path per kind of event (the event table, `adage_trace::Harness::events`). Each reads the
    // index and the configuration through `ctx`, on first use: `SkillPrompt` and `PreToolUse`
    // never read the index before they know they need it.
    let class = adapter.classify(event, &p, &cwd_text);
    // Registration alone cannot prove delivery: a shell call, a session start or an event the
    // runtime decides, in a repository, is recorded ([`receipt`]) without changing output or
    // opening the session state or spool; a receipt failure never interrupts the agent. The mod's
    // asks for the standing text or a skill's text answer the mod, not an agent's event, and the
    // press returned above, so none of them counts.
    if in_repo && matches!(class, HookEvent::PreToolUse | HookEvent::SessionStart | HookEvent::Decide(_)) {
        let _ = receipt::record(paths, &root, adapter.harness().id());
    }
    let ev = match class {
        // Answered by the press gate above.
        HookEvent::Press | HookEvent::Ignored => return Ok(None),
        HookEvent::SkillPrompt => return skill::prompt(adapter, &p, &session, &root, paths, started),
        HookEvent::PreToolUse => return pretool::pretool(&ctx),
        HookEvent::Standing => return standing::standing(&ctx),
        HookEvent::SessionStart => return session_start(&ctx, ctx.index_damaged(), &learner(opts)),
        HookEvent::Decide(ev) => ev,
    };
    let stop = match ev {
        Event::Stop { stop_hook_active } => Some(stop_hook_active),
        _ => None,
    };
    // Decide first, always: the session's bookkeeping (a learned obligation's Stop request, an
    // at-Stop action) must not depend on what the finish-line gate finds (review F1).
    let responded = respond(&ctx, ev);
    let out = match (stop, opts.extension) {
        (Some(stop_hook_active), Some(x)) if in_repo => stop::gated(&ctx, x, stop_hook_active, responded),
        _ => responded.map(|r| r.out.and_then(|o| adapter.render(event, o))),
    };
    if in_repo && adapter.is_learn_point(event) {
        let transcript = p.get("transcript_path").and_then(|v| v.as_str());
        let learned = learner(opts)()
            .and_then(|l| autolearn::on_turn_end(paths, &root, adapter.name(), transcript, &l, false).map(drop));
        if let Err(f) = learned {
            failopen::record(&paths.root, harness, event, &f);
        }
    }
    out
}

/// How to find the adage binary live learning starts: the binary's own finder, else this binary.
fn learner(opts: Options) -> impl Fn() -> Result<PathBuf, Failure> {
    move || match opts.learner {
        Some(find) => find().ok_or_else(|| Failure::new("condition", "finding the adage binary: not found")),
        None => std::env::current_exe().map_err(|e| Failure::io("condition", "finding the adage binary", &e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_runtime::Event;
    use harness::Harness;
    use serde_json::json;

    // ------------------------------------------------------------ the mod's capabilities (C9)

    /// T-C9-01 at the adapter: a payload the mod sent reports the mod's capabilities (it says
    /// `via: mod`, is an event only the mod sends, or carries a phase only the mod asks for); a
    /// command hook's payload reports the command hooks' four; Codex and OpenCode have no mod. A
    /// subagent's tool events carry its `agent_id` onto the canonical call.
    #[test]
    fn the_mods_payloads_report_the_mods_capabilities() {
        use harness::Adapter;
        let claude = &harness::claude::Claude;
        let hooks = Harness::ClaudeCode.capabilities();
        let with_mod = Harness::ClaudeCode.capabilities_with_mod();
        assert_eq!(claude.capabilities("PostToolBatch", &json!({ "tool_calls": [] })), hooks);
        assert_eq!(claude.capabilities("PreToolUse", &json!({ "tool_name": "Bash" })), hooks);
        assert_eq!(claude.capabilities("PostToolBatch", &json!({ "via": "mod" })), with_mod);
        assert_eq!(claude.capabilities("PreToolUse", &json!({ "dry_run": true })), with_mod);
        assert_eq!(claude.capabilities("PreToolUse", &json!({ "verdict": {} })), with_mod);
        for event in ["TurnStep", "Standing", "SkillPrompt"] {
            assert_eq!(claude.capabilities(event, &json!({})), with_mod, "{event}");
        }
        assert_eq!(claude.capabilities("Stop", &json!({ "via": "the mod" })), hooks, "only `mod`");
        for h in [Harness::Codex, Harness::OpenCode] {
            let a = harness::adapter_for(h);
            assert_eq!(a.capabilities("PostToolUse", &json!({ "via": "mod" })), h.capabilities(), "{h}");
        }
        let batch = json!({ "agent_id": "explore-1", "tool_calls": [{ "tool_name": "Read", "tool_use_id": "t1",
                            "tool_input": { "file_path": "/r/a.py" }, "tool_response": "x" }] });
        let Some(Event::Batch { calls }) = claude.translate("PostToolBatch", &batch, "/r") else { panic!("a batch") };
        assert_eq!(calls[0].agent.as_deref(), Some("explore-1"));
        let main = json!({ "tool_calls": [{ "tool_name": "Read", "tool_use_id": "t1", "tool_input": {}, "tool_response": "x" }] });
        let Some(Event::Batch { calls }) = claude.translate("PostToolBatch", &main, "/r") else { panic!("a batch") };
        assert_eq!(calls[0].agent, None);
        let failed =
            json!({ "agent_id": "explore-1", "tool_name": "Bash", "tool_input": { "command": "x" }, "error": "e" });
        let Some(Event::Failure { call }) = claude.translate("PostToolUseFailure", &failed, "/r") else { panic!() };
        assert_eq!(call.agent.as_deref(), Some("explore-1"));
    }
}
