//! Instincts: prebuilt routes that ship with Adage and work on day 0, with no session history.
//!
//! *Reflexes* are routes learned from sessions (every route the miners produce). An *instinct* is
//! a route template shipped as data (`data/catalogue.toml`) plus a detector. At learn time the
//! binder reads a repository's declared facts — Makefile rules, package.json scripts, lockfiles,
//! pyproject, Cargo and Go metadata, CI `run:` steps, generated-file markers — from bounded reads
//! of its files, never executing anything, and binds each enabled instinct into concrete
//! [`adage_tracks::Route`] values:
//!
//! - provenance `instinct:<pack>@<version>`, no evidence of its own;
//! - yellow, in probation (an environment step in draft, waiting for review);
//! - reactive: after a failure, or after an edit — never at session start, never a Stop block;
//! - ids equal to those the miner would derive, so a learned route for the same behaviour wins
//!   the index's de-duplication, and any learned route that contradicts an instinct suppresses it.
//!
//! Advice (don't hand-edit generated files or lockfiles; use the declared package manager) binds
//! as routes with an advisory step (`Step::Advise`) from a fixed template. `adage learn` writes
//! the routes as the track `<repo>/instincts` (`adage_learn::instincts`), the index gives every
//! other source precedence over them, and the runtime words them as Adage's defaults, never as
//! learned (docs/instincts.md).
//!
//! ```no_run
//! use std::collections::BTreeSet;
//! let result = adage_instincts::bind(std::path::Path::new("."), &[], &BTreeSet::new());
//! for r in &result.routes {
//!     println!("{} {:?}", r.id, r.step);
//! }
//! ```

pub mod bind;
pub mod catalogue;
pub mod declared;
pub mod detect;
pub mod files;
pub mod safety;

pub use bind::{bind, bind_facts, bind_with, Advice, BindOptions, BindResult};
pub use catalogue::{catalogue, Catalogue, Instinct};
pub use declared::{extract, DeclaredFacts};

/// Whether a route came from an instinct (by its provenance).
pub fn is_instinct(r: &adage_tracks::Route) -> bool {
    r.evidence.provenance.starts_with("instinct:")
}

/// The route kind an instinct route stands in for: the kind of the learned route that shares its
/// id (`dead-end-*` → dead-ends, `finish-line-*` → finish-line), advice for an advisory step,
/// else the kind its shape gives. `adage_tracks::route_kind` agrees on every bound route (so
/// steering by an instinct excludes sessions from the one miner it could influence).
pub fn intended_kind(r: &adage_tracks::Route) -> &'static str {
    if r.id.starts_with("dead-end-") {
        adage_tracks::KIND_DEAD_ENDS
    } else if r.id.starts_with("finish-line-") {
        adage_tracks::KIND_FINISH_LINE
    } else if matches!(r.step, adage_tracks::Step::Advise { .. }) {
        adage_tracks::KIND_ADVICE
    } else {
        adage_tracks::route_kind(r)
    }
}

/// The instinct pack's name and version (`core`, `0.1.0`).
pub fn pack() -> (&'static str, &'static str) {
    let c = catalogue();
    (&c.pack.name, &c.pack.version)
}
