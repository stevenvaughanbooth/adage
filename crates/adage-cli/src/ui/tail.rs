//! Following files as they grow, without reading them again: the session spools (CRC-framed
//! records, through `adage_store::spool::read_from`) and `learn.log` (JSON lines).
//!
//! Both keep a byte offset per file. A file that shrank was truncated or replaced (a rotated log,
//! a deleted and restarted session), so it is read again from the start. A frame or line still
//! being written is left for the next poll. At the first poll only recent history is read: the
//! last [`BACKFILL_BYTES`] of spools changed within [`BACKFILL_AGE`], and the last
//! [`LOG_BACKFILL_BYTES`] of the log.

use adage_store::spool;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

pub const BACKFILL_BYTES: u64 = 256 << 10;
pub const BACKFILL_AGE: Duration = Duration::from_secs(24 * 3600);
pub const LOG_BACKFILL_BYTES: u64 = 64 << 10;
/// Records read from one spool in one poll are bounded by `spool::MAX_READ_FROM`; a poll reads
/// at most this many chunks per file before moving on.
const CHUNKS_PER_POLL: usize = 4;

/// One spool record, with the session it came from (the spool's file stem).
#[derive(Debug, Clone)]
pub struct Record {
    pub session: String,
    pub value: Value,
}

pub struct SpoolTail {
    dir: PathBuf,
    cursors: HashMap<PathBuf, u64>,
    primed: bool,
}

impl SpoolTail {
    pub fn new(dir: PathBuf) -> SpoolTail {
        SpoolTail { dir, cursors: HashMap::new(), primed: false }
    }

    /// Records appended since the last poll, oldest first.
    pub fn poll(&mut self) -> Vec<Record> {
        let mut out = Vec::new();
        let Ok(rd) = std::fs::read_dir(&self.dir) else {
            self.primed = true;
            return out;
        };
        let now = SystemTime::now();
        let mut seen = HashSet::new();
        for e in rd.flatten() {
            let path = e.path();
            if path.extension().is_none_or(|x| x != "spool") {
                continue;
            }
            let Ok(meta) = e.metadata() else { continue };
            let len = meta.len();
            seen.insert(path.clone());
            let mut off = match self.cursors.get(&path) {
                Some(o) => *o,
                None if self.primed => 0,
                None => {
                    let recent = meta
                        .modified()
                        .ok()
                        .and_then(|m| now.duration_since(m).ok())
                        .is_some_and(|age| age <= BACKFILL_AGE);
                    if recent {
                        len.saturating_sub(BACKFILL_BYTES)
                    } else {
                        len
                    }
                }
            };
            if len < off {
                off = 0; // truncated or replaced
            }
            let session = path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            for _ in 0..CHUNKS_PER_POLL {
                if off >= len {
                    break;
                }
                let Ok((recs, next)) = spool::read_from::<Value>(&path, off) else { break };
                if next < off {
                    off = 0; // shrank between the listing and the read
                    continue;
                }
                let progressed = next > off;
                out.extend(recs.into_iter().map(|value| Record { session: session.clone(), value }));
                off = next;
                if !progressed {
                    break; // a partial frame waits for the next poll
                }
            }
            self.cursors.insert(path, off);
        }
        self.cursors.retain(|p, _| seen.contains(p));
        self.primed = true;
        out.sort_by_key(|r| r.value["ts"].as_u64().unwrap_or(0));
        out
    }
}

/// Follows a text file of lines (`learn.log`).
pub struct LineTail {
    path: PathBuf,
    offset: u64,
    primed: bool,
}

impl LineTail {
    pub fn new(path: PathBuf) -> LineTail {
        LineTail { path, offset: 0, primed: false }
    }

    /// Complete lines appended since the last poll.
    pub fn poll(&mut self) -> Vec<String> {
        let Ok(mut f) = std::fs::File::open(&self.path) else {
            // Not there (yet), or rotated away: start from the top of whatever appears next.
            self.primed = true;
            self.offset = 0;
            return vec![];
        };
        let len = f.metadata().map(|m| m.len()).unwrap_or(0);
        let mut skip_first = false;
        if !self.primed {
            self.primed = true;
            self.offset = len.saturating_sub(LOG_BACKFILL_BYTES);
            if self.offset > 0 {
                // Mid-line unless the byte before is a newline.
                let mut b = [0u8; 1];
                let at_start =
                    f.seek(SeekFrom::Start(self.offset - 1)).is_ok() && f.read_exact(&mut b).is_ok() && b[0] == b'\n';
                skip_first = !at_start;
            }
        }
        if len < self.offset {
            self.offset = 0;
        }
        if len == self.offset || f.seek(SeekFrom::Start(self.offset)).is_err() {
            return vec![];
        }
        let mut buf = Vec::new();
        if f.take((len - self.offset).min(4 << 20)).read_to_end(&mut buf).is_err() {
            return vec![];
        }
        let Some(last) = buf.iter().rposition(|b| *b == b'\n') else { return vec![] };
        self.offset += last as u64 + 1;
        let mut lines: Vec<String> =
            String::from_utf8_lossy(&buf[..last]).split('\n').map(|l| l.trim_end_matches('\r').to_string()).collect();
        if skip_first && !lines.is_empty() {
            lines.remove(0);
        }
        lines.retain(|l| !l.trim().is_empty());
        lines
    }
}
