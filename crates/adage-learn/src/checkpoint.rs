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

const SCHEMA: u32 = 1;
pub const MAX_BYTES: usize = 64 * 1024 * 1024;

/// `Learned` intentionally omits compiler-only fields in public reports. Store them explicitly:
/// losing unselected candidates, patterns or dormant-route counts would change a warm run.
#[derive(Serialize, Deserialize)]
struct Payload {
    learned: Learned,
    unselected: Vec<Reflex>,
    counts: BTreeMap<String, Counts>,
    selected_patterns: Vec<Option<Pattern>>,
    unselected_patterns: Vec<Option<Pattern>>,
    values: adage_graph::Learned,
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
        counts: learned.counts.clone(),
        selected_patterns: learned.reflexes.iter().map(|r| r.raw.clone()).collect(),
        unselected_patterns: learned.unselected.iter().map(|r| r.raw.clone()).collect(),
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
    let Payload { mut learned, unselected, counts, selected_patterns, unselected_patterns, values } =
        checkpoint.payload;
    if learned.reflexes.len() != selected_patterns.len() || unselected.len() != unselected_patterns.len() {
        return None;
    }
    learned.unselected = unselected;
    learned.counts = counts;
    for (r, raw) in learned.reflexes.iter_mut().zip(selected_patterns) {
        r.raw = raw;
    }
    for (r, raw) in learned.unselected.iter_mut().zip(unselected_patterns) {
        r.raw = raw;
    }
    Some((learned, values))
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
}
