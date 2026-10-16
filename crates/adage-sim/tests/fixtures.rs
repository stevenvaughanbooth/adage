//! The committed fixtures in `sim/fixtures/` are exactly what the generator produces. If this
//! fails after a deliberate change to the scenario, run `cargo run -p adage-sim` and review the diff.

use std::collections::BTreeSet;
use std::path::Path;

fn walk(dir: &Path, root: &Path, out: &mut BTreeSet<String>) {
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, root, out);
        } else {
            out.insert(p.strip_prefix(root).unwrap().to_string_lossy().replace('\\', "/"));
        }
    }
}

#[test]
fn committed_fixtures_are_current() {
    let root = adage_sim::fixtures_dir();
    let want = adage_sim::render_all();
    let mut on_disk = BTreeSet::new();
    walk(&root, &root, &mut on_disk);
    let names: BTreeSet<String> = want.iter().map(|(r, _)| r.clone()).collect();
    assert_eq!(on_disk, names, "fixture set differs; run `cargo run -p adage-sim`");
    for (rel, body) in &want {
        let got = std::fs::read_to_string(root.join(rel)).unwrap();
        assert!(got == *body, "{rel} is stale; run `cargo run -p adage-sim`");
    }
    let total: usize = want.iter().map(|(_, b)| b.len()).sum();
    assert!(total < 600_000, "fixtures should stay small: {total} bytes");
}

#[test]
fn three_harnesses_twelve_sessions_each() {
    for h in ["claude", "opencode", "codex"] {
        assert_eq!(adage_sim::fixture_files(h).len(), 12, "{h}");
    }
}
