//! Adage graph engine (Track G, `docs/tracks/track-g-graph-engine.md`).
//!
//! G1, value types: [`flow`] finds which argv positions carry data that came from earlier in a
//! session (value slots), and [`types`] induces a learned, anchored, bounded type for each slot
//! (run skeletons, anti-unification, MDL, safety constraints). [`learn`] runs both.
//!
//! Learning is a pure function of the *set* of sessions and the parameters: the same bytes
//! whatever the input order or the thread count (`tests/determinism.rs`).
//!
//! G0, the substrate: [`facts`] turns one session into a small typed event graph (events,
//! value-flow and file edges, token occurrences) with node embeddings from [`embed`], cached by
//! content in [`cache`].
//!
//! `adage learn` uses both when `LearnParams::engine` is `shadow` or `graph`
//! (`adage-learn/src/graph.rs`): the lookup route's slot is a learned value type, and session
//! facts are computed and cached.

pub mod cache;
pub mod compile;
pub mod embed;
pub mod events;
pub mod facts;
pub mod flow;
pub mod input;
pub mod mine;
pub mod proto;
pub mod score;
pub mod select;
pub mod stats;
pub mod types;

use flow::{FlowParams, Slot};
use serde::{Deserialize, Serialize};
use types::{Induced, Reject};

/// Parameters of a learn run.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Params {
    pub flow: FlowParams,
    /// Worker threads for per-session observation. Never changes the result.
    pub threads: usize,
}

impl Default for Params {
    fn default() -> Self {
        Params { flow: FlowParams::default(), threads: 1 }
    }
}

/// What a value slot's contract would be.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "contract", rename_all = "snake_case")]
pub enum Contract {
    /// A learned type (`Contract::Pattern { regex }` in a track).
    Pattern(Induced),
    /// Path-sourced: a repository path contract, never a regex (Track G §4.2).
    RepoPath,
    /// No safe type.
    None(Reject),
}

/// A value slot and its contract.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SlotType {
    pub slot: Slot,
    pub contract: Contract,
}

impl SlotType {
    pub fn vtype(&self) -> Option<&types::ValueType> {
        match &self.contract {
            Contract::Pattern(i) => Some(&i.vtype),
            _ => None,
        }
    }
}

/// The result of a learn run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Learned {
    pub sessions: usize,
    /// Sinks that saw at least one flow.
    pub sinks: usize,
    /// Sinks that passed the value slot test, in sink order, with their contracts.
    pub slots: Vec<SlotType>,
}

/// Find value slots in `sessions` and induce a type for each.
pub fn learn(sessions: &[input::Session], p: &Params) -> Learned {
    let obs = flow::observe_all(sessions, p.threads);
    let all = flow::fold(&obs, &p.flow);
    let sinks = all.len();
    let slots = all
        .into_iter()
        .filter(|s| s.is_slot)
        .map(|slot| {
            let contract = if slot.path_sourced() {
                Contract::RepoPath
            } else {
                match types::induce(&slot.sink.name(), slot.positives.keys(), slot.negatives.iter()) {
                    Ok(t) => Contract::Pattern(t),
                    Err(r) => Contract::None(r),
                }
            };
            SlotType { slot, contract }
        })
        .collect();
    Learned { sessions: sessions.len(), sinks, slots }
}
