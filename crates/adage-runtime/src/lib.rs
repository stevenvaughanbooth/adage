//! Hook-time decisions (Design §9, §13; Architecture §15).
//!
//! Pure logic: given the compiled index, the session's state, and one canonical event, decide
//! which directives to emit. No I/O of its own: what a decision reads of the working tree (that
//! a bound repository path exists, where a card's files stand) goes through the
//! [`checkout::Checkout`] port on [`Ctx`]. The hook owns reading payloads, locking state, writing
//! spools, and printing harness output.
//!
//! Every public item is re-exported here at the crate root, wherever its module is, so callers
//! name `adage_runtime::X` and never a private module. The module map is the crate's `AGENT.md`.

pub mod checkout;
pub mod context;
pub mod fast;
pub mod orient;
pub mod pretool;
pub mod pytest_case;
pub mod rebuild;
pub mod render;

mod binding;
mod decide;
mod decision;
mod noticed;
mod observe;
mod policy;
mod prediction;
mod pretool_decide;
mod standing;
mod state;

use adage_core::spans::SpanKind;

pub use adage_core::json::extract_json;
pub use adage_trace::{is_tool_use_error, By, Call, Event};
pub use context::CtxState;
pub use decide::decide;
pub use decision::{
    Decision, Directive, Judgement, ObligationState, PredictedCall, Prediction, PredictionKind, Routing, Substitution,
};
pub use noticed::{Noticed, MAX_NOTICED};
pub use observe::command_key;
pub use policy::{Ctx, Embed, Policy, PREDICT_CAP};
pub use pretool_decide::{answered_by_receipt, substitute, substitution};
pub use standing::{delegation, standing, standing_for, Standing};
pub use state::{
    assign, assign_arm, identity_hash, Arm, Obligation, Pending, PendingReads, SessionState, MAX_NEW_TESTS,
    SEEN_PRETOOL,
};

pub(crate) use observe::added_tests;
pub(crate) use state::push_history;

// The defaults of `Ctx::params` (`adage_tracks::params::RuntimeParams`), which is what decides.
pub use adage_tracks::params::RuntimeParams;
pub const MAX_DIRECTIVES_PER_SESSION: usize = adage_tracks::params::MAX_DIRECTIVES_PER_SESSION;
pub const MAX_DIRECTIVES_PER_DECISION: usize = adage_tracks::params::MAX_DIRECTIVES_PER_DECISION;
const HISTORY: usize = adage_tracks::params::HISTORY;

/// At most this many stop-hook requests per session, across all obligations (the default of
/// `RuntimeParams::max_stop_requests`).
pub const MAX_STOP_REQUESTS: u32 = adage_tracks::params::MAX_STOP_REQUESTS;

pub fn span_kind_label(k: SpanKind) -> &'static str {
    match k {
        SpanKind::Ticket => "work item",
        SpanKind::Path => "file",
        SpanKind::Sha => "commit",
        SpanKind::Url => "link",
    }
}

#[cfg(test)]
mod skill_tests;
#[cfg(test)]
mod tests;
