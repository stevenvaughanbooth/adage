//! Append-only per-session spool: `spool/<session>.log`.
//!
//! Frame  = MAGIC(4) | payload_len u32 LE | crc32(len_le ++ payload) u32 LE | payload
//! Every frame is emitted with exactly one `write(2)` on an `O_APPEND` descriptor.
//! Readers resynchronise on MAGIC + CRC, so a torn frame (power loss, ENOSPC, short
//! write) costs that one record and never the ones after it.

use crate::util::{crc32_update, R, W};
use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub const MAGIC: [u8; 4] = [0xAD, 0xA6, 0xE5, 0x01];
pub const HDR: usize = 12;
pub const MAX_FRAME: usize = 1 << 20;
pub static START: std::sync::OnceLock<std::time::Instant> = std::sync::OnceLock::new();
/// Diagnostics only (S3_TRACE_SLOW=1): print the syscall split of appends slower than 10 ms.
pub static TRACE_SLOW: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

// Record kinds. Decisions are taken synchronously under the session lock and get a
// per-session decision sequence number (dseq); observations are lock-free appends.
pub const K_SESSION_START: u8 = 1; // decision (nonce announce, pre-warm, arm)
pub const K_PROMPT: u8 = 2; // decision: UserPromptSubmit
pub const K_TOOL: u8 = 3; // observation: PostToolUse (async)
pub const K_BATCH: u8 = 4; // decision: PostToolBatch (carries the ordered tool_use ids)
pub const K_SUB_START: u8 = 5; // decision: SubagentStart
pub const K_SUB_STOP: u8 = 6; // observation: SubagentStop
pub const K_STOP: u8 = 7; // decision: Stop
pub const K_SESSION_END: u8 = 8; // observation: SessionEnd
pub const K_AGENT_PRE: u8 = 9; // decision: PreToolUse(Agent) — captures delegation description

pub fn is_decision(kind: u8) -> bool {
    matches!(kind, K_SESSION_START | K_PROMPT | K_BATCH | K_SUB_START | K_STOP | K_AGENT_PRE)
}
pub fn kind_name(kind: u8) -> &'static str {
    match kind {
        K_SESSION_START => "SessionStart",
        K_PROMPT => "UserPromptSubmit",
        K_TOOL => "PostToolUse",
        K_BATCH => "PostToolBatch",
        K_SUB_START => "SubagentStart",
        K_SUB_STOP => "SubagentStop",
        K_STOP => "Stop",
        K_SESSION_END => "SessionEnd",
        K_AGENT_PRE => "PreToolUse(Agent)",
        _ => "?",
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Record {
    pub kind: u8,
    pub outcome: u8,
    pub agent: u64,    // hashed agent id (0 = main agent)
    pub tool_use: u64, // hashed tool_use_id (tool events, Agent pre-hook)
    pub ts_ns: u64,    // wall clock at the hook — informational only, never used to order
    pub pid: u32,
    pub eid: u32,      // instrumentation: script-assigned event id (tests / reconstruction check)
    pub template: u64, // canonical template hash (tool events) / agent type (SubagentStart)
    pub dseq: u32,     // decision sequence (decisions only; 0 for observations)
    pub directive: u64, // route id emitted (0 = none)
    pub aux: u64,      // prompt hash / root lineage / parent agent / description hash
    pub ids: Vec<u64>, // PostToolBatch: ordered tool_use ids of the batch
    pub raw: Vec<u8>,  // raw text head (prompt, command, output/error head) — 14-day retention
}

impl Record {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W::default();
        w.u8(self.kind);
        w.u8(self.outcome);
        w.u64(self.agent);
        w.u64(self.tool_use);
        w.u64(self.ts_ns);
        w.u32(self.pid);
        w.u32(self.eid);
        w.u64(self.template);
        w.u32(self.dseq);
        w.u64(self.directive);
        w.u64(self.aux);
        w.u16(self.ids.len() as u16);
        for id in &self.ids {
            w.u64(*id);
        }
        w.bytes(&self.raw);
        w.0
    }
    pub fn decode(b: &[u8]) -> Option<Record> {
        let mut r = R::new(b);
        let mut rec = Record {
            kind: r.u8()?,
            outcome: r.u8()?,
            agent: r.u64()?,
            tool_use: r.u64()?,
            ts_ns: r.u64()?,
            pid: r.u32()?,
            eid: r.u32()?,
            template: r.u64()?,
            dseq: r.u32()?,
            directive: r.u64()?,
            aux: r.u64()?,
            ..Default::default()
        };
        let n = r.u16()? as usize;
        for _ in 0..n {
            rec.ids.push(r.u64()?);
        }
        rec.raw = r.bytes()?;
        if !r.done() {
            return None;
        }
        Some(rec)
    }
    /// Size the same record would take as a JSON line (for the disk comparison).
    pub fn json_len(&self) -> usize {
        let ids: Vec<String> = self.ids.iter().map(|i| format!("\"toolu_{i:016x}\"")).collect();
        let raw = String::from_utf8_lossy(&self.raw).replace('\\', "\\\\").replace('"', "\\\"");
        format!(
            "{{\"v\":1,\"kind\":\"{}\",\"outcome\":{},\"agent_id\":\"{:016x}\",\"tool_use_id\":\"toolu_{:016x}\",\"ts\":{},\"pid\":{},\"template\":\"{:016x}\",\"dseq\":{},\"directive\":\"{:016x}\",\"aux\":\"{:016x}\",\"ids\":[{}],\"raw\":\"{}\"}}\n",
            kind_name(self.kind), self.outcome, self.agent, self.tool_use, self.ts_ns, self.pid,
            self.template, self.dseq, self.directive, self.aux, ids.join(","), raw
        )
        .len()
    }
}

pub fn frame(payload: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(HDR + payload.len());
    v.extend_from_slice(&MAGIC);
    let len = (payload.len() as u32).to_le_bytes();
    v.extend_from_slice(&len);
    let c = !crc32_update(crc32_update(!0, &len), payload);
    v.extend_from_slice(&c.to_le_bytes());
    v.extend_from_slice(payload);
    v
}

pub fn spool_path(root: &Path, session: &str) -> std::path::PathBuf {
    root.join("spool").join(format!("{session}.log"))
}

/// Outcome of a single-`write(2)` append.
#[derive(Debug, PartialEq)]
pub enum Append {
    Ok,
    Short(usize), // the kernel wrote fewer bytes than asked — the frame is torn
}

/// One hook-process append: open(O_APPEND|O_CREAT) → one write → close. No lock.
pub fn append_frame(path: &Path, frame: &[u8]) -> io::Result<Append> {
    Ok(append_frame_pos(path, frame)?.0)
}

/// As `append_frame`, also returning the file offset just past our frame (after a write
/// on an O_APPEND descriptor the offset is the end of *our* data, whatever others did).
pub fn append_frame_pos(path: &Path, frame: &[u8]) -> io::Result<(Append, u64)> {
    let t0 = std::time::Instant::now();
    let mut f = match OpenOptions::new().append(true).create(true).open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            std::fs::create_dir_all(path.parent().unwrap())?;
            OpenOptions::new().append(true).create(true).open(path)?
        }
        Err(e) => return Err(e),
    };
    let t1 = t0.elapsed();
    let r = append_on(&mut f, frame)?;
    let end = f.stream_position()?;
    let t2 = t0.elapsed();
    drop(f);
    let t3 = t0.elapsed();
    if TRACE_SLOW.load(std::sync::atomic::Ordering::Relaxed) && t3.as_millis() >= 10 {
        eprintln!("slow-append at={:?} open={:?} write={:?} close={:?} path={}", START.get_or_init(std::time::Instant::now).elapsed(), t1, t2 - t1, t3 - t2, path.display());
    }
    Ok((r, end))
}

pub fn append_on(f: &mut File, frame: &[u8]) -> io::Result<Append> {
    // Deliberately *not* write_all: a retry loop would let another writer's frame land
    // between our two halves. One syscall; a short write is reported, and the reader
    // resynchronises past it.
    loop {
        match f.write(frame) {
            Ok(n) if n == frame.len() => return Ok(Append::Ok),
            Ok(n) => return Ok(Append::Short(n)),
            Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        }
    }
}

#[derive(Debug, Default)]
pub struct Scan {
    pub records: Vec<(u64, Vec<u8>)>, // (file offset, payload)
    pub next: u64,                    // cursor: first byte not consumed
    pub corrupt: Vec<(u64, u64)>,     // skipped byte ranges [from, to)
    pub tail_pending: bool,           // stopped at an incomplete/invalid tail with nothing valid after it
}

fn valid_at(buf: &[u8], pos: usize) -> Option<usize> {
    if buf.len() < pos + HDR || buf[pos..pos + 4] != MAGIC {
        return None;
    }
    let len = u32::from_le_bytes(buf[pos + 4..pos + 8].try_into().unwrap()) as usize;
    if len > MAX_FRAME || buf.len() < pos + HDR + len {
        return None;
    }
    let want = u32::from_le_bytes(buf[pos + 8..pos + 12].try_into().unwrap());
    let got = !crc32_update(crc32_update(!0, &buf[pos + 4..pos + 8]), &buf[pos + HDR..pos + HDR + len]);
    (want == got).then_some(HDR + len)
}

fn next_valid(buf: &[u8], from: usize) -> Option<usize> {
    let mut i = from;
    while i + HDR <= buf.len() {
        match buf[i..].windows(4).position(|w| w == MAGIC) {
            None => return None,
            Some(off) => {
                let p = i + off;
                if valid_at(buf, p).is_some() {
                    return Some(p);
                }
                i = p + 1;
            }
        }
    }
    None
}

/// Scan `buf` (the spool bytes starting at file offset `base`).
/// * valid frame → consumed
/// * invalid bytes followed somewhere by a valid frame → skipped and reported as corrupt
/// * invalid/incomplete bytes with nothing valid after → stop; the cursor stays there
///   (it may be a write still in flight — a later scan will retry)
pub fn scan(buf: &[u8], base: u64) -> Scan {
    let mut s = Scan::default();
    let mut pos = 0usize;
    while pos < buf.len() {
        if let Some(n) = valid_at(buf, pos) {
            s.records.push((base + pos as u64, buf[pos + HDR..pos + n].to_vec()));
            pos += n;
            continue;
        }
        match next_valid(buf, pos + 1) {
            Some(q) => {
                s.corrupt.push((base + pos as u64, base + q as u64));
                pos = q;
            }
            None => {
                s.tail_pending = true;
                break;
            }
        }
    }
    s.next = base + pos as u64;
    s
}

/// Read a spool from `cursor` to EOF and scan it.
pub fn read_from(path: &Path, cursor: u64) -> io::Result<(Scan, usize)> {
    let mut f = match File::open(path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok((Scan { next: cursor, ..Default::default() }, 0)),
        Err(e) => return Err(e),
    };
    f.seek(SeekFrom::Start(cursor))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let n = buf.len();
    Ok((scan(&buf, cursor), n))
}

pub fn read_all(path: &Path) -> io::Result<(Vec<Record>, Scan)> {
    let (s, _) = read_from(path, 0)?;
    let recs = s.records.iter().filter_map(|(_, p)| Record::decode(p)).collect();
    Ok((recs, s))
}

/// Deterministic reconstruction of a session's canonical event order from the *set* of
/// spool records — arrival order is ignored entirely.
///
/// 1. Decisions are totally ordered by `dseq` (assigned under the session lock).
/// 2. Each PostToolBatch decision is preceded by its tool observations, in the order of
///    the batch's `ids` list (the harness's order, not arrival).
/// 3. SubagentStop(agent) follows that agent's last PostToolBatch (or its SubagentStart).
/// 4. Leftovers (tool events never claimed by a batch, e.g. a session killed mid-batch)
///    go at the end, sorted by (agent, tool_use); SessionEnd is last.
///
/// Returns (ordered eids, duplicates dropped, missing batch members).
pub fn reconstruct(recs: &[Record]) -> (Vec<u32>, usize, usize) {
    let mut dup = 0;
    let mut decisions: Vec<&Record> = Vec::new();
    let mut seen_dseq = HashSet::new();
    let mut tools: HashMap<(u64, u64), &Record> = HashMap::new();
    let mut substops: HashMap<u64, &Record> = HashMap::new();
    let mut ends: Vec<&Record> = Vec::new();
    for r in recs {
        if is_decision(r.kind) {
            if seen_dseq.insert(r.dseq) { decisions.push(r) } else { dup += 1 }
        } else if r.kind == K_TOOL {
            if tools.insert((r.agent, r.tool_use), r).is_some() { dup += 1 }
        } else if r.kind == K_SUB_STOP {
            if substops.insert(r.agent, r).is_some() { dup += 1 }
        } else if r.kind == K_SESSION_END {
            ends.push(r);
        }
    }
    decisions.sort_by_key(|r| r.dseq);
    // anchor for SubagentStop: dseq of the agent's last batch, else its SubagentStart
    let mut anchor: HashMap<u64, u32> = HashMap::new();
    for d in &decisions {
        if (d.kind == K_BATCH || d.kind == K_SUB_START) && d.agent != 0 {
            anchor.insert(d.agent, d.dseq);
        }
    }
    let mut out = Vec::with_capacity(recs.len());
    let mut missing = 0;
    for d in &decisions {
        if d.kind == K_BATCH {
            for id in &d.ids {
                match tools.remove(&(d.agent, *id)) {
                    Some(t) => out.push(t.eid),
                    None => missing += 1,
                }
            }
        }
        out.push(d.eid);
        if d.agent != 0 && anchor.get(&d.agent) == Some(&d.dseq) {
            if let Some(s) = substops.remove(&d.agent) {
                out.push(s.eid);
            }
        }
    }
    let mut rest: Vec<&Record> = tools.into_values().collect();
    rest.sort_by_key(|r| (r.agent, r.tool_use));
    out.extend(rest.iter().map(|r| r.eid));
    let mut rest: Vec<&Record> = substops.into_values().collect();
    rest.sort_by_key(|r| r.agent);
    out.extend(rest.iter().map(|r| r.eid));
    ends.sort_by_key(|r| r.eid);
    let n_ends = ends.len();
    ends.dedup_by_key(|r| r.eid);
    dup += n_ends - ends.len();
    out.extend(ends.iter().map(|r| r.eid));
    (out, dup, missing)
}
