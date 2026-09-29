//! The index's generation survives a write and a read: `adage doctor` re-hashes the parsed routes,
//! so every float in them (evidence rates, Wilson bounds) must parse back to the same value. Without
//! serde_json's `float_roundtrip`, 18/19 = 0.9473684210526315 came back one unit in the last place
//! off, and doctor reported a healthy index as edited or damaged.

use adage_core::Effect;
use adage_tracks::*;
use sha2::{Digest, Sha256};

fn track(support: u32, opportunities: u32) -> Track {
    let rate = support as f64 / opportunities as f64;
    Track {
        manifest: Manifest {
            track: TrackInfo {
                name: "acme/work".into(),
                version: "0.1.0".into(),
                description: String::new(),
                provenance: "desire-path".into(),
            },
            scope: Scope { repo: "acme".into() },
            gauge: Gauge::default(),
        },
        routes: vec![Route {
            id: "finish-gen".into(),
            summary: "gen".into(),
            trigger: Trigger::AfterEdit { paths: "schema/*.json".into() },
            step: Step::Run { command: "make gen".into() },
            slots: vec![],
            effect: Effect::Regenerate,
            strength: Strength::Fire,
            state: RouteState::Firm,
            exit: vec![],
            evidence: Evidence {
                support,
                opportunities,
                rate,
                lower_bound: wilson_lower(support, opportunities),
                provenance: "desire-path".into(),
                ..Default::default()
            },
            authored: false,
        }],
    }
}

#[test]
fn generation_survives_a_write_and_a_read() {
    let dir = tempfile::tempdir().unwrap();
    for (s, n) in [(18, 19), (7, 9), (23, 33), (1, 3), (41, 47)] {
        let index = Index::compile(&[track(s, n)]);
        let path = dir.path().join("index.json");
        index.write_atomic(&path).unwrap();
        let back = Index::load(&path);
        let body = serde_json::to_vec(&back.routes).unwrap();
        let generation = io::hex(&Sha256::digest(&body))[..16].to_string();
        assert_eq!(generation, back.generation, "{s}/{n}: generation changed across a write and read");
    }
}
