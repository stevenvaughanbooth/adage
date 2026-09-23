//! Adage core: the canonical action vocabulary, the shell normaliser (tree-sitter-bash plus a
//! data-driven command table), effect classes, typed spans, and repository identity.

pub mod keys;
pub mod repo;
pub mod shell;
pub mod spans;
pub mod table;
pub mod template;
pub mod tools;
pub mod types;

pub use table::CommandTable;
pub use types::{CanonicalAction, Effect, Normalized};
