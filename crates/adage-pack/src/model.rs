//! The pack model (format `adage-pack/1`, Track C §1): the manifest (`pack.toml`), route v2
//! (`routes/<id>.yaml`), named types (`types/<id>.yaml`) and aggregate evidence.
//!
//! Every struct here refuses unknown fields: a pack is untrusted input, and a field this build does
//! not know is a field whose meaning nobody checked. Choices between alternatives that YAML files
//! carry (`from: { prompt: … }`, a step's `run:` / `read:` / …) are structs of optional fields, so
//! the same shape parses from YAML, TOML and JSON alike, and [`crate::validate`] requires exactly
//! one of them.

use adage_core::spans::SpanKind;
use adage_core::Effect;
pub use adage_tracks::Aspect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// The `format` key of every `pack.toml` this build reads and writes.
pub const FORMAT: &str = "adage-pack/1";

// ---------------------------------------------------------------------------------------------
// pack.toml
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackManifest {
    /// Always [`FORMAT`].
    pub format: String,
    pub pack: PackInfo,
    pub applicability: Applicability,
    #[serde(default, skip_serializing_if = "Requires::is_empty")]
    pub requires: Requires,
    #[serde(default)]
    pub exec: ExecCeiling,
    #[serde(default)]
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PackInfo {
    /// `owner/name`: exactly one track namespace.
    pub name: String,
    /// `major.minor.patch`.
    pub version: String,
    pub origin: PackOrigin,
    /// For people only; never rendered to an agent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackOrigin {
    Reflex,
    Instinct,
    Trained,
    Authored,
}

/// Where a pack applies: exactly one form.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Applicability {
    /// Repositories by `repo_key()` (an origin URL form, or `name@<root commit>`).
    Repo {
        repo_keys: Vec<String>,
    },
    /// All of these predicates over the repository's declared facts.
    Fingerprint {
        fingerprint: Vec<Fingerprint>,
    },
    /// A detector compiled into Adage (instinct packs only): the pack names it, never supplies it.
    Detector {
        detector: String,
        catalogue: String,
    },
    Any,
}

/// One predicate over declared facts (`adage_instincts::declared`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Fingerprint {
    MakefileTarget(String),
    PackageScript(String),
    /// A repository-relative glob.
    PathExists(String),
    /// A root lockfile name.
    Lockfile(String),
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Requires {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub os: Vec<Os>,
    /// Programs an L-none step runs (argv[0]), found by PATH lookup at bind.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolReq>,
    /// `claude-code`, or `claude-code >=2.1.200`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub harness: Vec<String>,
    /// Embedding models intent triggers need (`name@<12 hex>`); never vendored.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    /// Fixed templates the routes render with (`run@1`, `advise:generated-file@1`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub templates: Vec<String>,
    /// The command-table schema version the classification assumed (`>=3`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command_table: Option<String>,
    /// Repository files L-ref steps run, pinned by SHA-256.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub repo_files: Vec<RepoFile>,
}

impl Requires {
    pub fn is_empty(&self) -> bool {
        self == &Requires::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Linux,
    Macos,
    Windows,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolReq {
    pub name: String,
    /// `>=1.6`; probed only by adage-exec in a sandbox, or at `add`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepoFile {
    pub path: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
}

/// Execution levels (Track C §3.1), shared with the index (`IndexedRoute::exec`).
pub use adage_tracks::index::Level;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecCeiling {
    /// The author's ceiling request; grants nothing.
    #[serde(default)]
    pub requested: Level,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    /// `adage 0.5.0`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool: String,
    #[serde(default)]
    pub origins: OriginCounts,
}

/// Counts by the origin of the evidence (roadmap E1).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OriginCounts {
    #[serde(default)]
    pub organic: u64,
    #[serde(default)]
    pub synthetic: u64,
    #[serde(default)]
    pub declared: u64,
    #[serde(default)]
    pub instinct: u64,
    #[serde(default)]
    pub imported: u64,
}

/// The origin names [`OriginCounts`] has.
pub const ORIGINS: [&str; 5] = ["organic", "synthetic", "declared", "instinct", "imported"];

// ---------------------------------------------------------------------------------------------
// routes/<id>.yaml
// ---------------------------------------------------------------------------------------------

/// Delivery shapes (Track G's closed set, plus the two Track C needs for chains).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    /// Run a command at a prompt (the lookup).
    Run,
    /// A known dead end: not this, that.
    Avoid,
    /// After a command ran (or failed), run this.
    After,
    /// Read a batch of files.
    Read,
    /// Fixed advice with validated values.
    Advise,
    /// After editing these files, this must be run (the finish line).
    Obligation,
    /// Several run steps, in order (at most four).
    Sequence,
}

impl Shape {
    pub fn as_str(self) -> &'static str {
        match self {
            Shape::Run => "run",
            Shape::Avoid => "avoid",
            Shape::After => "after",
            Shape::Read => "read",
            Shape::Advise => "advise",
            Shape::Obligation => "obligation",
            Shape::Sequence => "sequence",
        }
    }

    /// The fixed template a route of this shape renders with (`run@1`).
    pub fn template(self) -> String {
        format!("{}@1", self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteV2 {
    pub id: String,
    /// For people (review); never rendered to an agent.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    pub shape: Shape,
    pub trigger: TriggerV2,
    /// A route with `entry: false` is reachable only through another route's `links`.
    #[serde(default = "yes", skip_serializing_if = "is_true")]
    pub entry: bool,
    /// Typed parameters (bound from the stimulus; an execution binds them against their types).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Input>,
    /// At least one; E0 renders `steps[0]` only.
    pub steps: Vec<StepV2>,
    /// Explicit intra-pack edges: `#other-route-id`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub links: Vec<String>,
    pub aspect: Aspect,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exit: Vec<ExitV2>,
    /// The route's execution request (at most the pack's `[exec].requested`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exec: Option<ExecSpec>,
    /// Not part of the route exec digest.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evidence: Option<RouteEvidence>,
}

fn yes() -> bool {
    true
}

fn is_true(b: &bool) -> bool {
    *b
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case", deny_unknown_fields)]
pub enum TriggerV2 {
    /// A prompt containing a span of this kind.
    Prompt { span: SpanKind },
    /// The agent just ran a command matching this canonical key (`exec make gen`), successfully.
    AfterCommand { command: String },
    /// The agent ran a command matching this key and it failed with this signature.
    AfterFailure { command: String, signature: String },
    /// The agent edited a file matching this repository-relative glob.
    AfterEdit { paths: String },
    /// Once, at the first prompt of a session.
    SessionStart,
    /// A prompt in an intent family (as `adage_tracks::Trigger::Intent`).
    Intent { family: String, model: String, centroid: Vec<i16>, threshold: f32 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// A built-in type or a `types/<id>` of this pack.
    #[serde(rename = "type")]
    pub ty: String,
    pub from: InputFrom,
    /// For `type: enum` only: the values.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    /// For list types only: at most this many items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

/// Where an input's value comes from: exactly one field is set.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputFrom {
    /// A typed span in the prompt.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prompt: Option<PromptSource>,
    /// A JSON field of the output of the command that triggered the route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<ResultSource>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PromptSource {
    pub span: SpanKind,
    /// A learned value type (Track G §4.3): the first prompt token it accepts binds the input,
    /// instead of a span of `span`'s kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vtype: Option<adage_tracks::ValueType>,
    /// How people wrote the value (`#` in `#482`), when every learning trace wrote it the same
    /// one or two ways: only such a token binds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mentions: Vec<adage_core::tokens::Mention>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ResultSource {
    pub field: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepV2 {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<RunSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<ReadSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avoid: Option<AvoidSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub advise: Option<AdviseSpec>,
    /// Declared; re-classified locally, and lower than the classification is refused.
    pub effect: Effect,
    /// The script a run step's argv names (L-ref or L-vendor).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<ScriptRef>,
    /// Typed output contract (the E1 projection).
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub outputs: BTreeMap<String, Output>,
}

/// What a step does, once validation has checked exactly one kind is set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StepKind<'a> {
    Run(&'a RunSpec),
    Read(&'a ReadSpec),
    Avoid(&'a AvoidSpec),
    Advise(&'a AdviseSpec),
}

impl StepV2 {
    /// The step's kind; `None` unless exactly one of `run`, `read`, `avoid`, `advise` is set.
    pub fn kind(&self) -> Option<StepKind<'_>> {
        let set = [self.run.is_some(), self.read.is_some(), self.avoid.is_some(), self.advise.is_some()];
        if set.iter().filter(|x| **x).count() != 1 {
            return None;
        }
        if let Some(r) = &self.run {
            Some(StepKind::Run(r))
        } else if let Some(r) = &self.read {
            Some(StepKind::Read(r))
        } else if let Some(a) = &self.avoid {
            Some(StepKind::Avoid(a))
        } else {
            self.advise.as_ref().map(StepKind::Advise)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunSpec {
    /// Plain argv; `{input}` tokens are bound from the route's inputs.
    pub argv: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadSpec {
    /// `<step>.<output>` of an earlier step, or an input, of a list-of-`repo-path` type.
    pub from: String,
    pub max: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AvoidSpec {
    /// The failing runners (each an argv).
    pub runners: Vec<Vec<String>>,
    /// What works instead.
    pub instead: Vec<String>,
    /// The failure signature (rendered text: `io::safe_text`).
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdviseSpec {
    /// One of `adage_tracks::advice::TEMPLATES`.
    pub template: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub slots: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClosureLevel {
    /// A repository file pinned by hash (`requires.repo_files`).
    Ref,
    /// The bytes, in `scripts/sha256-<hex>`.
    Vendor,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScriptRef {
    /// Repository-relative (ref), or `scripts/sha256-<hex>` (vendor).
    pub path: String,
    pub closure: ClosureLevel,
    /// Lowercase hex SHA-256 of the entry file's bytes.
    pub sha256: String,
    /// Repository-relative globs hashed as a sorted manifest (ref only).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Output {
    #[serde(rename = "type")]
    pub ty: String,
    pub from: OutputFrom,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputFrom {
    /// A field of the step's JSON stdout.
    pub json: String,
}

/// Typed exit predicates, rendered from fixed templates.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "snake_case", deny_unknown_fields)]
pub enum ExitV2 {
    IntentChanges,
    ExploratoryChange,
    /// A step's typed output field equals a value (which must pass the field's type).
    ResultEquals {
        step: String,
        field: String,
        value: String,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    #[default]
    Typed,
    Raw,
}

/// Default wall timeout of an execution request.
pub const DEFAULT_TIMEOUT_MS: u32 = 1500;
/// No execution request may ask for longer.
pub const MAX_TIMEOUT_MS: u32 = 600_000;

fn default_timeout() -> u32 {
    DEFAULT_TIMEOUT_MS
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecSpec {
    pub level: Level,
    #[serde(default = "default_timeout")]
    pub timeout_ms: u32,
    #[serde(default)]
    pub output: OutputMode,
    /// E2 only: the repository-relative globs an action may change. A regenerate action must
    /// declare them; any other change refuses the application (Track C §3.6).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub outputs: Vec<String>,
    /// E2 only: cache globs. Changes there are kept by a verify action and discarded, never
    /// refused, by a regenerate action.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cache: Vec<String>,
}

/// Most globs an action may declare in `outputs` or `cache`.
pub const MAX_ACTION_GLOBS: usize = 16;

/// A route's aggregate evidence (as `adage_tracks::Evidence`, without per-machine state such as
/// `stale_since`).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteEvidence {
    #[serde(default)]
    pub opportunities: u32,
    #[serde(default)]
    pub support: u32,
    #[serde(default)]
    pub days: u32,
    #[serde(default)]
    pub successes: u32,
    #[serde(default)]
    pub deviations_failed: u32,
    #[serde(default)]
    pub rate: f64,
    #[serde(default)]
    pub lower_bound: f64,
    #[serde(default)]
    pub median_position: f64,
    #[serde(default)]
    pub turns_saved: f64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub provenance: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub exposed: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<RouteOrigin>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_origin: BTreeMap<String, u32>,
}

fn is_zero(n: &u32) -> bool {
    *n == 0
}

/// Where a route came from (as `adage_tracks::Origin`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RouteOrigin {
    Learned,
    Instinct {
        pack: String,
        version: String,
        instinct: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        ecosystem: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        declared: Option<String>,
    },
    Imported,
    Authored,
    Synthetic,
}

// ---------------------------------------------------------------------------------------------
// types/<id>.yaml
// ---------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Base {
    String,
    Int,
    Enum,
    List,
}

/// A named contract type. Each base admits only its own fields ([`crate::types`] checks them).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TypeDef {
    pub id: String,
    pub base: Base,
    /// `string`: an anchored, strict, bounded regex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pattern: Option<String>,
    /// `int`: inclusive bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<[i64; 2]>,
    /// `enum`: the values (at most 32).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub values: Option<Vec<String>>,
    /// `list`: the item type (not itself a list).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub item: Option<String>,
    /// `list`: at most this many items.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<u32>,
}

impl TypeDef {
    /// A string type from a learned value type's run skeleton (`adage_core::vtype`): its
    /// anchored regex, which uses explicit ASCII classes and is bounded by construction.
    pub fn from_runs(id: &str, runs: &adage_core::vtype::Runs) -> TypeDef {
        TypeDef {
            id: id.to_string(),
            base: Base::String,
            pattern: Some(adage_core::vtype::to_regex(runs)),
            range: None,
            values: None,
            item: None,
            max: None,
        }
    }

    /// A string type with this pattern.
    pub fn pattern(id: &str, pattern: &str) -> TypeDef {
        TypeDef {
            id: id.to_string(),
            base: Base::String,
            pattern: Some(pattern.to_string()),
            range: None,
            values: None,
            item: None,
            max: None,
        }
    }
}

// ---------------------------------------------------------------------------------------------
// evidence.json
// ---------------------------------------------------------------------------------------------

/// `evidence.json`: aggregate evidence per route id, by origin.
pub type PackEvidence = BTreeMap<String, OriginCounts>;
