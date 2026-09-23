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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};

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
