//! Two-slot state files under an exclusive file lock (spike S3's winning design): `flock` on Unix,
//! `LockFileEx` on Windows, both through `std`'s `File::lock`.

use crate::crc::crc32;
use serde::{de::DeserializeOwned, Serialize};
use std::fs::{File, OpenOptions};
use std::io;
use std::path::Path;

pub const SLOT: usize = 64 * 1024;
const HEADER: usize = 4 + 8 + 4 + 4;
const MAGIC: [u8; 4] = [0xAD, 0xA6, 0x57, 0x01];

/// Positional I/O: `pread`/`pwrite` on Unix, `ReadFile`/`WriteFile` with an offset on Windows.
/// Windows' `seek_read`/`seek_write` may transfer fewer bytes than asked (and move the cursor, which
/// nothing here relies on), so they are looped to the full length like Unix's `*_exact_at`.
#[cfg(unix)]
mod pos {
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::FileExt;

    pub fn read_exact_at(file: &File, buf: &mut [u8], off: u64) -> io::Result<()> {
        file.read_exact_at(buf, off)
    }
    pub fn write_all_at(file: &File, buf: &[u8], off: u64) -> io::Result<()> {
        file.write_all_at(buf, off)
    }
}

#[cfg(windows)]
mod pos {
    use std::fs::File;
    use std::io;
    use std::os::windows::fs::FileExt;

    pub fn read_exact_at(file: &File, mut buf: &mut [u8], mut off: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match file.seek_read(buf, off) {
                Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Ok(n) => {
                    buf = &mut buf[n..];
                    off += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                // Reading past the end of a file with an offset is ERROR_HANDLE_EOF.
                Err(e) if e.raw_os_error() == Some(38) => return Err(io::ErrorKind::UnexpectedEof.into()),
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
    pub fn write_all_at(file: &File, mut buf: &[u8], mut off: u64) -> io::Result<()> {
        while !buf.is_empty() {
            match file.seek_write(buf, off) {
                Ok(0) => return Err(io::ErrorKind::WriteZero.into()),
                Ok(n) => {
                    buf = &buf[n..];
                    off += n as u64;
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
                Err(e) => return Err(e),
            }
        }
        Ok(())
    }
}

/// The CRC covers the sequence number and length as well as the payload. With the payload alone, a
/// write torn after the header left a new sequence number over the slot's *old* payload, whose CRC
/// still matched: the reader then took a state two writes stale for the newest.
fn frame_crc(seq: u64, len: u32, payload: &[u8]) -> u32 {
    let mut b = Vec::with_capacity(12 + payload.len());
    b.extend_from_slice(&seq.to_le_bytes());
    b.extend_from_slice(&len.to_le_bytes());
    b.extend_from_slice(payload);
    crc32(&b)
}

fn read_slot(file: &File, idx: usize) -> Option<(u64, Vec<u8>)> {
    let mut hdr = [0u8; HEADER];
    pos::read_exact_at(file, &mut hdr, (idx * SLOT) as u64).ok()?;
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
    pos::read_exact_at(file, &mut payload, (idx * SLOT + HEADER) as u64).ok()?;
    (frame_crc(seq, len as u32, &payload) == crc).then_some((seq, payload))
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
    update_or_rebuild(path, || None, f)
}

/// [`update`], but a state that was written and can no longer be read (both slots fail their
/// CRC, or the newest payload no longer parses) starts from `rebuild()` instead of the default,
/// when it gives one. `rebuild` runs under the file's exclusive lock, so it must be bounded. A new
/// (empty) file starts from the default without it.
pub fn update_or_rebuild<T, R>(
    path: &Path,
    rebuild: impl FnOnce() -> Option<T>,
    f: impl FnOnce(&mut T) -> R,
) -> io::Result<R>
where
    T: Serialize + DeserializeOwned + Default,
{
    let file = open(path)?;
    file.lock()?;
    let current = newest(&file);
    let (target, seq, mut state) = match &current {
        Some((idx, seq, payload)) => {
            let state = serde_json::from_slice::<T>(payload).ok().or_else(rebuild).unwrap_or_default();
            (1 - idx, seq + 1, state)
        }
        // Written before, but neither slot is valid now: corrupt. (Neither can win over slot 0's
        // next write, so its sequence can start again.)
        None if file.metadata()?.len() > 0 => (0, 1, rebuild().unwrap_or_default()),
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
    buf.extend_from_slice(&frame_crc(seq, payload.len() as u32, &payload).to_le_bytes());
    buf.extend_from_slice(&payload);
    pos::write_all_at(&file, &buf, (target * SLOT) as u64)?;
    file.unlock()?;
    Ok(out)
}

/// Read state without modifying it (shared lock): the default for a missing or empty file, and
/// `None` for one that was written but can no longer be read (which [`update_or_rebuild`] would
/// rebuild).
pub fn try_read<T: DeserializeOwned + Default>(path: &Path) -> io::Result<Option<T>> {
    let file = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Some(T::default())),
        Err(e) => return Err(e),
    };
    file.lock_shared()?;
    let v = match newest(&file) {
        Some((_, _, p)) => serde_json::from_slice(&p).ok(),
        None if file.metadata()?.len() > 0 => None,
        None => Some(T::default()),
    };
    file.unlock()?;
    Ok(v)
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
        pos::write_all_at(&f, b"garbage", (SLOT + HEADER) as u64).unwrap();
        let s: S = read(&p).unwrap();
        assert_eq!(s.n, 1);
    }

    /// Regression (found by the adage-eval property tests): a write torn right after the header's
    /// sequence number left the slot's old payload, with its old, matching CRC, under a new
    /// sequence number, and the reader took that state, two writes stale, as the newest.
    #[test]
    fn a_write_torn_after_the_sequence_number_is_not_read_as_newest() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.state");
        for n in 1..=3 {
            update::<S, _>(&p, |s| s.n = n).unwrap();
        }
        // Slots: 0 holds n=3 (seq 3), 1 holds n=2 (seq 2). The next write goes to slot 1; tear it
        // after the new sequence number only.
        let f = OpenOptions::new().write(true).open(&p).unwrap();
        pos::write_all_at(&f, &4u64.to_le_bytes(), (SLOT + 4) as u64).unwrap();
        assert_eq!(read::<S>(&p).unwrap().n, 3, "the previous state, not an older one");
    }

    fn corrupt_both(p: &Path) {
        let f = OpenOptions::new().write(true).open(p).unwrap();
        for slot in 0..2 {
            pos::write_all_at(&f, b"garbage", (slot * SLOT + HEADER) as u64).unwrap();
        }
    }

    #[test]
    fn unreadable_state_is_rebuilt_and_only_then() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.state");
        let rebuilt = || Some(S { n: 40, v: vec!["from the spool".into()] });
        // A new file: the default, and no rebuild.
        update_or_rebuild::<S, _>(&p, || panic!("nothing to rebuild"), |s| s.n += 1).unwrap();
        update::<S, _>(&p, |s| s.n += 1).unwrap();
        // A readable state: no rebuild.
        update_or_rebuild::<S, _>(&p, || panic!("state is readable"), |s| s.n += 1).unwrap();
        assert_eq!(read::<S>(&p).unwrap().n, 3);
        // Both slots corrupt: the rebuilt state, and the update applies to it.
        corrupt_both(&p);
        assert_eq!(read::<S>(&p).unwrap(), S::default(), "unreadable");
        update_or_rebuild::<S, _>(&p, rebuilt, |s| s.n += 1).unwrap();
        assert_eq!(read::<S>(&p).unwrap(), S { n: 41, v: vec!["from the spool".into()] });
        // And the file is healthy again.
        update_or_rebuild::<S, _>(&p, || panic!("healthy"), |s| s.n += 1).unwrap();
        assert_eq!(read::<S>(&p).unwrap().n, 42);
        // A reader can tell unreadable from missing.
        assert_eq!(try_read::<S>(&p).unwrap().map(|s| s.n), Some(42));
        assert_eq!(try_read::<S>(&d.path().join("missing.state")).unwrap(), Some(S::default()));
        corrupt_both(&p);
        assert_eq!(try_read::<S>(&p).unwrap(), None);
        // Nothing to rebuild from: the default, as before.
        update_or_rebuild::<S, _>(&p, || None, |s| s.n += 1).unwrap();
        assert_eq!(read::<S>(&p).unwrap().n, 1);
    }

    #[test]
    fn a_payload_that_no_longer_parses_is_rebuilt() {
        #[derive(Default, Serialize, Deserialize, Debug, PartialEq)]
        struct Other {
            n: String,
        }
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.state");
        update::<Other, _>(&p, |s| s.n = "not a number".into()).unwrap();
        update_or_rebuild::<S, _>(&p, || Some(S { n: 7, v: vec![] }), |s| s.n += 1).unwrap();
        assert_eq!(read::<S>(&p).unwrap().n, 8);
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
