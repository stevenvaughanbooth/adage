//! Learning as a person sees it (docs/design/everyday-ux.md, rules 1 and 2): whether a learner is
//! running for a repository and for how long, how long a run usually takes, whether it is stuck,
//! and whether a session waits to be learned from. The status card and `adage doctor` read it here,
//! so they give one answer.

use crate::autolearn::{read_json, Queue, Request, RunState};
use adage_store::Paths;
use serde_json::Value;

/// A run is stuck past the larger of this and [`STUCK_TIMES`] its usual duration.
pub const STUCK_FLOOR_MS: u64 = 60_000;
/// How many times its usual duration a run may take before it counts as stuck.
pub const STUCK_TIMES: u64 = 20;
/// With no usual duration yet (no automatic run has finished), a run is stuck after this long.
pub const STUCK_FIRST_MS: u64 = 15 * 60_000;
/// How many of the latest automatic runs the usual duration is the median of.
const USUAL_OVER: usize = 10;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Learning {
    /// When the running learner started, if one holds the repository's lock now (a learner that
    /// died released it, whatever its state file says).
    pub running_since: Option<u64>,
    /// The learner's process, from its state file.
    pub pid: Option<u32>,
    /// The median duration of the latest automatic runs (`learn.log`; manual runs are not
    /// the learner's).
    pub usual_ms: Option<u64>,
    /// A session ended after the last request the learner handled: it is learned from next.
    pub waiting: bool,
}

impl Learning {
    /// How long a run may take before it counts as stuck.
    pub fn stuck_after(&self) -> u64 {
        self.usual_ms.map_or(STUCK_FIRST_MS, |u| u.saturating_mul(STUCK_TIMES).max(STUCK_FLOOR_MS))
    }

    /// A learner is running, and has run longer than [`Learning::stuck_after`].
    pub fn stuck(&self, now: u64) -> bool {
        self.running_since.is_some_and(|since| now.saturating_sub(since) > self.stuck_after())
    }
}

/// The learning state of `repo` (its key), with `log` the lines of `learn.log`.
pub fn of(paths: &Paths, repo: &str, log: &[Value]) -> Learning {
    let q = Queue::new(paths, repo);
    let state: Option<RunState> = read_json(&q.state());
    let held = adage_store::lock::is_held(&q.lock()).unwrap_or(false);
    let running = held && state.as_ref().is_some_and(|s| s.outcome == "running");
    let processed = state.as_ref().map_or(0, |s| s.processed);
    let request: Option<Request> = read_json(&q.request());
    let mut durations: Vec<u64> = log
        .iter()
        .rev()
        .filter(|l| l["repo"].as_str() == Some(repo) && l["trigger"] == "auto")
        // `learn`: lines written before the lexicon's rename.
        .filter(|l| matches!(l["kind"].as_str(), Some("condition" | "learn")))
        .filter_map(|l| l["duration_ms"].as_u64())
        .take(USUAL_OVER)
        .collect();
    durations.sort_unstable();
    Learning {
        running_since: if running { state.as_ref().map(|s| s.started_at) } else { None },
        pid: state.as_ref().map(|s| s.pid),
        usual_ms: (!durations.is_empty()).then(|| durations[durations.len() / 2]),
        waiting: request.is_some_and(|r| r.requested_at > processed),
    }
}

/// A duration the way people say it: `0.3 s`, `8 s`, `4 min`, `2 h`.
pub fn duration(ms: u64) -> String {
    match ms {
        0..=9_949 => format!("{:.1} s", ms as f64 / 1000.0).replace(".0 s", " s"),
        9_950..=59_999 => format!("{} s", (ms + 500) / 1000),
        60_000..=5_399_999 => format!("{} min", (ms + 30_000) / 60_000),
        _ => format!("{} h", (ms + 1_800_000) / 3_600_000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn durations_read_as_people_say_them() {
        assert_eq!(duration(300), "0.3 s");
        assert_eq!(duration(8_000), "8 s");
        assert_eq!(duration(12_689), "13 s");
        assert_eq!(duration(240_000), "4 min");
        assert_eq!(duration(7_200_000), "2 h");
    }

    #[test]
    fn usual_is_the_median_of_automatic_runs_and_stuck_is_relative_to_it() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let repo = "github.com/acme/billing";
        let run = |ms: u64, trigger: &str| json!({ "kind": "condition", "repo": repo, "trigger": trigger, "duration_ms": ms });
        // Manual runs (`adage condition --instincts-only`, 0.3 s) are not the learner's.
        let log = vec![run(8_000, "auto"), run(300, "manual"), run(12_000, "auto"), run(9_000, "auto")];
        let l = of(&paths, repo, &log);
        assert_eq!(l.usual_ms, Some(9_000));
        assert_eq!(l.stuck_after(), 180_000, "20 times 9 s");
        assert_eq!((l.running_since, l.waiting), (None, false), "no lock held, no request");
        assert!(!l.stuck(u64::MAX), "nothing running is never stuck");

        let running = Learning { running_since: Some(0), ..l.clone() };
        assert!(!running.stuck(170_000) && running.stuck(181_000));
        // Fast runs: never stuck under a minute.
        let quick = Learning { running_since: Some(0), usual_ms: Some(200), ..Default::default() };
        assert_eq!(quick.stuck_after(), STUCK_FLOOR_MS);
        // No run has finished yet: the first runs get the old fifteen minutes.
        assert_eq!(Learning::default().stuck_after(), STUCK_FIRST_MS);
    }
}
