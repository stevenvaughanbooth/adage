//! What changed since a session last looked, and how it lands (the design's "Classification
//! rules"): a change concerning an artifact the session is working on that blocks or invalidates
//! its work, or a schedule boundary, is an interrupt; everything else is a delta, coalesced into
//! one block. Snapshot refreshes are not decided here: they happen at the first message and after
//! a compaction, by the harness's own rule.

use crate::render;
use crate::text::until;
use crate::window::is_open;
use crate::WorldCtx;
use adage_trace::{ArtifactKind, WorldView};

/// One change, in words.
#[derive(Debug, Clone, PartialEq)]
pub struct Change {
    /// The artifact or block it is about.
    pub id: String,
    pub text: String,
    /// Delivered at once, as its own row, rather than with the next prompt.
    pub urgent: bool,
}

/// The classifier's answer.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Changes {
    /// The coalesced delta text, inside the delta budget; `None` when nothing changed.
    pub delta: Option<String>,
    /// Each interrupt's text, inside the interrupt budget, in order; empty when none, or when the
    /// session has had its share, or when a focus block forbids them.
    pub interrupts: Vec<String>,
    /// What the view should record as seen now.
    pub next: WorldView,
    /// Every change found, for the record.
    pub changes: Vec<Change>,
}

/// A state that blocks or invalidates work on the artifact.
fn blocking(state: &str) -> bool {
    let s = state.to_ascii_lowercase();
    s.contains("red")
        || s.contains("fail")
        || s.contains("closed")
        || s.contains("conflict")
        || s.contains("review_requested")
        || s.contains("changes_requested")
}

/// Classify the world at `ctx.now` against `prev`, the session's last view. `focus` is what the
/// session is working on (its branch's pull request, the files it touched in the last ten turns).
pub fn classify(ctx: &WorldCtx<'_>, prev: &WorldView, focus: &[String]) -> Changes {
    let p = ctx.params;
    let in_focus_block = ctx.blocks.iter().any(|b| b.focus && b.covers(ctx.now));
    let mut changes: Vec<Change> = Vec::new();
    let mut states: Vec<(String, String)> = Vec::new();
    let mut items: Vec<_> =
        ctx.artifacts.iter().filter(|a| matches!(a.kind, ArtifactKind::Pr | ArtifactKind::Ticket)).collect();
    items.sort_by(|a, b| a.id.cmp(&b.id));
    for a in items {
        let state = a.state.clone().unwrap_or_default();
        if is_open(a) || prev.states.iter().any(|(id, _)| *id == a.id) {
            states.push((a.id.clone(), state.clone()));
        }
        let before = prev.states.iter().find(|(id, _)| *id == a.id).map(|(_, s)| s.as_str());
        let label = match a.kind {
            ArtifactKind::Pr => "pull request",
            _ => "ticket",
        };
        match before {
            None if prev.ts > 0 && is_open(a) => changes.push(Change {
                id: a.id.clone(),
                text: format!("new {label} {} \"{}\"{}", a.locator, a.title, note(&state)),
                urgent: false,
            }),
            Some(b) if b != state => {
                let urgent = focus.contains(&a.id) && blocking(&state);
                changes.push(Change {
                    id: a.id.clone(),
                    text: format!("{label} {} \"{}\" is now {}", a.locator, a.title, state.replace(':', " ")),
                    urgent,
                });
            }
            _ => {}
        }
    }
    let mut blocks: Vec<_> = ctx.blocks.iter().filter(|b| b.accepted && b.busy).collect();
    blocks.sort_by_key(|b| (b.start, b.id.clone()));
    for b in &blocks {
        let soon = b.start > ctx.now && b.start - ctx.now <= 5 * 60_000;
        if soon && !prev.blocks.contains(&b.id) {
            changes.push(Change { id: b.id.clone(), text: render::block_starting(ctx, b), urgent: true });
        }
    }
    let mut interrupts = Vec::new();
    let mut deltas = Vec::new();
    let mut sent = prev.interrupts;
    for c in &changes {
        if c.urgent && !in_focus_block && sent < p.max_interrupts {
            interrupts.push(render::interrupt(ctx, &c.text));
            sent += 1;
        } else {
            deltas.push(c.text.clone());
        }
    }
    let delta = (!deltas.is_empty()).then(|| render::delta(ctx, &deltas));
    let next = WorldView {
        ts: ctx.now,
        snapshot_hash: prev.snapshot_hash.clone(),
        states,
        blocks: blocks
            .iter()
            .filter(|b| b.start > ctx.now && b.start - ctx.now <= 5 * 60_000)
            .map(|b| b.id.clone())
            .collect(),
        focus: focus.to_vec(),
        interrupts: sent,
        last_delta_ts: if delta.is_some() { ctx.now } else { prev.last_delta_ts },
    };
    let _ = until;
    Changes { delta, interrupts, next, changes }
}

fn note(state: &str) -> String {
    if state.is_empty() {
        String::new()
    } else {
        format!(" ({})", state.replace(':', " "))
    }
}
