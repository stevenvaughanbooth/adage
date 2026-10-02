//! The application layer (docs/architecture.md): what Adage does for a person, as typed
//! operations every front end calls. The CLI, the TUI and the `adage map` HTTP API render what
//! these return; none of them holds the rules.
//!
//! - [`review`]: the review queue, and the decisions a person makes about reflexes.
//! - [`person`]: the proof that a person, not an agent, is deciding.
//! - [`proposal`]: proposed changes to pinned routes.
//! - [`error`]: errors that say what to do instead.

pub mod error;
pub mod person;
pub mod proposal;
pub mod review;

pub use person::Person;
