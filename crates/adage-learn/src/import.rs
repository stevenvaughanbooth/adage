//! Sessions as the learning engine reads them.
//!
//! Decoding traces is ingestion's job (`adage_ingest`), and the session is the canonical model's
//! (`adage_trace`); both are re-exported here so existing paths keep working. Which commands
//! Adage's own directives named, so a session is never counted as evidence for what it was told
//! to do, is the graph engine's reading of a session (`adage_graph::canonical`).

pub use adage_ingest::claude::{decode, discover, discover_project, parse_transcript};
pub use adage_ingest::signals::{asks_for_help, call_steps, Signals};
pub use adage_trace::session::*;

pub use adage_graph::canonical::{DirectiveIdentities, Exposure};
