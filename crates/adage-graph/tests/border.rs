//! Incremental mining with a negative border equals a full re-mine (Track G §9), whatever the
//! order in which sessions arrive or leave.
mod support;

use adage_graph::events::LSession;
use adage_graph::mine::border::{full, Incremental};
use adage_graph::mine::MineParams;
use adage_graph::proto::{self, ProtoParams};
use proptest::prelude::*;

fn corpus(seed: u64) -> (adage_graph::events::LabelTable, Vec<LSession>) {
    let sessions = support::synthetic_sessions(90, 12, seed);
    // A frozen epoch: every session is labelled with one table.
    let epoch = proto::build(&sessions, &ProtoParams { calibrate: false, ..Default::default() });
    let c = epoch.label(&sessions);
    (c.labels, c.sessions)
}

fn params() -> MineParams {
    MineParams { min_support: 3, max_back: 2, max_fwd: 1, border: 4, max_children: 1000, ..Default::default() }
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 12, ..ProptestConfig::default() })]

    #[test]
    fn incremental_equals_full(seed in 0u64..1000, order in prop::collection::vec(any::<u64>(), 90), start in 76usize..86) {
        let (labels, sessions) = corpus(seed);
        let mut idx: Vec<usize> = (0..sessions.len()).collect();
        idx.sort_by_key(|&i| order[i]);
        let p = params();
        let first: Vec<LSession> = idx[..start].iter().map(|&i| sessions[i].clone()).collect();
        let mut inc = Incremental::new(&labels, first, &p);
        let mut live: Vec<usize> = idx[..start].to_vec();
        for (step, &i) in idx[start..].iter().enumerate() {
            inc.add(sessions[i].clone());
            live.push(i);
            // Now and then a session leaves.
            if order[i] % 5 == 0 {
                let gone = live.remove((order[i] as usize / 5) % live.len());
                inc.remove(&sessions[gone].id);
            }
            let refs: Vec<&LSession> = live.iter().map(|&j| &sessions[j]).collect();
            prop_assert_eq!(inc.output(), full(&labels, &refs, &p), "after step {}", step);
        }
    }
}

/// The incremental path itself: sessions that change no decision (here, empty ones: a session
/// whose events are all outside the alphabet looks the same) are applied without a full mine,
/// and the result still equals a full one.
/// (On small random corpora most real additions flip a taxonomy-climb decision and re-mine;
/// `reasons` counts why.)
#[test]
fn unchanged_and_empty_sessions_update_incrementally() {
    let (labels, sessions) = corpus(5);
    let p = params();
    let mut inc = Incremental::new(&labels, sessions[..80].to_vec(), &p);
    for s in &sessions[80..] {
        inc.add(s.clone());
    }
    let (full_before, inc_before) = (inc.full_mines, inc.incremental);
    let mut empty = sessions[4].clone();
    empty.id = "zz-empty".into();
    empty.events.clear();
    empty.flows.clear();
    empty.same_art.clear();
    inc.add(empty.clone());
    let mut empty2 = empty.clone();
    empty2.id = "zz-empty-2".into();
    inc.add(empty2.clone());
    let mut all: Vec<&LSession> = sessions.iter().collect();
    all.push(&empty);
    all.push(&empty2);
    assert_eq!(inc.output(), full(&labels, &all, &p));
    inc.remove("zz-empty");
    let mut refs: Vec<&LSession> = sessions.iter().collect();
    refs.push(&empty2);
    assert_eq!(inc.output(), full(&labels, &refs, &p));
    eprintln!("full mines {}, incremental updates {}, reasons {:?}", inc.full_mines, inc.incremental, inc.reasons);
    assert_eq!(inc.full_mines, full_before, "{:?}", inc.reasons);
    assert!(inc.incremental >= inc_before + 3);
}
