//! The workspace's test kit (docs/claude-mod/impl/testkit.md): builders for routes, tracks and
//! indexes, the routes of each kind the suites keep writing, and a learned home on disk with its
//! session state and spools. Compiled only for this crate's own tests and behind the `testkit`
//! feature, which a crate turns on from its `[dev-dependencies]`:
//!
//! ```toml
//! [dev-dependencies]
//! adage-tracks = { path = "../adage-tracks", features = ["testkit"] }
//! ```
//!
//! A builder starts from what most fixtures write (a firm route, no slots, default evidence; a
//! track at `0.1.0` learned as a desire path) and each test states only what it is about. The
//! values it builds are the plain model types, so a test that needs a field the builder does
//! not name sets it on the result.
//!
//! The kit sits in layer 2 (`xtask/src/layers.rs`) so every crate above can use it; the one
//! lower crate it links for this, `adage-store`, is an optional dependency the feature enables.
//! It never spawns a process (`adage-exec`'s `no_spawn` test scans this directory).

mod build;
mod home;
pub mod kinds;

pub use build::{route, track, IndexBuilder, RouteBuilder, TrackBuilder};
pub use home::{LearnedHome, TempWorld};

#[cfg(test)]
mod tests;
