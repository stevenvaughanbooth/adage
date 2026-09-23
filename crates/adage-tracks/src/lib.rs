//! The track format (Design §10): routes with typed slots and contracts, effect ceilings,
//! evidence, reading and writing track directories, validation, canonical digests, and the
//! compiled index the runtime reads.

pub mod index;
pub mod io;
pub mod model;

pub use index::{Index, IndexedRoute};
pub use model::*;

#[cfg(test)]
mod tests {
    use super::*;
    use adage_core::spans::SpanKind;
    use adage_core::Effect;

    fn sample() -> Track {
        Track {
            manifest: Manifest {
                track: TrackInfo {
                    name: "acme-billing/work-items".into(),
                    version: "0.1.0".into(),
                    description: "test".into(),
                    provenance: "desire-path".into(),
                },
                scope: Scope { repo: "acme-billing".into() },
                gauge: Gauge::default(),
            },
            routes: vec![Route {
                id: "lookup".into(),
                summary: "look up the work item".into(),
                trigger: Trigger::Prompt { span: SpanKind::Ticket },
                step: Step::Run { command: "./scripts/wi-lookup {item} --json".into() },
                slots: vec![Slot {
                    name: "item".into(),
                    source: SlotSource::Prompt { span: SpanKind::Ticket },
                    contract: Contract::Pattern { regex: r"^W-\d{4}$".into() },
                }],
                effect: Effect::Observe,
                aspect: Aspect::Green,
                state: RouteState::Active,
                exit: vec![Exit::IntentChanges],
                evidence: Evidence::default(),
                authored: false,
            }],
        }
    }

    #[test]
    fn round_trip_and_digest_is_stable() {
        let d = tempfile::tempdir().unwrap();
        let t = sample();
        io::write(d.path(), &t).unwrap();
        let back = io::read(d.path()).unwrap();
        assert_eq!(back, t);
        let a = io::digest(d.path()).unwrap();
        io::write(d.path(), &t).unwrap();
        assert_eq!(a, io::digest(d.path()).unwrap());
        std::fs::write(d.path().join("routes/lookup.yaml"), "tampered").unwrap();
        assert_ne!(a, io::digest(d.path()).unwrap());
    }

    #[test]
    fn validation_refuses_unsafe_routes() {
        let mut t = sample();
        t.routes[0].effect = Effect::Publish;
        assert!(io::validate(&t).is_err());
        let mut t = sample();
        t.routes[0].step = Step::Run { command: "./x {nope}".into() };
        assert!(io::validate(&t).is_err());
        let mut t = sample();
        t.routes[0].slots[0].contract = Contract::Pattern { regex: "W-.*".into() };
        assert!(io::validate(&t).is_err());
        let mut t = sample();
        t.routes[0].effect = Effect::Environment;
        assert!(io::validate(&t).is_err(), "environment steps may not be green");
    }

    #[test]
    fn index_is_deterministic() {
        let a = Index::compile(&[sample()]);
        let b = Index::compile(&[sample()]);
        assert_eq!(serde_json::to_string(&a).unwrap(), serde_json::to_string(&b).unwrap());
        assert_eq!(a.for_repo("acme-billing").count(), 1);
    }
}
