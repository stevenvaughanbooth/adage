//! A model of one Claude Code session's hook traffic, and a driver that replays it
//! against an executor (daemonless files, one process per hook, or a daemon).
//!
//! Per turn: UserPromptSubmit (decide) → 1–4 tool batches of 8 parallel PostToolUse
//! (async observe) each closed by PostToolBatch (decide) → sometimes a sub-agent inside a
//! batch: PreToolUse(Agent) (decide) → SubagentStart (decide) → the sub-agent's own
//! batches → SubagentStop (observe) → the Agent tool's PostToolUse → Stop (decide).
//! SessionStart (decide) opens the session, SessionEnd (observe) closes it.

use crate::spool::*;
use crate::util::{hash64, Rng};
use std::time::Duration;

pub const PARALLEL: usize = 8;
/// Diagnostic (S3_SERIAL_OBS=1): run a batch's observations inline instead of on 8 threads.
pub static SERIAL_OBS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Clone, Debug)]
pub enum Step {
    Decide(Record),
    Observe(Record),
    /// `early` observations start concurrently at batch start; `sub` runs sequentially on
    /// the driver meanwhile; then `late` observations fire; then the batch decision —
    /// *without* waiting for the async observations to land (they are async hooks).
    Batch { early: Vec<Record>, sub: Vec<Step>, late: Vec<Record>, decide: Record },
}

pub struct Script {
    pub session: String,
    pub steps: Vec<Step>,
    pub canonical: Vec<u32>, // eids in the order reconstruction must produce
    pub events: usize,
}

struct Gen {
    rng: Rng,
    eid: u32,
    canonical: Vec<u32>,
}
impl Gen {
    fn rec(&mut self, kind: u8) -> Record {
        self.eid += 1;
        Record { kind, eid: self.eid, ..Default::default() }
    }
    fn tool(&mut self, agent: u64) -> Record {
        let mut r = self.rec(K_TOOL);
        r.agent = agent;
        r.tool_use = self.rng.next_u64() | 1;
        r.template = self.rng.below(40); // small vocabulary → repetition, follows, obligations
        r.outcome = self.rng.below(3) as u8;
        let n = self.rng.below(385) as usize; // command + output/error head, 0–384 B
        r.raw = self.rng.bytes(n);
        r
    }
    /// A batch for `agent`; if `sub` is Some, one of the tools is an Agent call whose
    /// sub-agent runs inside the batch.
    fn batch(&mut self, agent: u64, with_sub: bool) -> Step {
        let mut tools: Vec<Record> = (0..PARALLEL).map(|_| self.tool(agent)).collect();
        let mut sub = Vec::new();
        let mut late = Vec::new();
        let mut sub_canon = Vec::new();
        if with_sub {
            // the Agent tool call is one of the batch's tools; its PostToolUse lands last
            let agent_call = tools.pop().unwrap();
            let mut pre = self.rec(K_AGENT_PRE);
            pre.agent = agent;
            pre.tool_use = agent_call.tool_use;
            pre.aux = self.rng.next_u64(); // delegation description hash
            pre.raw = self.rng.bytes(120);
            let child = self.rng.next_u64() | 1;
            let mut start = self.rec(K_SUB_START);
            start.agent = child;
            start.aux = agent;
            start.template = self.rng.below(5);
            sub_canon.push(pre.eid);
            sub_canon.push(start.eid);
            sub.push(Step::Decide(pre));
            sub.push(Step::Decide(start));
            let saved = std::mem::take(&mut self.canonical);
            for _ in 0..self.rng.range(1, 2) {
                let b = self.batch(child, false);
                sub.push(b);
            }
            sub_canon.extend(std::mem::replace(&mut self.canonical, saved));
            let mut stop = self.rec(K_SUB_STOP);
            stop.agent = child;
            sub_canon.push(stop.eid);
            sub.push(Step::Observe(stop));
            late.push(agent_call.clone());
            tools.push(agent_call);
        }
        // the harness's batch order (tool_use order in the assistant message)
        let mut order: Vec<usize> = (0..tools.len()).collect();
        self.rng.shuffle(&mut order);
        let ids: Vec<u64> = order.iter().map(|&i| tools[i].tool_use).collect();
        let mut decide = self.rec(K_BATCH);
        decide.agent = agent;
        decide.ids = ids;
        self.canonical.extend(sub_canon);
        self.canonical.extend(order.iter().map(|&i| tools[i].eid));
        self.canonical.push(decide.eid);
        let late_ids: Vec<u64> = late.iter().map(|r| r.tool_use).collect();
        let early = tools.into_iter().filter(|t| !late_ids.contains(&t.tool_use)).collect();
        Step::Batch { early, sub, late, decide }
    }
}

pub fn script(seed: u64, session_idx: usize, turns: usize) -> Script {
    let mut g = Gen { rng: Rng::new(seed ^ hash64(session_idx as u64)), eid: 0, canonical: Vec::new() };
    let session = format!("s{session_idx:03}-{:08x}", g.rng.next_u64() as u32);
    let mut steps = Vec::new();
    let mut start = g.rec(K_SESSION_START);
    start.aux = hash64(seed ^ session_idx as u64); // root lineage
    start.template = 42; // repo key
    g.canonical.push(start.eid);
    steps.push(Step::Decide(start));
    for _ in 0..turns {
        let mut p = g.rec(K_PROMPT);
        p.aux = g.rng.next_u64();
        let n = g.rng.range(40, 300) as usize;
        p.raw = g.rng.bytes(n);
        g.canonical.push(p.eid);
        steps.push(Step::Decide(p));
        let nb = g.rng.range(1, 4);
        let sub_at = if g.rng.chance(0.5) { Some(g.rng.below(nb)) } else { None };
        for b in 0..nb {
            let s = g.batch(0, sub_at == Some(b));
            steps.push(s);
        }
        let stop = g.rec(K_STOP);
        g.canonical.push(stop.eid);
        steps.push(Step::Decide(stop));
    }
    let end = g.rec(K_SESSION_END);
    g.canonical.push(end.eid);
    steps.push(Step::Observe(end));
    let events = g.eid as usize;
    Script { session, steps, canonical: g.canonical, events }
}

/// Flatten a script into the list of records it produces (any order-free use).
pub fn records(steps: &[Step]) -> Vec<(bool, Record)> {
    let mut out = Vec::new();
    for s in steps {
        match s {
            Step::Decide(r) => out.push((true, r.clone())),
            Step::Observe(r) => out.push((false, r.clone())),
            Step::Batch { early, sub, late, decide } => {
                out.extend(early.iter().map(|r| (false, r.clone())));
                out.extend(records(sub));
                out.extend(late.iter().map(|r| (false, r.clone())));
                out.push((true, decide.clone()));
            }
        }
    }
    out
}

// ---------------------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default)]
pub struct Sample {
    pub op: u8, // record kind; 100 = peek
    pub decide: bool,
    pub total_ns: u64,
    pub access_ns: u64, // lock + load + store (decisions)
    pub lock_ns: u64,
    pub fold_ns: u64,
    pub folded: u32,
    pub wall_ns: u64, // proc mode: including process spawn
}
pub const OP_PEEK: u8 = 100;

pub trait Exec: Sync {
    fn decide(&self, session: &str, r: Record) -> Sample;
    fn observe(&self, session: &str, r: &Record) -> Sample;
    fn peek(&self, session: &str) -> Option<Sample>;
}

pub struct DriveOpts {
    pub overlap: bool,      // fire the batch decision without waiting for async observations
    pub max_lag_us: u64,    // random delay before the batch decision
    pub peek_per_turn: bool,
    pub pace_us: u64,       // think time between top-level steps (0 = back-to-back stress)
}

fn run_steps(exec: &dyn Exec, session: &str, steps: &[Step], rng: &mut Rng, o: &DriveOpts, out: &mut Vec<Sample>) {
    for s in steps {
        if o.pace_us > 0 {
            // jittered think time, like model / tool latency between hook events
            std::thread::sleep(Duration::from_micros(o.pace_us / 2 + rng.below(o.pace_us + 1)));
        }
        match s {
            Step::Decide(r) => {
                out.push(exec.decide(session, r.clone()));
                if r.kind == K_PROMPT && o.peek_per_turn {
                    if let Some(p) = exec.peek(session) { out.push(p) }
                }
            }
            Step::Observe(r) => out.push(exec.observe(session, r)),
            Step::Batch { early, sub, late, decide } if SERIAL_OBS.load(std::sync::atomic::Ordering::Relaxed) => {
                // diagnostic mode: no thread churn — observations inline on the driver thread
                out.extend(early.iter().map(|r| exec.observe(session, r)));
                let mut sub_rng = Rng::new(rng.next_u64());
                run_steps(exec, session, sub, &mut sub_rng, o, out);
                out.extend(late.iter().map(|r| exec.observe(session, r)));
                out.push(exec.decide(session, decide.clone()));
            }
            Step::Batch { early, sub, late, decide } => {
                let lag = Duration::from_micros(rng.below(o.max_lag_us + 1));
                let seed = rng.next_u64();
                std::thread::scope(|sc| {
                    let hs: Vec<_> = early.iter().map(|r| sc.spawn(move || exec.observe(session, r))).collect();
                    let mut sub_rng = Rng::new(seed);
                    run_steps(exec, session, sub, &mut sub_rng, o, out);
                    let hl: Vec<_> = late.iter().map(|r| sc.spawn(move || exec.observe(session, r))).collect();
                    let mut hs: Vec<_> = hs.into_iter().chain(hl).collect();
                    if !o.overlap {
                        out.extend(hs.drain(..).map(|h| h.join().unwrap()));
                    }
                    std::thread::sleep(lag);
                    out.push(exec.decide(session, decide.clone()));
                    out.extend(hs.into_iter().map(|h| h.join().unwrap()));
                });
            }
        }
    }
}

pub fn drive(exec: &dyn Exec, sc: &Script, seed: u64, o: &DriveOpts) -> Vec<Sample> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::with_capacity(sc.events + 64);
    run_steps(exec, &sc.session, &sc.steps, &mut rng, o, &mut out);
    out
}
