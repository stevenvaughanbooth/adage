//! Property tests: a crash at any byte never yields a corrupt read.
//!
//! Hooks write per-session spools (append-only, CRC-framed) and two-slot state files, and any
//! hook can be killed mid-write. The store's promise: a reader sees only whole records, in order,
//! and a state read returns a state that was actually written (the newest intact one) or the
//! default, never a mixture. These tests cut files at random offsets and splice partial writes to
//! check it.

use adage_store::{spool, state};
use proptest::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

fn cases() -> u32 {
    if cfg!(feature = "slow") {
        1_000
    } else {
        64
    }
}

fn records() -> impl Strategy<Value = Vec<Value>> {
    prop::collection::vec(("\\PC{0,64}", any::<u32>()).prop_map(|(s, n)| json!({ "s": s, "n": n })), 1..12)
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
struct S {
    n: u64,
    items: Vec<String>,
}

fn states() -> impl Strategy<Value = Vec<S>> {
    prop::collection::vec(
        (any::<u64>(), prop::collection::vec("\\PC{0,24}", 0..6)).prop_map(|(n, items)| S { n, items }),
        2..6,
    )
}

proptest! {
    #![proptest_config(ProptestConfig { cases: cases(), ..ProptestConfig::default() })]

    /// Cut the spool anywhere (a crash mid-append), keep appending: the reader returns every whole
    /// record before the cut and every record after it, in order, and nothing else.
    #[test]
    fn a_torn_spool_loses_only_the_torn_record(before in records(), after in records(), cut in any::<prop::sample::Index>()) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.spool");
        let mut ends = vec![];
        for r in &before {
            spool::append(&p, r).unwrap();
            ends.push(std::fs::metadata(&p).unwrap().len() as usize);
        }
        let len = std::fs::metadata(&p).unwrap().len() as usize;
        let at = cut.index(len + 1);
        let bytes = std::fs::read(&p).unwrap();
        std::fs::write(&p, &bytes[..at]).unwrap();
        for r in &after {
            spool::append(&p, r).unwrap();
        }
        let got: Vec<Value> = spool::read_all(&p).unwrap();
        let mut want: Vec<Value> = before.iter().zip(&ends).filter(|(_, e)| **e <= at).map(|(r, _)| r.clone()).collect();
        want.extend(after.iter().cloned());
        prop_assert_eq!(got, want);
    }

    /// Flip bytes anywhere: every record read back is one that was written, in order.
    #[test]
    fn a_corrupted_spool_never_invents_a_record(recs in records(), flips in prop::collection::vec((any::<prop::sample::Index>(), 1u8..=255), 1..6)) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("s.spool");
        for r in &recs {
            spool::append(&p, r).unwrap();
        }
        let mut bytes = std::fs::read(&p).unwrap();
        for (i, x) in &flips {
            let i = i.index(bytes.len());
            bytes[i] ^= x;
        }
        std::fs::write(&p, &bytes).unwrap();
        let got: Vec<Value> = spool::read_all(&p).unwrap();
        // A subsequence of what was written.
        let mut it = recs.iter();
        for g in &got {
            prop_assert!(it.any(|r| r == g), "read a record that was never written, or out of order: {}", g);
        }
    }

    /// A state write torn at any byte leaves the previous state readable; a truncated file reads
    /// as a written state or the default; and the next update works either way.
    #[test]
    fn a_torn_state_write_reads_as_a_written_state(ss in states(), cut in any::<prop::sample::Index>(), trunc in any::<prop::sample::Index>()) {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.state");
        let (last, prev) = (ss.last().unwrap().clone(), ss[ss.len() - 2].clone());
        for s in &ss[..ss.len() - 1] {
            state::update::<S, _>(&p, |x| *x = s.clone()).unwrap();
        }
        let before = std::fs::read(&p).unwrap();
        state::update::<S, _>(&p, |x| *x = last.clone()).unwrap();
        let after = std::fs::read(&p).unwrap();

        // Splice: the new write reached only part of its bytes.
        let mut torn = before.clone();
        torn.resize(after.len().max(before.len()), 0);
        if let Some(start) = (0..after.len()).find(|&i| before.get(i) != Some(&after[i])) {
            let end = start + cut.index(after.len() - start + 1);
            torn[start..end].copy_from_slice(&after[start..end]);
        }
        std::fs::write(&p, &torn).unwrap();
        let got: S = state::read(&p).unwrap();
        prop_assert!(got == prev || got == last, "torn write read as {:?}", got);

        // Truncation anywhere: some written state, or the default.
        let at = trunc.index(after.len() + 1);
        std::fs::write(&p, &after[..at]).unwrap();
        let got: S = state::read(&p).unwrap();
        prop_assert!(ss.contains(&got) || got == S::default(), "truncated state read as {:?}", got);

        // And the file is still usable.
        state::update::<S, _>(&p, |x| x.n = 7).unwrap();
        prop_assert_eq!(state::read::<S>(&p).unwrap().n, 7);
    }
}
