//! The application layer (docs/architecture.md): what Adage does for a person, as typed
//! operations every front end calls. The CLI, the TUI and the `adage map` HTTP API render what
//! these return; none of them holds the rules.
//!
//! - [`review`]: the review queue, and the decisions a person makes about reflexes.
//! - [`person`]: the proof that a person, not an agent, is deciding.
//! - [`proposal`]: proposed changes to pinned routes.
//! - [`graph`]: the map's read model, Adage's live state as one graph (`/api/v1/graph`).
//! - [`savings`]: the savings estimate against the control arm.
//! - [`evidence`], [`words`]: a reflex's evidence and the lexicon, as a person reads them.
//! - [`error`]: errors that say what to do instead.

pub mod error;
pub mod evidence;
pub mod graph;
pub mod person;
pub mod proposal;
pub mod review;
pub mod savings;
pub mod words;

pub use person::Person;
