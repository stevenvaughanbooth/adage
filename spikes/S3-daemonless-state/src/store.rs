//! Daemonless hot path: state files under `flock`, spools under `O_APPEND`.
//!
//! Two state backends are compared:
//! * `Rename` — `state/<s>.state`, replaced by write-temp + `rename(2)`; the lock lives on a
//!   separate `state/<s>.lock` (the state inode changes on every write, so it can't carry
//!   the lock). Readers need no lock at all: they see the old inode or the new one.
//! * `Slots`  — `state/<s>.slots`, two fixed 8 KiB slots overwritten alternately with one
//!   `pwrite(2)`; the newest slot with a valid CRC wins. `flock` sits on the file itself.
//!   No directory operations on the hot path.
//! * `UnsafeInPlace` — negative control for the crash test: truncate + two writes in place,
//!   no checksum fallback. Expected to tear.

use crate::spool::{self, Append, Record};
use crate::state::{apply, policy, SessionState, C_CORRUPT, C_ORDER_SLIP, C_REPLAYED};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::FileExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

pub const SLOT: usize = 8192;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Backend {
    Rename,
    Slots,
    UnsafeInPlace,
}
impl Backend {
    pub fn parse(s: &str) -> Backend {
        match s {
            "rename" => Backend::Rename,
            "slots" => Backend::Slots,
            "unsafe" => Backend::UnsafeInPlace,
            _ => panic!("unknown backend {s}"),
        }
    }
    pub fn name(&self) -> &'static str {
        match self {
            Backend::Rename => "rename",
            Backend::Slots => "slots",
            Backend::UnsafeInPlace => "unsafe",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Durability {
    None,      // page cache only: survives kill -9, not power loss
    Fsync,     // fsync(2): on macOS reaches the drive, not necessarily its cache
    FullFsync, // F_FULLFSYNC (std's sync_all on Apple platforms)
}
impl Durability {
    pub fn parse(s: &str) -> Durability {
        match s {
            "none" => Durability::None,
            "fsync" => Durability::Fsync,
            "full" => Durability::FullFsync,
            _ => panic!("unknown durability {s}"),
        }
    }
}

unsafe extern "C" {
    fn fsync(fd: i32) -> i32;
}
fn sync(f: &File, d: Durability) -> io::Result<()> {
    use std::os::fd::AsRawFd;
    match d {
        Durability::None => Ok(()),
        Durability::Fsync => {
            if unsafe { fsync(f.as_raw_fd()) } == 0 { Ok(()) } else { Err(io::Error::last_os_error()) }
        }
        Durability::FullFsync => f.sync_all(),
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub lock_ns: u64,  // open + flock acquire (includes waiting for other holders)
    pub load_ns: u64,  // read + decode
    pub fold_ns: u64,  // read spool tail + fold + decide + append decision record
    pub store_ns: u64, // encode + write (+ rename) (+ sync)
    pub total_ns: u64,
    pub folded: u32,   // spool records folded by this call
    pub state_bytes: u32,
}
impl Timing {
    /// "State access" in the pass/fail sense: everything except the fold/decide work.
    pub fn state_access_ns(&self) -> u64 {
        self.lock_ns + self.load_ns + self.store_ns
    }
}

pub struct Store {
    pub root: PathBuf,
    pub backend: Backend,
    pub durability: Durability,
}

fn ns(t: Instant) -> u64 {
    t.elapsed().as_nanos() as u64
}

impl Store {
    pub fn new(root: &Path, backend: Backend, durability: Durability) -> Store {
        fs::create_dir_all(root.join("state")).unwrap();
        fs::create_dir_all(root.join("spool")).unwrap();
        Store { root: root.to_path_buf(), backend, durability }
    }
    pub fn state_path(&self, s: &str) -> PathBuf {
        let ext = match self.backend {
            Backend::Rename => "state",
            Backend::Slots => "slots",
            Backend::UnsafeInPlace => "inplace",
        };
        self.root.join("state").join(format!("{s}.{ext}"))
    }
    pub fn lock_path(&self, s: &str) -> PathBuf {
        self.root.join("state").join(format!("{s}.lock"))
    }
    pub fn spool_path(&self, s: &str) -> PathBuf {
        spool::spool_path(&self.root, s)
    }

    /// Read-modify-write under an exclusive flock. `f` gets the current state (default if
    /// none or undecodable — the state is a cache and the fold will rebuild it).
    pub fn with_exclusive<T>(&self, s: &str, f: impl FnOnce(&mut SessionState, &mut Timing) -> T) -> io::Result<(T, Timing)> {
        let t_all = Instant::now();
        let mut t = Timing::default();
        let t0 = Instant::now();
        match self.backend {
            Backend::Rename => {
                let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.lock_path(s))?;
                lock.lock()?;
                t.lock_ns = ns(t0);
                let t1 = Instant::now();
                let path = self.state_path(s);
                let mut st = match fs::read(&path) {
                    Ok(b) => SessionState::decode(&b).unwrap_or_default(),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => SessionState::default(),
                    Err(e) => return Err(e),
                };
                t.load_ns = ns(t1);
                let out = f(&mut st, &mut t);
                let t2 = Instant::now();
                st.seq += 1;
                let img = st.encode();
                t.state_bytes = img.len() as u32;
                let tmp = path.with_extension("state.tmp");
                let mut tf = File::create(&tmp)?;
                tf.write_all(&img)?;
                sync(&tf, self.durability)?;
                drop(tf);
                fs::rename(&tmp, &path)?;
                t.store_ns = ns(t2);
                drop(lock); // close releases the flock
                t.total_ns = ns(t_all);
                Ok((out, t))
            }
            Backend::Slots => {
                let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.state_path(s))?;
                file.lock()?;
                t.lock_ns = ns(t0);
                let t1 = Instant::now();
                let (mut st, newest) = read_slots(&file)?;
                t.load_ns = ns(t1);
                let out = f(&mut st, &mut t);
                let t2 = Instant::now();
                st.seq += 1;
                let img = st.encode();
                assert!(img.len() <= SLOT, "state image {} exceeds slot", img.len());
                t.state_bytes = img.len() as u32;
                let target = match newest { Some(0) => 1, _ => 0 };
                file.write_all_at(&img, (target * SLOT) as u64)?;
                sync(&file, self.durability)?;
                t.store_ns = ns(t2);
                drop(file);
                t.total_ns = ns(t_all);
                Ok((out, t))
            }
            Backend::UnsafeInPlace => {
                let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(self.lock_path(s))?;
                lock.lock()?;
                t.lock_ns = ns(t0);
                let path = self.state_path(s);
                let mut st = match fs::read(&path) {
                    Ok(b) => SessionState::decode(&b).unwrap_or_default(),
                    Err(_) => SessionState::default(),
                };
                let out = f(&mut st, &mut t);
                st.seq += 1;
                let img = st.encode();
                let mut file = OpenOptions::new().write(true).create(true).truncate(true).open(&path)?;
                let half = img.len() / 2;
                file.write_all(&img[..half])?;
                std::thread::yield_now();
                file.write_all(&img[half..])?;
                drop(lock);
                t.total_ns = ns(t_all);
                Ok((out, t))
            }
        }
    }

    /// Read-only access for a non-mutating consumer (status line, `adage status`).
    /// Rename: no lock at all. Slots: shared flock.
    pub fn peek(&self, s: &str) -> io::Result<(Option<SessionState>, Timing)> {
        let t_all = Instant::now();
        let mut t = Timing::default();
        let st = match self.backend {
            Backend::Rename | Backend::UnsafeInPlace => match fs::read(self.state_path(s)) {
                Ok(b) => SessionState::decode(&b),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e),
            },
            Backend::Slots => {
                let file = match File::open(self.state_path(s)) {
                    Ok(f) => f,
                    Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((None, t)),
                    Err(e) => return Err(e),
                };
                let t0 = Instant::now();
                file.lock_shared()?;
                t.lock_ns = ns(t0);
                let (st, newest) = read_slots(&file)?;
                newest.map(|_| st)
            }
        };
        t.total_ns = ns(t_all);
        t.load_ns = t.total_ns - t.lock_ns;
        Ok((st, t))
    }

    /// A synchronous decision hook (UserPromptSubmit, PostToolBatch, SubagentStart, Stop, …).
    /// Under the session lock: fold the spool tail into state, decide, append the decision
    /// record to the spool (so it is never lost — F6), apply it, write state back.
    pub fn decide(&self, s: &str, mut ev: Record) -> io::Result<(Record, Timing)> {
        let spool_path = self.spool_path(s);
        let (res, t) = self.with_exclusive(s, |st, t| -> io::Result<Record> {
            let t0 = Instant::now();
            fold_tail(st, &spool_path, t)?;
            ev.dseq = st.decision_seq + 1;
            ev.directive = policy(st, &ev);
            // spool first, then state: a crash between the two is healed by the next fold,
            // which replays the decision record (dseq > state.decision_seq).
            let fr = spool::frame(&ev.encode());
            let end = match spool::append_frame_pos(&spool_path, &fr)? {
                (Append::Ok, end) => end,
                (Append::Short(n), _) => return Err(io::Error::other(format!("short spool write {n}"))),
            };
            // Observations may have landed between our fold and our append. Fold them
            // first, so the state is always the fold of the spool *in file order* —
            // identical to a rebuild from the spool alone.
            let start = end - fr.len() as u64;
            if start > st.spool_cursor {
                fold_range(st, &spool_path, start, t)?;
            }
            apply(st, &ev);
            if st.spool_cursor == start {
                st.spool_cursor = end;
            } else {
                st.c[C_ORDER_SLIP] += 1; // leave the cursor; our record is skipped by dseq on refold
            }
            t.fold_ns = ns(t0);
            Ok(ev)
        })?;
        Ok((res?, t))
    }

    /// An asynchronous observation hook (PostToolUse, SubagentStop, SessionEnd):
    /// no lock, one O_APPEND write.
    pub fn observe(&self, s: &str, ev: &Record) -> io::Result<Timing> {
        let t0 = Instant::now();
        let r = spool::append_frame(&self.spool_path(s), &spool::frame(&ev.encode()))?;
        if let Append::Short(n) = r {
            return Err(io::Error::other(format!("short spool write {n}")));
        }
        Ok(Timing { total_ns: ns(t0), ..Default::default() })
    }

    /// Rebuild a session's state from its spool alone (state file lost or corrupt).
    pub fn rebuild(&self, s: &str) -> io::Result<SessionState> {
        let mut st = SessionState::default();
        let mut t = Timing::default();
        fold_tail(&mut st, &self.spool_path(s), &mut t)?;
        Ok(st)
    }
}

pub fn fold_tail(st: &mut SessionState, spool_path: &Path, t: &mut Timing) -> io::Result<()> {
    let (scan, _) = spool::read_from(spool_path, st.spool_cursor)?;
    fold_scan(st, &scan, t);
    Ok(())
}

/// Fold [cursor, end) only.
pub fn fold_range(st: &mut SessionState, spool_path: &Path, end: u64, t: &mut Timing) -> io::Result<()> {
    let mut f = File::open(spool_path)?;
    use std::io::{Seek, SeekFrom};
    f.seek(SeekFrom::Start(st.spool_cursor))?;
    let mut buf = vec![0u8; (end - st.spool_cursor) as usize];
    f.read_exact(&mut buf)?;
    let scan = spool::scan(&buf, st.spool_cursor);
    fold_scan(st, &scan, t);
    Ok(())
}

fn fold_scan(st: &mut SessionState, scan: &spool::Scan, t: &mut Timing) {
    for (_, p) in &scan.records {
        if let Some(r) = Record::decode(p) {
            if spool::is_decision(r.kind) && r.dseq > st.decision_seq {
                st.c[C_REPLAYED] += 1;
            }
            apply(st, &r);
            t.folded += 1;
        }
    }
    st.c[C_CORRUPT] += scan.corrupt.len() as u64;
    st.spool_cursor = scan.next;
}

/// Returns (state from the newest valid slot or default, index of that slot).
pub fn read_slots(file: &File) -> io::Result<(SessionState, Option<usize>)> {
    let mut buf = vec![0u8; 2 * SLOT];
    let mut n = 0;
    let mut fr = file;
    loop {
        match fr.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
        if n == buf.len() { break; }
    }
    buf.truncate(n);
    let a = buf.get(..SLOT.min(n)).and_then(SessionState::decode);
    let b = buf.get(SLOT..).and_then(SessionState::decode);
    Ok(match (a, b) {
        (Some(a), Some(b)) => if b.seq > a.seq { (b, Some(1)) } else { (a, Some(0)) },
        (Some(a), None) => (a, Some(0)),
        (None, Some(b)) => (b, Some(1)),
        (None, None) => (SessionState::default(), None),
    })
}
