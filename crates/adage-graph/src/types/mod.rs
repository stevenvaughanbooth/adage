//! Grammar induction for learned value types (Track G §4.2–4.3): run skeletons, the lattice,
//! pointwise anti-unification with star folding and unions, the MDL choice, and the mandatory
//! safety constraints.
//!
//! [`induce`] turns a slot's positive samples (flow-bound values) and negatives into a
//! [`ValueType`], or says why it cannot.

pub mod anti;
pub mod lattice;
pub mod safety;
pub mod skeleton;

use adage_core::vtype::{to_regex, Run, Runs};
use lattice::Mode;
pub use safety::Reject;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

/// A learned value type, as tracks will store it (samples never; only their hash).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ValueType {
    /// Stable: a hash of the sink identity and the skeleton without lengths, so a widening
    /// length range keeps the id.
    pub id: String,
    pub runs: Runs,
    /// The anchored regex; agrees with `adage_core::vtype::matches` on every string.
    pub regex: String,
    /// SHA-256 of the sorted distinct samples, one per line.
    pub samples_hash: String,
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The skeleton without lengths: what the id hashes.
pub fn shape_key(runs: &Runs) -> String {
    let run = |r: &Run| match r {
        Run::Lit { text } => format!("lit:{text}"),
        Run::Set { texts } => format!("set:{}", texts.join(",")),
        Run::Class { class, .. } => format!("class:{}", class.name()),
        Run::Star { sep, class, .. } => format!("star:{sep}{}", class.name()),
    };
    runs.branches.iter().map(|b| b.iter().map(run).collect::<Vec<_>>().join(" ")).collect::<Vec<_>>().join(" | ")
}

pub fn type_id(sink: &str, runs: &Runs) -> String {
    let mut h = Sha256::new();
    h.update(b"adage-vtype/1\n");
    h.update(sink.as_bytes());
    h.update(b"\n");
    h.update(shape_key(runs).as_bytes());
    format!("vt-{}", &hex(&h.finalize())[..16])
}

pub fn samples_hash(samples: &[String]) -> String {
    let mut h = Sha256::new();
    for s in samples {
        h.update(s.as_bytes());
        h.update(b"\n");
    }
    hex(&h.finalize())
}

/// A type with its description length.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Induced {
    pub vtype: ValueType,
    pub bits: f64,
    /// True when the MDL choice accepted a negative and the least general type was used.
    pub least_general: bool,
}

/// Induce a type for the sink named `sink` from its positives (any order, repeats allowed) and
/// negatives. Positives that no runtime could bind (outside `plain_token`, a leading `-`, over
/// 128 bytes) are not samples: they become negatives. A pure function of the *sets* of values.
pub fn induce<'a>(
    sink: &str,
    positives: impl IntoIterator<Item = &'a String>,
    negatives: impl IntoIterator<Item = &'a String>,
) -> Result<Induced, Reject> {
    let mut neg: BTreeSet<String> = negatives.into_iter().cloned().collect();
    let mut pos: BTreeSet<String> = BTreeSet::new();
    for v in positives {
        if safety::plain_value(v) {
            pos.insert(v.clone());
        } else {
            neg.insert(v.clone());
        }
    }
    for p in &pos {
        neg.remove(p);
    }
    if pos.len() < 2 {
        return Err(Reject::TooFew);
    }
    let samples: Vec<String> = pos.into_iter().collect();
    let mut last = Reject::NoShape;
    for (mode, least) in [(Mode::Mdl, false), (Mode::LeastGeneral, true)] {
        let Some(c) = anti::generalise(&samples, mode) else { return Err(Reject::NoShape) };
        safety::check_shape(&c.runs)?;
        if let Some(n) = safety::accepted_negative(&c.runs, &neg) {
            last = Reject::AcceptsNegative(n.clone());
            continue;
        }
        let vtype = ValueType {
            id: type_id(sink, &c.runs),
            regex: to_regex(&c.runs),
            samples_hash: samples_hash(&samples),
            runs: c.runs,
        };
        return Ok(Induced { vtype, bits: c.bits, least_general: least });
    }
    Err(last)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::vtype::matches;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn learns_and_ids_are_stable_under_widening() {
        let a = induce("x #0", &s(&["BIL-1041", "BIL-1046", "BIL-2042"]), &s(&["Please"])).unwrap();
        assert_eq!(a.vtype.regex, r"^(?:BIL\-[0-9]{4})$");
        let b = induce("x #0", &s(&["BIL-104", "BIL-1046", "BIL-2042"]), &s(&[])).unwrap();
        assert_eq!(a.vtype.id, b.vtype.id);
        assert_ne!(a.vtype.samples_hash, b.vtype.samples_hash);
        assert_ne!(a.vtype.id, induce("y #0", &s(&["BIL-104", "BIL-1046"]), &s(&[])).unwrap().vtype.id);
    }

    #[test]
    fn negatives_force_the_least_general_type_or_rejection() {
        let r = induce("x #0", &s(&["BIL-1041", "BIL-1046", "BIL-2042"]), &s(&["BIL-9999"])).unwrap();
        assert!(r.least_general);
        assert!(matches(&r.vtype.runs, "BIL-1046") && !matches(&r.vtype.runs, "BIL-9999"));
        // Five distinct numbers: no literal set; the negative cannot be rejected.
        let e = induce("x #0", &s(&["BIL-1041", "BIL-1046", "BIL-2042", "BIL-3000", "BIL-4000"]), &s(&["BIL-9999"]));
        assert_eq!(e, Err(Reject::AcceptsNegative("BIL-9999".into())));
    }

    #[test]
    fn word_like_types_are_rejected() {
        assert!(matches!(induce("x #0", &s(&["login", "cache", "audit"]), &s(&[])), Err(Reject::WordLike(_))));
        assert!(matches!(induce("x #0", &s(&["ABC", "XYZ", "QRS", "DEF", "KLM"]), &s(&[])), Err(Reject::WordLike(_))));
        // A bare number is fine.
        assert!(induce("x #0", &s(&["482", "517", "903"]), &s(&[])).is_ok());
    }
}
