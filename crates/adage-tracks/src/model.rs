//! The route and track model (Design §8, §10).

use adage_core::spans::SpanKind;
use adage_core::Effect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

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
    /// In service, but no longer re-found by learning *because of its own exposure*: most of the
    /// sessions where it could apply were steered by routes of its kind, so they are excluded from
    /// its miner (Design §11), and too few unsteered sessions remain to confirm it. Its last
    /// evidence is kept, with [`Evidence::stale_since`] set, and it is delivered at yellow at most
    /// (red dead ends stay red). Re-found, it returns to the state the miner gives it; contrary
    /// evidence among unsteered sessions drops it as before (`adage_learn`, E1).
    Stale,
}

impl RouteState {
    /// Compiled into the index and delivered (`Active`, `Probation`, `Stale`).
    pub fn is_live(self) -> bool {
        matches!(self, RouteState::Active | RouteState::Probation | RouteState::Stale)
    }
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
    /// The first token of the prompt (`adage_core::tokens`, sub-tokens included) that the learned
    /// value type accepts (Track G §4.3). Bound by a prompt trigger. With `mentions`, only a token
    /// written one of these ways counts (`#` before `482` in `#482`; an empty mention is the
    /// whole token): learned from how people named the value, so a stray number in a prompt is
    /// not taken for a `#482`-style id.
    PromptValue {
        vtype: ValueType,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        mentions: Vec<adage_core::tokens::Mention>,
    },
    /// A value from the triggering command's output: the JSON leaf at `field` (a dotted path),
    /// or, without a field, the first token of the text output, that the learned type accepts.
    /// Bound by an after-command trigger.
    OutputValue {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        field: Option<String>,
        vtype: ValueType,
    },
    /// Segment `depth` (0 = the first directory) of the path whose edit fired the route's
    /// after-edit trigger, which must match `glob`. Bound by an after-edit trigger.
    EditSegment { glob: String, depth: u8 },
}

impl SlotSource {
    /// The learned value type this source binds with, if any.
    pub fn vtype(&self) -> Option<&ValueType> {
        match self {
            SlotSource::PromptValue { vtype, .. } | SlotSource::OutputValue { vtype, .. } => Some(vtype),
            _ => None,
        }
    }
}

/// A learned value type as a track stores it (Track G §4.3): the samples themselves never, only
/// their hash. The slot's contract is still `Contract::Pattern { regex }` with this `regex`, so
/// validation is unchanged; the runtime matches `runs` with `adage_core::vtype::matches`, which
/// agrees with the regex on every string, and so compiles no regex in the hook. `io::validate`
/// checks that `runs` and `regex` agree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ValueType {
    /// Stable: a hash of the sink identity and the skeleton without lengths (`vt-<16 hex>`).
    pub id: String,
    pub runs: adage_core::vtype::Runs,
    /// The anchored regex of `runs` (`adage_core::vtype::to_regex`).
    pub regex: String,
    /// SHA-256 (hex) of the sorted distinct samples; empty when unknown.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub samples_hash: String,
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
    /// Advice from a fixed template ([`crate::advice::TEMPLATES`]: `generated-file`, `lockfile`,
    /// `package-manager`), with validated slot values. No text comes from the track.
    Advise {
        template: String,
        #[serde(default)]
        slots: BTreeMap<String, String>,
    },
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
    /// Distinct sessions where the trigger occurred but which were steered by a route of this
    /// route's kind, and so are excluded from its evidence (Design §11). Not part of
    /// `opportunities`. High exposure explains a route that learning can no longer confirm.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub exposed: u32,
    /// Set while the route is [`RouteState::Stale`]: the UTC date (`YYYY-MM-DD`) of the learn run
    /// that first kept it on its last evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stale_since: Option<String>,
    /// Where the route came from ([`Origin`]); `None` in a route written before origins were
    /// recorded, whose origin is read from `provenance` ([`Route::origin`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Counts by the origin of the evidence (roadmap E1): `support` and `opportunities` are
    /// organic sessions only; anything else is counted here. An instinct route records
    /// `declared: 1` (the declared fact that bound it) and, once the runtime has judged it, its
    /// `followed`, `followed_failed` and `derailed` judgements from treated sessions. An
    /// instinct never earns green on these.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_origin: BTreeMap<String, u32>,
}

/// Where a route came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Origin {
    /// Mined from this repository's agent sessions (a reflex).
    Learned,
    /// Bound from the repository's declared files by an instinct (`adage_instincts`).
    Instinct {
        /// The instinct pack and its version (`core`, `0.1.0`).
        pack: String,
        version: String,
        /// The catalogue id (`U1`, `U5`, …).
        instinct: String,
        /// The ecosystem, one of [`ECOSYSTEMS`], for the directive's wrapper.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ecosystem: Option<String>,
        /// The repository file that declares what the route directs (`Makefile`,
        /// `package.json`, a generated file), repository-relative.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        declared: Option<String>,
    },
    /// Installed from a shared track (`adage add`).
    Imported,
    /// Written or edited by a person.
    Authored,
    /// Generated for evaluation (never delivered outside an eval).
    Synthetic,
}

/// The ecosystems an instinct's wrapper may name.
pub const ECOSYSTEMS: [&str; 5] = ["Python", "Node.js", "Rust", "Go", "Make"];

fn is_zero(n: &u32) -> bool {
    *n == 0
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

impl Route {
    /// Where this route came from: the recorded origin, else what its provenance says
    /// (`instinct:<pack>@<version>`, `authored`, `imported…`), else learned.
    pub fn origin(&self) -> Origin {
        if let Some(o) = &self.evidence.origin {
            return o.clone();
        }
        let p = self.evidence.provenance.as_str();
        if let Some(rest) = p.strip_prefix("instinct:") {
            let (pack, version) = rest.split_once('@').unwrap_or((rest, ""));
            return Origin::Instinct {
                pack: pack.into(),
                version: version.into(),
                instinct: String::new(),
                ecosystem: None,
                declared: None,
            };
        }
        if self.authored || p == "authored" {
            Origin::Authored
        } else if p.starts_with("imported") {
            Origin::Imported
        } else if p == "synthetic" {
            Origin::Synthetic
        } else {
            Origin::Learned
        }
    }

    /// Whether an instinct bound this route (weaker than any reflex: yellow at most, never a
    /// Stop request, lowest precedence in the index).
    pub fn is_instinct(&self) -> bool {
        matches!(self.evidence.origin, Some(Origin::Instinct { .. }))
            || self.evidence.provenance.starts_with("instinct:")
    }
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

/// Route kinds: the miner (and learned track `<repo>/<kind>`) a route belongs to.
pub const KIND_WORK_ITEMS: &str = "work-items";
pub const KIND_FINISH_LINE: &str = "finish-line";
pub const KIND_DEAD_ENDS: &str = "dead-ends";
pub const KIND_INTENTS: &str = "intents";
/// Advisory steps ([`Step::Advise`]): no miner learns them, and they steer no miner but their own.
pub const KIND_ADVICE: &str = "advice";
/// Anything no miner produces (an imported or hand-written shape): it may influence anything.
pub const KIND_OTHER: &str = "other";
/// Every kind a miner learns.
pub const MINER_KINDS: [&str; 4] = [KIND_WORK_ITEMS, KIND_FINISH_LINE, KIND_DEAD_ENDS, KIND_INTENTS];

/// The kind of a route, from its *shape* (trigger and step), never from its id or track name,
/// which an imported track chooses freely. Each shape is one a miner produces:
///
/// | Trigger | Step | Kind |
/// | --- | --- | --- |
/// | `prompt` | `run` | `work-items` (the lookup) |
/// | `after_command` | `read_batch` | `work-items` (the linked files) |
/// | `after_edit` | `run` | `finish-line` |
/// | `session_start` | `avoid` | `dead-ends` |
/// | `after_failure` | `run` | `dead-ends` (the instinct form of a dead end: the ids line up) |
/// | `intent` | `run` | `intents` |
/// | any | `advise` | `advice` |
/// | anything else | | `other` |
pub fn route_kind(r: &Route) -> &'static str {
    match (&r.trigger, &r.step) {
        (Trigger::Prompt { .. }, Step::Run { .. }) => KIND_WORK_ITEMS,
        (Trigger::AfterCommand { .. }, Step::ReadBatch { .. }) => KIND_WORK_ITEMS,
        (Trigger::AfterEdit { .. }, Step::Run { .. }) => KIND_FINISH_LINE,
        (Trigger::SessionStart, Step::Avoid { .. }) => KIND_DEAD_ENDS,
        (Trigger::AfterFailure { .. }, Step::Run { .. }) => KIND_DEAD_ENDS,
        (Trigger::Intent { .. }, Step::Run { .. }) => KIND_INTENTS,
        (_, Step::Advise { .. }) => KIND_ADVICE,
        _ => KIND_OTHER,
    }
}

/// The aspect a route is delivered at: its own, except that a route on probation or stale, or
/// an instinct, is never delivered green (and an instinct never red).
pub fn delivered_aspect(r: &Route) -> Aspect {
    if r.is_instinct() {
        return Aspect::Yellow;
    }
    match (r.state, r.aspect) {
        (RouteState::Probation | RouteState::Stale, Aspect::Green) => Aspect::Yellow,
        (_, a) => a,
    }
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
