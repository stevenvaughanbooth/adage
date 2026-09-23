//! Property and correctness tests (small parameters; `adage-s3 correctness --scale full`
//! runs the same checks at scale and writes results/correctness-full.json).
//!
//! Run: cargo test --release

use s3_daemonless_state::checks::{self, gen_case};
use s3_daemonless_state::spool::{self, Record};
use s3_daemonless_state::state::SessionState;
use s3_daemonless_state::store::{Backend, Durability, Store};
use s3_daemonless_state::util::Rng;
use s3_daemonless_state::workload::script;
use std::path::{Path, PathBuf};

fn exe() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_adage-s3"))
}
fn dir(name: &str) -> PathBuf {
    checks::fresh_dir(&Path::new(env!("CARGO_MANIFEST_DIR")).join("target/test-scratch").join(name))
}

/// Property: N threads appending framed records (16 B – 65 KB) with O_APPEND never lose,
/// duplicate, reorder (per writer) or tear a record.
#[test]
fn prop_concurrent_thread_writers_never_lose_or_tear() {
    let d = dir("threads");
    for seed in 0..24 {
        let c = gen_case(seed);
        checks::spool_threads(&d, &c).unwrap_or_else(|e| panic!("seed {seed} ({} writers, max {} B): {e}", c.writers, c.max_size));
    }
}

/// Property: the same with separate *processes*, each hook-style open → write → close.
#[test]
fn prop_concurrent_process_writers_never_lose_or_tear() {
    let d = dir("procs");
    for seed in 0..6 {
        let c = gen_case(50_000 + seed);
        checks::spool_procs(&exe(), &d, &c).unwrap_or_else(|e| panic!("seed {seed}: {e}"));
    }
}

/// Property: the canonical event order is reconstructed from the spool by batch / turn /
/// tool-use id, whatever the arrival order — including observations that land after
/// their batch decision, and duplicate deliveries.
#[test]
fn prop_batch_order_reconstructed_regardless_of_arrival() {
    for (lag, dup) in [(0, 0.0), (1, 0.0), (1, 0.03), (6, 0.0)] {
        let r = checks::ordering(0..150, 10, lag, dup);
        assert_eq!(r.reconstruct_ok, r.cases, "lag {lag} dup {dup}: {r:?}");
        assert_eq!(r.dups_injected, r.dups_detected);
    }
}

/// Property: hot-path state converges to the same value regardless of arrival order, as
/// long as async observations land within one decision of their batch.
#[test]
fn prop_hot_state_converges_within_one_decision_of_lag() {
    let r = checks::ordering(0..150, 10, 1, 0.03);
    assert_eq!(r.converge_ok, r.cases, "{r:?}");
    assert!(r.late_folds > 0, "the test must exercise late arrivals");
}

/// Real concurrency end to end: 4 sessions, 8 parallel observers per batch, overlapping
/// decisions; every spool reconstructs to its script's canonical order.
#[test]
fn concurrent_sessions_reconstruct_exactly() {
    use s3_daemonless_state::exec::FileExec;
    use s3_daemonless_state::workload::{drive, DriveOpts};
    let d = dir("sessions");
    for backend in [Backend::Rename, Backend::Slots] {
        let root = checks::fresh_dir(&d.join(backend.name()));
        let exec = FileExec(Store::new(&root, backend, Durability::None));
        let scripts: Vec<_> = (0..4).map(|i| script(99, i, 6)).collect();
        let opts = DriveOpts { overlap: true, max_lag_us: 200, peek_per_turn: true, pace_us: 0 };
        std::thread::scope(|sc| {
            for (i, s) in scripts.iter().enumerate() {
                let (exec, opts) = (&exec, &opts);
                sc.spawn(move || drive(exec, s, i as u64, opts));
            }
        });
        for s in &scripts {
            let (recs, scan) = spool::read_all(&spool::spool_path(&root, &s.session)).unwrap();
            assert!(scan.corrupt.is_empty());
            let (order, dup, missing) = spool::reconstruct(&recs);
            assert_eq!((dup, missing), (0, 0));
            assert_eq!(order, s.canonical, "{} {}", backend.name(), s.session);
            // state file == rebuild from spool (modulo write generation / own-record skips)
            let store = Store::new(&root, backend, Durability::None);
            let (_, _) = store.decide(&s.session, Record { kind: spool::K_STOP, ..Default::default() }).unwrap();
            let mut live = store.peek(&s.session).unwrap().0.unwrap();
            let mut rebuilt = store.rebuild(&s.session).unwrap();
            for st in [&mut live, &mut rebuilt] {
                st.seq = 0;
                st.c[s3_daemonless_state::state::C_REPLAY_SKIP] = 0;
                st.c[s3_daemonless_state::state::C_REPLAYED] = 0;
            }
            assert_eq!(live, rebuilt);
        }
    }
}

/// kill -9 of state writers mid-write: the state is always the old or the new image.
#[test]
fn crash_state_is_old_or_new_never_torn() {
    let d = dir("crash-state");
    for b in [Backend::Rename, Backend::Slots] {
        let r = checks::crash_state(&exe(), &d, b, 40, 2, 3);
        assert_eq!(r.torn, 0, "{r:?}");
        assert_eq!(r.regressed, 0, "{r:?}");
        // liveness: a killed lock holder never wedges the session (flock dies with the fd)
        assert!(r.lock_free_after && r.final_value > 0, "{r:?}");
    }
}

/// Negative control: the same test catches a non-atomic in-place writer.
#[test]
fn crash_test_detects_unsafe_inplace_writer() {
    let d = dir("crash-unsafe");
    let r = checks::crash_state(&exe(), &d, Backend::UnsafeInPlace, 80, 2, 5);
    assert!(r.torn > 0, "control should tear: {r:?}");
}

/// kill -9 between the decision's spool append and its state write: the next fold
/// replays it; decision seqs stay contiguous; state == rebuild from spool.
#[test]
fn crash_during_decide_heals_by_replay() {
    let d = dir("crash-decide");
    for b in [Backend::Rename, Backend::Slots] {
        let r = checks::crash_decide(&exe(), &d, b, 30, 9);
        assert!(r.dseq_contiguous, "{r:?}");
        assert!(r.state_equals_rebuild, "{r:?}");
        assert_eq!(r.corrupt_regions, 0, "{r:?}");
    }
}

/// kill -9 of spool writers: every writer's records form a contiguous prefix.
#[test]
fn crash_spool_writers_leave_contiguous_prefixes() {
    let d = dir("crash-spool");
    let r = checks::crash_spool(&exe(), &d, 20, 4, 480, 1);
    assert_eq!((r.corrupt_regions, r.gaps_or_dups), (0, 0), "{r:?}");
    let r = checks::crash_spool(&exe(), &d.join("big"), 10, 4, 65_000, 2);
    assert_eq!((r.corrupt_regions, r.gaps_or_dups), (0, 0), "{r:?}");
}

/// Torn frames (truncation, zero holes, garbage, bit flips) cost only themselves.
#[test]
fn prop_reader_resyncs_past_injected_damage() {
    let r = checks::inject(0..300);
    assert_eq!(r.ok, r.cases, "{r:?}");
    assert_eq!(r.false_records, 0);
}

/// A torn tail is held back (could be in flight), then skipped once a valid frame follows.
#[test]
fn torn_tail_waits_then_skips() {
    let mut rng = Rng::new(1);
    let a = checks::test_frame(1, 0, 50, &mut rng);
    let b = checks::test_frame(1, 1, 50, &mut rng);
    let c = checks::test_frame(1, 2, 50, &mut rng);
    let mut buf = a.clone();
    buf.extend_from_slice(&b[..b.len() / 2]);
    let s = spool::scan(&buf, 0);
    assert_eq!(s.records.len(), 1);
    assert!(s.tail_pending);
    assert_eq!(s.next, a.len() as u64);
    buf.extend_from_slice(&c);
    let s = spool::scan(&buf[a.len()..], a.len() as u64);
    assert_eq!(s.records.len(), 1);
    assert_eq!(s.corrupt.len(), 1);
    assert!(!s.tail_pending);
}

/// A learn-style reader polling concurrently with writers only ever sees a consistent
/// prefix of the final file.
#[test]
fn concurrent_reader_sees_consistent_prefix() {
    let d = dir("reader");
    for (w, n, m) in [(8, 300, 480), (8, 30, 65_000)] {
        let r = checks::reader_prefix(&exe(), &d, w, n, m, 7);
        assert!(r.prefix_ok, "{r:?}");
        assert_eq!(r.corrupt_seen, 0);
    }
}

#[test]
fn state_image_roundtrip_and_torn_detection() {
    let sc = script(3, 0, 8);
    let (_, st, _) = checks::simulate(&sc.steps, 3, 1, 0.0);
    let img = st.encode();
    assert_eq!(SessionState::decode(&img).unwrap(), st);
    for cut in [0, 5, 12, img.len() / 2, img.len() - 1] {
        assert!(SessionState::decode(&img[..cut]).is_none());
    }
    let mut flipped = img.clone();
    flipped[img.len() / 2] ^= 0x10;
    assert!(SessionState::decode(&flipped).is_none());
}
