//! `adage hook <harness> <event>`: the hook pipeline in `adage-hook`, run with no extension. The
//! same pipeline serves `adage-exec hook`, which adds E1 prefetch (Track C §3.2); with nothing
//! approved its output equals this one byte for byte (the goldens pin both).

/// Run the hook, with live learning re-executing this binary. Never returns.
pub fn main(harness: String, event: String) -> ! {
    adage_hook::main(harness, event, adage_hook::Options::default())
}

#[cfg(test)]
mod tests {
    use crate::harness;
    use adage_core::spans::SpanKind;
    use adage_core::Effect;
    use adage_runtime::render;
    use adage_runtime::MAX_DIRECTIVES_PER_DECISION;
    use adage_tracks::*;

    fn route(trigger: Trigger, step: Step, slots: Vec<Slot>, effect: Effect, aspect: Aspect) -> Route {
        Route {
            id: "r".repeat(64),
            summary: String::new(),
            trigger,
            step,
            slots,
            effect,
            aspect,
            state: RouteState::Active,
            exit: vec![Exit::IntentChanges, Exit::ExploratoryChange],
            evidence: Evidence::default(),
            authored: false,
        }
    }

    fn track(routes: Vec<Route>) -> Track {
        Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: format!("{}/{}", "a".repeat(63), "b".repeat(64)),
                    version: format!("{}.0.0", "1".repeat(196)),
                    description: String::new(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "r".into() },
                gauge: Gauge::default(),
            },
            routes,
        }
    }

    /// Design W8 says overflow is spilled to a file. It cannot happen: with the longest fields
    /// validation accepts, three directives (the most one decision gives) fit every harness's cap,
    /// so the truncation in `run` is a backstop that never fires. If a limit is raised, this fails
    /// and the spill must be built.
    #[test]
    fn the_largest_decision_fits_every_harness_cap() {
        let cmd = |n: usize| format!("m{}", "x".repeat(n - 1));
        // 160 bytes, safe_text's limit, in two-byte characters.
        let signature = "é".repeat(80);
        let slot = |name: &str, source: SlotSource, contract: Contract| Slot { name: name.into(), source, contract };
        let item = slot(
            "item",
            SlotSource::Prompt { span: SpanKind::Ticket },
            Contract::Pattern { regex: r"^[a-z0-9]{1,200}$".into() },
        );
        let files = slot("files", SlotSource::ResultField { field: "f".into() }, Contract::RepoPath);
        let routes = vec![
            route(
                Trigger::Prompt { span: SpanKind::Ticket },
                Step::Run { command: format!("{} {{item}}", cmd(193)) },
                vec![item],
                Effect::Environment,
                Aspect::Yellow,
            ),
            route(
                Trigger::AfterEdit { paths: format!("{}/*.json", "s".repeat(113)) },
                Step::Run { command: cmd(200) },
                vec![],
                Effect::Environment,
                Aspect::Yellow,
            ),
            route(
                Trigger::AfterCommand { command: "exec x".into() },
                Step::ReadBatch { slot: "files".into(), max: 10 },
                vec![files],
                Effect::Observe,
                Aspect::Yellow,
            ),
            route(
                Trigger::SessionStart,
                Step::Avoid { avoid: cmd(200), instead: format!("pytest t{}", "x".repeat(192)), signature },
                vec![],
                Effect::Verify,
                Aspect::Red,
            ),
        ];
        for r in &routes {
            io::validate(&track(vec![r.clone()])).unwrap_or_else(|e| panic!("the longest fields must be valid: {e:#}"));
        }
        // One more of anything is refused.
        let refused = |mutate: &dyn Fn(&mut Track)| {
            let mut t = track(vec![routes[3].clone()]);
            mutate(&mut t);
            assert!(io::validate(&t).is_err());
        };
        refused(&|t| t.manifest.track.name.push('b'));
        refused(&|t| t.routes[0].id.push('r'));
        refused(&|t| t.routes[0].step = Step::Avoid { avoid: cmd(201), instead: "m".into(), signature: "s".into() });
        refused(&|t| {
            t.routes[0].step = Step::Avoid { avoid: "m".into(), instead: "m".into(), signature: "é".repeat(81) }
        });
        let mut edit = track(vec![routes[1].clone()]);
        edit.routes[0].trigger = Trigger::AfterEdit { paths: format!("{}/*.json", "s".repeat(114)) };
        assert!(io::validate(&edit).is_err());
        let mut batch = track(vec![routes[2].clone()]);
        batch.routes[0].step = Step::ReadBatch { slot: "files".into(), max: 11 };
        assert!(io::validate(&batch).is_err());

        // The longest text each route kind renders, slots at their longest: a bound value is a
        // plain token of at most 200 bytes, a listed file shorter than 200 bytes.
        let t = track(vec![]);
        let longest = routes
            .iter()
            .map(|r| {
                let bound: Vec<(String, String)> = match &r.step {
                    Step::ReadBatch { max, .. } => vec![("files".into(), vec!["f".repeat(199); *max].join(", "))],
                    _ => vec![("item".into(), "i".repeat(200))],
                };
                let body = render::route_text(r, &bound).expect("renders");
                let kind = adage_tracks::shape(r);
                render::wrap_kind(&t.manifest.track.name, &t.manifest.track.version, &r.id, kind, r.aspect, &body).len()
            })
            .max()
            .unwrap();
        let decision = MAX_DIRECTIVES_PER_DECISION * longest + (MAX_DIRECTIVES_PER_DECISION - 1) * 2;
        eprintln!("longest directive {longest} bytes; largest decision {decision} bytes");
        for h in ["claude", "opencode", "codex"] {
            let cap = harness::adapter(h).unwrap().max_context();
            assert!(decision <= cap, "{h}: {decision} bytes of directives can exceed the {cap}-byte cap");
        }
    }
}
