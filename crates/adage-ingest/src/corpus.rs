//! Streaming, versioned canonical JSONL. A record is bounded before JSON is allocated; a bad
//! record is counted with a line number and reason, never silently treated as evidence.

use adage_trace::Session;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::{self, BufRead, Read, Write};

pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Record {
    pub schema: u32,
    pub session: Session,
}

impl Record {
    pub fn new(session: Session) -> Self {
        Self { schema: SCHEMA, session }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Limits {
    pub record_bytes: usize,
    pub steps: usize,
    pub reported_issues: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self { record_bytes: 8 * 1024 * 1024, steps: 50_000, reported_issues: 100 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Rejection {
    Oversized,
    InvalidJson,
    UnsupportedSchema,
    InvalidSession,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Issue {
    pub line: u64,
    pub reason: Rejection,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReadReport {
    pub bytes: u64,
    pub lines: u64,
    pub accepted: u64,
    pub rejected: BTreeMap<Rejection, u64>,
    /// Bounded diagnostics, with no prompt or tool output copied into the report.
    pub issues: Vec<Issue>,
    pub unreported_issues: u64,
}

impl ReadReport {
    pub fn rejected_count(&self) -> u64 {
        self.rejected.values().sum()
    }

    fn reject(&mut self, reason: Rejection, limit: usize) {
        *self.rejected.entry(reason).or_default() += 1;
        if self.issues.len() < limit {
            self.issues.push(Issue { line: self.lines, reason });
        } else {
            self.unreported_issues += 1;
        }
    }
}

/// Structural invariants of a canonical session. Legacy sessions can omit prompt anchors.
pub fn valid_session(s: &Session, limits: Limits) -> bool {
    !s.id.trim().is_empty()
        && !s.repo.trim().is_empty()
        && !s.harness.trim().is_empty()
        && s.steps.len() <= limits.steps
        && s.prompts.len() <= adage_trace::session::MAX_PROMPTS
        && s.prompts.iter().all(|p| p.at_step as usize <= s.steps.len())
        && s.prompts.windows(2).all(|p| p[0].at_step <= p[1].at_step)
        && s.steps.iter().all(|st| usize::from(st.prompt) < s.prompts.len().max(1))
        && s.steps.windows(2).all(|st| st[0].turn <= st[1].turn)
}

/// Visit one record at a time. The caller decides whether rejected records invalidate the import
/// (the production importer rejects the whole transaction). Sink and source I/O errors propagate.
pub fn read(
    mut reader: impl BufRead,
    limits: Limits,
    mut sink: impl FnMut(Session) -> io::Result<()>,
) -> io::Result<ReadReport> {
    if limits.record_bytes == 0 || limits.record_bytes == usize::MAX || limits.steps == 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid corpus limits"));
    }
    let mut report = ReadReport::default();
    let mut line = Vec::new();
    loop {
        line.clear();
        let n = reader.by_ref().take(limits.record_bytes as u64 + 1).read_until(b'\n', &mut line)?;
        if n == 0 {
            break;
        }
        report.lines += 1;
        report.bytes += n as u64;
        if line.len() > limits.record_bytes {
            if line.last() != Some(&b'\n') {
                report.bytes += reader.skip_until(b'\n')? as u64;
            }
            report.reject(Rejection::Oversized, limits.reported_issues);
            continue;
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        let record: Record = match serde_json::from_slice(&line) {
            Ok(record) => record,
            Err(_) => {
                report.reject(Rejection::InvalidJson, limits.reported_issues);
                continue;
            }
        };
        if record.schema != SCHEMA {
            report.reject(Rejection::UnsupportedSchema, limits.reported_issues);
        } else if !valid_session(&record.session, limits) {
            report.reject(Rejection::InvalidSession, limits.reported_issues);
        } else {
            sink(record.session)?;
            report.accepted += 1;
        }
    }
    Ok(report)
}

pub fn write(mut writer: impl Write, session: &Session) -> io::Result<()> {
    serde_json::to_writer(&mut writer, &Record::new(session.clone()))?;
    writer.write_all(b"\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn record() -> serde_json::Value {
        json!({"schema":1,"session":{
            "id":"s1","root":"/fixture","repo":"repo","model":null,"day":"2026-10-01",
            "prompt":"fix billing","steps":[],"steered":false,"directed":[],"outcome":null,
            "harness":"codex","identity":{"task_id":"TASK-7"}
        }})
    }

    #[test]
    fn canonical_records_round_trip_with_declared_identity() {
        let input = format!("{}\n", record());
        let mut sessions = Vec::new();
        let report = read(input.as_bytes(), Limits::default(), |s| {
            sessions.push(s);
            Ok(())
        })
        .unwrap();
        assert_eq!((report.accepted, report.rejected_count()), (1, 0));
        assert_eq!(sessions[0].identity.task_id.as_deref(), Some("TASK-7"));
        let mut encoded = vec![];
        write(&mut encoded, &sessions[0]).unwrap();
        read(encoded.as_slice(), Limits::default(), |s| {
            assert_eq!(s, sessions[0]);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn every_bad_record_has_a_reason_and_diagnostics_are_bounded() {
        let mut future = record();
        future["schema"] = json!(2);
        let mut invalid = record();
        invalid["session"]["repo"] = json!("");
        let input = format!("{{broken\n{future}\n{invalid}\n{}\n", record());
        let report = read(input.as_bytes(), Limits { reported_issues: 2, ..Limits::default() }, |_| Ok(())).unwrap();
        assert_eq!((report.accepted, report.rejected_count(), report.unreported_issues), (1, 3, 1));
        assert_eq!(
            report.rejected,
            BTreeMap::from([
                (Rejection::InvalidJson, 1),
                (Rejection::UnsupportedSchema, 1),
                (Rejection::InvalidSession, 1),
            ])
        );
        assert_eq!(report.issues.iter().map(|i| i.line).collect::<Vec<_>>(), [1, 2]);
    }

    #[test]
    fn oversized_records_drain_without_losing_the_next_record() {
        let input = format!("{}\n{}\n", "x".repeat(50_000), record());
        let report = read(input.as_bytes(), Limits { record_bytes: 512, ..Limits::default() }, |_| Ok(())).unwrap();
        assert_eq!(report.bytes, input.len() as u64);
        assert_eq!((report.accepted, report.rejected_count()), (1, 1));
        assert_eq!(report.rejected[&Rejection::Oversized], 1);
    }

    #[test]
    fn a_sink_failure_stops_reading_and_is_not_reported_as_acceptance() {
        let input = format!("{}\n", record());
        let error =
            read(input.as_bytes(), Limits::default(), |_| Err(io::Error::other("storage unavailable"))).unwrap_err();
        assert_eq!(error.to_string(), "storage unavailable");
    }
}
