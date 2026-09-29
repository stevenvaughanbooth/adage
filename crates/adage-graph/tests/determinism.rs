//! Learning is a pure function of the set of sessions: the same bytes whatever the input order or
//! the thread count (Track G §1.3).

mod common;

use adage_graph::{learn, Params};
use common::{sessions, Rng, SHAPES};

#[test]
fn same_bytes_for_any_order_and_thread_count() {
    let mut all = Vec::new();
    for (k, shape) in SHAPES.iter().enumerate() {
        let (mut ss, _) = sessions(shape, 11 + k as u64, 8, 2);
        // Each shape gets its own lookup tool, so every shape has a slot of its own.
        for s in &mut ss {
            for step in &mut s.steps {
                if let adage_graph::input::Step::Exec { argv, .. } = step {
                    if argv[0] == "./scripts/wi-lookup" {
                        argv[0] = format!("./scripts/lookup-{shape}");
                    }
                }
            }
        }
        all.extend(ss);
    }
    let base = serde_json::to_string(&learn(&all, &Params::default())).unwrap();
    let typed = learn(&all, &Params::default()).slots.iter().filter(|s| s.vtype().is_some()).count();
    assert_eq!(typed, SHAPES.len(), "{base}");
    let mut rng = Rng(5);
    for threads in [1, 2, 4, 7] {
        let mut shuffled = all.clone();
        // Fisher-Yates with the test's own generator.
        for i in (1..shuffled.len()).rev() {
            let j = rng.below(i as u64 + 1) as usize;
            shuffled.swap(i, j);
        }
        let p = Params { threads, ..Params::default() };
        assert_eq!(serde_json::to_string(&learn(&shuffled, &p)).unwrap(), base, "threads {threads}");
    }
}
