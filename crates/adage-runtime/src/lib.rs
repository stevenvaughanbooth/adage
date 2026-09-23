//! Hook-time decisions (Design §9, §13; Architecture §15).
//!
//! Pure logic: given the compiled index, the session's state, and one canonical event, decide
//! which directives to emit. No I/O except checking that bound repository paths exist. The CLI
//! owns reading payloads, locking state, writing spools, and printing harness output.

pub mod render;

use adage_core::keys::{self, template_key};
use adage_core::spans::{self, SpanKind};
use adage_core::tools::normalize_tool;
use adage_core::{repo, CommandTable};
use adage_tracks::{Aspect, Contract, Index, IndexedRoute, Route, SlotSource, Step, Trigger};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::Path;

pub const MAX_DIRECTIVES_PER_SESSION: usize = 8;
pub const MAX_DIRECTIVES_PER_DECISION: usize = 3;
const HISTORY: usize = 32;
const FOLLOW_WINDOW: u32 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Arm {
    #[default]
    Treated,
    Holdout,
}

/// Deterministic experiment assignment (Architecture A12): stable per session, reproducible.
pub fn assign_arm(session: &str, holdout_share: f64) -> Arm {
    let h = Sha256::digest(format!("adage-arm:{session}").as_bytes());
    let x = u32::from_le_bytes([h[0], h[1], h[2], h[3]]) as f64 / u32::MAX as f64;
    if x < holdout_share { Arm::Holdout } else { Arm::Treated }
}

#[derive(Debug, Clone)]
pub struct Call {
    pub tool: String,
    pub input: Value,
    pub output: String,
    pub failed: bool,
}

#[derive(Debug, Clone)]
pub enum Event {
    SessionStart,
    Prompt { text: String },
    Batch { calls: Vec<Call> },
    Failure { call: Call },
    Stop { stop_hook_active: bool },
    SessionEnd,
}

impl Event {
    pub fn name(&self) -> &'static str {
        match self {
            Event::SessionStart => "session_start",
            Event::Prompt { .. } => "prompt",
            Event::Batch { .. } => "batch",
            Event::Failure { .. } => "failure",
            Event::Stop { .. } => "stop",
            Event::SessionEnd => "session_end",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Pending {
    pub route: String,
    pub expect_key: String,
    /// The directed command's head; a variant with the same head (added flags, interpreter
    /// prefix) counts as following.
    #[serde(default)]
    pub expect_head: String,
    pub batches_left: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Obligation {
    pub route: String,
    pub expect_key: String,
    pub command: String,
    pub paths: String,
    pub met: bool,
    pub stop_requested: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionState {
    pub arm: Option<Arm>,
    pub repo: Option<String>,
    pub prompts: u32,
    pub batches: u32,
    pub emitted: Vec<String>,
    pub pending: Vec<Pending>,
    pub obligations: Vec<Obligation>,
    pub history: Vec<String>,
    pub first_edit_at_batch: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Directive {
    pub route: String,
    pub track: String,
    pub version: String,
    pub aspect: Aspect,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Judgement {
    Followed { route: String },
    Derailed { route: String },
    ObligationMet { route: String },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Decision {
    /// What the agent receives (empty in the holdout arm).
    pub directives: Vec<Directive>,
    /// A stop-hook request to continue, with its reason.
    pub continue_reason: Option<String>,
    /// What would have been emitted — recorded in both arms for exposure logging.
    pub would_have: Vec<Directive>,
    pub judgements: Vec<Judgement>,
    /// Canonical template keys of the calls observed in this event.
    pub observed: Vec<String>,
}

pub struct Ctx<'a> {
    pub index: &'a Index,
    pub repo: String,
    pub root: &'a Path,
    pub table: &'a CommandTable,
}

/// The canonical template key of a command string, for matching the agent's calls.
pub fn command_key(command: &str, table: &CommandTable) -> String {
    let n = adage_core::shell::normalize_shell(command, "/", table);
    keys::flatten(&n)
        .first()
        .map(|x| {
            let mut x = x.clone();
            keys::canonicalise(&mut x, Path::new("/"));
            template_key(&x.action)
        })
        .unwrap_or_default()
}

fn call_keys(call: &Call, ctx: &Ctx) -> Vec<(String, adage_core::Normalized)> {
    let cwd = ctx.root.to_string_lossy();
    keys::flatten(&normalize_tool(&call.tool, &call.input, &cwd, ctx.table))
        .into_iter()
        .map(|mut n| {
            keys::canonicalise(&mut n, ctx.root);
            (template_key(&n.action), n)
        })
        .collect()
}

pub fn decide(ctx: &Ctx, st: &mut SessionState, ev: &Event) -> Decision {
    let mut d = Decision::default();
    let routes: Vec<&IndexedRoute> = ctx.index.for_repo(&ctx.repo).collect();
    let mut out: Vec<Directive> = Vec::new();
    let emit_ok = |st: &SessionState, out: &Vec<Directive>, id: &str| {
        !st.emitted.iter().any(|e| e == id)
            && !out.iter().any(|o| o.route == id)
            && st.emitted.len() + out.len() < MAX_DIRECTIVES_PER_SESSION
            && out.len() < MAX_DIRECTIVES_PER_DECISION
    };

    match ev {
        Event::SessionStart | Event::SessionEnd => {}
        Event::Prompt { text } => {
            st.prompts += 1;
            let spans = spans::extract(text);
            // 1. Prompt-triggered routes, strongest aspect first.
            let mut prompt_routes: Vec<&&IndexedRoute> =
                routes.iter().filter(|r| matches!(r.route.trigger, Trigger::Prompt { .. })).collect();
            prompt_routes.sort_by_key(|r| (r.route.aspect, -(r.route.evidence.support as i64)));
            for ir in prompt_routes {
                if !emit_ok(st, &out, &ir.route.id) {
                    continue;
                }
                if let Some((text, key, head)) = bind_prompt_route(&ir.route, &spans, ctx) {
                    st.pending.push(Pending { route: ir.route.id.clone(), expect_key: key, expect_head: head, batches_left: FOLLOW_WINDOW });
                    out.push(directive(ir, text));
                }
            }
            // 2. Known dead ends, once, at the first prompt.
            if st.prompts == 1 {
                for ir in routes.iter().filter(|r| matches!(r.route.trigger, Trigger::SessionStart)) {
                    if emit_ok(st, &out, &ir.route.id) {
                        if let Some(t) = render::route_text(&ir.route, &[]) {
                            out.push(directive(ir, t));
                        }
                    }
                }
            }
        }
        Event::Batch { calls } => {
            st.batches += 1;
            for call in calls {
                for (key, n) in call_keys(call, ctx) {
                    d.observed.push(key.clone());
                    push_history(st, key.clone());
                    // Follow / derail judgement.
                    let head = match &n.action {
                        adage_core::CanonicalAction::Exec { argv, .. } => keys::command_head(argv),
                        _ => String::new(),
                    };
                    st.pending.retain(|p| {
                        if p.expect_key == key || (!p.expect_head.is_empty() && p.expect_head == head) {
                            d.judgements.push(Judgement::Followed { route: p.route.clone() });
                            false
                        } else {
                            true
                        }
                    });
                    // Obligations met.
                    for o in st.obligations.iter_mut().filter(|o| !o.met && o.expect_key == key) {
                        o.met = true;
                        d.judgements.push(Judgement::ObligationMet { route: o.route.clone() });
                    }
                    // Edits trigger finish-line obligations.
                    if keys::is_author(&n.action) {
                        st.first_edit_at_batch.get_or_insert(st.batches);
                        if let Some(p) = keys::action_path(&n.action) {
                            let rel = repo::relativize(ctx.root, p);
                            for ir in routes.iter() {
                                if let (Trigger::AfterEdit { paths }, Step::Run { command }) = (&ir.route.trigger, &ir.route.step) {
                                    let hit = glob::Pattern::new(paths).map(|g| g.matches(&rel)).unwrap_or(false);
                                    if hit && !st.obligations.iter().any(|o| o.route == ir.route.id) {
                                        st.obligations.push(Obligation {
                                            route: ir.route.id.clone(),
                                            expect_key: command_key(command, ctx.table),
                                            command: command.clone(),
                                            paths: paths.clone(),
                                            met: false,
                                            stop_requested: false,
                                        });
                                        if emit_ok(st, &out, &ir.route.id) {
                                            if let Some(t) = render::route_text(&ir.route, &[]) {
                                                out.push(directive(ir, t));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    // Routes triggered by a successful command (e.g. read the files it listed).
                    if !call.failed {
                        for ir in routes.iter() {
                            if let Trigger::AfterCommand { command } = &ir.route.trigger {
                                if *command == key && emit_ok(st, &out, &ir.route.id) {
                                    if let Some(t) = bind_result_route(&ir.route, &call.output, ctx.root) {
                                        out.push(directive(ir, t));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            for p in st.pending.iter_mut() {
                p.batches_left = p.batches_left.saturating_sub(1);
                if p.batches_left == 0 {
                    d.judgements.push(Judgement::Derailed { route: p.route.clone() });
                }
            }
            st.pending.retain(|p| p.batches_left > 0);
        }
        Event::Failure { call } => {
            for (key, _) in call_keys(call, ctx) {
                d.observed.push(format!("{key} !failed"));
                push_history(st, format!("{key} !failed"));
                for ir in routes.iter() {
                    if let Trigger::AfterFailure { command, signature } = &ir.route.trigger {
                        if *command == key && call.output.contains(signature.as_str()) && emit_ok(st, &out, &ir.route.id) {
                            if let Some(t) = render::route_text(&ir.route, &[]) {
                                out.push(directive(ir, t));
                            }
                        }
                    }
                }
            }
        }
        Event::Stop { stop_hook_active } => {
            if !*stop_hook_active {
                if let Some(o) = st.obligations.iter_mut().find(|o| !o.met && !o.stop_requested) {
                    o.stop_requested = true;
                    let reason = render::stop_reason(&o.paths, &o.command);
                    if st.arm != Some(Arm::Holdout) {
                        d.continue_reason = Some(reason);
                    }
                }
            }
        }
    }

    for o in &out {
        st.emitted.push(o.route.clone());
    }
    d.would_have = out.clone();
    if st.arm != Some(Arm::Holdout) {
        d.directives = out;
    }
    d
}

fn push_history(st: &mut SessionState, key: String) {
    st.history.push(key);
    if st.history.len() > HISTORY {
        let extra = st.history.len() - HISTORY;
        st.history.drain(..extra);
    }
}

fn directive(ir: &IndexedRoute, text: String) -> Directive {
    Directive {
        route: ir.route.id.clone(),
        track: ir.track.clone(),
        version: ir.version.clone(),
        aspect: ir.route.aspect,
        text,
    }
}

fn contract_ok(c: &Contract, v: &str, root: &Path) -> bool {
    match c {
        Contract::Pattern { regex } => Regex::new(regex).map(|r| r.is_match(v)).unwrap_or(false),
        Contract::RepoPath => repo::is_safe_relative(v) && root.join(v).is_file(),
    }
}

/// Bind a prompt route's slots from prompt spans; every slot must validate.
fn bind_prompt_route(r: &Route, spans: &[spans::Span], ctx: &Ctx) -> Option<(String, String, String)> {
    let Step::Run { command } = &r.step else { return None };
    let mut bound: Vec<(String, String)> = Vec::new();
    for s in &r.slots {
        let SlotSource::Prompt { span } = &s.source else { return None };
        let v = spans.iter().find(|x| x.kind == *span && contract_ok(&s.contract, &x.value, ctx.root))?;
        bound.push((s.name.clone(), v.value.clone()));
    }
    let text = render::route_text(r, &bound)?;
    let rendered = render::bind(command, &bound);
    let argv: Vec<String> = rendered.split_whitespace().map(str::to_string).collect();
    Some((text, command_key(&rendered, ctx.table), keys::command_head(&argv)))
}

/// Bind a route's result-field slot from the triggering command's JSON output.
fn bind_result_route(r: &Route, output: &str, root: &Path) -> Option<String> {
    let Step::ReadBatch { slot, max } = &r.step else { return None };
    let s = r.slots.iter().find(|s| &s.name == slot)?;
    let SlotSource::ResultField { field } = &s.source else { return None };
    let candidates: Vec<String> = match extract_json(output).and_then(|j| j.get(field).and_then(|v| v.as_array()).cloned()) {
        Some(list) => list.iter().filter_map(|v| v.as_str().map(str::to_string)).collect(),
        // Plain-text output, or the "*paths" pseudo-field: repository paths it mentions.
        None => spans::extract(output).into_iter().filter(|x| x.kind == SpanKind::Path).map(|x| x.value).collect(),
    };
    let files: Vec<String> = candidates
        .into_iter()
        .filter(|p| contract_ok(&s.contract, p, root))
        .take(*max)
        .collect();
    if files.is_empty() {
        return None;
    }
    render::route_text(r, &[(slot.clone(), files.join(", "))])
}

/// The first JSON object in a tool's text output.
pub fn extract_json(s: &str) -> Option<Value> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    serde_json::from_str(&s[start..=end]).ok()
}

pub fn span_kind_label(k: SpanKind) -> &'static str {
    match k {
        SpanKind::Ticket => "work item",
        SpanKind::Path => "file",
        SpanKind::Sha => "commit",
        SpanKind::Url => "link",
    }
}

#[cfg(test)]
mod tests;
