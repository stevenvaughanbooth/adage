//! The learned follow judge (evals/lab/research/uptake-delivery): weighted containment of the
//! directed command's tokens in what the agent ran, with each token weighted by how many of the
//! repository's sessions use it. No command, flag or tool name is written by hand.
//!
//! - **Tokens**: the first line of a command (a heredoc body is data), `/` read as a separator,
//!   split on anything but word characters, dots and dashes: `source .venv/bin/activate && python
//!   -m pytest tests/x.py` is `source .venv bin activate python -m pytest tests x.py`.
//! - **Weight** of a token: `ln(1 + the number of distinct sessions whose commands use it)`, at
//!   least `ln 2`. Tokens that recur across sessions are how the repository does things (`.venv`,
//!   `bin`, `python`, `-m`, `pytest`); a session's own paths and test ids weigh the least.
//! - **Score** `s(d, a)` = the weight of `d`'s tokens found in `a` over the weight of all of them;
//!   followed at [`THRESHOLD`] or above (fitted leave-one-repository-out at 0.85–0.86 on the
//!   RepoBench traces, where it agreed with the reference on 0.997 of units, against 0.42 for the
//!   literal judge).
//!
//! So `source .venv/bin/activate && python -m pytest tests/x.py` follows `.venv/bin/python -m
//! pytest` (every directed token is there), and `python -m pytest tests/x.py` does not (`.venv` and
//! `bin` are missing, and they weigh as much as the runner). It complements the structural and
//! action judges ([`crate::pattern::follows`]) where a repository's weights are known.

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// A directed command is followed when its score reaches this.
pub const THRESHOLD: f64 = 0.85;

/// The tokens of a command's first line (see the module documentation).
pub fn tokens(command: &str) -> BTreeSet<String> {
    let first = command.split('\n').next().unwrap_or("");
    first
        .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-'))
        .filter(|t| !t.is_empty() && *t != "." && *t != "..")
        .map(str::to_string)
        .collect()
}

/// Programs that set up the commands after them rather than run anything of their own.
const SETUP: &[&str] = &["cd", "pushd", "source", ".", "export", "set", "conda", "workon", "nvm"];

/// A line's simple commands: split on `&&`, `||`, `;`, `|` and `&` outside quotes.
fn segments(line: &str) -> Vec<&str> {
    let b = line.as_bytes();
    let mut out = Vec::new();
    let (mut start, mut quote) = (0, None::<u8>);
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == b'\'' || c == b'"' => quote = Some(c),
            None if c == b'\\' => i += 1,
            None if matches!(c, b';' | b'|' | b'&') => {
                out.push(line[start..i].trim());
                while i + 1 < b.len() && matches!(b[i + 1], b'|' | b'&') {
                    i += 1;
                }
                start = i + 1;
            }
            None => {}
        }
        i += 1;
    }
    out.push(line[start.min(line.len())..].trim());
    out.retain(|s| !s.is_empty());
    out
}

/// The program a simple command runs (its base name), after any environment assignments.
fn program(seg: &str) -> Option<&str> {
    let word = seg.split_whitespace().find(|w| {
        !w.split_once('=')
            .is_some_and(|(n, _)| !n.is_empty() && n.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
    })?;
    word.rsplit('/').next()
}

/// Per repository: in how many distinct sessions each token appears (tokens seen in fewer than two
/// are left out: they weigh the floor anyway).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Weights {
    pub sessions: u32,
    pub tokens: BTreeMap<String, u32>,
}

impl Weights {
    /// Fit from `(session, command)` pairs.
    pub fn fit<'a>(pairs: impl IntoIterator<Item = (&'a str, &'a str)>) -> Weights {
        let mut by: BTreeMap<String, BTreeSet<&'a str>> = BTreeMap::new();
        let mut sessions: BTreeSet<&str> = BTreeSet::new();
        for (s, c) in pairs {
            sessions.insert(s);
            for t in tokens(c) {
                by.entry(t).or_default().insert(s);
            }
        }
        Weights {
            sessions: sessions.len() as u32,
            // Weights are stored beside the tracks: never a secret a command carried (review B3).
            tokens: by
                .into_iter()
                .map(|(t, s)| (t, s.len() as u32))
                .filter(|(t, n)| *n >= 2 && !crate::secrets::has_secret(t))
                .collect(),
        }
    }

    pub fn weight(&self, t: &str) -> f64 {
        let n = self.tokens.get(t).copied().unwrap_or(0);
        (f64::from(n)).ln_1p().max(std::f64::consts::LN_2)
    }

    /// `s(directed, observed)` in [0, 1]: the best score of a simple command of `observed` that
    /// runs the directed program (review S10), read with the setup before it (`cd`, `source
    /// …/activate`, `export`). `ls .venv/bin && python -m pytest` does not follow `.venv/bin/python
    /// -m pytest`: the `ls` is not the run, and the run lacks `.venv/bin`.
    pub fn score(&self, directed: &str, observed: &str) -> f64 {
        let d = tokens(directed);
        if d.is_empty() {
            return 0.0;
        }
        let first = |c: &'_ str| c.split('\n').next().unwrap_or("").to_string();
        let dfirst = first(directed);
        let want = segments(&dfirst).into_iter().rev().find_map(|s| program(s).filter(|p| !SETUP.contains(p)));
        let total: f64 = d.iter().map(|t| self.weight(t)).sum();
        let mut setup = String::new();
        let mut best: f64 = 0.0;
        for seg in segments(&first(observed)) {
            let Some(p) = program(seg) else { continue };
            if SETUP.contains(&p) {
                setup.push(' ');
                setup.push_str(seg);
                continue;
            }
            if want.is_some_and(|w| w != p) {
                continue;
            }
            let o = tokens(&format!("{setup} {seg}"));
            let hit: f64 = d.iter().filter(|t| o.contains(*t)).map(|t| self.weight(t)).sum();
            best = best.max(hit / total);
        }
        best
    }

    /// Whether `observed` follows `directed` ([`THRESHOLD`]). Never with weights fitted on fewer
    /// than two sessions: then every token weighs the same.
    pub fn follows(&self, directed: &str, observed: &str) -> bool {
        self.sessions >= 2 && self.score(directed, observed) >= THRESHOLD
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Weights as a repository's sessions give them: the environment's tokens in most sessions,
    /// each session's own test paths in one.
    fn click() -> Weights {
        let mut pairs: Vec<(String, String)> = Vec::new();
        for i in 0..12 {
            let s = format!("s{i}");
            pairs.push((s.clone(), format!(".venv/bin/python -m pytest tests/test_{i}.py -xvs")));
            pairs.push((s.clone(), "source .venv/bin/activate && python -m pytest tests -q".into()));
            pairs.push((s.clone(), format!("grep -n option src/click/core{i}.py")));
            pairs.push((s, "python -m pytest tests".into()));
        }
        let v: Vec<(&str, &str)> = pairs.iter().map(|(a, b)| (a.as_str(), b.as_str())).collect();
        Weights::fit(v)
    }

    #[test]
    fn follows_by_the_repository_s_own_tokens() {
        let w = click();
        let d = ".venv/bin/python -m pytest";
        for o in [
            ".venv/bin/python -m pytest tests/test_formatting.py::test_x -xvs 2>&1 | head -50",
            "source .venv/bin/activate && python -m pytest tests/test_options.py -q",
            "cd /w/click && .venv/bin/python -m pytest",
        ] {
            assert!(w.follows(d, o), "{o}: {}", w.score(d, o));
        }
        for o in
            ["python -m pytest tests/test_formatting.py -xvs", "python3 -m pytest", "pip install pytest", "ls .venv"]
        {
            assert!(!w.follows(d, o), "{o}: {}", w.score(d, o));
        }
        // A heredoc body is data.
        assert!(!w.follows(d, "cat <<EOF\n.venv/bin/python -m pytest\nEOF"));
        // Review S10: every directed token is on the line, but the run is the system Python's.
        for o in [
            "ls .venv/bin && python -m pytest tests",
            "ls .venv/bin; python -m pytest tests",
            "echo .venv/bin/python -m pytest",
            "cat .venv/bin/activate | grep python && pytest",
        ] {
            assert!(!w.follows(d, o), "{o}: {}", w.score(d, o));
        }
        assert!(w.follows(d, "cd /w/click && source .venv/bin/activate && ls && python -m pytest tests -q"));
        // No weights: no judgement.
        assert!(!Weights::default().follows(d, d));
    }

    #[test]
    fn weights_keep_only_shared_tokens() {
        let w = click();
        assert_eq!(w.sessions, 12);
        assert_eq!(w.tokens.get("venv"), None);
        assert_eq!(w.tokens.get(".venv"), Some(&12));
        assert_eq!(w.tokens.get("test_3.py"), None, "one session's own argument");
        assert!(w.weight(".venv") > w.weight("test_3.py"));
    }
}
