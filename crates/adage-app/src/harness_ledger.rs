//! What a session cost, as its harness reported it (docs/claude-mod/architecture.md §3.9): the
//! lines `adage ledger note` records, and their merge into the ledger.
//!
//! The ledger ([`adage_learn::ledger`]) prices a session from its transcript. A harness that can
//! say more (the Claude Code mod, at each `turn.complete`) records one line per session in
//! `$ADAGE_HOME/ledger/.harness.jsonl` instead of touching the ledger: per model step the tokens
//! and whether Adage took the step in the model's place, the hook layer's wall times, and the
//! engine's own cost where it keeps one. The file is the harness's record and is never rewritten
//! from a transcript. Whenever the savings estimate is refreshed (by the learner after it updates
//! the ledger, by `adage savings`, by the note itself) each repository's lines are merged into its
//! ledger by session id, under the ledger's lock: the harness's figures win over the transcript's
//! (tokens, model turns, models, times, and the cost when the engine reported one), so a later
//! transcript pass never replaces them for good; what only a transcript holds (the first edit, how
//! the agent ended) is kept, and so are its tool calls unless the note counts them.
//!
//! A step Adage took in the model's place (`predicted`) made no model request: `model_turns`
//! counts only the steps the model answered, and the savings estimate reads the rest as turns not
//! spent.
//!
//! **Integrity.** The command runs inside agent sessions (it tunes nothing), so a note is accepted
//! only for a session whose spool holds a `session_start` in the repository it names, from a
//! harness version string, and replaces any earlier note for that session: an agent cannot mint
//! a session it did not have, and one session is one line.

use adage_core::pricing::{self, Tokens};
use adage_learn::ledger::{Cost, Ledger, SessionSummary};
use adage_store::{lock, Paths};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

/// The file, in `$ADAGE_HOME/ledger/`. A repository's ledger is `<slug>.jsonl` and a slug never
/// begins with a dot, so no repository's ledger can be this file.
pub const FILE: &str = ".harness.jsonl";
/// The harnesses that record notes (the only one with a mod).
pub const HARNESSES: &[&str] = &["claude-code"];
/// A note larger than this is refused.
pub const MAX_NOTE_BYTES: usize = 4 << 20;
const MAX_STEPS: usize = 20_000;
const MAX_HOOK_MS: usize = 100_000;
/// Notes for sessions that ended longer ago than spools are kept are dropped when the file is
/// next written.
const KEEP_DAYS: u64 = 180;
/// How long a writer waits for the ledger's or the file's lock before giving up (best effort).
const PATIENCE: Duration = Duration::from_secs(10);

/// One model request of the session, or a step Adage took in its place.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StepUsage {
    pub index: u32,
    /// The model that answered it, or (predicted) the one that would have.
    pub model: String,
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_write: u64,
    /// Adage took this step in the model's place: no model request was made.
    pub predicted: bool,
}

/// One session, as the harness reported it (the line in [`FILE`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Note {
    pub session_id: String,
    /// The repository key (`adage ledger note` resolves a path to it).
    pub repo: String,
    pub harness: String,
    pub harness_version: String,
    /// Milliseconds since the Unix epoch (0 if unknown).
    pub started_at: u64,
    pub ended_at: u64,
    pub steps: Vec<StepUsage>,
    /// The engine's own figure for the session, US dollars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// The hook layer's wall time per hook call, milliseconds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hook_ms: Vec<u32>,
    /// The tool calls the session's main loop made, as the harness counted them (the steps Adage
    /// took in the model's place included, as a transcript lists them). `None` when the note
    /// carries no count: the transcript's figure is kept then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<u32>,
    /// When the line was recorded.
    #[serde(default)]
    pub noted_at: u64,
}

impl Note {
    /// Steps the model answered.
    pub fn model_turns(&self) -> u32 {
        self.steps.iter().filter(|s| !s.predicted).count() as u32
    }

    /// Steps Adage took in the model's place.
    pub fn predicted_steps(&self) -> u32 {
        self.steps.iter().filter(|s| s.predicted).count() as u32
    }

    /// The four token counts, summed over every step.
    pub fn tokens(&self) -> Tokens {
        let mut t = Tokens::default();
        for s in &self.steps {
            t.add(&Tokens {
                input: s.input,
                output: s.output,
                cache_write: s.cache_write,
                cache_read: s.cache_read,
                ..Default::default()
            });
        }
        t
    }

    /// The engine's cost when it reported one (`harness`), else the steps priced from the pinned
    /// table, else none: never a partial figure.
    pub fn cost(&self) -> Cost {
        if let Some(usd) = self.cost_usd {
            return Cost { usd: Some(usd), source: "harness".into() };
        }
        let table = pricing::table();
        let mut usd = 0.0;
        let mut any = false;
        for s in self.steps.iter().filter(|s| !s.predicted) {
            let t = Tokens {
                input: s.input,
                output: s.output,
                cache_write: s.cache_write,
                cache_read: s.cache_read,
                ..Default::default()
            };
            if t.total() == 0 {
                continue;
            }
            match table.cost(&s.model, &t) {
                Some(c) => {
                    usd += c;
                    any = true;
                }
                None => return Cost { usd: None, source: "none".into() },
            }
        }
        if any {
            Cost { usd: Some(usd), source: format!("priced@{}", table.as_of) }
        } else {
            Cost { usd: None, source: "none".into() }
        }
    }

    /// This note over what the transcript gave for the same session (`base`), if anything.
    pub fn over(&self, base: Option<&SessionSummary>) -> SessionSummary {
        let mut models: Vec<String> = Vec::new();
        for s in self.steps.iter().filter(|s| !s.predicted && !s.model.is_empty()) {
            if !models.contains(&s.model) {
                models.push(s.model.clone());
            }
        }
        if models.is_empty() {
            models = base.map(|b| b.models.clone()).unwrap_or_default();
        }
        let pick = |n: u64, b: Option<u64>| if n > 0 { n } else { b.unwrap_or(0) };
        SessionSummary {
            session_id: self.session_id.clone(),
            harness: self.harness.clone(),
            harness_version: self.harness_version.clone(),
            models,
            started_at: pick(self.started_at, base.map(|b| b.started_at)),
            ended_at: self.ended_at.max(base.map_or(0, |b| b.ended_at)),
            model_turns: self.model_turns(),
            tool_calls: self.tool_calls.or(base.map(|b| b.tool_calls)).unwrap_or(0),
            first_edit_turn: base.and_then(|b| b.first_edit_turn),
            tokens: self.tokens(),
            cost: self.cost(),
            asked_for_help: base.is_some_and(|b| b.asked_for_help),
        }
    }
}

/// Why a note was refused.
#[derive(Debug, Clone, PartialEq)]
pub struct Refused(pub String);

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Refused {}

fn refuse<T>(why: impl Into<String>) -> Result<T, Refused> {
    Err(Refused(why.into()))
}

/// `2.1.288`, `2.1.288-beta.1`: digits and dots first, then an optional suffix.
pub fn is_version(v: &str) -> bool {
    let (core, suffix) = match v.find(['-', '+']) {
        Some(i) => (&v[..i], Some(&v[i + 1..])),
        None => (v, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    v.len() <= 64
        && (2..=4).contains(&parts.len())
        && parts.iter().all(|p| !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()))
        && suffix.is_none_or(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-.+".contains(&b)))
}

/// A time as milliseconds since the epoch, or an RFC 3339 UTC string.
fn time(v: &Value, field: &str) -> Result<u64, Refused> {
    match v {
        Value::Null => Ok(0),
        Value::Number(n) => n.as_u64().map_or_else(|| refuse(format!("`{field}` is not a time")), Ok),
        Value::String(s) => {
            adage_learn::ledger::parse_ts(s).map_or_else(|| refuse(format!("`{field}` is not a time")), Ok)
        }
        _ => refuse(format!("`{field}` is not a time")),
    }
}

fn count(v: &Value, field: &str) -> Result<u64, Refused> {
    match v {
        Value::Null => Ok(0),
        v => v.as_u64().map_or_else(|| refuse(format!("`{field}` is not a count")), Ok),
    }
}

fn text<'a>(v: &'a Value, field: &str, max: usize) -> Result<&'a str, Refused> {
    match v.as_str() {
        Some(s) if !s.trim().is_empty() && s.len() <= max => Ok(s),
        Some(s) if s.len() > max => refuse(format!("`{field}` is longer than {max} bytes")),
        _ => refuse(format!("`{field}` is missing or not a string")),
    }
}

/// Parse and check one note (the shape only; [`check_session`] checks it against the spool). The
/// `repo` is returned as given: the caller resolves it.
pub fn parse(input: &str) -> Result<Note, Refused> {
    if input.len() > MAX_NOTE_BYTES {
        return refuse(format!("the note is larger than {} MiB", MAX_NOTE_BYTES >> 20));
    }
    let v: Value = match serde_json::from_str(input.trim()) {
        Ok(v @ Value::Object(_)) => v,
        Ok(_) => return refuse("stdin is not one JSON object"),
        Err(e) => return refuse(format!("stdin is not one JSON object ({e})")),
    };
    let session_id = text(&v["session_id"], "session_id", 256)?.to_string();
    let repo = text(&v["repo"], "repo", 4096)?.to_string();
    let harness = text(&v["harness"], "harness", 64)?.to_string();
    if !HARNESSES.contains(&harness.as_str()) {
        return refuse(format!("`harness` is {harness:?}; notes are recorded for {}", HARNESSES.join(", ")));
    }
    let harness_version = text(&v["harness_version"], "harness_version", 64)?.to_string();
    if !is_version(&harness_version) {
        return refuse(format!("`harness_version` {harness_version:?} is not a version string"));
    }
    let Some(raw) = v["steps"].as_array() else { return refuse("`steps` is missing or not a list") };
    if raw.len() > MAX_STEPS {
        return refuse(format!("more than {MAX_STEPS} steps"));
    }
    let mut steps = Vec::with_capacity(raw.len());
    for (i, s) in raw.iter().enumerate() {
        if !s.is_object() {
            return refuse(format!("step {i} is not an object"));
        }
        let index = match s["index"].as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => n,
            None => return refuse(format!("step {i} has no `index`")),
        };
        let model = match &s["model"] {
            Value::String(m) if m.len() <= 128 => m.clone(),
            Value::Null => String::new(),
            _ => return refuse(format!("step {i}'s `model` is not a model name")),
        };
        let predicted = match &s["predicted"] {
            Value::Null => false,
            Value::Bool(b) => *b,
            _ => return refuse(format!("step {i}'s `predicted` is not true or false")),
        };
        steps.push(StepUsage {
            index,
            model,
            input: count(&s["input"], "input")?,
            output: count(&s["output"], "output")?,
            cache_read: count(&s["cache_read"], "cache_read")?,
            cache_write: count(&s["cache_write"], "cache_write")?,
            predicted,
        });
    }
    let cost_usd = match &v["cost_usd"] {
        Value::Null => None,
        c => match c.as_f64() {
            Some(x) if x.is_finite() && (0.0..1e6).contains(&x) => Some(x),
            _ => return refuse("`cost_usd` is not an amount"),
        },
    };
    let hook_ms = match &v["hook_ms"] {
        Value::Null => Vec::new(),
        Value::Array(a) if a.len() <= MAX_HOOK_MS => {
            let mut out = Vec::with_capacity(a.len());
            for x in a {
                match x.as_u64().and_then(|n| u32::try_from(n).ok()) {
                    Some(n) => out.push(n),
                    None => return refuse("`hook_ms` holds something that is not milliseconds"),
                }
            }
            out
        }
        _ => return refuse("`hook_ms` is not a list of milliseconds"),
    };
    let tool_calls = match &v["tool_calls"] {
        Value::Null => None,
        c => match c.as_u64().and_then(|n| u32::try_from(n).ok()) {
            Some(n) => Some(n),
            None => return refuse("`tool_calls` is not a count"),
        },
    };
    let started_at = time(&v["started_at"], "started_at")?;
    let ended_at = time(&v["ended_at"], "ended_at")?;
    if ended_at > 0 && started_at > ended_at {
        return refuse("`ended_at` is before `started_at`");
    }
    Ok(Note {
        session_id,
        repo,
        harness,
        harness_version,
        started_at,
        ended_at,
        steps,
        cost_usd,
        hook_ms,
        tool_calls,
        noted_at: 0,
    })
}

/// The spool must hold a `session_start` for this session in this repository.
pub fn check_session(paths: &Paths, note: &Note) -> Result<(), Refused> {
    let spool = paths.spool(&note.session_id);
    let recs: Vec<Value> = adage_store::spool::read_all(&spool).unwrap_or_default();
    let started = recs.iter().any(|r| r["kind"] == "session_start" && r["repo"].as_str() == Some(note.repo.as_str()));
    if started {
        Ok(())
    } else {
        refuse(format!(
            "no session {} started in {} (its spool holds no session_start there)",
            note.session_id, note.repo
        ))
    }
}

fn file(paths: &Paths) -> PathBuf {
    paths.ledger().join(FILE)
}

/// Every note, by session id (a later line replaces an earlier one).
pub fn read_all(paths: &Paths) -> BTreeMap<String, Note> {
    std::fs::read_to_string(file(paths))
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Note>(l).ok())
        .map(|n| (n.session_id.clone(), n))
        .collect()
}

/// The notes for one repository.
pub fn read(paths: &Paths, repo: &str) -> Vec<Note> {
    read_all(paths).into_values().filter(|n| n.repo == repo).collect()
}

/// Record `note` (already checked), replacing any earlier note for its session; notes for
/// sessions that ended long ago are dropped on the way.
pub fn record(paths: &Paths, mut note: Note, now: u64) -> std::io::Result<()> {
    let _lock = lock::exclusive_within(&paths.ledger().join(".harness.lock"), PATIENCE)?
        .ok_or_else(|| std::io::Error::other("another process holds the harness ledger's lock"))?;
    note.noted_at = now;
    let keep_after = now.saturating_sub(KEEP_DAYS * 86_400_000);
    let mut all = read_all(paths);
    all.retain(|_, n| n.ended_at.max(n.noted_at) >= keep_after);
    all.insert(note.session_id.clone(), note);
    let mut body = String::new();
    for n in all.values() {
        body.push_str(&serde_json::to_string(n).map_err(std::io::Error::other)?);
        body.push('\n');
    }
    adage_store::config::write_atomic(&file(paths), body.as_bytes())
}

/// Lay `notes` over a ledger in memory: each note's session becomes the harness's figures.
pub fn overlay(ledger: &mut BTreeMap<String, SessionSummary>, notes: &[Note]) -> bool {
    let mut changed = false;
    for n in notes {
        let merged = n.over(ledger.get(&n.session_id));
        if ledger.get(&n.session_id) != Some(&merged) {
            ledger.insert(n.session_id.clone(), merged);
            changed = true;
        }
    }
    changed
}

/// The lock every writer of a repository's ledger takes to merge into it.
pub fn ledger_lock(paths: &Paths, repo: &str) -> Option<lock::Lock> {
    let l = Ledger::new(&paths.ledger(), repo);
    let p = l.path().with_extension("lock");
    lock::exclusive_within(&p, PATIENCE).ok().flatten()
}

/// Merge `repo`'s notes into its ledger on disk, under the ledger's lock: the file is read again
/// under the lock (a concurrent writer's sessions are kept), `ledger` adds any session not yet
/// written, and the notes go over both. Returns the merged ledger; best effort (a ledger that
/// cannot be locked or written is merged in memory only, and again next time).
pub fn merge(paths: &Paths, repo: &str, ledger: &BTreeMap<String, SessionSummary>) -> BTreeMap<String, SessionSummary> {
    let notes = read(paths, repo);
    let l = Ledger::new(&paths.ledger(), repo);
    let held = ledger_lock(paths, repo);
    let on_disk = l.read();
    let mut merged = on_disk.clone();
    for (id, s) in ledger {
        merged.entry(id.clone()).or_insert_with(|| s.clone());
    }
    overlay(&mut merged, &notes);
    if held.is_some() && merged != on_disk {
        let body: String = merged.values().filter_map(|s| serde_json::to_string(s).ok()).map(|l| l + "\n").collect();
        let _ = adage_store::config::write_atomic(&l.path(), body.as_bytes());
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    pub(crate) fn line(session: &str, repo: &str, predicted: &[bool], cost: Option<f64>) -> Value {
        let steps: Vec<Value> = predicted
            .iter()
            .enumerate()
            .map(|(i, p)| {
                json!({ "index": i, "model": "claude-sonnet-4-6", "input": 10, "output": 20, "cache_read": 300,
                        "cache_write": 40, "predicted": p })
            })
            .collect();
        let mut v = json!({ "session_id": session, "repo": repo, "harness": "claude-code", "harness_version": "2.1.288",
                            "started_at": 1_000, "ended_at": 2_000, "steps": steps, "hook_ms": [3, 4] });
        if let Some(c) = cost {
            v["cost_usd"] = json!(c);
        }
        v
    }

    fn start(paths: &Paths, session: &str, repo: &str) {
        adage_store::spool::append(
            &paths.spool(session),
            &json!({ "ts": 1, "event": "SessionStart", "kind": "session_start", "arm": "treated", "repo": repo }),
        )
        .unwrap();
    }

    fn transcript_line(session: &str) -> SessionSummary {
        SessionSummary {
            session_id: session.into(),
            harness: "claude-code".into(),
            harness_version: "2.1.288".into(),
            models: vec!["claude-sonnet-4-6".into()],
            started_at: 900,
            ended_at: 2_500,
            model_turns: 9,
            tool_calls: 7,
            first_edit_turn: Some(2),
            tokens: Tokens { input: 1, ..Default::default() },
            cost: Cost { usd: Some(0.5), source: "priced@2026-06-24".into() },
            asked_for_help: true,
        }
    }

    /// T-A4-02: six steps, two of them Adage's: four model turns, two predicted, and the tokens are
    /// the four counts summed over all six.
    #[test]
    fn steps_and_predictions() {
        let n = parse(&line("s", "r", &[false, true, false, false, true, false], None).to_string()).unwrap();
        assert_eq!((n.model_turns(), n.predicted_steps()), (4, 2));
        let t = n.tokens();
        assert_eq!((t.input, t.output, t.cache_read, t.cache_write), (60, 120, 1800, 240));
        let s = n.over(None);
        assert_eq!(s.model_turns, 4);
        assert!(s.cost.source.starts_with("priced@"), "no engine figure: priced from the table, {:?}", s.cost);
    }

    /// O0 live notes, defect 10: with no transcript pass (`learn.auto` off) every session's line
    /// read `tool_calls: 0`, because the note carried no count. A note that counts them is
    /// recorded with its count, over a transcript's; one that does not keeps the transcript's, and
    /// a count that is not one is refused.
    #[test]
    fn a_notes_tool_calls_are_kept() {
        let mut v = line("s", "r", &[false, true], None);
        let n = parse(&v.to_string()).unwrap();
        assert_eq!((n.tool_calls, n.over(None).tool_calls), (None, 0));
        assert_eq!(n.over(Some(&transcript_line("s"))).tool_calls, 7, "the transcript's, without a count");
        v["tool_calls"] = json!(5);
        let n = parse(&v.to_string()).unwrap();
        assert_eq!(n.tool_calls, Some(5));
        assert_eq!((n.over(None).tool_calls, n.over(Some(&transcript_line("s"))).tool_calls), (5, 5));
        let text = serde_json::to_string(&n).unwrap();
        assert!(text.contains("\"tool_calls\":5"), "{text}");
        for bad in [json!(-1), json!("5"), json!(1.5), json!(u64::from(u32::MAX) + 1)] {
            v["tool_calls"] = bad.clone();
            assert!(parse(&v.to_string()).is_err(), "{bad}");
        }
    }

    /// T-A4-01: a note is merged under the ledger's lock; a later transcript pass does not keep its
    /// figures, a second note replaces the first, and the cost is the harness's.
    #[test]
    fn the_merge() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let repo = "github.com/acme/billing";
        start(&paths, "S", repo);
        let n = parse(&line("S", repo, &[false, false, true], Some(0.12)).to_string()).unwrap();
        check_session(&paths, &n).unwrap();
        record(&paths, n, 10).unwrap();
        let merged = merge(&paths, repo, &BTreeMap::new());
        let l = Ledger::new(&paths.ledger(), repo);
        let s = &l.read()["S"];
        assert_eq!(merged["S"], *s, "written under the lock");
        assert_eq!((s.cost.source.as_str(), s.cost.usd, s.model_turns), ("harness", Some(0.12), 2));

        // The learner's transcript pass writes its own line for S; the next merge puts the
        // harness's figures back and keeps what only the transcript knows.
        let mut ledger = l.read();
        ledger.insert("S".into(), transcript_line("S"));
        let body: String = ledger.values().map(|s| serde_json::to_string(s).unwrap() + "\n").collect();
        std::fs::write(l.path(), body).unwrap();
        merge(&paths, repo, &l.read());
        let s = &l.read()["S"];
        assert_eq!((s.cost.source.as_str(), s.model_turns, s.tool_calls), ("harness", 2, 7));
        assert_eq!((s.first_edit_turn, s.asked_for_help, s.started_at, s.ended_at), (Some(2), true, 1_000, 2_500));

        // A second note for S replaces the first: one line per session.
        let n2 = parse(&line("S", repo, &[false, false, false, false], Some(0.2)).to_string()).unwrap();
        record(&paths, n2, 20).unwrap();
        merge(&paths, repo, &l.read());
        assert_eq!(read_all(&paths).len(), 1);
        let s = &l.read()["S"];
        assert_eq!((s.cost.usd, s.model_turns), (Some(0.2), 4));
        assert_eq!(std::fs::read_to_string(l.path()).unwrap().lines().count(), 1);
    }

    #[test]
    fn a_note_needs_its_session_and_a_version() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let n = parse(&line("S", "r", &[false], None).to_string()).unwrap();
        assert!(check_session(&paths, &n).is_err(), "no spool");
        start(&paths, "S", "other");
        assert!(check_session(&paths, &n).is_err(), "started in another repository");
        start(&paths, "S", "r");
        assert!(check_session(&paths, &n).is_ok());

        for (field, value) in [
            ("harness_version", json!("latest")),
            ("harness_version", json!("2")),
            ("harness", json!("codex")),
            ("session_id", json!("")),
            ("steps", json!("many")),
            ("cost_usd", json!(-1)),
            ("started_at", json!("yesterday")),
            ("hook_ms", json!([-3])),
        ] {
            let mut v = line("S", "r", &[false], None);
            v[field] = value.clone();
            assert!(parse(&v.to_string()).is_err(), "{field} = {value}");
        }
        let mut v = line("S", "r", &[false], None);
        v["steps"][0]["predicted"] = json!("yes");
        assert!(parse(&v.to_string()).is_err());
        assert!(parse("[1]").is_err() && parse("{").is_err());
        let mut v = line("S", "r", &[false], None);
        v["started_at"] = json!("2026-10-03T10:00:00Z");
        v["ended_at"] = json!("2026-10-03T10:05:00.5Z");
        let n = parse(&v.to_string()).unwrap();
        assert_eq!(n.ended_at - n.started_at, 300_500);
        assert!(is_version("2.1.288") && is_version("2.1.288-beta.1") && !is_version("2.1.") && !is_version("v2.1"));
    }

    /// Notes of sessions that ended long ago go when the file is next written.
    #[test]
    fn old_notes_are_dropped() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let day = 86_400_000;
        let mut old = parse(&line("old", "r", &[false], None).to_string()).unwrap();
        old.ended_at = day;
        record(&paths, old, day).unwrap();
        let fresh = parse(&line("new", "r", &[false], None).to_string()).unwrap();
        record(&paths, fresh, 200 * day).unwrap();
        assert_eq!(read_all(&paths).keys().collect::<Vec<_>>(), vec!["new"]);
    }
}
