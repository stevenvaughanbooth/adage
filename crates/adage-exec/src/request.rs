//! What a caller asks `adage-exec` to run. C1/C3 adapt packs and bound routes to this shape; the
//! `run` command builds it from flags.
//!
//! A request carries an argv *template* (`["./scripts/wi-lookup", "{item}", "--json"]`) and the
//! values bound to its slots. There is never a shell: each argv element is passed to `execve` as
//! is, after its slots are replaced by values that passed their contracts.

use adage_core::Effect;
use anyhow::{bail, Context, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

use crate::sandbox::Profile;

/// Execution levels (Track C §3.1). E0 (directives only) never reaches this
/// crate. E1 prefetches read-only, typed data at the stimulus; E2 is an action a hook runs: staged,
/// verified against its declared outputs, applied atomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    E1,
    E2,
}

impl Level {
    pub fn parse(s: &str) -> Option<Level> {
        match s.to_ascii_lowercase().as_str() {
            "e1" => Some(Level::E1),
            "e2" => Some(Level::E2),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::E1 => "e1",
            Level::E2 => "e2",
        }
    }
}

/// How output reaches the agent (Track C §3.4). Typed is the default and the only mode that can be
/// approved automatically; raw needs its own approval and policy's `allow_raw_output`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OutputMode {
    #[default]
    Typed,
    Raw,
}

impl OutputMode {
    pub fn parse(s: &str) -> Option<OutputMode> {
        match s {
            "typed" => Some(OutputMode::Typed),
            "raw" => Some(OutputMode::Raw),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            OutputMode::Typed => "typed",
            OutputMode::Raw => "raw",
        }
    }
}

/// The built-in output types (Track C §1.4). User types reduce to one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FieldType {
    RepoPath,
    RepoPathList,
    GitRev,
    Int,
    Bool,
    Enum,
    Pattern,
    /// Bounded plain text (a work item's title, description, state): control and invisible
    /// characters stripped, whitespace collapsed to single spaces, capped at `max` characters
    /// ([`DEFAULT_TEXT`], never above [`MAX_TEXT`]), and withheld when the output scanner finds
    /// instruction-shaped text, a secret or an Adage tag in it (`crate::scan`).
    Text,
}

/// Default cap on a text field, in characters.
pub const DEFAULT_TEXT: usize = 600;
/// Hard cap on a text field, in characters.
pub const MAX_TEXT: usize = 2000;

/// Hard cap on a list field, whatever the spec says.
pub const MAX_LIST: usize = 64;
/// Default cap on a list field when the spec gives none.
pub const DEFAULT_LIST: usize = 20;
/// Most values an enum may declare (§1.4).
pub const MAX_ENUM: usize = 32;
/// Most fields a typed output may declare.
pub const MAX_FIELDS: usize = 16;

/// One declared output field: its type, where it comes from in stdout's JSON, and its bounds.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FieldSpec {
    #[serde(rename = "type")]
    pub ty: FieldType,
    /// Dotted path of object keys in stdout's JSON; the field's own name when absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub json: Option<String>,
    /// `repo-path-list`: at most this many entries are kept (default 20, never above 64). `text`:
    /// at most this many characters (default 600, never above 2000).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max: Option<usize>,
    /// `enum`: the allowed values (each a safe id, at most 32).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub values: Vec<String>,
    /// `pattern`: an anchored regex that passes the strictness probes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regex: Option<String>,
    /// `int`: inclusive bounds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_value: Option<i64>,
    /// When true, a missing field is left out instead of withholding the block.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub optional: bool,
    /// `repo-path-list`: read from line-oriented **text** output, not JSON: the repository
    /// paths its lines mention, in order, each validated as a `repo-path` (an existing file or
    /// directory inside the repository); nothing else of the text passes. So a plain-text lookup
    /// (`Linked files: acme/customers.py, tests/test_customers.py`) is approved typed, no raw
    /// output needed. `text` (the type): read from whichever shape the output has, the JSON
    /// object's field, or else the text line labelled with the field's name (`Title: ...` for
    /// `title`, `_` read as a space, any case).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub text: bool,
    /// Preserve complete stdout as bounded, scanned text, including its line breaks. Unlike
    /// a labelled field this is an explicit unstructured-output contract: it requires the raw
    /// output policy and a person grant. Overflow withholds instead of returning a partial task.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stdout: bool,
}

/// The output contract: typed fields, or raw text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct OutputSpec {
    pub mode: OutputMode,
    #[serde(default)]
    pub fields: BTreeMap<String, FieldSpec>,
}

/// A script the argv runs (L-ref: a repository path; L-vendor: a CAS path), pinned by SHA-256.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ScriptRef {
    pub path: String,
    pub sha256: String,
}

/// Labels for the data block and the approval store: which track, route and step this is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Labels {
    pub track: String,
    pub route: String,
    pub step: String,
}

impl Default for Labels {
    fn default() -> Self {
        Labels { track: "local/adhoc@0.0.0".into(), route: "adhoc".into(), step: "run".into() }
    }
}

/// Where a route came from, for automatic approval (§4.2): local (a reflex or instinct), or an
/// imported pack with its signer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Origin {
    #[default]
    Local,
    Pack {
        signer: Option<String>,
        /// The signer is in the trust anchors.
        trusted: bool,
        /// The pack has passed local forming.
        forming_passed: bool,
    },
}

/// What an E2 action does (Track C §3.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActionKind {
    /// Rewrites generated files: only the declared outputs may change.
    Regenerate,
    /// Checks: no source may change; only cache paths are kept.
    Verify,
}

impl ActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::Regenerate => "regenerate",
            ActionKind::Verify => "verify",
        }
    }
}

/// An E2 action's declared change set: the globs it may change (`outputs`) and cache globs
/// (`cache`). Part of the route exec digest, so an approval covers exactly these.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ActionSpec {
    pub kind: ActionKind,
    #[serde(default)]
    pub outputs: Vec<String>,
    #[serde(default)]
    pub cache: Vec<String>,
}

impl ActionSpec {
    /// Check the globs: safe, repository-relative, never under `.git`, bounded; a regenerate
    /// action declares at least one output.
    pub fn validate(&self) -> Result<()> {
        for (what, globs) in [("outputs", &self.outputs), ("cache", &self.cache)] {
            if globs.len() > adage_pack::MAX_ACTION_GLOBS {
                bail!("{what}: at most {} globs", adage_pack::MAX_ACTION_GLOBS);
            }
            if let Some(g) = globs.iter().find(|g| !adage_pack::validate::action_glob_ok(g)) {
                bail!("{what}: {g:?} is not a repository-relative glob outside .git");
            }
        }
        if self.kind == ActionKind::Regenerate && self.outputs.is_empty() {
            bail!("a regenerate action declares the outputs it may change");
        }
        Ok(())
    }
}

/// A request to run one step.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecRequest {
    /// The repository's root. Canonicalised before use; the working directory of the run.
    pub repo_root: PathBuf,
    /// The argv template: literal elements and `{slot}` placeholders.
    pub argv: Vec<String>,
    /// Values bound to the template's slots.
    #[serde(default)]
    pub inputs: BTreeMap<String, String>,
    /// Each slot's contract: an anchored regex. A slot without one cannot be bound.
    #[serde(default)]
    pub contracts: BTreeMap<String, String>,
    /// The effect the person approves (or the route declares).
    pub effect: Effect,
    pub level: Level,
    pub output: OutputSpec,
    #[serde(default)]
    pub scripts: Vec<ScriptRef>,
    /// An exact, locally approved Python environment for staged E2 verification.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_python: Option<crate::project_python::ProjectPython>,
    pub profile: Profile,
    #[serde(default)]
    pub labels: Labels,
    #[serde(default)]
    pub origin: Origin,
    /// The route exec digest (§1.5) when a pack supplies one; otherwise derived from the request.
    #[serde(default)]
    pub route_exec_digest: Option<String>,
    /// E2 only: what the action may change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<ActionSpec>,
}

/// A slot placeholder: `{name}` with a safe id inside.
static SLOT: Lazy<Regex> = Lazy::new(|| Regex::new(r"\{([a-z][a-z0-9_-]{0,31})\}").unwrap());

/// A bound value: plain argv characters only (`plain_token`), no leading `-`, no `..` segment.
pub fn plain_token(v: &str) -> bool {
    !v.is_empty()
        && v.len() <= 200
        && !v.starts_with('-')
        && v.chars().all(|c| c.is_ascii_alphanumeric() || "_./:=@,+-".contains(c))
        && !v.split('/').any(|seg| seg == "..")
}

/// An anchored regex that rejects every hostile probe (the same probes as
/// `adage_tracks::io::contract_is_strict`).
pub fn contract_is_strict(regex: &str) -> bool {
    let Ok(re) = Regex::new(regex) else { return false };
    let probes = ["a b", "a;b", "a|b", "$(x)", "`x`", "a&b", "a\nb", "../x", "-x", "a>b", "a<b", "a'b", "a\"b", ""];
    regex.starts_with('^') && regex.ends_with('$') && !probes.iter().any(|p| re.is_match(p))
}

/// A safe identifier: field names, enum values, labels.
pub fn safe_id(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 64
        && s.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
        && s.chars().all(|c| c.is_ascii_alphanumeric() || "_-".contains(c))
}

/// The slot names an argv template uses, in order of first use.
pub fn slots(argv: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for a in argv {
        for c in SLOT.captures_iter(a) {
            let n = c[1].to_string();
            if !out.contains(&n) {
                out.push(n);
            }
        }
    }
    out
}

impl OutputSpec {
    /// Whole stdout carries unstructured content even when wrapped in a typed JSON field.
    pub fn unstructured(&self) -> bool {
        self.mode == OutputMode::Raw || self.fields.values().any(|f| f.stdout)
    }

    /// Check the spec itself: safe names, bounded enums, strict patterns.
    pub fn validate(&self) -> Result<()> {
        if self.mode == OutputMode::Typed && self.fields.is_empty() {
            bail!("typed output declares no fields");
        }
        if self.fields.len() > MAX_FIELDS {
            bail!("typed output declares {} fields (at most {MAX_FIELDS})", self.fields.len());
        }
        for (name, f) in &self.fields {
            if !safe_id(name) {
                bail!("field name {name:?} is not a safe id");
            }
            if f.stdout && (f.ty != FieldType::Text || f.text || f.json.is_some() || f.optional) {
                bail!("field {name}: stdout must be required text with no text or json selector");
            }
            if f.text && f.ty == FieldType::RepoPathList && f.json.is_some() {
                bail!("field {name}: only a repo-path-list with no json path is read from text output");
            }
            if f.text && !matches!(f.ty, FieldType::RepoPathList | FieldType::Text) {
                bail!("field {name}: only a repo-path-list or a text field is read from text output");
            }
            if let Some(j) = &f.json {
                if j.is_empty() || !j.split('.').all(safe_id) {
                    bail!("field {name}: json path {j:?} is not a dotted path of safe ids");
                }
            }
            match f.ty {
                FieldType::Enum => {
                    if f.values.is_empty() || f.values.len() > MAX_ENUM {
                        bail!("field {name}: an enum needs 1 to {MAX_ENUM} values");
                    }
                    if let Some(v) = f.values.iter().find(|v| !safe_id(v)) {
                        bail!("field {name}: enum value {v:?} is not a safe id");
                    }
                }
                FieldType::Pattern => {
                    let Some(re) = &f.regex else { bail!("field {name}: a pattern needs a regex") };
                    if !contract_is_strict(re) {
                        bail!("field {name}: pattern {re:?} is not strict (anchored, and rejects shell syntax)");
                    }
                }
                FieldType::RepoPathList => {
                    if f.max.is_some_and(|m| m == 0 || m > MAX_LIST) {
                        bail!("field {name}: max must be 1 to {MAX_LIST}");
                    }
                }
                FieldType::Text if f.max.is_some_and(|m| m == 0 || m > MAX_TEXT) => {
                    bail!("field {name}: max must be 1 to {MAX_TEXT} characters");
                }
                _ => {}
            }
        }
        Ok(())
    }
}

impl ExecRequest {
    /// The argv with every slot replaced by its bound value, after checking each value against its
    /// contract and `plain_token`. An unbound slot, a missing or lax contract, or a bad value
    /// refuses the request.
    pub fn bind(&self) -> Result<Vec<String>> {
        if self.argv.is_empty() || self.argv.len() > 64 {
            bail!("argv must have 1 to 64 elements");
        }
        for (i, a) in self.argv.iter().enumerate() {
            if a.is_empty() || a.len() > 400 || a.contains('\0') {
                bail!("argv[{i}] is empty, too long or contains NUL");
            }
            if a.chars().any(|c| c.is_control() || adage_core_invisible(c)) {
                bail!("argv[{i}] contains a control or invisible character");
            }
        }
        let used = slots(&self.argv);
        if let Some(extra) = self.inputs.keys().find(|k| !used.contains(k)) {
            bail!("input {extra:?} is not a slot of the argv template");
        }
        let mut values = BTreeMap::new();
        for name in &used {
            let v = self.inputs.get(name).with_context(|| format!("slot {{{name}}} is not bound"))?;
            let re = self.contracts.get(name).with_context(|| format!("slot {{{name}}} has no contract"))?;
            if !contract_is_strict(re) {
                bail!("slot {{{name}}}: contract {re:?} is not strict");
            }
            if !Regex::new(re).map(|r| r.is_match(v)).unwrap_or(false) {
                bail!("slot {{{name}}}: value {v:?} fails its contract {re}");
            }
            if !plain_token(v) {
                bail!("slot {{{name}}}: value {v:?} is not a plain token");
            }
            values.insert(name.clone(), v.clone());
        }
        let bound: Vec<String> = self
            .argv
            .iter()
            .map(|a| SLOT.replace_all(a, |c: &regex::Captures| values[&c[1]].clone()).into_owned())
            .collect();
        // A brace left over is a malformed placeholder, not a literal.
        if bound.iter().any(|a| a.contains('{') || a.contains('}')) {
            bail!("argv contains a malformed slot placeholder");
        }
        Ok(bound)
    }

    /// The route exec digest: the pack's, or SHA-256 of this request's canonical, value-free form
    /// (argv template, contracts, output spec, scripts and effect). Bound values and labels are
    /// not part of it, so an approval covers every value the contracts accept.
    pub fn exec_digest(&self) -> String {
        if let Some(d) = &self.route_exec_digest {
            return match &self.project_python {
                None => d.clone(),
                Some(p) => crate::sha256_hex(
                    crate::canonical_json(&serde_json::json!({ "pack": d, "project_python": p })).as_bytes(),
                ),
            };
        }
        let mut v = serde_json::json!({
            "argv": self.argv,
            "contracts": self.contracts,
            "output": self.output,
            "scripts": self.scripts,
            "effect": self.effect,
        });
        // Only when present, so requests without an action keep their digests.
        if let Some(a) = &self.action {
            v["action"] = serde_json::json!(a);
        }
        if let Some(p) = &self.project_python {
            v["project_python"] = serde_json::json!(p);
        }
        crate::sha256_hex(crate::canonical_json(&v).as_bytes())
    }
}

fn adage_core_invisible(c: char) -> bool {
    crate::scan::invisible(c)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn req(argv: &[&str]) -> ExecRequest {
        ExecRequest {
            repo_root: PathBuf::from("/nonexistent"),
            project_python: None,
            argv: argv.iter().map(|s| s.to_string()).collect(),
            inputs: BTreeMap::new(),
            contracts: BTreeMap::new(),
            effect: Effect::Observe,
            level: Level::E1,
            output: OutputSpec::default(),
            scripts: vec![],
            profile: Profile::for_level(Level::E1),
            labels: Labels::default(),
            origin: Origin::Local,
            route_exec_digest: None,
            action: None,
        }
    }

    #[test]
    fn binds_only_values_that_pass_their_contracts() {
        let mut r = req(&["./scripts/wi-lookup", "{item}", "--json"]);
        assert!(r.bind().is_err(), "unbound");
        r.inputs.insert("item".into(), "W-1234".into());
        assert!(r.bind().is_err(), "no contract");
        r.contracts.insert("item".into(), r"^.*$".into());
        assert!(r.bind().is_err(), "lax contract");
        r.contracts.insert("item".into(), r"^W-\d{4,9}$".into());
        assert_eq!(r.bind().unwrap(), vec!["./scripts/wi-lookup", "W-1234", "--json"]);
        r.inputs.insert("item".into(), "W-12".into());
        assert!(r.bind().is_err(), "fails contract");
        r.inputs.insert("item".into(), "W-1234".into());
        r.inputs.insert("other".into(), "x".into());
        assert!(r.bind().is_err(), "stray input");
    }

    #[test]
    fn digest_ignores_values_and_labels_but_not_the_template() {
        let mut a = req(&["cat", "{f}"]);
        a.contracts.insert("f".into(), r"^[a-z]{1,8}\.json$".into());
        let mut b = a.clone();
        b.inputs.insert("f".into(), "x.json".into());
        b.labels.route = "other".into();
        assert_eq!(a.exec_digest(), b.exec_digest());
        b.argv[0] = "head".into();
        assert_ne!(a.exec_digest(), b.exec_digest());
    }

    #[test]
    fn output_specs_are_checked() {
        let parse = |s: &str| serde_json::from_str::<BTreeMap<String, FieldSpec>>(s).unwrap();
        let ok = OutputSpec {
            mode: OutputMode::Typed,
            fields: parse(
                r#"{"files":{"type":"repo-path-list","max":5},"state":{"type":"enum","values":["Active","Closed"]}}"#,
            ),
        };
        assert!(ok.validate().is_ok());
        let lax = OutputSpec { mode: OutputMode::Typed, fields: parse(r#"{"x":{"type":"pattern","regex":".*"}}"#) };
        assert!(lax.validate().is_err());
        let bad_enum =
            OutputSpec { mode: OutputMode::Typed, fields: parse(r#"{"x":{"type":"enum","values":["a b"]}}"#) };
        assert!(bad_enum.validate().is_err());
        assert!(serde_json::from_str::<BTreeMap<String, FieldSpec>>(r#"{"x":{"type":"int","evil":1}}"#).is_err());
    }
}
