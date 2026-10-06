//! Reuse only pure mining. Compilation, outcome qualification, checkout facts, review decisions,
//! effect ceilings and index publication still run against current inputs on every learn.

use crate::{corpus::digest, store::Store};
use adage_graph::{
    mine::Pattern,
    select::{Counts, Learned, Params, Reflex},
};
use adage_trace::Session;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

/// 2: the candidates (`Learned::candidates`) and each reflex's supporting sessions and lag.
const SCHEMA: u32 = 2;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// `Learned` intentionally omits compiler-only fields in public reports. Store them explicitly:
/// losing unselected reflexes, candidates, patterns, supporting sessions or dormant-route counts
/// would change a warm run.
#[derive(Serialize, Deserialize)]
struct Payload {
    learned: Learned,
    unselected: Vec<Reflex>,
    candidates: Vec<Reflex>,
    counts: BTreeMap<String, Counts>,
    selected_patterns: Vec<Skipped>,
    unselected_patterns: Vec<Skipped>,
    candidate_patterns: Vec<Skipped>,
    values: adage_graph::Learned,
}

/// What a `Reflex` leaves out of reports and the compiler still reads: its raw pattern, every
/// supporting session (which traces a candidate was seen in, and which tipped it into a reflex,
/// the candidates' one-per-habit key) and a candidate's lag (which context is nearest its step).
#[derive(Serialize, Deserialize)]
struct Skipped {
    raw: Option<Pattern>,
    sessions: Vec<String>,
    lag: u32,
}

impl Skipped {
    fn of(reflexes: &[Reflex]) -> Vec<Skipped> {
        reflexes.iter().map(|r| Skipped { raw: r.raw.clone(), sessions: r.sessions.clone(), lag: r.lag }).collect()
    }

    /// Puts each back on its reflex; `false` when the lists do not pair up.
    fn restore(reflexes: &mut [Reflex], skipped: Vec<Skipped>) -> bool {
        if reflexes.len() != skipped.len() {
            return false;
        }
        for (r, s) in reflexes.iter_mut().zip(skipped) {
            r.raw = s.raw;
            r.sessions = s.sessions;
            r.lag = s.lag;
        }
        true
    }
}

#[derive(Serialize, Deserialize)]
struct Checkpoint {
    schema: u32,
    key: String,
    digest: String,
    payload: Payload,
}

struct HashWriter(Sha256);
impl std::io::Write for HashWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.update(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn key(sessions: &[&Session], params: &Params, stamp: &str) -> Option<String> {
    let mut sessions = sessions.to_vec();
    sessions.sort_by(|a, b| (&a.repo, &a.id).cmp(&(&b.repo, &b.id)));
    let mut hash = HashWriter(Sha256::new());
    serde_json::to_writer(
        &mut hash,
        &(SCHEMA, stamp, params.clone().with_threads(1), adage_graph::Params::default(), sessions),
    )
    .ok()?;
    Some(format!("{:x}", hash.0.finalize()))
}

fn encode(key: &str, learned: &Learned, values: &adage_graph::Learned) -> Option<Vec<u8>> {
    let payload = Payload {
        learned: learned.clone(),
        unselected: learned.unselected.clone(),
        candidates: learned.candidates.clone(),
        counts: learned.counts.clone(),
        selected_patterns: Skipped::of(&learned.reflexes),
        unselected_patterns: Skipped::of(&learned.unselected),
        candidate_patterns: Skipped::of(&learned.candidates),
        values: values.clone(),
    };
    let checksum = digest(&serde_json::to_vec(&payload).ok()?);
    let encoded =
        serde_json::to_vec(&Checkpoint { schema: SCHEMA, key: key.into(), digest: checksum, payload }).ok()?;
    (encoded.len() <= MAX_BYTES).then_some(encoded)
}

fn decode(bytes: &[u8], key: &str) -> Option<(Learned, adage_graph::Learned)> {
    if bytes.len() > MAX_BYTES {
        return None;
    }
    let checkpoint: Checkpoint = serde_json::from_slice(bytes).ok()?;
    if checkpoint.schema != SCHEMA
        || checkpoint.key != key
        || checkpoint.digest != digest(&serde_json::to_vec(&checkpoint.payload).ok()?)
    {
        return None;
    }
    let Payload {
        mut learned,
        unselected,
        candidates,
        counts,
        selected_patterns,
        unselected_patterns,
        candidate_patterns,
        values,
    } = checkpoint.payload;
    learned.unselected = unselected;
    learned.candidates = candidates;
    learned.counts = counts;
    let paired = Skipped::restore(&mut learned.reflexes, selected_patterns)
        && Skipped::restore(&mut learned.unselected, unselected_patterns)
        && Skipped::restore(&mut learned.candidates, candidate_patterns);
    paired.then_some((learned, values))
}

pub(crate) fn mining(
    sessions: &[&Session],
    params: &Params,
    store: Option<&dyn Store>,
    compute: impl FnOnce() -> (Learned, adage_graph::Learned),
) -> (Learned, adage_graph::Learned, bool) {
    let repo = sessions.first().map(|s| s.repo.as_str());
    let key = store.and_then(Store::computation_stamp).and_then(|stamp| key(sessions, params, &stamp));
    if let (Some(store), Some(repo), Some(key)) = (store, repo, key.as_deref()) {
        if let Some(result) = store.mining_checkpoint(repo).and_then(|bytes| decode(&bytes, key)) {
            return (result.0, result.1, true);
        }
    }
    let (learned, values) = compute();
    if let (Some(store), Some(repo), Some(key)) = (store, repo, key.as_deref()) {
        if let Some(bytes) = encode(key, &learned, &values) {
            store.write_mining_checkpoint(repo, &bytes);
        }
    }
    (learned, values, false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkpoint_restores_every_compiler_input_and_detects_corruption() {
        let learned = adage_graph::select::learn(&[], &Params::default());
        let values = adage_graph::learn(&[], &adage_graph::Params::default());
        let bytes = encode("input-key", &learned, &values).unwrap();
        let (mut restored, v) = decode(&bytes, "input-key").unwrap();
        restored.timings = learned.timings.clone();
        assert_eq!(restored, learned);
        assert_eq!(v, values);
        assert!(decode(&bytes, "other-input").is_none());
        let mut corrupt: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        corrupt["payload"]["learned"]["sessions"] = serde_json::json!(99);
        assert!(decode(&serde_json::to_vec(&corrupt).unwrap(), "input-key").is_none());
    }

    /// The candidates are compiled from a warm run too (`compile::compile_candidates`), one per
    /// habit by supporting sessions and lag: a checkpoint that dropped them would change the index.
    #[test]
    fn checkpoint_keeps_the_candidates_and_each_reflexs_supporting_sessions() {
        let reflex = |pattern: &str, sessions: &[&str], lag: u32| {
            let mut r: Reflex = serde_json::from_value(serde_json::json!({
                "pattern": pattern, "shape": "context", "context": [], "continuation": [], "constraints": [],
                "support": sessions.len(), "days": 1, "conf": 1.0, "conf_lb": 0.5, "lift": 1.0, "contrast": 0.0,
                "value": 0.0, "gain": 0.0, "exposed": 0, "examples": [], "why": ""
            }))
            .unwrap();
            r.sessions = sessions.iter().map(|s| s.to_string()).collect();
            r.lag = lag;
            r
        };
        let mut learned = adage_graph::select::learn(&[], &Params::default());
        learned.reflexes.push(reflex("selected", &["a", "b"], 0));
        learned.unselected.push(reflex("unselected", &["c"], 0));
        learned.candidates.push(reflex("candidate", &["a", "c"], 2));
        let values = adage_graph::learn(&[], &adage_graph::Params::default());
        let bytes = encode("input-key", &learned, &values).unwrap();
        let (mut restored, _) = decode(&bytes, "input-key").unwrap();
        restored.timings = learned.timings.clone();
        assert_eq!(restored, learned);
        assert_eq!(
            (restored.candidates[0].sessions.as_slice(), restored.candidates[0].lag),
            (&["a".to_string(), "c".to_string()][..], 2)
        );
        assert_eq!(restored.reflexes[0].sessions, ["a", "b"]);
    }
}
