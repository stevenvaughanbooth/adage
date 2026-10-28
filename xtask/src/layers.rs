//! The workspace's layering (docs/architecture.md), checked on every test run.
//!
//! Each crate sits in one layer, and links only crates in strictly lower layers: kernel
//! primitives at the bottom, the canonical models above them, then the engines (ingestion,
//! learning, the live runtime), then orchestration, then the binaries. Dev-dependencies (tests)
//! are free, since cargo already refuses cycles.
//!
//! Some edges are forbidden even between allowed layers, because they would couple components
//! that must stay independent ([`FORBIDDEN`]): ingestion must not know learning or the runtime,
//! the learner and the live runtime are siblings, and the live hook must not link the learner.
//!
//! Today's violations are listed in [`EXCEPTIONS`], each with the plan that removes it. The check
//! fails on a violation not listed, and on a listed one that no longer exists, so the list only
//! ever shrinks.

/// Every workspace crate and its layer.
pub const LAYERS: &[(&str, u8)] = &[
    // 0: kernel primitives, no internal dependencies.
    ("adage-core", 0),
    ("adage-sign", 0),
    ("adage-term", 0),
    ("adage-store", 0),
    ("adage-sim", 0),
    ("xtask", 0),
    // 1: the canonical trace model; the intent model.
    ("adage-trace", 1),
    ("adage-intent", 1),
    // 2: the reflex model (tracks, the index) and the matching primitives over traces that the
    // learner and the live runtime share.
    ("adage-tracks", 2),
    // 3: engines: ingestion, the graph learner, the live runtime, instincts, the world's resolver.
    ("adage-ingest", 3),
    ("adage-graph", 3),
    ("adage-runtime", 3),
    ("adage-instincts", 3),
    ("adage-safety", 3),
    ("adage-world", 3),
    // 4: distribution: packs, and the record of how instincts were bound.
    ("adage-pack", 4),
    // 5: learning orchestration.
    ("adage-learn", 5),
    // 6: the live hook pipeline; offline evaluation.
    ("adage-hook", 6),
    ("adage-eval", 6),
    // 7: execution.
    ("adage-exec", 7),
    // 8: the application layer: the operations front ends call.
    ("adage-app", 8),
    // 9: binaries: argument parsing, rendering, the TUI and the HTTP server.
    ("adage-train", 9),
    ("adage-cli", 9),
];

/// Edges forbidden whatever the layers say: `(from, to, why)`.
pub const FORBIDDEN: &[(&str, &str, &str)] = &[
    ("adage-ingest", "adage-tracks", "ingestion produces sessions, not tracks"),
    ("adage-ingest", "adage-store", "ingestion reads traces, never Adage's own state"),
    ("adage-runtime", "adage-store", "the runtime decides; the hook does the I/O"),
    ("adage-world", "adage-store", "the resolver decides; the hook and the follow child do the I/O"),
    ("adage-learn", "adage-runtime", "the learner and the live runtime are siblings over the canonical model"),
    ("adage-hook", "adage-learn", "the live hook must not link the learner"),
    ("adage-exec", "adage-eval", "execution must not link the offline evaluation harness"),
    ("adage-exec", "adage-learn", "execution must not link the learner"),
];

/// Known violations, each with the work that removes it: `(from, to, plan)`.
pub const EXCEPTIONS: &[(&str, &str, &str)] = &[];

/// Every violation of the rules in a dependency graph `(crate, links)`.
pub fn violations<'a>(graph: impl IntoIterator<Item = (&'a str, Vec<&'a str>)>) -> Vec<String> {
    let layer = |c: &str| LAYERS.iter().find(|(n, _)| *n == c).map(|(_, l)| *l);
    let mut out = Vec::new();
    for (from, links) in graph {
        let Some(lf) = layer(from) else {
            out.push(format!("{from}: not assigned a layer (xtask/src/layers.rs)"));
            continue;
        };
        for to in links {
            let excepted = EXCEPTIONS.iter().any(|(f, t, _)| *f == from && *t == to);
            if let Some((_, _, why)) = FORBIDDEN.iter().find(|(f, t, _)| *f == from && *t == to) {
                if !excepted {
                    out.push(format!("{from} -> {to}: forbidden ({why})"));
                }
                continue;
            }
            match layer(to) {
                Some(lt) if lt < lf => {}
                Some(lt) if !excepted => {
                    out.push(format!("{from} (layer {lf}) -> {to} (layer {lt}): not a lower layer"))
                }
                _ => {}
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn the_workspace_respects_its_layers() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let members = crate::affected::members(root).unwrap();
        let graph: Vec<(&str, Vec<&str>)> =
            members.iter().map(|m| (m.name.as_str(), m.links.iter().map(String::as_str).collect())).collect();
        let v = violations(graph.clone());
        assert!(v.is_empty(), "layering violations:\n  {}", v.join("\n  "));
        // Every exception is still needed: a fixed one comes off the list.
        for (from, to, plan) in EXCEPTIONS {
            let present = graph.iter().any(|(f, links)| f == from && links.contains(to));
            assert!(present, "{from} -> {to} no longer exists: remove its exception ({plan})");
        }
        // Every crate has a layer, and every layered crate exists.
        for (name, _) in LAYERS {
            assert!(members.iter().any(|m| m.name == *name), "{name} is layered but not a workspace member");
        }
    }

    #[test]
    fn the_rules_catch_what_they_should() {
        assert!(violations([("adage-core", vec!["adage-trace"])])[0].contains("not a lower layer"));
        assert!(violations([("adage-runtime", vec!["adage-graph"])])[0].contains("not a lower layer"), "siblings");
        assert!(violations([("adage-ingest", vec!["adage-tracks"])])[0].contains("forbidden"));
        assert!(violations([("adage-new", vec![])])[0].contains("not assigned a layer"));
        assert!(violations([("adage-cli", vec!["adage-core", "adage-learn"])]).is_empty());
        assert!(violations([("adage-hook", vec!["adage-learn"])])[0].contains("forbidden"));
        assert!(violations([("adage-exec", vec!["adage-eval"])])[0].contains("forbidden"));
    }
}
