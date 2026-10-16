//! The route and track model (Design §8, §10).

use crate::tagged::tagged_enum;
use adage_core::spans::SpanKind;
use adage_core::Effect;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    /// Do this: strong evidence (shown fire).
    Fire,
    /// Consider this: a suggestion (shown prime).
    Prime,
    /// Don't do this: a known dead end (shown inhibit).
    Inhibit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RouteState {
    /// Learned, but waits for a person's review; never delivered.
    Nascent,
    /// Delivered while its evidence builds.
    Forming,
    /// Established.
    Firm,
    /// Extinguished: kept on disk, never delivered.
    Extinguished,
    /// In service, but no longer re-found by learning *because of its own exposure*: most of the
    /// sessions where it could apply were steered by routes of its kind, so they are excluded from
    /// its learner (Design §11), and too few unsteered sessions remain to confirm it. Its last
    /// evidence is kept, with [`Evidence::dormant_since`] set, and it is delivered at prime at most
    /// (inhibit dead ends stay inhibit). Re-found, it returns to the state the learner gives it; contrary
    /// evidence among unsteered sessions drops it as before (`adage_learn`, E1).
    Dormant,
}

impl RouteState {
    /// Compiled into the index and delivered (`Firm`, `Forming`, `Dormant`).
    pub fn is_live(self) -> bool {
        matches!(self, RouteState::Firm | RouteState::Forming | RouteState::Dormant)
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
    /// The tests of the file whose edit fired the route's after-edit trigger (which must match
    /// `glob`): at most `k` test files from a learned map (edited source path → its test files,
    /// best first; from the repository's naming and co-change, confirmed by the traces). A file
    /// the map does not list binds nothing; each bound file must exist (the slot's contract is a
    /// repository path), and they are bound space-separated. Bound by an after-edit trigger.
    EditTests { glob: String, k: u8, map: std::collections::BTreeMap<String, Vec<String>> },
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

tagged_enum! {
#[derive(Debug, Clone, PartialEq)]
pub enum Trigger (tag = "on", mirror = TriggerRepr) {
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
    /// A learned context of two or three events (Track G §6.2): `items` in time order, each
    /// within `within` alphabet events of the one before, the last one the event that fires the
    /// route. Matched as a subsequence over **alphabet events only** (events satisfying some
    /// context item of the repository, [`crate::context`]), so no other event can change a match;
    /// the runtime keeps the session's recent symbols and walks the index's discrimination net
    /// (Track G §7, `adage_runtime::context`).
    Context { items: Vec<CtxItem>, within: u8 },
}
}

/// At most this many items in a context trigger, and at most this gap between them.
pub const CONTEXT_MAX_ITEMS: usize = 3;
pub const CONTEXT_MAX_WITHIN: u8 = 8;

/// One item of a [`Trigger::Context`] (Track G §6.2): symbolic, never an embedding (an intent
/// item is the existing intent family test).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "item", rename_all = "snake_case")]
pub enum CtxItem {
    /// A successful command whose canonical key meets one of `keys` (`pattern::meets`).
    Act { keys: Vec<String> },
    /// A failed command whose key meets one of `keys`, with this failure signature.
    Fail { keys: Vec<String>, signature: String },
    /// An edit of a file matching `glob`.
    Edit { glob: String },
    /// A prompt holding a token of the learned value type.
    PromptValue { vtype: ValueType },
    /// A prompt of an intent family (the [`Trigger::Intent`] test).
    Intent { family: String, model: String, centroid: Vec<i16>, threshold: f32 },
}

/// At most this many commands in a [`Step::Sequence`].
pub const SEQUENCE_MAX_COMMANDS: usize = 3;

/// Bounds on intent triggers (tracks are untrusted input).
pub const INTENT_MIN_DIM: usize = 8;
pub const INTENT_MAX_DIM: usize = 1024;
pub const INTENT_MIN_THRESHOLD: f32 = 0.5;
pub const INTENT_MAX_THRESHOLD: f32 = 0.99;
/// At most this many intent routes per track: matching is one dot product each, on the hot path.
pub const MAX_INTENT_ROUTES: usize = 64;

tagged_enum! {
#[derive(Debug, Clone, PartialEq)]
pub enum Step (tag = "do", mirror = StepRepr) {
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
    /// Two or three commands, run in this order (Track G §6.1). Rendered as a numbered list from a
    /// fixed template; the route's strength is at most the lowest ceiling of its commands, and
    /// whether it was followed is judged on the first.
    Sequence { commands: Vec<String> },
    /// A procedure card for a family of similar tasks ([`crate::card`]): learned from this
    /// repository's traces, triggered by an intent (the family's prompts), delivered once, prime.
    Card { card: crate::card::Card },
    /// Use a skill the harness offers (mod architecture §3.11): `skill` is its name as the harness
    /// lists it ([`SKILL_NAME_MAX`] bytes of `[A-Za-z0-9_-]`), `args` a plain argument string the
    /// Skill tool is invoked with. Followed when the agent uses that skill (the canonical key
    /// `control Skill <skill>`). The text a skill loads can carry commands of its own, so the step
    /// is never delivered above prime and is never a dead end.
    Skill {
        skill: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        args: Option<String>,
    },
}
}

/// At most this many bytes in a [`Step::Skill`] name.
pub const SKILL_NAME_MAX: usize = 64;

/// Whether `name` may be named by a [`Step::Skill`]: 1 to [`SKILL_NAME_MAX`] bytes of ASCII
/// letters, digits, `_` and `-`. Nothing else can reach the directive, the canonical key or the
/// Skill tool's input.
pub fn safe_skill_name(name: &str) -> bool {
    (1..=SKILL_NAME_MAX).contains(&name.len()) && name.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
}

/// The canonical key of using the skill `name` (`adage_core::tools`: a Skill tool use is a
/// `Control` action whose tool is `Skill <name>`): what a [`Step::Skill`] is judged followed on.
pub fn skill_key(name: &str) -> String {
    format!("control Skill {name}")
}

/// Typed exit predicates, rendered by Adage from fixed templates — never free text from a track.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "when", rename_all = "snake_case")]
pub enum Exit {
    /// The user is asking for something the route doesn't cover.
    IntentChanges,
    /// The change is exploratory or a nascent.
    ExploratoryChange,
}

/// Observational evidence for replacing a routine agent action with CPU execution. This is
/// never permission: the existing local execution grant is still required. Counts cover every eligible
/// predictive trigger, including those without a compatible successful continuation. They
/// measure observational support, not the success rate of a broader compiled action.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExecutionEvidence {
    pub opportunities: u32,
    pub successful: u32,
    pub sessions: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Evidence {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionEvidence>,
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
    /// Set while the route is [`RouteState::Dormant`]: the UTC date (`YYYY-MM-DD`) of the learn run
    /// that first kept it on its last evidence.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dormant_since: Option<String>,
    /// Where the route came from ([`Origin`]); `None` in a route written before origins were
    /// recorded, whose origin is read from `provenance` ([`Route::origin`]).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub origin: Option<Origin>,
    /// Counts by the origin of the evidence (roadmap E1): `support` and `opportunities` are
    /// organic sessions only; anything else is counted here. An instinct route records
    /// `declared: 1` (the declared fact that bound it) and, once the runtime has judged it, its
    /// `followed`, `misfired` and `ignored` judgements from treated sessions. An
    /// instinct never earns fire on these.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub by_origin: BTreeMap<String, u32>,
    /// The outcome lift the step showed (`crate::lift`): recorded whenever the traces' outcomes
    /// were known, and the reason the route exists when it formed on lift alone (prime at most).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_lift: Option<crate::lift::OutcomeLift>,
    /// Why a repository script's step was classified observe from the traces rather than by its
    /// name or the command table (`adage_learn::effects`); `None` for every other route.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effect_evidence: Option<String>,
    /// What delivering the route changes (`adage_learn::value::uplift`): how much more often an
    /// agent takes the directed step when told than it does unaided. `None` on a route written
    /// before uplift was estimated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uplift: Option<Uplift>,
}

/// A route's uplift, `p_follow − p_self` (evals/lab/research/uptake-delivery): agents follow
/// nearly every directive, so what a directive is worth is how often the agent would **not** have
/// taken the step unaided. Each probability is a Beta posterior; the counts are what it rests on.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Uplift {
    /// The kind whose prior the posteriors start from (`avoid`, `run-after-edit`, `read-batch`,
    /// `lookup`, `obligation`, `sequence`, `advise`, `run`).
    pub kind: String,
    /// Posterior mean of `p_follow` (followed when delivered).
    pub p_follow: f64,
    /// Posterior mean of `p_self` (took the step unaided in the matching situation).
    pub p_self: f64,
    /// Posterior mean of the uplift, and its one-sided 90% lower bound.
    pub mean: f64,
    pub lower: f64,
    /// `p_self`'s prior mean, and whether it was fitted across repositories (else the research
    /// constant of its kind).
    pub prior_self: f64,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub prior_fitted: bool,
    /// Training traces in the matching situation, and those whose agent took the step unaided.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub traces: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub traces_took: u32,
    /// Control-arm `would_have` situations judged, and those whose agent took the step unaided.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub control: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub control_took: u32,
    /// Treated judgements: delivered, and followed.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub delivered: u32,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub followed: u32,
    /// Below the bar: delivering it changes too little, so it is not delivered (nascent) until a
    /// person approves it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub gated: bool,
}

impl Uplift {
    /// One line for people: why a reflex speaks or is quiet.
    pub fn line(&self) -> String {
        let mut from = Vec::new();
        if self.traces > 0 {
            from.push(format!("{} of {} traces did it unaided", self.traces_took, self.traces));
        }
        if self.control > 0 {
            from.push(format!("{} of {} control situations did it unaided", self.control_took, self.control));
        }
        if self.delivered > 0 {
            from.push(format!("followed {} of {} deliveries", self.followed, self.delivered));
        }
        if from.is_empty() {
            from.push(format!(
                "no evidence yet: the {} prior{}",
                self.kind,
                if self.prior_fitted { ", fitted across repositories" } else { "" }
            ));
        }
        format!(
            "uplift {:+.2} (lower bound {:+.2}): followed {:.2} when told, {:.2} unaided; {}{}",
            self.mean,
            self.lower,
            self.p_follow,
            self.p_self,
            from.join("; "),
            if self.gated { ". Quiet: agents already do this, so delivering it changes little" } else { "" }
        )
    }
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
    pub strength: Strength,
    pub state: RouteState,
    #[serde(default)]
    pub exit: Vec<Exit>,
    pub evidence: Evidence,
    /// A person edited this route; the learner proposes changes instead of overwriting it.
    #[serde(default)]
    pub authored: bool,
}

impl Route {
    /// What this reflex tells the agent, in one sentence a person reads at a glance: when it
    /// applies and what it says (`after editing crates/**/*.rs, run cargo fmt --all`). People see
    /// this wherever a reflex is listed; the id is for commands and `--json`
    /// (docs/design/everyday-ux.md, rule 4). Advice is the advisory template's own sentence.
    pub fn gist(&self) -> String {
        if let Step::Advise { template, slots } = &self.step {
            // The instruction first, so a clipped line still says it; else the agent's sentence.
            let slot = |k: &str| slots.get(k).map(String::as_str).unwrap_or("?");
            let short = match template.as_str() {
                "generated-file" => Some(match slots.get("command") {
                    Some(c) => format!("{}: generated; run {c}, don't edit it by hand", slot("path")),
                    None => format!("{}: generated; regenerate it, don't edit it by hand", slot("path")),
                }),
                "lockfile" => {
                    Some(format!("{}: let {} update it; don't edit it by hand", slot("path"), slot("manager")))
                }
                "package-manager" => Some(format!("use {} here, not npm", slot("manager"))),
                "project-python" => Some(format!("run Python as {}", slot("interpreter"))),
                "test-command" => Some(format!("run the tests with {}", slot("command"))),
                _ => None,
            };
            if let Some(s) = short {
                return s;
            }
            if let Some(t) = crate::advice::text(template, slots) {
                return t.strip_prefix("In this repository, ").unwrap_or(&t).trim_end_matches('.').to_string();
            }
        }
        let when = match &self.trigger {
            Trigger::Prompt { span } => format!("on a prompt that names a {}", span.as_str()),
            // Command keys spell a script run as `exec <script>`; people read the script.
            Trigger::AfterCommand { command } => format!("after {}", command.strip_prefix("exec ").unwrap_or(command)),
            Trigger::AfterFailure { command, .. } => {
                format!("when {} fails", command.strip_prefix("exec ").unwrap_or(command))
            }
            Trigger::AfterEdit { paths } => format!("after editing {paths}"),
            Trigger::SessionStart => "at the start of a session".into(),
            Trigger::Intent { .. } => "on requests like these".into(),
            // The context's own events, in order (`after editing src/**, then make build`), so a
            // person reads which sequence it is rather than that there is one.
            Trigger::Context { items, .. } => {
                let event = |i: &CtxItem| match i {
                    CtxItem::Act { keys } => {
                        keys.first().map_or("a command", |k| k.strip_prefix("exec ").unwrap_or(k)).to_string()
                    }
                    CtxItem::Fail { keys, .. } => {
                        format!("{} fails", keys.first().map_or("a command", |k| k.strip_prefix("exec ").unwrap_or(k)))
                    }
                    CtxItem::Edit { glob } => format!("editing {glob}"),
                    CtxItem::PromptValue { .. } => "a prompt that names a value seen before".into(),
                    CtxItem::Intent { .. } => "a request like these".into(),
                };
                let events: Vec<String> = items.iter().map(event).collect();
                if events.is_empty() {
                    "after a sequence of steps seen before".into()
                } else {
                    format!("after {}", events.join(", then "))
                }
            }
        };
        let what = match &self.step {
            Step::Run { command } => format!("run {command}"),
            Step::ReadBatch { .. } => "read the files it names".into(),
            Step::Avoid { avoid, instead, .. } => format!("run {instead}, not {avoid}"),
            Step::Advise { template, .. } => format!("advise ({template})"),
            Step::Sequence { commands } => format!("run {}", commands.join(", then ")),
            Step::Card { .. } => "show the checklist these tasks needed before".into(),
            Step::Skill { skill, .. } => format!("use the {skill} skill"),
        };
        format!("{when}, {what}")
    }

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

    /// Whether an instinct bound this route (weaker than any reflex: prime at most, never a
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

/// The delivery shape of a route (Track G §6.1), from its trigger and step alone, never from its
/// id or track name. The learner knows no route kinds; shapes are the closed set of things a
/// directive can do, and the directive wrapper names one (`kind="obligation"`).
///
/// | Step | Trigger | Shape |
/// | --- | --- | --- |
/// | `avoid` | any | `avoid` |
/// | `run` | `after_failure` | `avoid` (announced after the failure) |
/// | `read_batch` | any | `read-batch` |
/// | `sequence` | any | `sequence` |
/// | `advise` | any | `advise` |
/// | `card` | any | `card` |
/// | `skill` | any | `skill` |
/// | `run`, a regenerate step with no slot | `after_edit` | `obligation` |
/// | `run` | anything else | `run` |
pub fn shape(r: &Route) -> &'static str {
    match (&r.trigger, &r.step) {
        (_, Step::Avoid { .. }) | (Trigger::AfterFailure { .. }, Step::Run { .. }) => "avoid",
        (_, Step::ReadBatch { .. }) => "read-batch",
        (_, Step::Sequence { .. }) => "sequence",
        (_, Step::Advise { .. }) => "advise",
        (_, Step::Card { .. }) => "card",
        (_, Step::Skill { .. }) => "skill",
        (Trigger::AfterEdit { .. }, Step::Run { .. }) if r.effect == Effect::Regenerate && r.slots.is_empty() => {
            "obligation"
        }
        _ => "run",
    }
}

/// Every delivery shape [`shape`] names.
pub const SHAPES: [&str; 8] = ["run", "avoid", "read-batch", "obligation", "sequence", "advise", "card", "skill"];

/// The strength a route is delivered at: its own, except that a route forming or dormant, or
/// an instinct, is never delivered fire (and an instinct never inhibit).
pub fn delivered_strength(r: &Route) -> Strength {
    if r.is_instinct() {
        return Strength::Prime;
    }
    match (r.state, r.strength) {
        (RouteState::Forming | RouteState::Dormant, Strength::Fire) => Strength::Prime,
        (_, a) => a,
    }
}

/// The strongest strength a [`Step::Skill`] may ever be given, whatever its effect: the text a
/// skill loads can carry commands, so its use is suggested, never said as an instruction
/// (mod architecture §3.11).
pub const SKILL_CEILING: Strength = Strength::Prime;

/// The strongest strength a route may be given: its effect's [`ceiling`], and never above
/// [`SKILL_CEILING`] for a skill step.
pub fn step_ceiling(step: &Step, effect: Effect, triggered_by_edit: bool) -> Option<Strength> {
    match step {
        Step::Skill { .. } => ceiling(effect, triggered_by_edit).map(|_| SKILL_CEILING),
        _ => ceiling(effect, triggered_by_edit),
    }
}

/// The strongest strength an effect class may ever be given (Design §8.6).
pub fn ceiling(effect: Effect, triggered_by_edit: bool) -> Option<Strength> {
    match effect {
        Effect::Observe | Effect::Verify => Some(Strength::Fire),
        Effect::Regenerate => Some(if triggered_by_edit { Strength::Fire } else { Strength::Prime }),
        Effect::Environment => Some(Strength::Prime),
        Effect::Author | Effect::Publish => None,
    }
}

/// The state a learned or imported route starts in (Design §10.4, O11). An environment step may
/// be delivered only on a route a person approved in `adage review`, so it starts as nascent, which
/// is never delivered; `approve` puts it into service (prime at most, its ceiling). Every other
/// directable step starts in service.
pub fn initial_state(effect: Effect) -> RouteState {
    if effect == Effect::Environment {
        RouteState::Nascent
    } else {
        RouteState::Firm
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

    /// A reflex as people read it: when it applies and what it says, instruction first for advice.
    #[test]
    fn a_reflex_reads_as_what_it_tells_the_agent() {
        let r = |trigger: Trigger, step: Step| Route {
            id: "x".into(),
            summary: String::new(),
            trigger,
            step,
            slots: vec![],
            effect: Effect::Verify,
            strength: Strength::Prime,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence::default(),
            authored: false,
        };
        let edit = |p: &str| Trigger::AfterEdit { paths: p.into() };
        let run = |c: &str| Step::Run { command: c.into() };
        let advise = |t: &str, kv: &[(&str, &str)]| Step::Advise {
            template: t.into(),
            slots: kv.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
        };
        let cases = [
            (r(edit("crates/**/*.rs"), run("cargo fmt --all")), "after editing crates/**/*.rs, run cargo fmt --all"),
            (
                r(
                    Trigger::AfterFailure { command: "python3 -m pytest".into(), signature: "x".into() },
                    Step::Avoid {
                        avoid: "python3 -m pytest".into(),
                        instead: "make test".into(),
                        signature: "x".into(),
                    },
                ),
                "when python3 -m pytest fails, run make test, not python3 -m pytest",
            ),
            (
                r(
                    Trigger::Intent { family: "f".into(), model: "m".into(), centroid: vec![], threshold: 0.8 },
                    run("git status"),
                ),
                "on requests like these, run git status",
            ),
            (
                r(edit("x"), Step::Sequence { commands: vec!["make gen".into(), "make check".into()] }),
                "after editing x, run make gen, then make check",
            ),
            (
                r(
                    Trigger::Context {
                        items: vec![
                            CtxItem::Edit { glob: "crates/**/*.rs".into() },
                            CtxItem::Act { keys: vec!["exec scripts/check".into()] },
                        ],
                        within: 8,
                    },
                    run("cargo build"),
                ),
                "after editing crates/**/*.rs, then scripts/check, run cargo build",
            ),
            (
                r(edit("Cargo.lock"), advise("lockfile", &[("path", "Cargo.lock"), ("manager", "cargo")])),
                "Cargo.lock: let cargo update it; don't edit it by hand",
            ),
            (
                r(
                    edit("src/tokens.rs"),
                    advise("generated-file", &[("path", "src/tokens.rs"), ("command", "cargo xtask tokens")]),
                ),
                "src/tokens.rs: generated; run cargo xtask tokens, don't edit it by hand",
            ),
        ];
        for (route, want) in cases {
            assert_eq!(route.gist(), want);
        }
    }

    #[test]
    fn wilson_matches_known_values() {
        assert!((wilson_lower(50, 50) - 0.9287).abs() < 1e-3);
        assert_eq!(wilson_lower(0, 0), 0.0);
    }

    #[test]
    fn ceilings() {
        assert_eq!(ceiling(Effect::Author, false), None);
        assert_eq!(ceiling(Effect::Publish, true), None);
        assert_eq!(ceiling(Effect::Environment, true), Some(Strength::Prime));
        assert_eq!(ceiling(Effect::Regenerate, false), Some(Strength::Prime));
        assert_eq!(ceiling(Effect::Regenerate, true), Some(Strength::Fire));
    }
}
