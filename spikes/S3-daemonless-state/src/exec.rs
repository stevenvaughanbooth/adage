//! Executors: how a hook event reaches session state.

use crate::spool::Record;
use crate::store::{Store, Timing};
use crate::util::to_hex;
use crate::workload::{Exec, Sample, OP_PEEK};
use std::path::PathBuf;
use std::process::Command;
use std::time::Instant;

fn sample(op: u8, decide: bool, t: &Timing) -> Sample {
    Sample {
        op,
        decide,
        total_ns: t.total_ns,
        access_ns: t.state_access_ns(),
        lock_ns: t.lock_ns,
        fold_ns: t.fold_ns,
        folded: t.folded,
        wall_ns: 0,
    }
}

/// Daemonless, in-process: every call opens its files afresh (no cached descriptors),
/// i.e. the same syscalls a short-lived hook process makes, minus the process spawn.
pub struct FileExec(pub Store);
impl Exec for FileExec {
    fn decide(&self, s: &str, r: Record) -> Sample {
        let k = r.kind;
        let (_, t) = self.0.decide(s, r).expect("decide");
        sample(k, true, &t)
    }
    fn observe(&self, s: &str, r: &Record) -> Sample {
        let t = self.0.observe(s, r).expect("observe");
        sample(r.kind, false, &t)
    }
    fn peek(&self, s: &str) -> Option<Sample> {
        let (_, t) = self.0.peek(s).expect("peek");
        Some(sample(OP_PEEK, false, &t))
    }
}

/// Daemonless, one real process per hook event (`adage-s3 hook …`), as a harness runs it.
/// `total_ns` etc. are measured inside the child; `wall_ns` includes spawn + exit.
pub struct ProcExec {
    pub exe: PathBuf,
    pub root: PathBuf,
    pub backend: &'static str,
    pub durability: &'static str,
}
impl ProcExec {
    fn run(&self, mode: &str, s: &str, r: Option<&Record>, op: u8, decide: bool) -> Sample {
        let t0 = Instant::now();
        let out = Command::new(&self.exe)
            .arg("hook")
            .arg(&self.root)
            .arg(self.backend)
            .arg(self.durability)
            .arg(s)
            .arg(mode)
            .arg(r.map(|r| to_hex(&r.encode())).unwrap_or_else(|| "-".into()))
            .output()
            .expect("spawn hook");
        let wall = t0.elapsed().as_nanos() as u64;
        assert!(out.status.success(), "hook failed: {}", String::from_utf8_lossy(&out.stderr));
        let v: Vec<u64> = String::from_utf8_lossy(&out.stdout).split_whitespace().map(|x| x.parse().unwrap()).collect();
        Sample { op, decide, total_ns: v[0], access_ns: v[1], lock_ns: v[2], fold_ns: v[3], folded: v[4] as u32, wall_ns: wall }
    }
}
impl Exec for ProcExec {
    fn decide(&self, s: &str, r: Record) -> Sample {
        self.run("decide", s, Some(&r), r.kind, true)
    }
    fn observe(&self, s: &str, r: &Record) -> Sample {
        self.run("observe", s, Some(r), r.kind, false)
    }
    fn peek(&self, s: &str) -> Option<Sample> {
        Some(self.run("peek", s, None, OP_PEEK, false))
    }
}
