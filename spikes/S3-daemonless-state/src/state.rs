//! Session state (Architecture §4.4) and the deterministic fold that maintains it.
//!
//! The state is a *cache*: it is exactly `fold(spool records up to spool_cursor)`.
//! Losing it costs a refold of one session's spool, never correctness.

use crate::spool::*;
use crate::util::{crc32, hash64, R, W};
use std::collections::VecDeque;

pub const RING: usize = 16;
pub const UNBATCHED_CAP: usize = 64;
pub const DIRECTIVE_CAP: u16 = 8;
pub const STATE_MAGIC: [u8; 4] = *b"ADS1";
pub const STATE_HDR: usize = 12; // magic, len u32, crc u32

// counter indices
pub const C_EVENTS: usize = 0;
pub const C_TOOL: usize = 1;
pub const C_LATE: usize = 2; // tool event folded after its batch decision
pub const C_DECISIONS: usize = 3;
pub const C_DIRECTIVES: usize = 4;
pub const C_FOLLOWED: usize = 5;
pub const C_DERAILED: usize = 6;
pub const C_EXPIRED: usize = 7;
pub const C_DROPPED: usize = 8; // unbatched overflow
pub const C_REPLAY_SKIP: usize = 9; // decision record already applied (normal after own append)
pub const C_CORRUPT: usize = 10; // corrupt spool regions skipped
pub const C_DUP: usize = 11; // duplicate observations ignored
pub const C_STOP_REQ: usize = 12;
pub const C_REPLAYED: usize = 13; // decision applied by a fold, not by its own decide (crash recovery / rebuild)
pub const C_ORDER_SLIP: usize = 14; // in-flight bytes before our decision record: applied out of file order
pub const NC: usize = 15;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obs {
    pub tool_use: u64,
    pub agent: u64,
    pub template: u64,
    pub outcome: u8,
    pub filled: bool, // false = batch decided before this async observation arrived
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AgentNode {
    pub id: u64,
    pub parent: u64,
    pub agent_type: u64,
    pub desc: u64, // delegation description hash, joined from PreToolUse(Agent)
    pub depth: u8,
    pub active: bool,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pending {
    pub route: u64,
    pub expect: u64,
    pub batch: u32,
}
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Obligation {
    pub id: u64,
    pub met: bool,
    pub stop_requested: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct SessionState {
    pub seq: u64,          // write generation of this file
    pub spool_cursor: u64, // spool bytes folded so far
    pub decision_seq: u32,
    pub turn: u32,
    pub batch: u32,
    pub harness: u8,
    pub arm: u8, // 0 unknown, 1 holdout, 2 treated — hashed on the root lineage (F7)
    pub lineage: u64,
    pub repo: u64,
    pub intent: Option<(u32, f32)>,
    pub agents: Vec<AgentNode>,
    pub delegations: VecDeque<u64>,
    pub history: VecDeque<Obs>,
    pub unbatched: Vec<Obs>,
    pub pending: Vec<Pending>,
    pub obligations: Vec<Obligation>,
    pub directives_emitted: u16,
    pub ended: bool,
    pub last_seen_ns: u64,
    pub c: [u64; NC],
}

fn put_obs(w: &mut W, o: &Obs) {
    w.u64(o.tool_use);
    w.u64(o.agent);
    w.u64(o.template);
    w.u8(o.outcome);
    w.bool(o.filled);
}
fn get_obs(r: &mut R) -> Option<Obs> {
    Some(Obs { tool_use: r.u64()?, agent: r.u64()?, template: r.u64()?, outcome: r.u8()?, filled: r.bool()? })
}

impl SessionState {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = W::default();
        w.u64(self.seq);
        w.u64(self.spool_cursor);
        w.u32(self.decision_seq);
        w.u32(self.turn);
        w.u32(self.batch);
        w.u8(self.harness);
        w.u8(self.arm);
        w.u64(self.lineage);
        w.u64(self.repo);
        match self.intent {
            Some((f, s)) => { w.u8(1); w.u32(f); w.f32(s); }
            None => w.u8(0),
        }
        w.u16(self.agents.len() as u16);
        for a in &self.agents {
            w.u64(a.id); w.u64(a.parent); w.u64(a.agent_type); w.u64(a.desc); w.u8(a.depth); w.bool(a.active);
        }
        w.u16(self.delegations.len() as u16);
        for d in &self.delegations { w.u64(*d); }
        w.u16(self.history.len() as u16);
        for o in &self.history { put_obs(&mut w, o); }
        w.u16(self.unbatched.len() as u16);
        for o in &self.unbatched { put_obs(&mut w, o); }
        w.u16(self.pending.len() as u16);
        for p in &self.pending { w.u64(p.route); w.u64(p.expect); w.u32(p.batch); }
        w.u16(self.obligations.len() as u16);
        for o in &self.obligations { w.u64(o.id); w.bool(o.met); w.bool(o.stop_requested); }
        w.u16(self.directives_emitted);
        w.bool(self.ended);
        w.u64(self.last_seen_ns);
        for v in self.c { w.u64(v); }
        let payload = w.0;
        let mut out = Vec::with_capacity(STATE_HDR + payload.len());
        out.extend_from_slice(&STATE_MAGIC);
        out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        out.extend_from_slice(&crc32(&payload).to_le_bytes());
        out.extend_from_slice(&payload);
        out
    }

    /// Decode a state image. Returns None on bad magic, length, or CRC — i.e. a torn image
    /// is *detected*, never silently used.
    pub fn decode(b: &[u8]) -> Option<SessionState> {
        if b.len() < STATE_HDR || b[..4] != STATE_MAGIC { return None; }
        let len = u32::from_le_bytes(b[4..8].try_into().ok()?) as usize;
        if b.len() < STATE_HDR + len { return None; }
        let payload = &b[STATE_HDR..STATE_HDR + len];
        if crc32(payload) != u32::from_le_bytes(b[8..12].try_into().ok()?) { return None; }
        let mut r = R::new(payload);
        let mut s = SessionState {
            seq: r.u64()?, spool_cursor: r.u64()?, decision_seq: r.u32()?, turn: r.u32()?, batch: r.u32()?,
            harness: r.u8()?, arm: r.u8()?, lineage: r.u64()?, repo: r.u64()?,
            ..Default::default()
        };
        if r.u8()? == 1 { s.intent = Some((r.u32()?, r.f32()?)); }
        for _ in 0..r.u16()? {
            s.agents.push(AgentNode { id: r.u64()?, parent: r.u64()?, agent_type: r.u64()?, desc: r.u64()?, depth: r.u8()?, active: r.bool()? });
        }
        for _ in 0..r.u16()? { s.delegations.push_back(r.u64()?); }
        for _ in 0..r.u16()? { s.history.push_back(get_obs(&mut r)?); }
        for _ in 0..r.u16()? { s.unbatched.push(get_obs(&mut r)?); }
        for _ in 0..r.u16()? { s.pending.push(Pending { route: r.u64()?, expect: r.u64()?, batch: r.u32()? }); }
        for _ in 0..r.u16()? { s.obligations.push(Obligation { id: r.u64()?, met: r.bool()?, stop_requested: r.bool()? }); }
        s.directives_emitted = r.u16()?;
        s.ended = r.bool()?;
        s.last_seen_ns = r.u64()?;
        for i in 0..NC { s.c[i] = r.u64()?; }
        if !r.done() { return None; }
        Some(s)
    }

    /// The part of the state that must converge to the same value regardless of the
    /// order in which async observations arrived (used by the convergence property test).
    pub fn convergent_view(&self) -> (Vec<Obs>, Vec<AgentNode>, u64, u32, u32, usize) {
        let mut hist: Vec<Obs> = self.history.iter().cloned().collect();
        for o in &mut hist { o.filled = true; } // late-filled == filled once converged
        (hist, self.agents.clone(), self.c[C_TOOL], self.turn, self.batch, self.unbatched.len())
    }
}

/// Dummy decision policy standing in for T1 backoff lookup + contracts + EV rule.
/// Reads state, returns a route id or 0. Pure function of (state, event).
pub fn policy(st: &SessionState, ev: &Record) -> u64 {
    if ev.kind == K_STOP {
        return match st.obligations.iter().find(|o| !o.met && !o.stop_requested) {
            Some(o) => 0xB000_0000_0000_0000 | o.id,
            None => 0,
        };
    }
    if st.directives_emitted >= DIRECTIVE_CAP { return 0; }
    let mut key = ev.kind as u64 ^ ((st.intent.map(|i| i.0).unwrap_or(0) as u64) << 8);
    for o in st.history.iter().rev().filter(|o| o.filled).take(3) {
        key = hash64(key ^ o.template);
    }
    if key % 5 == 0 { key | 1 } else { 0 }
}

/// Fold one record into the state. Deterministic; decisions are idempotent by dseq.
pub fn apply(st: &mut SessionState, rec: &Record) {
    if is_decision(rec.kind) {
        if rec.dseq <= st.decision_seq {
            st.c[C_REPLAY_SKIP] += 1;
            return;
        }
        st.decision_seq = rec.dseq;
        st.c[C_DECISIONS] += 1;
    }
    st.c[C_EVENTS] += 1;
    st.last_seen_ns = st.last_seen_ns.max(rec.ts_ns);
    match rec.kind {
        K_SESSION_START => {
            st.lineage = rec.aux;
            st.repo = rec.template;
            st.harness = 1;
            st.arm = 1 + (hash64(rec.aux) & 1) as u8;
        }
        K_PROMPT => {
            st.turn += 1;
            let fam = (rec.aux % 50) as u32;
            st.intent = Some((fam, 0.5 + fam as f32 / 100.0));
        }
        K_TOOL => {
            st.c[C_TOOL] += 1;
            if let Some(o) = st.history.iter_mut().find(|o| o.tool_use == rec.tool_use && o.agent == rec.agent) {
                if o.filled {
                    st.c[C_DUP] += 1;
                    st.c[C_TOOL] -= 1;
                    return;
                }
                o.template = rec.template;
                o.outcome = rec.outcome;
                o.filled = true;
                st.c[C_LATE] += 1;
            } else if st.unbatched.iter().any(|o| o.tool_use == rec.tool_use && o.agent == rec.agent) {
                st.c[C_DUP] += 1;
                st.c[C_TOOL] -= 1;
                return;
            } else {
                if st.unbatched.len() == UNBATCHED_CAP {
                    st.unbatched.remove(0);
                    st.c[C_DROPPED] += 1;
                }
                st.unbatched.push(Obs { tool_use: rec.tool_use, agent: rec.agent, template: rec.template, outcome: rec.outcome, filled: true });
            }
            // finish-line obligations: an "edit-like" template triggers, a "test-like" one meets
            if rec.template % 13 == 0 && st.obligations.len() < 8 && !st.obligations.iter().any(|o| o.id == rec.template) {
                st.obligations.push(Obligation { id: rec.template, met: false, stop_requested: false });
            } else if rec.template % 13 == 1 {
                for o in &mut st.obligations { o.met = true; }
            }
        }
        K_BATCH => {
            st.batch += 1;
            for id in &rec.ids {
                let o = match st.unbatched.iter().position(|o| o.tool_use == *id && o.agent == rec.agent) {
                    Some(i) => st.unbatched.remove(i),
                    None => Obs { tool_use: *id, agent: rec.agent, template: 0, outcome: 0, filled: false },
                };
                if st.history.len() == RING { st.history.pop_front(); }
                st.history.push_back(o);
            }
            // follow / derail judgement
            let batch = st.batch;
            let hist = &st.history;
            let mut keep = Vec::new();
            for p in st.pending.drain(..) {
                if hist.iter().any(|o| o.filled && o.template == p.expect) {
                    st.c[C_FOLLOWED] += 1;
                } else if batch.saturating_sub(p.batch) >= 2 {
                    st.c[C_DERAILED] += 1;
                } else {
                    keep.push(p);
                }
            }
            st.pending = keep;
        }
        K_AGENT_PRE => {
            if st.delegations.len() == 8 { st.delegations.pop_front(); }
            st.delegations.push_back(rec.aux);
        }
        K_SUB_START => {
            let parent_depth = st.agents.iter().find(|a| a.id == rec.aux).map(|a| a.depth).unwrap_or(0);
            let desc = st.delegations.pop_front().unwrap_or(0); // FIFO join (heuristic, see FINDINGS)
            if st.agents.len() < 32 {
                st.agents.push(AgentNode { id: rec.agent, parent: rec.aux, agent_type: rec.template, desc, depth: parent_depth + 1, active: true });
            }
        }
        K_SUB_STOP => {
            if let Some(a) = st.agents.iter_mut().find(|a| a.id == rec.agent) { a.active = false; }
        }
        K_STOP => {
            if rec.directive != 0 {
                for o in st.obligations.iter_mut().filter(|o| !o.met) { o.stop_requested = true; }
                st.c[C_STOP_REQ] += 1;
            }
            for p in st.pending.drain(..) { let _ = p; st.c[C_EXPIRED] += 1; }
        }
        K_SESSION_END => st.ended = true,
        _ => {}
    }
    if is_decision(rec.kind) && rec.directive != 0 && rec.kind != K_STOP {
        st.directives_emitted += 1;
        st.c[C_DIRECTIVES] += 1;
        st.pending.push(Pending { route: rec.directive, expect: rec.directive % 40, batch: st.batch });
    }
}
