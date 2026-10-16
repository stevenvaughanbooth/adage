//! Append-only, CRC-framed spools: `magic | len | crc | payload`, one `O_APPEND` write each.
//!
//! **Following a spool live.** `adage ui` / `adage watch` tail `$ADAGE_HOME/sessions/*.spool` with
//! [`read_from`]: keep the offset it returns per file and call it again when the file grows. A
//! record still being written when it is read (its frame runs past the end of the file) is not
//! returned, and the offset stops at its start, so the next call reads it whole. A torn frame (a
//! crash mid-append) is skipped once later records make it provably corrupt, exactly as
//! [`read_all`] does. Each record is the JSON object the hook appended (Architecture §4.4):
//! `{ ts, event, kind, arm, repo, generation, harness?, decision }`, with no prompt text.

use crate::crc::crc32;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

const MAGIC: [u8; 4] = [0xAD, 0xA6, 0xE5, 0x01];
const HEADER: usize = 12;
pub const MAX_RECORD: usize = 1 << 20;

pub fn append<T: Serialize>(path: &Path, record: &T) -> io::Result<()> {
    append_impl(path, record, false)
}

/// Append and durably flush a complete record before returning. Callers that use a spool as
/// an execution journal must serialize writers, use [`read_all_strict`], and treat any error as
/// uncertain storage. The ordinary telemetry writer deliberately keeps its cheaper semantics.
pub fn append_durable<T: Serialize>(path: &Path, record: &T) -> io::Result<()> {
    append_impl(path, record, true)
}

fn append_impl<T: Serialize>(path: &Path, record: &T, durable: bool) -> io::Result<()> {
    let payload = serde_json::to_vec(record).map_err(io::Error::other)?;
    if payload.len() > MAX_RECORD {
        return Err(io::Error::other("spool record too large"));
    }
    let mut buf = Vec::with_capacity(HEADER + payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&crc32(&payload).to_le_bytes());
    buf.extend_from_slice(&payload);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(&buf)?;
    if durable {
        f.sync_all()?;
        // Persist the directory entry as well, including any freshly created ancestors.
        #[cfg(unix)]
        for dir in path.ancestors().skip(1).filter(|p| !p.as_os_str().is_empty()) {
            std::fs::File::open(dir)?.sync_all()?;
        }
    }
    Ok(())
}

/// Read every frame without resynchronizing or discarding corrupt, truncated, or invalid JSON
/// records. Execution authority must never silently lose a prepared or completed operation.
pub fn read_all_strict<T: DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    let mut at = 0;
    while at < bytes.len() {
        if !valid_frame_at(&bytes, at) {
            return Err(io::Error::new(io::ErrorKind::InvalidData, format!("invalid spool frame at byte {at}")));
        }
        let len = u32::from_le_bytes(bytes[at + 4..at + 8].try_into().unwrap()) as usize;
        let end = at + HEADER + len;
        out.push(
            serde_json::from_slice(&bytes[at + HEADER..end])
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?,
        );
        at = end;
    }
    Ok(out)
}

/// The valid records from byte `offset` on, and the offset to continue from. A frame that runs past
/// the current end of the file (still being appended) ends the read, and the returned offset
/// points at it. An offset past the end of the file (the spool was replaced or truncated) starts
/// again from the beginning. A missing file is empty at offset 0.
pub fn read_from<T: DeserializeOwned>(path: &Path, offset: u64) -> io::Result<(Vec<T>, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e),
    };
    let len = f.metadata()?.len();
    let start = if offset > len { 0 } else { offset };
    f.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    f.take(len - start).read_to_end(&mut bytes)?;
    let (out, consumed) = parse(&bytes, false);
    Ok((out, start + consumed as u64))
}

/// Records in `bytes`, and how many bytes were consumed. With `to_end`, an incomplete frame at the
/// end is skipped over like a corrupt one (the whole file is read); without it, parsing stops at
/// it so that a tail can read it once it is complete.
fn parse<T: DeserializeOwned>(bytes: &[u8], to_end: bool) -> (Vec<T>, usize) {
    let mut out = Vec::new();
    let mut i = 0;
    while i + HEADER <= bytes.len() {
        if bytes[i..i + 4] != MAGIC {
            i += 1;
            continue;
        }
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        let crc = u32::from_le_bytes(bytes[i + 8..i + 12].try_into().unwrap());
        let end = i + HEADER + len;
        if len <= MAX_RECORD && end > bytes.len() && !to_end {
            // Still being written, unless a complete record follows it (then it was torn by a
            // crash, and is skipped as read_all would): wait for more.
            match (i + 1..bytes.len()).find(|&j| valid_frame_at(bytes, j)) {
                Some(j) => {
                    i = j;
                    continue;
                }
                None => return (out, i),
            }
        }
        if len > MAX_RECORD || end > bytes.len() || crc32(&bytes[i + HEADER..end]) != crc {
            i += 1;
            continue;
        }
        if let Ok(v) = serde_json::from_slice(&bytes[i + HEADER..end]) {
            out.push(v);
        }
        i = end;
    }
    // Fewer than a header's bytes remain: they may begin a frame not yet complete.
    (out, if to_end { bytes.len() } else { i })
}

/// Does a complete frame with a valid CRC start at `j`?
fn valid_frame_at(bytes: &[u8], j: usize) -> bool {
    if j + HEADER > bytes.len() || bytes[j..j + 4] != MAGIC {
        return false;
    }
    let len = u32::from_le_bytes(bytes[j + 4..j + 8].try_into().unwrap()) as usize;
    let crc = u32::from_le_bytes(bytes[j + 8..j + 12].try_into().unwrap());
    len <= MAX_RECORD && j + HEADER + len <= bytes.len() && crc32(&bytes[j + HEADER..j + HEADER + len]) == crc
}

/// Every valid record, in file order. Corrupt or truncated frames are skipped by resynchronising
/// on the next magic, so a crash mid-append costs at most that one record.
pub fn read_all<T: DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    Ok(parse(&bytes, true).0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn durable_records_are_strictly_read_and_torn_tails_refuse() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("new/nested/journal.spool");
        append_durable(&p, &json!({ "state": "prepared" })).unwrap();
        assert_eq!(read_all_strict::<Value>(&p).unwrap().len(), 1);
        OpenOptions::new().append(true).open(&p).unwrap().write_all(&MAGIC).unwrap();
        assert_eq!(read_all_strict::<Value>(&p).unwrap_err().kind(), io::ErrorKind::InvalidData);
        assert_eq!(read_all::<Value>(&p).unwrap().len(), 1, "telemetry retains its tolerant reader");
    }

    #[test]
    fn append_and_read_back_in_order() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.spool");
        for i in 0..10 {
            append(&p, &json!({ "i": i })).unwrap();
        }
        let v: Vec<Value> = read_all(&p).unwrap();
        assert_eq!(v.len(), 10);
        assert_eq!(v[9]["i"], 9);
    }

    #[test]
    fn survives_torn_frame() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.spool");
        append(&p, &json!({ "a": 1 })).unwrap();
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(&[0xAD, 0xA6, 0xE5, 0x01, 200, 0, 0, 0, 1, 2]).unwrap();
        append(&p, &json!({ "b": 2 })).unwrap();
        let v: Vec<Value> = read_all(&p).unwrap();
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn read_from_resumes_and_waits_for_partial_records() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.spool");
        let (none, off): (Vec<Value>, u64) = read_from(&p, 0).unwrap();
        assert!(none.is_empty() && off == 0, "a missing spool is empty");
        append(&p, &json!({ "i": 0 })).unwrap();
        append(&p, &json!({ "i": 1 })).unwrap();
        let (v, off): (Vec<Value>, u64) = read_from(&p, 0).unwrap();
        assert_eq!(v.len(), 2);
        assert_eq!(off, std::fs::metadata(&p).unwrap().len());
        // Half a record: its first bytes are on disk, the rest not yet.
        let payload = serde_json::to_vec(&json!({ "i": 2 })).unwrap();
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&crc32(&payload).to_le_bytes());
        frame.extend_from_slice(&payload);
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        for cut in [3, HEADER, HEADER + 4] {
            let before = std::fs::metadata(&p).unwrap().len();
            f.write_all(&frame[..cut]).unwrap();
            let (v, at): (Vec<Value>, u64) = read_from(&p, off).unwrap();
            assert!(v.is_empty(), "nothing complete yet (cut at {cut})");
            assert_eq!(at, off, "the offset waits at the partial frame");
            // Undo, to try the next cut from the same place. Through its own write handle: Windows
            // refuses `set_len` on an append-only one.
            OpenOptions::new().write(true).open(&p).unwrap().set_len(before).unwrap();
        }
        f.write_all(&frame[..HEADER + 4]).unwrap();
        f.write_all(&frame[HEADER + 4..]).unwrap();
        let (v, off2): (Vec<Value>, u64) = read_from(&p, off).unwrap();
        assert_eq!(v, vec![json!({ "i": 2 })]);
        assert_eq!(off2, std::fs::metadata(&p).unwrap().len());
        // A torn frame followed by a complete one is skipped, as read_all does.
        f.write_all(&[0xAD, 0xA6, 0xE5, 0x01, 200, 0, 0, 0, 1, 2]).unwrap();
        append(&p, &json!({ "i": 3 })).unwrap();
        let (v, _): (Vec<Value>, u64) = read_from(&p, off2).unwrap();
        assert_eq!(v, vec![json!({ "i": 3 })]);
        // An offset past the end (the spool was replaced) starts again.
        let (v, _): (Vec<Value>, u64) = read_from(&p, 1 << 40).unwrap();
        assert_eq!(v.len(), 4);
    }

    #[test]
    fn read_from_follows_concurrent_appenders_exactly_once() {
        let d = tempfile::tempdir().unwrap();
        let p = std::sync::Arc::new(d.path().join("s.spool"));
        let writers: Vec<_> = (0..4)
            .map(|t| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for i in 0..300 {
                        append(&p, &json!({ "t": t, "i": i, "pad": "x".repeat(i * 7 % 900) })).unwrap();
                    }
                })
            })
            .collect();
        let mut seen: Vec<Value> = Vec::new();
        let mut off = 0;
        let mut done = false;
        while !done {
            done = writers.iter().all(|w| w.is_finished());
            let (v, o): (Vec<Value>, u64) = read_from(&p, off).unwrap();
            seen.extend(v);
            off = o;
        }
        for w in writers {
            w.join().unwrap();
        }
        let (v, _): (Vec<Value>, u64) = read_from(&p, off).unwrap();
        seen.extend(v);
        assert_eq!(seen.len(), 1200, "every record once, none torn");
        let mut keys: Vec<(u64, u64)> =
            seen.iter().map(|r| (r["t"].as_u64().unwrap(), r["i"].as_u64().unwrap())).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), 1200);
        let all: Vec<Value> = read_all(&p).unwrap();
        assert_eq!(all, seen, "the tail reads what read_all reads, in order");
    }

    #[test]
    fn concurrent_appenders_never_interleave() {
        let d = tempfile::tempdir().unwrap();
        let p = std::sync::Arc::new(d.path().join("s.spool"));
        let hs: Vec<_> = (0..8)
            .map(|t| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for i in 0..200 {
                        append(&p, &json!({ "t": t, "i": i, "pad": "x".repeat(300) })).unwrap();
                    }
                })
            })
            .collect();
        for h in hs {
            h.join().unwrap();
        }
        let v: Vec<Value> = read_all(&p).unwrap();
        assert_eq!(v.len(), 1600);
    }
}
