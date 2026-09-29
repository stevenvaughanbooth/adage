//! Manual profiling (ignored):
//! `cargo test -p adage-graph --release --test profile -- --ignored --nocapture`
//! (`DATASET=<name>` picks an `evals/datasets` entry, `BUNDLE=<dir>` a generated bundle;
//! `BUDGET=<n>` caps the node budget).
mod support;

#[test]
#[ignore]
fn profile_dataset() {
    let sessions = match std::env::var("BUNDLE") {
        Ok(b) => support::bundle(std::path::Path::new(&b)),
        Err(_) => {
            let name = std::env::var("DATASET").unwrap_or_else(|_| "demo-haiku45".into());
            let Some(s) = support::dataset(&name) else { return };
            s
        }
    };
    let n: usize = sessions.iter().map(|s| s.events.len()).sum();
    eprintln!("{} sessions, {} events", sessions.len(), n);
    let mut p = adage_graph::select::Params::demo();
    if let Ok(b) = std::env::var("BUDGET") {
        p.mine.budget = b.parse().unwrap();
    }
    let t = std::time::Instant::now();
    let l = adage_graph::select::learn(&sessions, &p);
    eprintln!(
        "{:?} {:?} targets {} exp {} frequent {} closed {} gated {} selected {}",
        t.elapsed(),
        l.timings,
        l.targets,
        l.expansions,
        l.frequent,
        l.closed,
        l.gated,
        l.reflexes.len()
    );
    for r in &l.reflexes {
        eprintln!("{} {}", r.shape, r.why);
    }
}

/// `FILTER=<substring>`: print every scored candidate whose pattern contains it, with its gate.
#[test]
#[ignore]
fn candidates() {
    let sessions = match std::env::var("BUNDLE") {
        Ok(b) => support::bundle(std::path::Path::new(&b)),
        Err(_) => {
            let name = std::env::var("DATASET").unwrap_or_else(|_| "demo-haiku45".into());
            let Some(s) = support::dataset(&name) else { return };
            s
        }
    };
    let f = std::env::var("FILTER").unwrap_or_default();
    let p = adage_graph::select::Params::demo();
    for (text, s) in adage_graph::select::candidates(&sessions, &p) {
        if text.contains(&f) {
            eprintln!(
                "{:>3} conf {:.2} ca {:.2} contrast {:.2} cons {:?} V {:.2} psoon {:.2} dt {} gate [{}] {}",
                s.support, s.conf, s.conf_anchor, s.contrast, s.consistency, s.value, s.p_soon, s.dturns, s.gate, text
            );
        }
    }
}
