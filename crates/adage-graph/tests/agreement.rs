//! The emitted regex and the hand-written matcher (`adage_core::vtype::matches`) agree on every
//! string: random types, random strings, and strings drawn from each type's own language.

use adage_core::vtype::{matches, to_regex, Class, Run, Runs};
use proptest::prelude::*;

const CLASSES: [Class; 5] = [Class::U, Class::L, Class::D, Class::H, Class::W];
const PUNCT: [char; 6] = ['-', '/', '.', '_', ':', '@'];

fn alnum_run() -> impl Strategy<Value = Run> {
    prop_oneof![
        "[A-Za-z0-9]{1,4}".prop_map(|t| Run::Lit { text: t }),
        prop::collection::btree_set("[a-z0-9]{1,3}", 2..4).prop_map(|s| Run::Set { texts: s.into_iter().collect() }),
        (0..5usize, 1u8..5, 0u8..4).prop_map(|(c, lo, extra)| Run::Class { class: CLASSES[c], lo, hi: lo + extra }),
    ]
}

fn branch() -> impl Strategy<Value = Vec<Run>> {
    (
        alnum_run(),
        prop::collection::vec((0..PUNCT.len(), alnum_run()), 0..3),
        prop::option::of((0..PUNCT.len(), 0..5usize, 1u8..3, 0u8..2, 0u8..2, 0u8..3)),
    )
        .prop_map(|(first, rest, star)| {
            let mut b = vec![first];
            for (p, r) in rest {
                b.push(Run::Lit { text: PUNCT[p].to_string() });
                b.push(r);
            }
            if let Some((p, c, lo, extra, min, more)) = star {
                // A star goes after an alphanumeric run: after the first one.
                b.insert(
                    1,
                    Run::Star { sep: PUNCT[p], class: CLASSES[c], lo, hi: lo + extra, min, max: (min + more).max(1) },
                );
            }
            b
        })
}

fn runs() -> impl Strategy<Value = Runs> {
    prop::collection::vec(branch(), 1..4).prop_map(|branches| Runs { branches })
}

/// A member of one branch, from a seed.
fn sample(b: &[Run], mut seed: u64) -> String {
    let mut next = |n: usize| {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((seed >> 33) as usize) % n.max(1)
    };
    let class_chars = |c: Class| -> Vec<char> { (0u8..128).map(char::from).filter(|ch| c.contains(*ch)).collect() };
    let mut s = String::new();
    for r in b {
        match r {
            Run::Lit { text } => s.push_str(text),
            Run::Set { texts } => s.push_str(&texts[next(texts.len())]),
            Run::Class { class, lo, hi } => {
                let cs = class_chars(*class);
                let n = *lo as usize + next((*hi - *lo) as usize + 1);
                (0..n).for_each(|_| s.push(cs[next(cs.len())]));
            }
            Run::Star { sep, class, lo, hi, min, max } => {
                let cs = class_chars(*class);
                let k = *min as usize + next((*max - *min) as usize + 1);
                for _ in 0..k {
                    s.push(*sep);
                    let n = *lo as usize + next((*hi - *lo) as usize + 1);
                    (0..n).for_each(|_| s.push(cs[next(cs.len())]));
                }
            }
        }
    }
    s
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(if cfg!(debug_assertions) { 256 } else { 2048 }))]

    #[test]
    fn regex_and_matcher_agree_on_random_strings(t in runs(), probes in prop::collection::vec("[a-fA-Z0-9/._:@#; é-]{0,14}", 1..24)) {
        let re = regex::Regex::new(&to_regex(&t)).unwrap();
        for p in &probes {
            prop_assert_eq!(re.is_match(p), matches(&t, p), "type {} on {:?}", to_regex(&t), p);
        }
    }

    #[test]
    fn regex_and_matcher_accept_the_types_own_samples(t in runs(), seed in any::<u64>()) {
        let re = regex::Regex::new(&to_regex(&t)).unwrap();
        for (i, b) in t.branches.iter().enumerate() {
            let s = sample(b, seed.wrapping_add(i as u64));
            if s.len() <= adage_core::vtype::MAX_LEN {
                prop_assert!(re.is_match(&s), "regex {} rejects its sample {:?}", to_regex(&t), s);
                prop_assert!(matches(&t, &s), "matcher {} rejects its sample {:?}", to_regex(&t), s);
            }
            // One character more or less, or a hostile suffix: both agree.
            for probe in [format!("{s}x"), format!("-{s}"), format!("{s};id"), s.chars().skip(1).collect()] {
                prop_assert_eq!(re.is_match(&probe), matches(&t, &probe), "type {} on {:?}", to_regex(&t), probe);
            }
        }
    }
}

#[test]
fn learned_types_agree_with_their_regex_on_their_samples() {
    let vals: Vec<String> =
        ["fix/login-42", "fix/cache-retry-7", "fix/audit-913", "fix/queue-58"].iter().map(|s| s.to_string()).collect();
    let t = adage_graph::types::induce("t #0", &vals, &[]).unwrap().vtype;
    let re = regex::Regex::new(&t.regex).unwrap();
    for v in &vals {
        assert!(re.is_match(v) && matches(&t.runs, v), "{v}");
    }
}
