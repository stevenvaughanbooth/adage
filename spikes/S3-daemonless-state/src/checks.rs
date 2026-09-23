//! Correctness checks, shared by `cargo test` (small parameters) and
//! `adage-s3 correctness` (large parameters, written to results/).

use crate::spool::{self, Record, K_TOOL};
use crate::state::{apply, policy, SessionState, C_EVENTS};
use crate::store::{Backend, Durability, Store};
use crate::util::Rng;
use crate::workload::{script, Step};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub type R<T> = Result<T, String>;

pub fn fresh_dir(p: &Path) -> PathBuf {
    let _ = fs::remove_dir_all(p);
    fs::create_dir_all(p).unwrap();
    p.to_path_buf()
}

/// A test record: writer id in `pid`, per-writer sequence in `eid`, random-size raw head.
pub fn test_frame(wid: u32, seq: u32, size: usize, rng: &mut Rng) -> Vec<u8> {
    let r = Record { kind: K_TOOL, pid: wid, eid: seq, tool_use: ((wid as u64) << 32) | seq as u64, raw: rng.bytes(size), ..Default::default() };
    spool::frame(&r.encode())
}

/// Verify a spool written by writers `wids`: no corrupt regions, each (wid, seq) exactly
/// once, per-writer sequences contiguous from 0 and in file order. Returns counts per wid.
pub fn verify_spool(path: &Path, expect: Option<&HashMap<u32, u32>>) -> R<(HashMap<u32, u32>, usize, bool)> {
    let (s, _) = spool::read_from(path, 0).map_err(|e| e.to_string())?;
    if !s.corrupt.is_empty() {
        return Err(format!("{} corrupt regions, first at {:?}", s.corrupt.len(), s.corrupt[0]));
    }
    let mut next: HashMap<u32, u32> = HashMap::new();
    for (off, p) in &s.records {
        let r = Record::decode(p).ok_or(format!("undecodable payload at {off}"))?;
        let n = next.entry(r.pid).or_insert(0);
        if r.eid != *n {
            return Err(format!("writer {} expected seq {} got {} at offset {off} (lost, duplicated or reordered)", r.pid, n, r.eid));
        }
        *n += 1;
    }
    if let Some(exp) = expect {
        for (w, n) in exp {
            let got = next.get(w).copied().unwrap_or(0);
            if got != *n {
                return Err(format!("writer {w}: {got} of {n} records present"));
            }
        }
    }
    Ok((next, s.records.len(), s.tail_pending))
}

// ---------------------------------------------------------------- 1. concurrent writers

pub struct SpoolCase {
    pub seed: u64,
    pub writers: u32,
    pub per_writer: u32,
    pub max_size: usize,
    pub reopen_each: bool,
}

pub fn gen_case(seed: u64) -> SpoolCase {
    let mut r = Rng::new(seed);
    let sizes = [256usize, 480, 4000, 16_000, 65_000];
    let max_size = sizes[r.below(sizes.len() as u64) as usize];
    SpoolCase {
        seed,
        writers: r.range(2, 64) as u32,
        per_writer: if max_size > 4000 { r.range(20, 100) as u32 } else { r.range(50, 400) as u32 },
        max_size,
        reopen_each: r.chance(0.5),
    }
}

pub fn spool_threads(dir: &Path, c: &SpoolCase) -> R<usize> {
    let path = dir.join(format!("threads-{}.log", c.seed));
    let _ = fs::remove_file(&path);
    std::thread::scope(|sc| {
        for w in 0..c.writers {
            let path = &path;
            sc.spawn(move || {
                let mut rng = Rng::new(c.seed * 1000 + w as u64);
                let mut f = OpenOptions::new().append(true).create(true).open(path).unwrap();
                for i in 0..c.per_writer {
                    let fr = test_frame(w, i, rng.below(c.max_size as u64 + 1) as usize, &mut rng);
                    if c.reopen_each {
                        assert_eq!(spool::append_frame(path, &fr).unwrap(), spool::Append::Ok);
                    } else {
                        assert_eq!(spool::append_on(&mut f, &fr).unwrap(), spool::Append::Ok);
                    }
                }
            });
        }
    });
    let exp: HashMap<u32, u32> = (0..c.writers).map(|w| (w, c.per_writer)).collect();
    let (_, n, tail) = verify_spool(&path, Some(&exp))?;
    if tail { return Err("unexpected pending tail".into()); }
    let _ = fs::remove_file(&path);
    Ok(n)
}

fn spawn_writer(exe: &Path, path: &Path, wid: u32, n: u32, max: usize, seed: u64) -> Child {
    Command::new(exe)
        .args(["spool-writer", path.to_str().unwrap(), &wid.to_string(), &n.to_string(), &max.to_string(), &seed.to_string()])
        .stdout(Stdio::null())
        .spawn()
        .expect("spawn spool-writer")
}

pub fn spool_procs(exe: &Path, dir: &Path, c: &SpoolCase) -> R<usize> {
    let path = dir.join(format!("procs-{}.log", c.seed));
    let _ = fs::remove_file(&path);
    let procs = c.writers.min(32);
    let kids: Vec<Child> = (0..procs).map(|w| spawn_writer(exe, &path, w, c.per_writer, c.max_size, c.seed)).collect();
    for mut k in kids {
        let st = k.wait().unwrap();
        if !st.success() { return Err(format!("writer exited {st}")); }
    }
    let exp: HashMap<u32, u32> = (0..procs).map(|w| (w, c.per_writer)).collect();
    let (_, n, tail) = verify_spool(&path, Some(&exp))?;
    if tail { return Err("unexpected pending tail".into()); }
    let _ = fs::remove_file(&path);
    Ok(n)
}

// ---------------------------------------------------------------- 2. ordering + convergence

/// Simulate the hot path over one scripted session with randomised async arrival.
/// Observations are delayed by up to `max_lag` decisions (0 = arrive before the next
/// decision); `dup_p` injects duplicate deliveries. Returns (spool, final state).
pub fn simulate(steps: &[Step], seed: u64, max_lag: u64, dup_p: f64) -> (Vec<Record>, SessionState, usize) {
    struct Sim { rng: Rng, delayed: Vec<(u64, Record)>, spool: Vec<Record>, st: SessionState, cursor: usize, max_lag: u64, dup_p: f64, dups: usize }
    impl Sim {
        fn obs(&mut self, r: &Record) {
            let lag = if self.max_lag == 0 { 0 } else { self.rng.below(self.max_lag + 1) };
            self.delayed.push((lag, r.clone()));
            if self.rng.chance(self.dup_p) {
                let lag2 = self.rng.below(self.max_lag + 1);
                self.delayed.push((lag2, r.clone()));
                self.dups += 1;
            }
        }
        fn release(&mut self, all: bool) {
            let mut out: Vec<Record> = Vec::new();
            let mut keep = Vec::new();
            for (lag, r) in self.delayed.drain(..) {
                if all || lag == 0 { out.push(r) } else { keep.push((lag - 1, r)) }
            }
            self.delayed = keep;
            self.rng.shuffle(&mut out); // arrival order within the window is arbitrary
            self.spool.extend(out);
        }
        fn fold(&mut self) {
            while self.cursor < self.spool.len() {
                let r = self.spool[self.cursor].clone();
                apply(&mut self.st, &r);
                self.cursor += 1;
            }
        }
        fn decide(&mut self, r: &Record) {
            self.release(false);
            self.fold();
            let mut ev = r.clone();
            ev.dseq = self.st.decision_seq + 1;
            ev.directive = policy(&self.st, &ev);
            self.spool.push(ev.clone());
            apply(&mut self.st, &ev);
            self.cursor += 1; // our own record: already applied (fold would skip it anyway)
        }
        fn run(&mut self, steps: &[Step]) {
            for s in steps {
                match s {
                    Step::Decide(r) => self.decide(r),
                    Step::Observe(r) => self.obs(r),
                    Step::Batch { early, sub, late, decide } => {
                        for r in early { self.obs(r) }
                        self.run(sub);
                        for r in late { self.obs(r) }
                        self.decide(decide);
                    }
                }
            }
        }
    }
    let mut sim = Sim { rng: Rng::new(seed), delayed: Vec::new(), spool: Vec::new(), st: SessionState::default(), cursor: 0, max_lag, dup_p, dups: 0 };
    sim.run(steps);
    sim.release(true);
    sim.fold();
    (sim.spool, sim.st, sim.dups)
}

#[derive(Debug, Default)]
pub struct OrderingReport {
    pub cases: usize,
    pub reconstruct_ok: usize,
    pub converge_ok: usize,
    pub dups_injected: usize,
    pub dups_detected: usize,
    pub late_folds: u64,
}

pub fn ordering(seeds: std::ops::Range<u64>, turns: usize, max_lag: u64, dup_p: f64) -> OrderingReport {
    let mut rep = OrderingReport::default();
    for seed in seeds {
        let sc = script(seed, seed as usize, turns);
        // reference: every observation lands before the next decision, in script order
        let (_, reference, _) = simulate(&sc.steps, seed, 0, 0.0);
        let (spool, st, dups) = simulate(&sc.steps, seed.wrapping_mul(7919) + 1, max_lag, dup_p);
        rep.cases += 1;
        rep.dups_injected += dups;
        rep.late_folds += st.c[crate::state::C_LATE];
        let (order, dup, missing) = spool::reconstruct(&spool);
        rep.dups_detected += dup;
        if order == sc.canonical && missing == 0 && dup == dups {
            rep.reconstruct_ok += 1;
        }
        if st.convergent_view() == reference.convergent_view() {
            rep.converge_ok += 1;
        }
    }
    rep
}

// ---------------------------------------------------------------- 3. crash safety

#[derive(Debug, Default)]
pub struct CrashReport {
    pub backend: String,
    pub iterations: usize,
    pub kills: usize,
    pub progressed: usize, // iterations in which the committed value advanced
    pub torn: usize,       // undecodable / invariant-violating image
    pub regressed: usize,  // committed value went backwards
    pub stale_tmp: usize,  // rename backend: temp file left by a killed writer (harmless)
    pub final_value: u64,
    pub lock_free_after: bool,
}

/// The crash child's invariant: seq == events, every history slot = events*7 + i.
pub fn state_invariant(st: &SessionState) -> bool {
    let v = st.c[C_EVENTS];
    st.seq == v && st.history.len() == 16 && st.history.iter().enumerate().all(|(i, o)| o.template == v * 7 + i as u64)
}

pub fn crash_child_loop(root: &Path, backend: Backend, session: &str) -> ! {
    let store = Store::new(root, backend, Durability::None);
    loop {
        let _ = store.with_exclusive(session, |st, _| {
            st.c[C_EVENTS] += 1;
            let v = st.c[C_EVENTS];
            st.history.clear();
            for i in 0..16u64 {
                st.history.push_back(crate::state::Obs { tool_use: i, template: v * 7 + i, filled: true, ..Default::default() });
            }
            st.raw_pad(v);
        });
    }
}

impl SessionState {
    /// Pad the image towards a realistic size so writes are not trivially tiny.
    pub fn raw_pad(&mut self, v: u64) {
        self.unbatched.clear();
        for i in 0..24u64 {
            self.unbatched.push(crate::state::Obs { tool_use: v + i, agent: i, template: v, outcome: 1, filled: true });
        }
    }
}

fn read_state(store: &Store, s: &str) -> Option<Option<SessionState>> {
    // Some(None) = file absent; None = present but torn
    match store.backend {
        Backend::Slots => {
            let f = match fs::File::open(store.state_path(s)) { Ok(f) => f, Err(_) => return Some(None) };
            let (st, newest) = crate::store::read_slots(&f).ok()?;
            match newest { Some(_) => Some(Some(st)), None => if f.metadata().ok()?.len() == 0 { Some(None) } else { None } }
        }
        _ => match fs::read(store.state_path(s)) {
            Ok(b) => SessionState::decode(&b).map(Some),
            Err(_) => Some(None),
        },
    }
}

pub fn crash_state(exe: &Path, dir: &Path, backend: Backend, iterations: usize, writers: usize, seed: u64) -> CrashReport {
    let root = fresh_dir(&dir.join(format!("crash-{}", backend.name())));
    let store = Store::new(&root, backend, Durability::None);
    let s = "crash";
    let mut rep = CrashReport { backend: backend.name().into(), iterations, ..Default::default() };
    let mut rng = Rng::new(seed);
    let mut last = 0u64;
    for _ in 0..iterations {
        let mut kids: Vec<Child> = (0..writers)
            .map(|_| {
                Command::new(exe)
                    .args(["crash-state-child", root.to_str().unwrap(), backend.name(), s])
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::thread::sleep(Duration::from_micros(rng.range(2_000, 12_000)));
        for k in &mut kids {
            let _ = k.kill(); // SIGKILL
            rep.kills += 1;
        }
        for k in &mut kids { let _ = k.wait(); }
        match read_state(&store, s) {
            None => { rep.torn += 1; last = 0; }
            Some(None) => {}
            Some(Some(st)) => {
                if !state_invariant(&st) {
                    rep.torn += 1;
                }
                let v = st.c[C_EVENTS];
                if v < last { rep.regressed += 1 }
                if v > last { rep.progressed += 1 }
                last = v;
            }
        }
        if backend == Backend::Rename && store.state_path(s).with_extension("state.tmp").exists() {
            rep.stale_tmp += 1;
        }
    }
    rep.final_value = last;
    // every writer was SIGKILLed while possibly holding the lock: it must be free now
    let lock_file = match backend { Backend::Slots => store.state_path(s), _ => store.lock_path(s) };
    rep.lock_free_after = fs::File::open(&lock_file).map(|f| f.try_lock().is_ok()).unwrap_or(false);
    rep
}

#[derive(Debug, Default)]
pub struct SpoolCrashReport {
    pub iterations: usize,
    pub writers_killed: usize,
    pub records: usize,
    pub corrupt_regions: usize,
    pub gaps_or_dups: usize,
    pub tail_pending: bool,
}

pub fn crash_spool(exe: &Path, dir: &Path, iterations: usize, per_iter: u32, max_size: usize, seed: u64) -> SpoolCrashReport {
    let d = fresh_dir(&dir.join("crash-spool"));
    let path = d.join("s.log");
    let mut rng = Rng::new(seed);
    let mut rep = SpoolCrashReport { iterations, ..Default::default() };
    let mut wid = 0u32;
    for _ in 0..iterations {
        let mut kids: Vec<Child> = (0..per_iter)
            .map(|_| { wid += 1; spawn_writer(exe, &path, wid, 0, max_size, rng.next_u64()) })
            .collect();
        std::thread::sleep(Duration::from_micros(rng.range(2_000, 10_000)));
        for k in &mut kids { let _ = k.kill(); rep.writers_killed += 1; }
        for k in &mut kids { let _ = k.wait(); }
    }
    let (s, _) = spool::read_from(&path, 0).unwrap();
    rep.records = s.records.len();
    rep.corrupt_regions = s.corrupt.len();
    rep.tail_pending = s.tail_pending;
    let mut next: HashMap<u32, u32> = HashMap::new();
    for (_, p) in &s.records {
        let r = Record::decode(p).unwrap();
        let n = next.entry(r.pid).or_insert(0);
        if r.eid != *n { rep.gaps_or_dups += 1 }
        *n = r.eid + 1;
    }
    rep
}

// ---------------------------------------------------------------- 4. torn-frame injection

#[derive(Debug, Default)]
pub struct InjectReport {
    pub cases: usize,
    pub ok: usize,
    pub damaged: usize,
    pub recovered: usize,
    pub false_records: usize,
    pub coincidental_repairs: usize,
}

/// Build spools with damage injected between/inside frames — truncated frames (short
/// write / power loss), zero-filled holes (delayed allocation after power loss), garbage,
/// and bit flips — and check the reader recovers exactly the intact records.
pub fn inject(seeds: std::ops::Range<u64>) -> InjectReport {
    let mut rep = InjectReport::default();
    for seed in seeds {
        let mut rng = Rng::new(seed);
        let mut buf = Vec::new();
        let mut intact = Vec::new();
        let mut written: HashMap<u32, Vec<u8>> = HashMap::new();
        let n = rng.range(20, 200) as u32;
        for i in 0..n {
            let fr = test_frame(1, i, rng.below(600) as usize, &mut rng);
            written.insert(i, fr[spool::HDR..].to_vec());
            match rng.below(12) {
                0 => { let cut = rng.range(1, fr.len() as u64 - 1) as usize; buf.extend_from_slice(&fr[..cut]); rep.damaged += 1; }
                1 => { let k = rng.range(1, 4096) as usize; buf.extend(vec![0u8; k]); buf.extend_from_slice(&fr); intact.push(i); }
                2 => { let k = rng.range(1, 64) as usize; buf.extend(rng.bytes(k)); buf.extend_from_slice(&fr); intact.push(i); }
                3 => { let mut f = fr.clone(); let k = rng.below(f.len() as u64) as usize; f[k] ^= 1 << rng.below(8); buf.extend(f); rep.damaged += 1; }
                _ => { buf.extend_from_slice(&fr); intact.push(i); }
            }
        }
        // always end with an intact frame so no damage is mistaken for an in-flight tail
        buf.extend(test_frame(1, n, 10, &mut rng));
        intact.push(n);
        let s = spool::scan(&buf, 0);
        rep.cases += 1;
        let mut got = Vec::new();
        for (_, p) in &s.records {
            let r = Record::decode(p).expect("CRC-valid frame must decode");
            if written.get(&r.eid).map(|w| w == p) != Some(true) && r.eid != n {
                rep.false_records += 1; // content that was never written
            } else if !intact.contains(&r.eid) {
                // a frame truncated by 1–2 bytes whose missing tail happened to equal the
                // following garbage byte(s): the recovered bytes are exactly what was written
                rep.coincidental_repairs += 1;
                continue;
            }
            got.push(r.eid);
        }
        rep.recovered += got.len();
        if got == intact && !s.tail_pending { rep.ok += 1 }
    }
    rep
}

// ---------------------------------------------------------------- 5. learn-style reader

#[derive(Debug, Default)]
pub struct ReaderReport {
    pub writers: u32,
    pub records: usize,
    pub polls: usize,
    pub pending_tail_seen: usize,
    pub corrupt_seen: usize,
    pub prefix_ok: bool,
}

pub fn reader_prefix(exe: &Path, dir: &Path, writers: u32, per_writer: u32, max_size: usize, seed: u64) -> ReaderReport {
    let d = fresh_dir(&dir.join(format!("reader-{seed}")));
    let path = d.join("s.log");
    fs::write(&path, b"").unwrap();
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let path = path.clone();
        let done = done.clone();
        std::thread::spawn(move || {
            let mut rep = ReaderReport::default();
            let mut cursor = 0u64;
            let mut seen: Vec<(u64, Vec<u8>)> = Vec::new();
            loop {
                let finished = done.load(Ordering::SeqCst);
                let (s, _) = spool::read_from(&path, cursor).unwrap();
                rep.polls += 1;
                rep.corrupt_seen += s.corrupt.len();
                if s.tail_pending { rep.pending_tail_seen += 1 }
                cursor = s.next;
                seen.extend(s.records);
                if finished { break; }
            }
            (rep, seen)
        })
    };
    let kids: Vec<Child> = (0..writers).map(|w| spawn_writer(exe, &path, w, per_writer, max_size, seed + w as u64)).collect();
    for mut k in kids { k.wait().unwrap(); }
    done.store(true, Ordering::SeqCst);
    let (mut rep, seen) = reader.join().unwrap();
    let (fin, _) = spool::read_from(&path, 0).unwrap();
    rep.writers = writers;
    rep.records = fin.records.len();
    rep.prefix_ok = seen == fin.records && fin.corrupt.is_empty() && fin.records.len() == (writers * per_writer) as usize;
    rep
}

// ---------------------------------------------------------------- 6. kill -9 during decide

#[derive(Debug, Default)]
pub struct DecideCrashReport {
    pub backend: String,
    pub iterations: usize,
    pub kills: usize,
    pub decisions: usize,
    pub dseq_contiguous: bool, // decision seqs 1..=N, each exactly once (lock + replay correct)
    pub replays: u64,          // decisions recovered by fold after a kill between spool append and state write
    pub state_equals_rebuild: bool,
    pub corrupt_regions: usize,
    pub order_slips: u64,
}

/// Child: an endless mini-session — 8 observations, then a PostToolBatch decision over
/// them, occasionally a prompt — on a shared session, alongside a sibling child.
pub fn crash_decide_child(root: &Path, backend: Backend, session: &str, seed: u64) -> ! {
    let store = Store::new(root, backend, Durability::None);
    let mut rng = Rng::new(seed ^ std::process::id() as u64);
    loop {
        let mut ids = Vec::new();
        for _ in 0..8 {
            let r = Record { kind: K_TOOL, tool_use: rng.next_u64() | 1, template: rng.below(40), raw: rng.bytes(64), ..Default::default() };
            ids.push(r.tool_use);
            let _ = store.observe(session, &r);
        }
        let kind = if rng.chance(0.2) { spool::K_PROMPT } else { spool::K_BATCH };
        let ev = Record { kind, ids: if kind == spool::K_BATCH { ids } else { vec![] }, aux: rng.next_u64(), ..Default::default() };
        let _ = store.decide(session, ev);
    }
}

pub fn crash_decide(exe: &Path, dir: &Path, backend: Backend, iterations: usize, seed: u64) -> DecideCrashReport {
    let root = fresh_dir(&dir.join(format!("crash-decide-{}", backend.name())));
    let store = Store::new(&root, backend, Durability::None);
    let s = "crashdecide";
    let mut rng = Rng::new(seed);
    let mut rep = DecideCrashReport { backend: backend.name().into(), iterations, ..Default::default() };
    for i in 0..iterations {
        let mut kids: Vec<Child> = (0..2)
            .map(|k| {
                Command::new(exe)
                    .args(["crash-decide-child", root.to_str().unwrap(), backend.name(), s, &(seed * 1000 + i as u64 * 2 + k).to_string()])
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap()
            })
            .collect();
        std::thread::sleep(Duration::from_micros(rng.range(2_000, 12_000)));
        for k in &mut kids { let _ = k.kill(); rep.kills += 1; }
        for k in &mut kids { let _ = k.wait(); }
    }
    // one clean decision folds everything outstanding
    let (_, _) = store.decide(s, Record { kind: spool::K_STOP, ..Default::default() }).unwrap();
    let (recs, scan) = spool::read_all(&store.spool_path(s)).unwrap();
    rep.corrupt_regions = scan.corrupt.len();
    let mut dseqs: Vec<u32> = recs.iter().filter(|r| spool::is_decision(r.kind)).map(|r| r.dseq).collect();
    rep.decisions = dseqs.len();
    dseqs.sort_unstable();
    rep.dseq_contiguous = dseqs.iter().enumerate().all(|(i, d)| *d == i as u32 + 1);
    let (live, _) = store.peek(s).unwrap();
    let mut live = live.unwrap();
    let mut rebuilt = store.rebuild(s).unwrap();
    // own decisions are applied directly and skipped on re-fold in the live path; a
    // replay after a kill is the only way the live path *applies* a decision from the spool
    rep.replays = live.c[crate::state::C_REPLAYED];
    rep.order_slips = live.c[crate::state::C_ORDER_SLIP];
    for st in [&mut live, &mut rebuilt] {
        st.seq = 0;
        st.c[crate::state::C_REPLAY_SKIP] = 0;
        st.c[crate::state::C_REPLAYED] = 0;
    }
    if live != rebuilt {
        eprintln!("live   {:?}\nrebuilt {:?}", live, rebuilt);
    }
    rep.state_equals_rebuild = live == rebuilt;
    rep
}
