//! The situational awareness layer's decisions (docs/architecture.md, layer 3): given what the
//! person was doing (`adage_trace::world`), say what a prompt refers to, what changed since a
//! session last looked, and what the calendar means for the session. Pure logic, like
//! `adage-runtime`: the hook, the follow child and the mod do every piece of I/O around it, and
//! `now`, the embedder and the parameters are inputs, never read here.
//!
//! - [`resolve`]: the reference resolver. A prompt with a referential signal ([`gate`]) is scored
//!   against the candidate window ([`window`], [`score`]) and the decision table ([`decide`])
//!   attaches one hint, a disambiguation of two or three contenders, or nothing.
//! - [`classify`]: what changed since a session's last view, as deltas (coalesced, one block per
//!   turn) and interrupts (a change that blocks the session's current work, or a schedule
//!   boundary).
//! - [`schedule`]: the calendar's levers: the prompt-cache TTL, an autonomy window, a finalize
//!   nudge, a proposed review block.
//! - [`render`]: the `<adage-world>` text the model reads, inside the token budgets.
//!
//! Deterministic: no wall clock, no hash-map iteration, `BTreeMap` only, ties broken by id.

pub mod classify;
pub mod decide;
pub mod descriptor;
pub mod gate;
pub mod ingest;
pub mod params;
pub mod render;
pub mod schedule;
pub mod score;
pub mod state;
pub mod text;
pub mod window;

pub use classify::{classify, Change, Changes};
pub use decide::{decide, Decision, Hint};
pub use descriptor::{Descriptor, Needs, Provider};
pub use gate::{gate, Signal};
pub use params::Params;
pub use schedule::{schedule, Autonomy, Liveness, Plan, Review, Ttl};
pub use score::{score, Scored};
pub use state::{ArtifactsState, CalendarState, EvalCase, HealthState, SourceHealth};
pub use window::candidates;

pub use adage_trace::{
    Action, ActivityEvent, Artifact, ArtifactKind, CalendarBlock, Injection, InjectionAt, InjectionClass, WorldView,
};

/// What a resolution reads: the world as the store holds it, the clock, the embedder and the
/// parameters.
pub struct WorldCtx<'a> {
    /// Milliseconds since the Unix epoch.
    pub now: u64,
    /// The person's local offset from UTC, in minutes, for the times a snapshot prints.
    pub tz_offset_min: i32,
    /// Every artifact the store holds, any order.
    pub artifacts: &'a [Artifact],
    /// The activity timeline's recent events, any order.
    pub events: &'a [ActivityEvent],
    /// The calendar blocks in the window a source last looked at.
    pub blocks: &'a [CalendarBlock],
    /// The embedder the artifacts' embeddings were made with (`adage_intent`): `None` when a text
    /// has no words, and `None` for every text when no model is loaded.
    pub embed: &'a dyn Fn(&str) -> Option<Vec<f32>>,
    pub params: &'a Params,
}

/// The reference resolver: the hint for `prompt`, or nothing when the prompt carries no
/// referential signal, names its own artifact, or no candidate scores high enough.
pub fn resolve(ctx: &WorldCtx<'_>, prompt: &str) -> Decision {
    let Some(signal) = gate(prompt, ctx.artifacts) else {
        return Decision::None { reason: "no referential signal" };
    };
    let cands = candidates(ctx);
    if cands.is_empty() {
        return Decision::None { reason: "no candidates in the window" };
    }
    let scored = score(ctx, &signal, &cands);
    decide(ctx, &signal, scored)
}
