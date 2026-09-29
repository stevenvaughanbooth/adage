//! Engine parameters: every tunable number the runtime and the learner use, in one place.
//!
//! [`EngineParams`] is plain data with serde defaults equal to the values Adage has always used,
//! so an empty file (`{}` or an empty TOML document) is exactly today's engine, and so is
//! [`EngineParams::default`]. [`EngineParams::demo`] is the `--demo` preset (lower evidence bars
//! for small histories); it differs only in `learn.min_support` and `learn.fire_lower_bound`.
//!
//! It lives here, in the track crate, because both the runtime and the learner depend on this
//! crate and on nothing of each other's in that direction.
//!
//! Who reads what:
//!
//! - **Production hooks** always decide with [`RuntimeParams::default`] (`adage hook` builds its
//!   runtime context from the defaults; the index does not carry runtime parameters), and learn
//!   with the `default` or `demo` preset chosen in `config.toml`. Nothing a person can edit
//!   changes these numbers today.
//! - **The offline eval and the L0 bench** (`adage eval --params <file>`, `adage eval bench`)
//!   build both from an [`EngineParams`], so a candidate setting can be measured before it ships.
//!
//! [`EngineParams::canonical_json`] (object keys sorted, no whitespace) and
//! [`EngineParams::params_hash`] (its SHA-256, hex) identify a setting in bench results.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

/// Directives per session, at most (runtime default).
pub const MAX_DIRECTIVES_PER_SESSION: usize = 8;
/// Directives per decision, at most (runtime default).
pub const MAX_DIRECTIVES_PER_DECISION: usize = 3;
/// Stop-hook requests per session, across all obligations (runtime default).
pub const MAX_STOP_REQUESTS: u32 = 2;
/// Tool batches within which a directed command counts as followed (runtime default).
pub const FOLLOW_WINDOW: u32 = 3;
/// Canonical keys of recent calls kept in the session state (runtime default).
pub const HISTORY: usize = 32;

/// All tunable parameters of the engine.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct EngineParams {
    pub runtime: RuntimeParams,
    pub learn: LearnParams,
}

/// Hook-time limits (`adage_runtime::Ctx::params`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RuntimeParams {
    /// Tool batches after a directive within which running its command counts as following it;
    /// after that the directive is judged ignored.
    pub follow_window: u32,
    pub max_directives_per_session: usize,
    pub max_directives_per_decision: usize,
    pub max_stop_requests: u32,
    /// Recent call keys kept in the session state.
    pub history: usize,
    /// The control share when neither `ADAGE_CONTROL` nor `runtime.control` sets one. Recorded
    /// for completeness: offline replay is always the control arm, so L0 does not read it.
    pub control_share: f64,
}

impl Default for RuntimeParams {
    fn default() -> Self {
        RuntimeParams {
            follow_window: FOLLOW_WINDOW,
            max_directives_per_session: MAX_DIRECTIVES_PER_SESSION,
            max_directives_per_decision: MAX_DIRECTIVES_PER_DECISION,
            max_stop_requests: MAX_STOP_REQUESTS,
            history: HISTORY,
            control_share: 0.1,
        }
    }
}

/// Learning thresholds (`adage_learn::LearnConfig::from_params`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LearnParams {
    /// Minimum distinct supporting sessions for any route.
    pub min_support: u32,
    /// Wilson lower bound required for fire.
    pub fire_lower_bound: f64,
    /// Minimum support rate for prime (a new route).
    pub prime_rate: f64,
    /// The lower rate bar that keeps an existing route (hysteresis).
    pub retain_rate: f64,
    /// An avoid names a runner only if it failed in at least `num/den` of the sessions
    /// that ran it: `[num, den]`, integers so the rule is exact.
    pub dead_end_consistency: [u32; 2],
    /// At most this many avoids (failure signatures) per working command.
    pub dead_end_signatures: usize,
    /// A step after an edit must be this much more common after the edit than without it.
    pub finish_line_contrast: f64,
    /// Prompt-intent prototypes: commands run before the first edit by more than this share of
    /// sessions are not specific to any intent.
    pub intent_ubiquitous: f64,
    /// A prompt-intent prototype's command must be this much more common inside it than outside.
    pub intent_contrast: f64,
    /// Review: absolute movement of the Wilson lower bound that counts as a material change.
    pub material_lower_bound: f64,
    /// Dormant routes: the share of occurrences that must be exposed for a route's absence to be
    /// put down to exposure alone.
    pub dormant_exposure: f64,
    /// The outcome-lift path (`crate::lift`): a step that too few traces took for the rate bar
    /// still forms a reflex, capped at prime (prime), when the traces that took it succeeded
    /// more often than those that did not, with the 95% lower bound on the difference at least
    /// this. Outcomes are the recorded ones (`--outcomes`), else transcript proxies. Omitted from
    /// the canonical JSON at its default (0.2), so the presets' hashes are unchanged.
    #[serde(skip_serializing_if = "is_default_lift")]
    pub outcome_lift_lower_bound: f64,
    /// The outcome-lift path needs at least this many traces that took the step; `None` (the
    /// default, omitted from the canonical JSON) is `min_support`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outcome_min_support: Option<u32>,
    /// Cross-repository priors of the directive-value model (`adage_learn::value::Priors`: cells
    /// of calls-to-goal statistics by situation), fitted over other repositories' traces. `None`
    /// (the default, omitted from the canonical JSON) is the learner's built-in weak prior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_priors: Option<Value>,
}

/// The default `outcome_lift_lower_bound`.
pub const OUTCOME_LIFT_LOWER_BOUND: f64 = 0.2;

fn is_default_lift(x: &f64) -> bool {
    *x == OUTCOME_LIFT_LOWER_BOUND
}

impl Default for LearnParams {
    fn default() -> Self {
        LearnParams {
            min_support: 5,
            fire_lower_bound: 0.8,
            prime_rate: 0.5,
            retain_rate: 0.3,
            dead_end_consistency: [9, 10],
            dead_end_signatures: 3,
            finish_line_contrast: 0.3,
            intent_ubiquitous: 0.8,
            intent_contrast: 0.3,
            material_lower_bound: crate::review::MATERIAL_LOWER_BOUND,
            dormant_exposure: 0.5,
            outcome_lift_lower_bound: OUTCOME_LIFT_LOWER_BOUND,
            outcome_min_support: None,
            outcome_priors: None,
        }
    }
}

impl LearnParams {
    /// The `--demo` thresholds: lower evidence bars for demonstrations and small histories.
    pub fn demo() -> Self {
        LearnParams { min_support: 3, fire_lower_bound: 0.5, ..Default::default() }
    }
}

impl EngineParams {
    /// The `--demo` preset.
    pub fn demo() -> Self {
        EngineParams { runtime: RuntimeParams::default(), learn: LearnParams::demo() }
    }

    /// A named preset: `default` or `demo`.
    pub fn preset(name: &str) -> Result<Self> {
        match name {
            "default" => Ok(EngineParams::default()),
            "demo" => Ok(EngineParams::demo()),
            other => bail!("unknown thresholds {other:?} (demo or default)"),
        }
    }

    /// Check every value is usable: limits positive, shares within 0..=1.
    pub fn validate(&self) -> Result<()> {
        let r = &self.runtime;
        if r.follow_window == 0 || r.max_directives_per_session == 0 || r.max_directives_per_decision == 0 {
            bail!("runtime: follow_window and the directive limits must be at least 1");
        }
        if r.history == 0 {
            bail!("runtime: history must be at least 1");
        }
        let l = &self.learn;
        if l.min_support == 0 {
            bail!("learn: min_support must be at least 1");
        }
        let [num, den] = l.dead_end_consistency;
        if den == 0 || num > den {
            bail!("learn: dead_end_consistency [num, den] needs 0 < den and num <= den");
        }
        if l.outcome_min_support == Some(0) {
            bail!("learn: outcome_min_support must be at least 1");
        }
        if l.dead_end_signatures == 0 {
            bail!("learn: dead_end_signatures must be at least 1");
        }
        for (name, x) in [
            ("runtime.control_share", r.control_share),
            ("learn.fire_lower_bound", l.fire_lower_bound),
            ("learn.prime_rate", l.prime_rate),
            ("learn.retain_rate", l.retain_rate),
            ("learn.finish_line_contrast", l.finish_line_contrast),
            ("learn.intent_ubiquitous", l.intent_ubiquitous),
            ("learn.intent_contrast", l.intent_contrast),
            ("learn.material_lower_bound", l.material_lower_bound),
            ("learn.dormant_exposure", l.dormant_exposure),
            ("learn.outcome_lift_lower_bound", l.outcome_lift_lower_bound),
        ] {
            if !(0.0..=1.0).contains(&x) {
                bail!("{name} must be within 0..=1 (got {x})");
            }
        }
        Ok(())
    }

    /// `self` with `overrides` (a partial parameter object) laid over it. Unknown keys and values
    /// of the wrong type are errors.
    pub fn with_overrides(&self, overrides: &Value) -> Result<Self> {
        let mut base = serde_json::to_value(self)?;
        merge(&mut base, overrides);
        let p: EngineParams = serde_json::from_value(base).context("parameters")?;
        p.validate()?;
        Ok(p)
    }

    /// Read a partial parameter file (`.toml` or `.json`) as an override object.
    pub fn read_overrides(path: &std::path::Path) -> Result<Value> {
        let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
        let v: Value = if path.extension().is_some_and(|e| e == "toml") {
            let t: toml::Value = toml::from_str(&text).with_context(|| path.display().to_string())?;
            serde_json::to_value(t)?
        } else {
            serde_json::from_str(&text).with_context(|| path.display().to_string())?
        };
        if !v.is_object() {
            bail!("{}: parameters must be a table / object", path.display());
        }
        // Check the keys against the schema now, whatever the base will be.
        EngineParams::default().with_overrides(&v).with_context(|| path.display().to_string())?;
        Ok(v)
    }

    /// Canonical JSON: object keys sorted, no whitespace, numbers as serde_json writes them.
    pub fn canonical_json(&self) -> String {
        canonical_json(&serde_json::to_value(self).unwrap_or(Value::Null))
    }

    /// SHA-256 of [`EngineParams::canonical_json`], lowercase hex.
    pub fn params_hash(&self) -> String {
        crate::io::hex(&Sha256::digest(self.canonical_json().as_bytes()))
    }
}

/// Lay `over` onto `base`: objects merge key by key, anything else replaces.
pub fn merge(base: &mut Value, over: &Value) {
    match (base, over) {
        (Value::Object(b), Value::Object(o)) => {
            for (k, v) in o {
                match b.get_mut(k) {
                    Some(slot) => merge(slot, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (b, o) => *b = o.clone(),
    }
}

/// Serialise a JSON value with object keys sorted at every level and no whitespace, whatever
/// map type serde_json was built with.
pub fn canonical_json(v: &Value) -> String {
    let mut s = String::new();
    write_canonical(v, &mut s);
    s
}

fn write_canonical(v: &Value, s: &mut String) {
    match v {
        Value::Object(m) => {
            let mut keys: Vec<&String> = m.keys().collect();
            keys.sort();
            s.push('{');
            for (i, k) in keys.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                s.push_str(&Value::String((*k).clone()).to_string());
                s.push(':');
                write_canonical(&m[*k], s);
            }
            s.push('}');
        }
        Value::Array(a) => {
            s.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    s.push(',');
                }
                write_canonical(x, s);
            }
            s.push(']');
        }
        other => s.push_str(&other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_overrides_are_the_defaults() {
        let d = EngineParams::default();
        assert_eq!(d.with_overrides(&serde_json::json!({})).unwrap(), d);
        let from_toml: EngineParams = toml::from_str("").unwrap();
        assert_eq!(from_toml, d);
        let demo = EngineParams::demo();
        assert_eq!((demo.learn.min_support, demo.learn.fire_lower_bound), (3, 0.5));
        assert_eq!((d.learn.outcome_min_support, demo.learn.outcome_min_support), (None, None));
        assert_eq!(d.learn.outcome_lift_lower_bound, 0.2);
        // At their defaults the outcome-lift settings leave the canonical JSON (and hash) as it was.
        assert!(!d.canonical_json().contains("outcome") && !demo.canonical_json().contains("outcome"));
        let o = d.with_overrides(&serde_json::json!({"learn": {"outcome_min_support": 4}})).unwrap();
        assert!(o.canonical_json().contains("\"outcome_min_support\":4"));
        assert_eq!(demo.runtime, d.runtime);
    }

    #[test]
    fn overrides_merge_and_reject_unknown_keys() {
        let p = EngineParams::demo()
            .with_overrides(&serde_json::json!({"learn": {"min_support": 4}, "runtime": {"follow_window": 5}}))
            .unwrap();
        assert_eq!((p.learn.min_support, p.learn.fire_lower_bound, p.runtime.follow_window), (4, 0.5, 5));
        assert!(EngineParams::default().with_overrides(&serde_json::json!({"learn": {"min_suport": 4}})).is_err());
        assert!(EngineParams::default().with_overrides(&serde_json::json!({"learn": {"prime_rate": 2.0}})).is_err());
    }

    #[test]
    fn canonical_json_is_sorted_and_the_hash_stable() {
        let c = EngineParams::default().canonical_json();
        assert!(c.starts_with("{\"learn\":{\"dead_end_consistency\":[9,10],"), "{c}");
        assert!(!c.contains(' '));
        // Changing any value changes the hash; the same value, the same hash.
        let a = EngineParams::default();
        let mut b = a.clone();
        assert_eq!(a.params_hash(), b.params_hash());
        b.learn.retain_rate = 0.31;
        assert_ne!(a.params_hash(), b.params_hash());
        assert_eq!(a.params_hash().len(), 64);
    }
}
