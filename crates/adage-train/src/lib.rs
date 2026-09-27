//! `adage-train`: compile a skill into reflexes.
//!
//! A *reflex* is a route learned from sessions; an *instinct* is a prebuilt route
//! (`adage-instincts`). `adage-train` makes reflexes from a skill (or a runbook, CONTRIBUTING.md,
//! a Makefile, package.json or CI workflow) before anyone has worked in the repository:
//!
//! 1. [`plan`]: extract the source's claims deterministically ([`claims`]), generate seeded tasks
//!    that exercise them and controls that do not ([`tasks`]), and estimate the cost. Free.
//! 2. [`run`]: drive the person's own agent harness headlessly ([`harness`]), sandboxed and
//!    isolated ([`isolate`]), on disposable clones, only when a person asks at a terminal; mine
//!    the transcripts with adage-learn into *staged* tracks (never activated), and optionally
//!    validate them against the skill in held-out arms.
//! 3. [`report`]: what became of each claim (compiled, contradicted, never exercised, unsafe),
//!    what it cost, and the acceptance table.
//!
//! The `adage` binary (which hooks run) never executes anything; this is a separate, optional
//! binary. See docs/training.md.

pub mod claims;
pub mod fake;
pub mod harness;
pub mod isolate;
pub mod oracle;
pub mod plan;
pub mod report;
pub mod run;
pub mod tasks;
