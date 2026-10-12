//! How far a learn run has got, for a person watching it (`adage condition --progress`, which the
//! app's conditioning card follows). A run passes through the [`Stage`]s in order; each is
//! reported as it starts, and reading as it advances, trace by trace. Reporting is optional and
//! never changes what is learned: without an observer ([`Progress::default`]) it does nothing.

use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// The stages of a learn run, in the order they run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    /// Reading the trace files (`done` of `total`).
    Read,
    /// Mining the traces for repeated patterns.
    Mine,
    /// Compiling the patterns into reflexes and judging their evidence.
    Compile,
    /// Settling overlaps between reflexes (interlocking, `adage_tracks::interlock`).
    Settle,
    /// Writing the compiled index the hooks read.
    Index,
}

impl Stage {
    pub const ALL: [Stage; 5] = [Stage::Read, Stage::Mine, Stage::Compile, Stage::Settle, Stage::Index];

    /// What the stage does, in plain words.
    pub fn label(self) -> &'static str {
        match self {
            Stage::Read => "Reading traces",
            Stage::Mine => "Finding patterns",
            Stage::Compile => "Compiling reflexes",
            Stage::Settle => "Settling overlaps",
            Stage::Index => "Writing the index",
        }
    }
}

/// One report: a stage, and how far into it (`done` of `total`; both 0 when it cannot say).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Event {
    pub progress: Stage,
    #[serde(default)]
    pub done: usize,
    #[serde(default)]
    pub total: usize,
}

/// Where a run reports its progress: an observer, or nowhere.
#[derive(Clone, Default)]
pub struct Progress(Option<Arc<dyn Fn(Event) + Send + Sync>>);

impl Progress {
    pub fn new(f: impl Fn(Event) + Send + Sync + 'static) -> Progress {
        Progress(Some(Arc::new(f)))
    }

    /// `stage` has started, or has got to `done` of `total`.
    pub fn report(&self, stage: Stage, done: usize, total: usize) {
        if let Some(f) = &self.0 {
            f(Event { progress: stage, done, total });
        }
    }

    /// Whether anyone is listening.
    pub fn is_on(&self) -> bool {
        self.0.is_some()
    }
}

impl std::fmt::Debug for Progress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.is_some() { "Progress(on)" } else { "Progress(off)" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn events_reach_the_observer_and_read_back_from_their_json() {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s = seen.clone();
        let p = Progress::new(move |e| s.lock().unwrap().push(e));
        p.report(Stage::Read, 3, 10);
        p.report(Stage::Mine, 0, 0);
        Progress::default().report(Stage::Index, 0, 0);
        let got = seen.lock().unwrap().clone();
        assert_eq!(got.len(), 2);
        let line = serde_json::to_string(&got[0]).unwrap();
        assert_eq!(line, r#"{"progress":"read","done":3,"total":10}"#);
        assert_eq!(serde_json::from_str::<Event>(r#"{"progress":"settle"}"#).unwrap().progress, Stage::Settle);
        assert!(Stage::ALL.windows(2).all(|w| w[0] < w[1]), "in the order they run");
    }
}
