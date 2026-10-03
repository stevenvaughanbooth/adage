//! `adage-exec hook <harness> <event>`: E1 prefetch and E2 actions (Track C §3.2, §3.4, §3.6).
//!
//! The same pipeline as `adage hook` (`adage-hook`: adapters, fast path, state lock, `decide()`,
//! spool, render), with one [`Extension`]. For each directive about to be delivered whose route
//! has an E1 approval for this repository, it derives the route's request ([`crate::route`]),
//! reads the bound values back from the directive, and runs it through [`crate::run::execute`]
//! (every gate: approval key equality, including the L-ref hash read again now; policy; a live
//! sandbox; limits; typed projection). Each block is re-checked by the safety oracle
//! (`adage_safety::check_data_block`), the directive adapts ("That lookup has been run; its
//! result is below."), and the block is placed beside it.
//!
//! E1 failures retain ordinary advice. E2 failures before application can return advice with a
//! reason; uncertain application, journal corruption and extension panics instead deliver a
//! recovery notice. Completed actions whose detail exceeds the harness cap deliver a bounded
//! completion notice, never an instruction to repeat the action. With no grants or pending
//! recovery, the output equals `adage hook`'s byte for byte.
//!
//! The spool record gains `exec`: per attempted route, `{ route, level, exit, bytes, sha256,
//! truncated, duration_ms, auto }` when delivered, `{ route, level, withheld }` when not. The
//! output itself is never recorded.
//!
//! **E2 actions.** When an edit arms a finish-line obligation (after editing `registry/*.csv`, run
//! `make gen`), or the obligation's Stop request is due, and the reflex holds an explicit E2
//! approval for this repository, the action runs here (the `act` module). A runtime-selected
//! learned context with execution evidence can also nominate its fixed command, without creating
//! an obligation. Both paths run at most one action per event, never in
//! the control arm, never under OpenCode (whose bridge waits one second), within 20 s. It is
//! staged, its change set verified against the declared outputs, and applied only if nothing it
//! touches changed meanwhile ([`crate::stage`]). Applied: the directive becomes a result block
//! ("Adage ran `make gen` after your edit to registry/currencies.csv: updated 1 file (…). Nothing
//! else changed."), the obligation is met, and no Stop request follows. Refused or failed: the
//! plain E0 directive and the reason. The outcome goes to `$ADAGE_HOME/exec/actions.spool` and to
//! this event's `exec` record; three refused change sets revoke the approval.
//!
//! This file is the [`Extension`] impl and the dispatch between the jobs, one module each:
//! `grants` (which grants hold here: the session contract, exec-only reflexes, predictions),
//! `prefetch` (E1), `act` (E2), `stop_gate` (the finish-line run), `receipt` (a `PreToolUse` test
//! run already covered), and `pytest` (reading pytest's command lines and cache records).

use adage_core::CommandTable;
use adage_hook::{Delivery, Extension, PreCall, PreNote, SessionTexts, Site, StopCall, StopGate, Turn};
use adage_runtime::{render, Directive};
use serde_json::Value;

use crate::approvals::{Approval, Store};
use crate::policy::Policy;
use crate::request::Level;
use crate::run::Context;
use crate::sandbox::Capability;

mod act;
mod grants;
mod prefetch;
mod pytest;
mod receipt;
mod stop_gate;
#[cfg(test)]
mod testkit;

pub use grants::{predict_grants, predict_off};
pub use prefetch::data_type;

/// The E1 prefetch extension.
pub struct Prefetch;

/// What `adage-exec hook` passes to the pipeline.
pub static PREFETCH: Prefetch = Prefetch;

/// The whole `hook` subcommand. Live learning starts the `adage` binary beside this one.
pub fn main(harness: String, event: String) -> ! {
    adage_hook::main(harness, event, adage_hook::Options { learner: Some(adage_beside), extension: Some(&PREFETCH) })
}

/// The `adage` binary installed beside `adage-exec` (live learning is `adage condition --auto`).
pub fn adage_beside() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let name = if cfg!(windows) { "adage.exe" } else { "adage" };
    let p = exe.parent()?.join(name);
    p.is_file().then_some(p)
}

impl Extension for Prefetch {
    fn session_context(&self, site: &Site<'_>) -> Option<String> {
        self.session_texts(site).context
    }

    /// The contract and, for the person, a grant that stopped matching: one planning pass over
    /// this repository's grants serves both.
    fn session_texts(&self, site: &Site<'_>) -> SessionTexts {
        grants::session_texts(site.repo_key, site.root, site.index, site.paths)
    }

    fn stop_gate(&self, call: &StopCall<'_>) -> Option<StopGate> {
        stop_gate::stop_gate(call)
    }

    fn deliver(&self, turn: &Turn<'_>, directives: &[Directive]) -> Delivery {
        let mut d = Delivery::default();
        let mut records: Vec<Value> = Vec::new();
        if !directives.is_empty() {
            match prefetch::prefetch(turn, directives, &mut d.notice) {
                Ok(None) => {}
                Ok(Some((texts, r))) => {
                    d.texts = Some(texts);
                    records.extend(r);
                }
                Err(r) => records.extend(r),
            }
        }
        let candidates = act::candidates(turn, directives);
        if !candidates.is_empty() {
            d.cpu_predictions =
                candidates.iter().filter(|c| c.context.is_some()).map(|c| c.route.to_string()).collect();
            // A prediction is execution-only. A missing or stale grant must not fall back to
            // asking the agent to perform it. Keep positions aligned for result replacement.
            if candidates.iter().any(|c| c.context.is_some()) {
                let texts = d.texts.get_or_insert_with(|| directives.iter().map(render::wrap_directive).collect());
                for (i, dir) in directives.iter().enumerate() {
                    if candidates
                        .iter()
                        .any(|c| c.context.is_some() && c.route == dir.route && c.track == Some(dir.track.as_str()))
                    {
                        texts[i].clear();
                    }
                }
            }
            let a = act::act(turn, directives, &candidates);
            records.extend(a.records);
            if !a.replace.is_empty() {
                let mut texts =
                    d.texts.take().unwrap_or_else(|| directives.iter().map(render::wrap_directive).collect());
                for (i, dir) in directives.iter().enumerate() {
                    if let Some(t) = a.replace.get(&dir.route) {
                        texts[i] = t.clone();
                    }
                }
                d.texts = Some(texts);
            }
            d.extra = a.extra;
            d.met = a.met;
            d.stop = a.stop;
            d.recovery = a.recovery;
            d.at_stop_action = a.at_stop_action;
            d.notice = d.notice.take().or(a.notice);
        }
        if !records.is_empty() {
            d.spool = Some(Value::Array(records));
        }
        d
    }

    fn pretool_context(&self, call: &PreCall<'_>) -> Option<PreNote> {
        receipt::covering_receipt(call)
    }

    /// Exec-only reflexes (vendored scripts) approved here at E1 or E2: only then are they
    /// directed, and only because this binary carries them out.
    fn exec_only(&self, site: &Site<'_>) -> Vec<(String, String)> {
        grants::exec_only(site.repo_key, site.index, site.paths)
    }

    /// The reflexes whose `predict` grant still holds here ([`predict_grants`]), in the checkout
    /// the pipeline resolved this step's repository from (`site.root`).
    fn predict_grants(&self, site: &Site<'_>) -> Vec<(String, String)> {
        predict_grants(site.repo_key, site.root, site.index, site.paths)
    }
}

/// A route as a grant and the journal name it: `track/route`.
fn label(track: &str, route: &str) -> String {
    format!("{track}/{route}")
}

/// Whether `a` lets this repository's reflex be carried out at `level` (an execute grant, never
/// a `predict` one: that only lets the agent's own step be predicted).
fn executes_at(a: &Approval, repo_key: &str, level: Level) -> bool {
    a.key.repo == repo_key && a.key.level == level && a.key.mode.is_execute()
}

/// The executor's context for a run this hook starts: this process's environment, the home's
/// content store when it has one, and no canaries.
fn run_context<'a>(
    policy: &'a Policy,
    store: &'a Store,
    capability: &'a Capability,
    table: &'a CommandTable,
    repo_key: &str,
    home: &std::path::Path,
) -> Context<'a> {
    Context {
        inherited_env: std::env::vars().collect(),
        policy,
        store,
        capability,
        table,
        repo_key: repo_key.to_string(),
        cas: Some(home.join("cas")).filter(|p| p.is_dir()),
        home: adage_store::home_dir(),
        canaries: Vec::new(),
    }
}
