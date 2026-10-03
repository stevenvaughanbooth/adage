//! `adage doctor`'s hook latency check (plan §2.5 item 4; zero-config P1): how long the hooks take
//! on this machine, from what they recorded. Every spool record carries `duration_us`, the hook's
//! own wall time (`adage_hook`); the check takes the nearest-rank p95 of the tool batches that
//! had nothing to say (a `batch` record whose decision would have delivered nothing), the
//! commonest event and the one whose cost is pure overhead.
//!
//! `adage watch` and `adage ui` re-run every check every few seconds, so the measurement is kept
//! for [`CACHE_MS`] in `$ADAGE_HOME/cache/hook-latency.json` (only once spools exist: a fresh
//! install's doctor creates nothing).

use adage_store::Paths;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Above this p95 the check warns.
pub const WARN_US: u64 = 10_000;
/// Above this p95 it fails.
pub const FAIL_US: u64 = 25_000;
/// Fewer qualifying records than this measure nothing: the check passes and says so.
pub const MIN_HOOKS: usize = 20;
/// A measurement this recent is reused.
pub const CACHE_MS: u64 = 5_000;
/// The newest spools read, and the most records taken from them.
const MAX_SPOOLS: usize = 200;
const MAX_SAMPLES: usize = 2_000;

/// What the spools say.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Measured {
    /// When it was measured (ms since the epoch).
    pub at: u64,
    /// Qualifying records.
    pub hooks: usize,
    /// Their nearest-rank 95th percentile, in microseconds (`None` with no records).
    pub p95_us: Option<u64>,
}

/// The nearest-rank percentile `p` (0 < p ≤ 1) of `v`: the smallest value at least `p` of the
/// values are at or below.
pub fn nearest_rank(v: &mut [u64], p: f64) -> Option<u64> {
    if v.is_empty() {
        return None;
    }
    v.sort_unstable();
    let rank = ((p * v.len() as f64).ceil() as usize).clamp(1, v.len());
    Some(v[rank - 1])
}

/// The hook time of a record that counts: a tool batch whose decision delivered (or would have
/// delivered) nothing, with its `duration_us`. Records from before the field, and every other
/// kind, do not count.
pub fn sample(record: &Value) -> Option<u64> {
    if adage_runtime::rebuild::kind_of(record) != "batch" {
        return None;
    }
    let quiet = record.pointer("/decision/would_have").and_then(Value::as_array).is_none_or(|w| w.is_empty());
    quiet.then(|| record.get("duration_us").and_then(Value::as_u64)).flatten()
}

/// Measure from the newest spools.
pub fn measure(paths: &Paths, now: u64) -> Measured {
    let mut spools: Vec<(std::time::SystemTime, std::path::PathBuf)> = std::fs::read_dir(paths.sessions())
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "spool"))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    spools.sort_by_key(|s| std::cmp::Reverse(s.0));
    let mut samples: Vec<u64> = Vec::new();
    for (_, path) in spools.into_iter().take(MAX_SPOOLS) {
        let records: Vec<Value> = adage_store::spool::read_all(&path).unwrap_or_default();
        // The newest records of each spool first.
        samples.extend(records.iter().rev().filter_map(sample));
        if samples.len() >= MAX_SAMPLES {
            samples.truncate(MAX_SAMPLES);
            break;
        }
    }
    let hooks = samples.len();
    Measured { at: now, hooks, p95_us: nearest_rank(&mut samples, 0.95) }
}

fn cache_file(paths: &Paths) -> std::path::PathBuf {
    paths.cache().join("hook-latency.json")
}

/// The measurement, from the cache when it is recent, else measured now and cached (only where
/// spools exist).
pub fn measured(paths: &Paths, now: u64) -> Measured {
    let file = cache_file(paths);
    let cached = std::fs::read(&file).ok().and_then(|b| serde_json::from_slice::<Measured>(&b).ok());
    if let Some(m) = cached.filter(|m| m.at <= now && now - m.at < CACHE_MS) {
        return m;
    }
    let m = measure(paths, now);
    if paths.sessions().is_dir() && std::fs::create_dir_all(paths.cache()).is_ok() {
        if let Ok(bytes) = serde_json::to_vec(&m) {
            // A cache that cannot be written only means measuring again next time.
            let _ = std::fs::write(&file, bytes);
        }
    }
    m
}

/// The check's verdict for a measurement.
pub fn check(m: &Measured) -> super::Check {
    const NAME: &str = "hook latency";
    let ms = |us: u64| format!("{:.1} ms", us as f64 / 1000.0);
    let p95 = match m.p95_us {
        Some(p) if m.hooks >= MIN_HOOKS => p,
        _ => return super::pass(NAME, "too few hooks to measure"),
    };
    let detail = format!("p95 {} over the last {} tool batches with nothing to say", ms(p95), m.hooks);
    let fix = "hooks wait on the disk that holds ADAGE_HOME: keep it on a local disk, and if hooks stay slow, report it with the output of `adage doctor --json`";
    if p95 > FAIL_US {
        super::fail(NAME, format!("{detail} (above {})", ms(FAIL_US)), fix)
    } else if p95 > WARN_US {
        super::warn(NAME, format!("{detail} (above {})", ms(WARN_US)), fix)
    } else {
        super::pass(NAME, detail)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::doctor::Status;
    use serde_json::json;

    fn batch(us: u64, quiet: bool) -> Value {
        let would = if quiet { json!([]) } else { json!([{ "route": "r" }]) };
        json!({ "ts": 1, "event": "PostToolBatch", "kind": "batch", "decision": { "would_have": would }, "duration_us": us })
    }

    /// T-A2-02: the nearest-rank p95 over 500 quiet batches matches the hand-computed value;
    /// batches with directives, other kinds and records from before the field are left out; under
    /// 20 records the check passes and says it cannot measure.
    #[test]
    fn the_p95_is_nearest_rank_over_quiet_batches_only() {
        // 1..=500 µs in a scrambled order: rank ceil(0.95 × 500) = 475, so 475 µs.
        let mut v: Vec<u64> = (1..=500u64).map(|i| (i * 7919) % 500 + 1).collect();
        assert_eq!(nearest_rank(&mut v, 0.95), Some(475));
        assert_eq!(nearest_rank(&mut [9], 0.95), Some(9));
        assert_eq!(nearest_rank(&mut [], 0.95), None);
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let spool = paths.spool("s1");
        for i in 1..=500u64 {
            adage_store::spool::append(&spool, &batch(i * 100, true)).unwrap();
        }
        // Excluded: a batch that directed something, a prompt, a batch from before the field.
        adage_store::spool::append(&spool, &batch(9_000_000, false)).unwrap();
        adage_store::spool::append(&spool, &json!({ "kind": "prompt", "duration_us": 9_000_000 })).unwrap();
        adage_store::spool::append(&spool, &json!({ "kind": "batch", "decision": { "would_have": [] } })).unwrap();
        let m = measure(&paths, 1);
        assert_eq!((m.hooks, m.p95_us), (500, Some(47_500)));
        let c = check(&m);
        assert_eq!(c.status, Status::Fail, "{c:?}");
        assert!(c.detail.starts_with("p95 47.5 ms over the last 500 tool batches"), "{}", c.detail);
        let few = Measured { at: 1, hooks: 19, p95_us: Some(1_000_000) };
        assert_eq!((check(&few).status, check(&few).detail.as_str()), (Status::Pass, "too few hooks to measure"));
        let none = Measured { at: 1, hooks: 0, p95_us: None };
        assert_eq!(check(&none).status, Status::Pass);
    }

    #[test]
    fn thresholds_warn_above_ten_and_fail_above_twenty_five_milliseconds() {
        let at = |p: u64| check(&Measured { at: 1, hooks: 20, p95_us: Some(p) }).status;
        assert_eq!(at(10_000), Status::Pass);
        assert_eq!(at(12_000), Status::Warn);
        assert_eq!(at(25_000), Status::Warn);
        assert_eq!(at(30_000), Status::Fail);
    }

    /// The measurement is kept for a few seconds; without spools nothing is written.
    #[test]
    fn a_recent_measurement_is_reused_and_a_fresh_install_gets_no_cache() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path().join("home"));
        assert_eq!(measured(&paths, 1_000).hooks, 0);
        assert!(!paths.root.exists(), "doctor creates nothing on a fresh install");
        adage_store::spool::append(&paths.spool("s"), &batch(100, true)).unwrap();
        assert_eq!(measured(&paths, 10_000).hooks, 1);
        adage_store::spool::append(&paths.spool("s"), &batch(100, true)).unwrap();
        assert_eq!(measured(&paths, 10_000 + CACHE_MS - 1).hooks, 1, "cached");
        assert_eq!(measured(&paths, 10_000 + CACHE_MS).hooks, 2, "measured again");
    }
}
