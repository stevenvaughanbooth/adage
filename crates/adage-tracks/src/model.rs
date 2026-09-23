//! The route and track model (Design §8, §10).

use adage_core::spans::SpanKind;
use adage_core::Effect;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Aspect {
    Green,
    Yellow,
    Red,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteState {
    Draft,
    Probation,
    Active,
    Demoted,
    Retired,
}

/// Where a slot's value comes from. Values are *proposed* from these sources and must pass the
/// slot's contract before they can appear in a directive.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum SlotSource {
    /// A typed span in the user's prompt.
    Prompt { span: SpanKind },
    /// A JSON field in the output of the command that triggered this route.
    ResultField { field: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Contract {
    /// Anchored regex; the whole value must match.
    Pattern { regex: String },
    /// A repository-relative path: no absolute paths, no `..`, no leading `-`, and it must exist.
    RepoPath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Slot {
    pub name: String,
    pub source: SlotSource,
    pub contract: Contract,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum Trigger {
    /// A prompt containing a span of this kind.
    Prompt { span: SpanKind },
    /// The agent just ran a command matching this template, successfully.
    AfterCommand { command: String },
    /// The agent just ran a command matching this template and it failed with this signature.
    AfterFailure { command: String, signature: String },
    /// The agent edited a file matching this glob.
    AfterEdit { paths: String },
    /// Delivered once, at the first prompt of a session in this repository.
    SessionStart,
    /// A prompt that means what an intent family means (Design §8.2.1): the normalised prompt's
    /// embedding, from exactly `model`, has cosine similarity >= `threshold` to `centroid`.
    /// Raw prompts never travel in a track; only the centroid does, as integers in units of 1e-4
    /// (`adage_intent::quantize`). A different or missing model means the route never fires.
    Intent { family: String, model: String, centroid: Vec<i16>, threshold: f32 },
}

/// Bounds on intent triggers (tracks are untrusted input).
pub const INTENT_MIN_DIM: usize = 8;
pub const INTENT_MAX_DIM: usize = 1024;
pub const INTENT_MIN_THRESHOLD: f32 = 0.5;
pub const INTENT_MAX_THRESHOLD: f32 = 0.99;
/// At most this many intent routes per track: matching is one dot product each, on the hot path.
pub const MAX_INTENT_ROUTES: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Step {
    /// Run a command. Tokens of the form `{slot}` are bound from the route's slots.
    Run { command: String },
    /// Read a batch of files, bound from a list-valued slot, in one turn.
    ReadBatch { slot: String, max: usize },
    /// A known dead end: `avoid` fails in this repository; `instead` works.
    Avoid { avoid: String, instead: String, signature: String },
}

/// Typed exit predicates, rendered by Adage from fixed templates — never free text from a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "snake_case")]
pub enum Exit {
    /// The user is asking for something the route doesn't cover.
    IntentChanges,
    /// The change is exploratory or a draft.
    ExploratoryChange,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    /// Distinct sessions in which the trigger occurred.
    pub opportunities: u32,
    /// Distinct sessions that did what the route directs.
    pub support: u32,
    /// Distinct calendar days represented in `support`.
    pub days: u32,
    /// Sessions following the route that succeeded, where outcomes are known.
    pub successes: u32,
    /// Sessions deviating from the route that failed, where outcomes are known.
    pub deviations_failed: u32,
    /// support / opportunities.
    pub rate: f64,
    /// Wilson 95% lower bound on `rate`, counted in sessions.
    pub lower_bound: f64,
    /// Median position of the directed action within the window (0 = first action).
    pub median_position: f64,
    /// Turns the route is expected to save when followed, estimated from the sessions.
    pub turns_saved: f64,
    /// `desire-path`, `proving`, or `authored`.
    pub provenance: String,
    /// Models the evidence was gathered on.
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Route {
    pub id: String,
    pub summary: String,
    pub trigger: Trigger,
    pub step: Step,
    #[serde(default)]
    pub slots: Vec<Slot>,
    pub effect: Effect,
    pub aspect: Aspect,
    pub state: RouteState,
    #[serde(default)]
    pub exit: Vec<Exit>,
    pub evidence: Evidence,
    /// A person edited this route; the miner proposes changes instead of overwriting it.
    #[serde(default)]
    pub authored: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub track: TrackInfo,
    pub scope: Scope,
    #[serde(default)]
    pub gauge: Gauge,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TrackInfo {
    pub name: String,
    pub version: String,
    pub description: String,
    pub provenance: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Scope {
    /// Repository key the track applies to.
    pub repo: String,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Gauge {
    #[serde(default)]
    pub harness: Vec<String>,
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Track {
    pub manifest: Manifest,
    pub routes: Vec<Route>,
}

/// The strongest aspect an effect class may ever be given (Design §8.6).
pub fn ceiling(effect: Effect, triggered_by_edit: bool) -> Option<Aspect> {
    match effect {
        Effect::Observe | Effect::Verify => Some(Aspect::Green),
        Effect::Regenerate => Some(if triggered_by_edit { Aspect::Green } else { Aspect::Yellow }),
        Effect::Environment => Some(Aspect::Yellow),
        Effect::Author | Effect::Publish => None,
    }
}

/// The state a learned or imported route starts in (Design §10.4, O11). An environment step may
/// be delivered only on a route a person approved in `adage review`, so it starts as a draft, which
/// is never delivered; `approve` puts it into service (yellow at most, its ceiling). Every other
/// directable step starts in service.
pub fn initial_state(effect: Effect) -> RouteState {
    if effect == Effect::Environment {
        RouteState::Draft
    } else {
        RouteState::Active
    }
}

/// Wilson score interval lower bound at 95%.
pub fn wilson_lower(successes: u32, n: u32) -> f64 {
    if n == 0 {
        return 0.0;
    }
    let z = 1.959_964_f64;
    let n_f = n as f64;
    let p = successes as f64 / n_f;
    let d = 1.0 + z * z / n_f;
    ((p + z * z / (2.0 * n_f)) - z * ((p * (1.0 - p) / n_f) + z * z / (4.0 * n_f * n_f)).sqrt()) / d
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wilson_matches_known_values() {
        assert!((wilson_lower(50, 50) - 0.9287).abs() < 1e-3);
        assert_eq!(wilson_lower(0, 0), 0.0);
    }

    #[test]
    fn ceilings() {
        assert_eq!(ceiling(Effect::Author, false), None);
        assert_eq!(ceiling(Effect::Publish, true), None);
        assert_eq!(ceiling(Effect::Environment, true), Some(Aspect::Yellow));
        assert_eq!(ceiling(Effect::Regenerate, false), Some(Aspect::Yellow));
        assert_eq!(ceiling(Effect::Regenerate, true), Some(Aspect::Green));
    }
}
