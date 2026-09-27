//! One learned type per id shape of `evals/gen` (Track G §4.4): the lookup's slot is found, the
//! type accepts fresh ids from the same distribution and rejects near misses. Plus the hostile
//! inputs: values carrying `;`, `$()`, spaces, a leading `-`, very long strings or Unicode never
//! yield a type that accepts them.

mod common;

use adage_core::vtype::matches;
use adage_graph::input::{Prompt, Session, Step};
use adage_graph::{learn, Contract, Learned, Params};
use common::{id, sessions, Rng, SHAPES};

fn lookup(l: &Learned) -> &adage_graph::SlotType {
    l.slots.iter().find(|s| s.slot.sink.name() == "scripts/wi-lookup #0").expect("the lookup's slot")
}

fn near_misses(shape: &str, v: &str) -> Vec<String> {
    let mut out = vec![
        format!("-{v}"),
        format!("{v};id"),
        format!("$({v})"),
        format!("{v} x"),
        format!("{v}`id`"),
        format!("{v}|sh"),
        format!("{v}x"),
        format!("{v}\u{e9}"),
    ];
    let wrong = match shape {
        "prefix_dash" => v.replacen("BIL", "BIM", 1),
        "hash_num" => format!("x{v}"),
        "uuid" => format!("g{}", &v[1..]),
        "path_num" => v.replacen("ops", "opt", 1),
        "branch" => v.replacen("fix", "fax", 1),
        _ if v.starts_with("BIL") => v.replacen("BIL", "BIM", 1),
        _ => format!("x{v}"),
    };
    out.push(wrong);
    out
}

#[test]
fn every_id_shape_learns_a_type_that_generalises() {
    for shape in SHAPES {
        let (ss, seen) = sessions(shape, 7, 12, 2);
        let l = learn(&ss, &Params::default());
        let slot = lookup(&l);
        let Contract::Pattern(t) = &slot.contract else { panic!("{shape}: no type: {:?}", slot.contract) };
        let t = &t.vtype;
        for v in &seen {
            assert!(matches(&t.runs, v), "{shape}: {} rejects its sample {v}", t.regex);
        }
        // Fresh ids from the same generator (lengths within the observed ranges by construction
        // of the seed), and near misses around each.
        let mut rng = Rng(1_000 + shape.len() as u64);
        let (mut ok, mut n) = (0, 0);
        for k in 0..200 {
            let (v, _) = id(shape, &mut rng, k);
            n += 1;
            ok += usize::from(matches(&t.runs, &v));
            for bad in near_misses(shape, &v) {
                assert!(!matches(&t.runs, &bad), "{shape}: {} accepts near miss {bad:?}", t.regex);
            }
        }
        assert!(ok as f64 / n as f64 >= 0.99, "{shape}: {} accepts {ok}/{n} fresh ids", t.regex);
        // Only the lookup is a slot in these sessions.
        assert_eq!(l.slots.len(), 1, "{shape}: {:?}", l.slots.iter().map(|s| s.slot.sink.name()).collect::<Vec<_>>());
    }
}

#[test]
fn design_examples() {
    let (ss, _) = sessions("prefix_dash", 3, 6, 1);
    let t = lookup(&learn(&ss, &Params::default())).vtype().unwrap().clone();
    assert!(t.regex.starts_with(r"^(?:BIL\-[0-9]{"), "{}", t.regex);
    let (ss, _) = sessions("uuid", 3, 6, 1);
    let t = lookup(&learn(&ss, &Params::default())).vtype().unwrap().clone();
    // Hex groups joined over the samples; 2^128 strings, allowed only because the type is rigid.
    assert_eq!(t.regex, r"^(?:[0-9a-f]{8}\-[0-9a-f]{4}\-[0-9a-f]{4}\-[0-9a-f]{4}\-[0-9a-f]{12})$");
    let (ss, _) = sessions("branch", 3, 12, 1);
    let t = lookup(&learn(&ss, &Params::default())).vtype().unwrap().clone();
    assert_eq!(t.regex, r"^(?:fix/[a-z]{5}\-[0-9]{2,3})$");
    let (ss, _) = sessions("mixed", 3, 12, 1);
    let t = lookup(&learn(&ss, &Params::default())).vtype().unwrap().clone();
    assert_eq!(t.regex, r"^(?:BIL\-[0-9]{3,4}|[0-9]{3})$");
}

fn hostile_session(sid: &str, value: &str) -> Session {
    Session {
        id: sid.into(),
        prompts: vec![Prompt { text: format!("Please work on {value} now"), at_step: 0 }],
        steps: vec![Step::Exec {
            argv: vec!["./scripts/wi-lookup".into(), value.into()],
            output: String::new(),
            failed: false,
        }],
    }
}

#[test]
fn hostile_values_never_get_an_accepting_type() {
    let long = format!("BIL-{}", "1".repeat(300));
    let hostile: Vec<String> = vec![
        "BIL-1041;rm".into(),
        "BIL-1042;id".into(),
        "$(id)".into(),
        "$(whoami)".into(),
        "BIL 1043".into(),
        "-rf".into(),
        "--exec=sh".into(),
        "-BIL-1044".into(),
        long.clone(),
        format!("BIL-{}", "2".repeat(130)),
        "BÏL-1045".into(),
        "ビル-1046".into(),
        "BIL-1047`x`".into(),
        "a|b".into(),
    ];
    // Hostile values alone, and hostile values mixed with good ones at the same sink.
    let mut ss: Vec<Session> = hostile.iter().enumerate().map(|(i, v)| hostile_session(&format!("h{i}"), v)).collect();
    let l = learn(&ss, &Params::default());
    for s in &l.slots {
        if let Some(t) = s.vtype() {
            for h in &hostile {
                assert!(!matches(&t.runs, h), "{} accepts {h:?}", t.regex);
            }
        }
    }
    for (i, good) in ["BIL-1101", "BIL-2202", "BIL-3303", "BIL-4404", "BIL-5505"].iter().enumerate() {
        ss.push(hostile_session(&format!("g{i}"), good));
    }
    let l = learn(&ss, &Params::default());
    let mut typed = 0;
    for s in &l.slots {
        if let Some(t) = s.vtype() {
            typed += 1;
            for h in &hostile {
                assert!(!matches(&t.runs, h), "{} accepts {h:?}", t.regex);
                let re = regex::Regex::new(&t.regex).unwrap();
                assert!(!re.is_match(h), "regex {} accepts {h:?}", t.regex);
            }
            assert!(!t.regex.contains(' ') && t.regex.starts_with('^') && t.regex.ends_with('$'));
        }
    }
    assert!(typed <= l.slots.len());
}
