//! Scripted sessions for the id shapes of `evals/gen` (Track G §4.4), without the generator.
#![allow(dead_code)]

use adage_graph::input::{Prompt, Session, Step};

pub const SHAPES: [&str; 6] = ["prefix_dash", "hash_num", "uuid", "path_num", "branch", "mixed"];
pub const WORDS: [&str; 8] = ["audit", "batch", "cache", "login", "merge", "parse", "query", "retry"];

/// A tiny deterministic generator (no dependency, same numbers everywhere).
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n.max(1)
    }
    pub fn digits(&mut self, lo: u32, hi: u32) -> String {
        let d = lo + self.below((hi - lo + 1) as u64) as u32;
        let min = if d > 1 { 10u64.pow(d - 1) } else { 1 };
        (min + self.below(10u64.pow(d) - min)).to_string()
    }
    pub fn hex(&mut self, n: usize) -> String {
        (0..n).map(|_| char::from_digit(self.below(16) as u32, 16).unwrap()).collect()
    }
}

/// (argv form, prompt mention) of the k-th id of a shape.
pub fn id(shape: &str, rng: &mut Rng, k: usize) -> (String, String) {
    let plain = |s: String| (s.clone(), s);
    match shape {
        "prefix_dash" => plain(format!("BIL-{}", rng.digits(3, 4))),
        "hash_num" => {
            let n = rng.digits(3, 4);
            (n.clone(), format!("#{n}"))
        }
        "uuid" => plain(format!("{}-{}-4{}-a{}-{}", rng.hex(8), rng.hex(4), rng.hex(3), rng.hex(3), rng.hex(12))),
        "path_num" => plain(format!("ops/{}", rng.digits(2, 3))),
        "branch" => {
            let w = WORDS[rng.below(WORDS.len() as u64) as usize];
            plain(format!("fix/{w}-{}", rng.digits(2, 3)))
        }
        "mixed" if k.is_multiple_of(2) => plain(format!("BIL-{}", rng.digits(3, 4))),
        "mixed" => {
            let n = rng.digits(3, 3);
            (n.clone(), format!("#{n}"))
        }
        _ => panic!("shape {shape}"),
    }
}

fn exec(argv: &[&str], output: &str) -> Step {
    Step::Exec { argv: argv.iter().map(|s| s.to_string()).collect(), output: output.into(), failed: false }
}

/// One scripted session: the agent explores, looks the item up, reads what it lists, edits, and
/// runs the checks.
pub fn session(sid: &str, value: &str, mention: &str, variant: usize) -> Session {
    let prompts = ["Please work on {}.", "Pick up {} and get it done.", "Can you take care of {}?"];
    let text = prompts[variant % prompts.len()].replace("{}", mention);
    let mut steps = vec![exec(&["ls"], "Makefile\nREADME.md\nscripts\ntally\ntests")];
    steps.push(Step::Read { path: "README.md".into() });
    let mut look = vec!["./scripts/wi-lookup", value];
    if variant % 5 == 4 {
        look.push("--json");
    }
    steps.push(exec(&look, &format!("{value}  Rounding is off\nstatus: open\n\nFiles:\n  tally/invoices.py")));
    steps.push(Step::Read { path: "tally/invoices.py".into() });
    steps.push(Step::Edit { path: "tally/invoices.py".into() });
    steps.push(exec(&["make", "check"], "ok"));
    Session { id: sid.into(), prompts: vec![Prompt { text, at_step: 0 }], steps }
}

/// `n` distinct ids of a shape, each worked `repeats` times, as sessions.
pub fn sessions(shape: &str, seed: u64, n: usize, repeats: usize) -> (Vec<Session>, Vec<String>) {
    let mut rng = Rng(seed);
    let mut ids: Vec<(String, String)> = Vec::new();
    while ids.len() < n {
        let x = id(shape, &mut rng, ids.len());
        if !ids.iter().any(|(v, _)| *v == x.0) {
            ids.push(x);
        }
    }
    let mut out = Vec::new();
    for r in 0..repeats {
        for (i, (v, m)) in ids.iter().enumerate() {
            out.push(session(&format!("{shape}-{r}-{i}"), v, m, i + r));
        }
    }
    (out, ids.into_iter().map(|(v, _)| v).collect())
}
