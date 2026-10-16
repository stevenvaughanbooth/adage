//! Ingestion (docs/architecture.md): from a trace source to the canonical [`Session`].
//!
//! ```text
//!   TraceSource ──bytes──► Format (sniffed) ──► decoder ──► Session ──► the learning engine
//!   (local history,        (Claude Code,         │
//!    a team store,          Codex, OpenCode)     └── Locator: which repository (this machine's
//!    memory)                                         checkout, or the trace alone)
//! ```
//!
//! Day 0 (`adage init`, `adage condition`) ingests a whole source; live learning ingests the trace
//! a hook names when a session stops. Both decode through the same [`format`] registry, so the same
//! trace always yields the same session.
//!
//! - [`source`]: where traces come from ([`source::TraceSource`]).
//! - [`format`]: what the bytes mean ([`format::Format`], one per harness history format).
//! - [`locate`]: which repository a trace belongs to ([`locate::Locator`]).
//! - [`signals`]: what every decoder shares (prompts, Adage's own directives, canonical steps).
//! - [`batch`]: a batch of local traces read for one learn run (day 0 or live).
//! - [`cache`]: the per-transcript parse cache; [`scope`]: picking one repository's local traces.

pub mod batch;
pub mod cache;
pub mod claude;
pub mod codex;
mod codex_exec;
pub mod corpus;
pub mod format;
mod lineage;
pub mod locate;
pub mod opencode;
pub mod scope;
pub mod signals;
pub mod source;

pub use adage_trace::Session;
pub use format::{decode_trace, read_session, read_trace, Decode, Format};
pub use locate::{AsRecorded, LocalCheckout, Locator};
pub use source::{ingest, ingest_one, TraceRef, TraceSource};
