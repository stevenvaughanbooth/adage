//! Digest stability, as properties: re-serialisation and line endings keep the pack's digests and
//! closure; every semantic change to a route alters its exec digest; evidence and aspect do not.

mod common;

use adage_core::spans::SpanKind;
use adage_pack::{route_exec_digest, Aspect, Level, Pack, RouteV2, TriggerV2};
use proptest::prelude::*;
use std::collections::BTreeMap;

fn sample() -> Pack {
    Pack::from_files(&common::files()).unwrap()
}

fn wic(p: &Pack) -> RouteV2 {
    p.routes.iter().find(|r| r.id == "work-item-context").unwrap().clone()
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 48, .. ProptestConfig::default() })]

    #[test]
    fn evidence_and_aspect_never_change_the_exec_digest(
        support in 0u32..10_000,
        opportunities in 0u32..10_000,
        rate in 0.0f64..=1.0,
        synthetic in 0u32..100,
        yellow in any::<bool>(),
        provenance in "[a-z]{1,12}",
    ) {
        let p = sample();
        let base = route_exec_digest(&wic(&p), &p.types);
        let mut r = wic(&p);
        let e = r.evidence.get_or_insert_with(Default::default);
        e.support = support;
        e.opportunities = opportunities;
        e.rate = rate;
        e.lower_bound = rate / 2.0;
        e.provenance = provenance;
        e.by_origin = BTreeMap::from([("synthetic".to_string(), synthetic)]);
        if yellow {
            r.aspect = Aspect::Yellow;
        }
        prop_assert_eq!(route_exec_digest(&r, &p.types), base.clone());
        // The pack stays valid and its closure is unchanged: approvals do not lapse.
        let mut q = p.clone();
        let i = q.routes.iter().position(|x| x.id == r.id).unwrap();
        q.routes[i] = r;
        q.check().unwrap();
        prop_assert_eq!(q.closure().unwrap(), p.closure().unwrap());
    }

    #[test]
    fn every_semantic_change_alters_the_exec_digest(which in 0usize..12, n in 1u32..5, word in "[a-z]{1,8}") {
        let p = sample();
        let base = route_exec_digest(&wic(&p), &p.types);
        let mut r = wic(&p);
        let mut types = p.types.clone();
        match which {
            0 => r.steps[0].run.as_mut().unwrap().argv.push(format!("--{word}")),
            1 => r.steps[0].run.as_mut().unwrap().argv[2] = format!("--{word}"),
            2 => r.trigger = TriggerV2::Prompt { span: SpanKind::Path },
            3 => r.inputs.get_mut("item").unwrap().ty = "git-rev".into(),
            4 => r.steps[0].script.as_mut().unwrap().sha256 = format!("{:064x}", n),
            5 => r.steps[0].outputs.get_mut("files").unwrap().max = Some(n),
            6 => r.exit.pop().map(|_| ()).unwrap_or(()),
            7 => r.exec.as_mut().unwrap().timeout_ms = 1500 + n,
            8 => r.exec.as_mut().unwrap().level = Level::E0,
            9 => r.steps[1].read.as_mut().unwrap().max = n,
            10 => types.get_mut("work-item-id").unwrap().pattern = Some(format!("^W-\\d{{4,{}}}$", 4 + n)),
            _ => types.get_mut("wi-state").unwrap().values.as_mut().unwrap().push(format!("S{word}")),
        }
        prop_assert_ne!(route_exec_digest(&r, &types), base);
    }

    #[test]
    fn reserialisation_and_line_endings_keep_the_digests(crlf in any::<u16>(), json in any::<u8>()) {
        let files = common::files();
        let p = Pack::from_files(&files).unwrap();
        let mut changed = BTreeMap::new();
        for (i, (k, v)) in files.iter().enumerate() {
            let mut v = v.clone();
            // Re-serialise some routes and types as JSON (a YAML subset): different bytes, same data.
            if (k.starts_with("routes/") || k.starts_with("types/")) && (json >> (i % 8)) & 1 == 1 {
                let val: serde_json::Value = serde_yaml::from_slice(&v).unwrap();
                v = serde_json::to_string_pretty(&val).unwrap().into_bytes();
            }
            if !k.starts_with("scripts/") && (crlf >> (i % 16)) & 1 == 1 {
                v = String::from_utf8(v).unwrap().replace('\n', "\r\n").into_bytes();
            }
            changed.insert(k.clone(), v);
        }
        let q = Pack::from_files(&changed).unwrap();
        prop_assert_eq!(&q, &p);
        prop_assert_eq!(q.closure().unwrap().render(), p.closure().unwrap().render());
        for r in &p.routes {
            prop_assert_eq!(q.route_exec_digest(&r.id), p.route_exec_digest(&r.id));
        }
        prop_assert_eq!(q.digest().unwrap(), p.digest().unwrap());
        if json == 0 {
            // Only line endings changed: the digest of the files as received is the same too.
            prop_assert_eq!(adage_pack::pack_digest(&changed), adage_pack::pack_digest(&files));
        }
    }
}

#[test]
fn the_exec_digest_inlines_types_and_scripts() {
    let p = sample();
    let json = adage_pack::digest::exec_json(&wic(&p), &p.types);
    assert!(json.contains("\"work-item-id\":{\"base\":\"string\""), "{json}");
    assert!(json.contains("\"repo-path-list\":{\"builtin\":\"repo-path-list\"}"), "{json}");
    assert!(json.contains(&format!("\"scripts\":{{\"scripts/wi-lookup\":\"{}\"}}", common::lookup_sha())), "{json}");
    assert!(!json.contains("evidence") && !json.contains("\"aspect\""), "{json}");
}
