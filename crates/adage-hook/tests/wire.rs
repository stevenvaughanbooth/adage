//! The wire to the Claude Code mod: every answer the binary sends it, one JSON file per answer
//! shape in `tests/wire/`, written from a `ModAnswer` by `adage_hook::answer::answer`. The mod's
//! own tests read the same files (docs/claude-mod/impl/wire.md), so the two sides cannot drift
//! apart unseen. `ADAGE_BLESS=1` rewrites the files from the cases below; review the diff like
//! code.

use adage_hook::answer::{answer, ModAnswer, Plan, PlanAdvice, PlanReceipt, PlanRewrite, ReceiptAnswer};
use adage_hook::ReceiptCard;
use adage_runtime::{PredictedCall, Prediction, PredictionKind, Substitution};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// The key a variant is read by on the wire, which every file of that shape is named after
/// (`<key>.json`, or `<key>-<case>.json`). Exhaustive: a new answer cannot go without a file.
fn key(a: &ModAnswer) -> &'static str {
    match a {
        ModAnswer::Plan { .. } => "plan",
        ModAnswer::Substitute { .. } => "substitute",
        ModAnswer::Receipt { .. } => "receipt",
        ModAnswer::Standing { .. } => "standing",
        ModAnswer::Text { .. } => "text",
        ModAnswer::Press { .. } => "press",
        ModAnswer::Predict { .. } => "predict",
        ModAnswer::Routine { .. } => "routine",
    }
}

fn substitution() -> Substitution {
    Substitution {
        route: "dead-end-pytest".into(),
        track: "acme/dead-ends".into(),
        command: "make test".into(),
        context: "adage ran make test in place of python3 -m pytest tests: in this repository it fails (\"No module named pytest\").".into(),
    }
}

fn card() -> ReceiptCard {
    ReceiptCard {
        operation: "op-1a2b3c4d".into(),
        ran_at: "2026-10-03T10:00:00Z".into(),
        ago: "4 min ago".into(),
        summary: "passed on files identical to the current ones".into(),
        tests: vec!["tests/test_rounding.py::test_half_up".into()],
        more: 2,
    }
}

const NOTE: &str = "<adage-receipt operation=\"op-1a2b3c4d\">\nAdage's run of .venv/bin/python -m pytest tests -q passed on files identical to the current ones.\n</adage-receipt>";
const RESULT: &str = "Adage answered this test run from its own run 4 min ago (op-1a2b3c4d): 3 passed.";

/// Every case, by file name.
fn cases() -> Vec<(&'static str, ModAnswer)> {
    vec![
        (
            "plan",
            ModAnswer::Plan {
                plan: Plan {
                    substitute: Some(substitution()),
                    rewrite: Some(PlanRewrite { command: ".venv/bin/python -m pytest tests -q".into(), kind: "test".into() }),
                    advice: None,
                    receipt: Some(PlanReceipt {
                        fresh: true,
                        covering: ".venv/bin/python -m pytest tests -q".into(),
                        operation: Some("op-1a2b3c4d".into()),
                        note: Some(NOTE.into()),
                        result: Some(RESULT.into()),
                    }),
                },
            },
        ),
        (
            "plan-advice",
            ModAnswer::Plan {
                plan: Plan {
                    advice: Some(PlanAdvice {
                        kind: "permissions".into(),
                        text: "Adage did not rewrite the command: your permission rules treat .venv/bin/python differently.".into(),
                    }),
                    receipt: Some(PlanReceipt {
                        fresh: false,
                        covering: ".venv/bin/python -m pytest tests -q".into(),
                        operation: Some("op-1a2b3c4d".into()),
                        note: None,
                        result: None,
                    }),
                    ..Plan::default()
                },
            },
        ),
        ("substitute", ModAnswer::Substitute { substitute: substitution() }),
        (
            "receipt",
            ModAnswer::Receipt {
                receipt: ReceiptAnswer {
                    result: Some(RESULT.into()),
                    note: NOTE.into(),
                    covering: ".venv/bin/python -m pytest tests -q".into(),
                    card: Some(card()),
                },
            },
        ),
        (
            "receipt-without-card",
            ModAnswer::Receipt {
                receipt: ReceiptAnswer {
                    result: Some(RESULT.into()),
                    note: NOTE.into(),
                    covering: ".venv/bin/python -m pytest tests -q".into(),
                    card: None,
                },
            },
        ),
        (
            "standing",
            ModAnswer::Standing {
                standing: "<adage-reflex route=\"dead-end-pytest\" track=\"acme/dead-ends\">\nIn this repository, running python3 -m pytest fails. Use this instead:\n  make test\n</adage-reflex>".into(),
                environment: "In this repository, Python runs in the project's environment, .venv/bin/python. Tests run with .venv/bin/python -m pytest.".into(),
                compact: adage_runtime::render::COMPACT_INSTRUCTION.into(),
                subagent: None,
            },
        ),
        (
            "standing-subagent",
            ModAnswer::Standing {
                standing: String::new(),
                environment: String::new(),
                compact: adage_runtime::render::COMPACT_INSTRUCTION.into(),
                subagent: Some("<adage-reflex route=\"lookup-work-item\" track=\"acme/lookups\">\nRun python3 scripts/wi-lookup W-2045 for what the work item asks.\n</adage-reflex>".into()),
            },
        ),
        (
            "text",
            ModAnswer::Text {
                text: "Run `make test` before you release.\n\nAdage verified these steps in this repository on 2026-10-03:\n- make test: works\n- make lint: stale (exit 2: no rule)".into(),
            },
        ),
        ("press", ModAnswer::Press { press: "9f".repeat(32) }),
        (
            "predict",
            ModAnswer::Predict {
                predict: Prediction {
                    kind: PredictionKind::Obligation,
                    route: "regenerate-registry".into(),
                    track: "acme/finish-lines".into(),
                    calls: vec![PredictedCall { tool: "Bash".into(), input: json!({ "command": "make gen" }) }],
                },
            },
        ),
        (
            "predict-chain",
            ModAnswer::Predict {
                predict: Prediction {
                    kind: PredictionKind::Chain,
                    route: "read-touched-files".into(),
                    track: "acme/reads".into(),
                    calls: vec![
                        PredictedCall { tool: "Read".into(), input: json!({ "file_path": "/r/acme/billing.py" }) },
                        PredictedCall { tool: "Read".into(), input: json!({ "file_path": "/r/acme/rounding.py" }) },
                    ],
                },
            },
        ),
        ("routine", ModAnswer::Routine { routine: true }),
    ]
}

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/wire")
}

/// A file's text: the answer's JSON, pretty-printed with its keys sorted, and a final newline.
fn file_text(a: &ModAnswer) -> String {
    let v: Value = serde_json::from_str(&answer(a)).unwrap();
    serde_json::to_string_pretty(&v).unwrap() + "\n"
}

/// Each answer as the binary prints it is the file of its case, and each file reads back as the
/// same answer; every answer shape has a file, and every file a case.
#[test]
fn every_answer_to_the_mod_matches_its_wire_file() {
    let bless = std::env::var("ADAGE_BLESS").is_ok_and(|v| v == "1");
    let cases = cases();
    if bless {
        std::fs::create_dir_all(dir()).unwrap();
    }
    for (name, a) in &cases {
        assert!(name.split('-').next() == Some(key(a)), "{name} is named after its key, {}", key(a));
        let path = dir().join(format!("{name}.json"));
        let want = file_text(a);
        if bless {
            std::fs::write(&path, &want).unwrap();
        }
        let got = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(got, want, "{name}: the binary's answer and its wire file differ (ADAGE_BLESS=1 rewrites it)");
        let printed = answer(a);
        assert!(!printed.contains('\n'), "{name}: one line");
        assert_eq!(serde_json::from_str::<Value>(&printed).unwrap(), serde_json::from_str::<Value>(&got).unwrap());
        assert_eq!(&serde_json::from_str::<ModAnswer>(&got).unwrap(), a, "{name} reads back as itself");
    }
    let named: Vec<String> = cases.iter().map(|(n, _)| format!("{n}.json")).collect();
    for e in std::fs::read_dir(dir()).unwrap().flatten() {
        let f = e.file_name().to_string_lossy().into_owned();
        assert!(named.contains(&f), "{f} has no case");
    }
    let keys: std::collections::BTreeSet<&str> = cases.iter().map(|(_, a)| key(a)).collect();
    assert_eq!(keys.len(), 8, "every answer shape has a file: {keys:?}");
}

/// The answer's bytes are those the answers had before they were typed: one line, keys sorted
/// (a `serde_json::Value` object), the way `json!(…).to_string()` printed them.
#[test]
fn an_answer_prints_as_its_json_did() {
    for (_, a) in cases() {
        let v = serde_json::to_value(&a).unwrap();
        assert_eq!(answer(&a), v.to_string());
    }
    assert_eq!(answer(&ModAnswer::Routine { routine: true }), r#"{"routine":true}"#);
    assert_eq!(answer(&ModAnswer::Press { press: "ab".into() }), r#"{"press":"ab"}"#);
    let s = ModAnswer::Standing { standing: "s".into(), environment: "e".into(), compact: "c".into(), subagent: None };
    assert_eq!(answer(&s), r#"{"compact":"c","environment":"e","standing":"s"}"#);
}
