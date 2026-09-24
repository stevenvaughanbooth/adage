//! Append-only, CRC-framed spools: `magic | len | crc | payload`, one `O_APPEND` write each.

use crate::crc::crc32;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::Path;

const MAGIC: [u8; 4] = [0xAD, 0xA6, 0xE5, 0x01];
const HEADER: usize = 12;
pub const MAX_RECORD: usize = 1 << 20;

pub fn append<T: Serialize>(path: &Path, record: &T) -> io::Result<()> {
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
    f.write_all(&buf)
}

/// Every valid record, in file order. Corrupt or truncated frames are skipped by resynchronising
/// on the next magic, so a crash mid-append costs at most that one record.
pub fn read_all<T: DeserializeOwned>(path: &Path) -> io::Result<Vec<T>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
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
        if len > MAX_RECORD || end > bytes.len() || crc32(&bytes[i + HEADER..end]) != crc {
            i += 1;
            continue;
        }
        if let Ok(v) = serde_json::from_slice(&bytes[i + HEADER..end]) {
            out.push(v);
        }
        i = end;
    }
    Ok(out)
}

/// At most this many bytes are read by one [`read_from`] call; the rest waits for the next.
pub const MAX_READ_FROM: u64 = 8 << 20;

/// Records appended after byte `offset`, for a reader that follows a spool as it grows (`adage
/// watch`). Returns the records and the offset to resume from: just past the last complete frame,
/// or at the start of a frame still being written, so a partial write is read once it completes.
/// Garbage between frames (a torn frame from a crash) is skipped by resynchronising on the next
/// magic, as [`read_all`] does. A file shorter than `offset` was truncated or replaced: the caller
/// sees `(vec![], len)` with `len < offset` and restarts from 0.
///
/// `offset` may land inside a frame (a reader starting near the end of a large spool): it
/// resynchronises on the next valid frame.
pub fn read_from<T: DeserializeOwned>(path: &Path, offset: u64) -> io::Result<(Vec<T>, u64)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Vec::new(), 0)),
        Err(e) => return Err(e),
    };
    let len = f.metadata()?.len();
    if len <= offset {
        return Ok((Vec::new(), len));
    }
    f.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    f.take(MAX_READ_FROM.min(len - offset)).read_to_end(&mut bytes)?;
    let (out, used) = parse_frames(&bytes);
    Ok((out, offset + used as u64))
}

/// Complete, valid frames in `bytes`, and how many bytes were consumed. A frame whose header or
/// payload runs past the end is left unconsumed: it may still be being written, or have been cut
/// off by [`MAX_READ_FROM`], and is read next time.
fn parse_frames<T: DeserializeOwned>(bytes: &[u8]) -> (Vec<T>, usize) {
    let mut out = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        if bytes.len() - i < 4 {
            // Possibly the start of a magic: wait for more.
            if MAGIC.starts_with(&bytes[i..]) {
                break;
            }
            i += 1;
            continue;
        }
        if bytes[i..i + 4] != MAGIC {
            i += 1;
            continue;
        }
        if i + HEADER > bytes.len() {
            break;
        }
        let len = u32::from_le_bytes(bytes[i + 4..i + 8].try_into().unwrap()) as usize;
        if len > MAX_RECORD {
            i += 1;
            continue;
        }
        let end = i + HEADER + len;
        if end > bytes.len() {
            // Incomplete: an append in progress (or a torn frame, skipped once more bytes arrive
            // and its CRC fails).
            break;
        }
        let crc = u32::from_le_bytes(bytes[i + 8..i + 12].try_into().unwrap());
        if crc32(&bytes[i + HEADER..end]) != crc {
            i += 1;
            continue;
        }
        if let Ok(v) = serde_json::from_slice(&bytes[i + HEADER..end]) {
            out.push(v);
        }
        i = end;
    }
    (out, i)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

    #[test]
    fn read_from_follows_appends_and_waits_for_partial_frames() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.spool");
        append(&p, &json!({ "i": 0 })).unwrap();
        let (v, off): (Vec<Value>, u64) = read_from(&p, 0).unwrap();
        assert_eq!(v.len(), 1);
        // A frame written in two pieces is read only once complete.
        let payload = serde_json::to_vec(&json!({ "i": 1 })).unwrap();
        let mut frame = MAGIC.to_vec();
        frame.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        frame.extend_from_slice(&crc32(&payload).to_le_bytes());
        frame.extend_from_slice(&payload);
        let mut f = OpenOptions::new().append(true).open(&p).unwrap();
        f.write_all(&frame[..7]).unwrap();
        let (v, off2): (Vec<Value>, u64) = read_from(&p, off).unwrap();
        assert!(v.is_empty());
        assert_eq!(off2, off, "a partial frame is not consumed");
        f.write_all(&frame[7..]).unwrap();
        let (v, off3): (Vec<Value>, u64) = read_from(&p, off2).unwrap();
        assert_eq!(v, vec![json!({ "i": 1 })]);
        assert_eq!(off3, std::fs::metadata(&p).unwrap().len());
        // Starting mid-frame resynchronises on the next one.
        append(&p, &json!({ "i": 2 })).unwrap();
        let (v, _): (Vec<Value>, u64) = read_from(&p, 3).unwrap();
        assert_eq!(v.last().unwrap()["i"], 2);
        // Truncation shows as a length below the offset.
        std::fs::write(&p, b"").unwrap();
        let (v, len): (Vec<Value>, u64) = read_from(&p, off3).unwrap();
        assert!(v.is_empty() && len == 0);
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
