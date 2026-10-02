//! The bridge from a canonical session to the graph engine's input now lives with the engine
//! (`adage_graph::canonical`); re-exported here so existing paths keep working.

pub use adage_graph::canonical::{annotate, annotate_prefix, effect_of, events, influence, to_input};
