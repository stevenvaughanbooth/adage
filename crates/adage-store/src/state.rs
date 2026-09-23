//! Two-slot state files under `flock` (spike S3's winning design).

use crate::crc::crc32;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::fs::FileExt;
use std::path::Path;

pub const SLOT: usize = 64 * 1024;
const HEADER: usize = 4 + 8 + 4 + 4;
const MAGIC: [u8; 4] = [0xAD, 0xA6, 0x57, 0x01];

fn read_slot(file: &File, idx: usize) -> Option<(u64, Vec<u8>)> {
    let mut hdr = [0u8; HEADER];
    file.read_exact_at(&mut hdr, (idx * SLOT) as u64).ok()?;
    if hdr[0..4] != MAGIC {
        return None;
    }
    let seq = u64::from_le_bytes(hdr[4..12].try_into().ok()?);
    let len = u32::from_le_bytes(hdr[12..16].try_into().ok()?) as usize;
    let crc = u32::from_le_bytes(hdr[16..20].try_into().ok()?);
    if len > SLOT - HEADER {
        return None;
    }
    let mut payload = vec![0u8; len];
    file.read_exact_at(&mut payload, (idx * SLOT + HEADER) as u64).ok()?;
    (crc32(&payload) == crc).then_some((seq, payload))
}

/// The newest valid slot, and which slot index it is in.
fn newest(file: &File) -> Option<(usize, u64, Vec<u8>)> {
    let a = read_slot(file, 0).map(|(s, p)| (0, s, p));
    let b = read_slot(file, 1).map(|(s, p)| (1, s, p));
    match (a, b) {
        (Some(x), Some(y)) => Some(if x.1 >= y.1 { x } else { y }),
        (x, y) => x.or(y),
    }
}

fn open(path: &Path) -> io::Result<File> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path)
}

/// Load, mutate, and store session state atomically with respect to other hook processes.
/// A missing or corrupt state yields `T::default()`.
pub fn update<T, R>(path: &Path, f: impl FnOnce(&mut T) -> R) -> io::Result<R>
where
    T: Serialize + DeserializeOwned + Default,
{
    let file = open(path)?;
    file.lock()?;
    let current = newest(&file);
    let (target, seq, mut state) = match &current {
        Some((idx, seq, payload)) => {
            (1 - idx, seq + 1, serde_json::from_slice::<T>(payload).unwrap_or_default())
        }
        None => (0, 1, T::default()),
    };
    let out = f(&mut state);
    let payload = serde_json::to_vec(&state).map_err(io::Error::other)?;
    if payload.len() > SLOT - HEADER {
        file.unlock()?;
        return Err(io::Error::other("session state exceeds slot size"));
    }
    let mut buf = Vec::with_capacity(HEADER + payload.len());
    buf.extend_from_slice(&MAGIC);
    buf.extend_from_slice(&seq.to_le_bytes());
    buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    buf.extend_from_slice(&crc32(&payload).to_le_bytes());
    buf.extend_from_slice(&payload);
    file.write_all_at(&buf, (target * SLOT) as u64)?;
    file.unlock()?;
    Ok(out)
}

/// Read state without modifying it (shared lock).
pub fn read<T: DeserializeOwned + Default>(path: &Path) -> io::Result<T> {
    if !path.exists() {
        return Ok(T::default());
    }
    let file = File::open(path)?;
    file.lock_shared()?;
    let v = newest(&file).and_then(|(_, _, p)| serde_json::from_slice(&p).ok()).unwrap_or_default();
    file.unlock()?;
    Ok(v)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
    struct S {
        n: u32,
        v: Vec<String>,
    }

    #[test]
    fn update_round_trips_and_alternates_slots() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.state");
        for i in 0..5 {
            update::<S, _>(&p, |s| {
                s.n += 1;
                s.v.push(format!("e{i}"));
            })
            .unwrap();
        }
        let s: S = read(&p).unwrap();
        assert_eq!(s.n, 5);
        assert_eq!(s.v.len(), 5);
    }

    #[test]
    fn corrupt_newest_slot_falls_back_to_previous() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.state");
        update::<S, _>(&p, |s| s.n = 1).unwrap();
        update::<S, _>(&p, |s| s.n = 2).unwrap();
        // Slot 1 holds n=2 now; corrupt its payload.
        let f = OpenOptions::new().write(true).open(&p).unwrap();
        f.write_all_at(b"garbage", (SLOT + HEADER) as u64).unwrap();
        let s: S = read(&p).unwrap();
        assert_eq!(s.n, 1);
    }

    #[test]
    fn concurrent_updates_are_serialised() {
        let d = tempfile::tempdir().unwrap();
        let p = std::sync::Arc::new(d.path().join("x.state"));
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let p = p.clone();
                std::thread::spawn(move || {
                    for _ in 0..50 {
                        update::<S, _>(&p, |s| s.n += 1).unwrap();
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().unwrap();
        }
        assert_eq!(read::<S>(&p).unwrap().n, 400);
    }
}
