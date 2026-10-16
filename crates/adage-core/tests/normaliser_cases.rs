//! Table-driven normaliser cases. They live in `cases/normaliser.json` — synthetic commands only, per
//! spike privacy rules — and assert the *shape* of the canonical action (op, effect, mutating,
//! and a confidence floor), not exact confidence values, so the suite doesn't become brittle to
//! reasonable scoring tweaks.
//!
//! Run: `cargo test -p adage-core --test normaliser_cases`

use serde::Deserialize;
use serde_json::Value;

use adage_core::shell::normalize_shell;
use adage_core::table::CommandTable;
use adage_core::types::{CanonicalAction, Normalized};

#[derive(Debug, Deserialize)]
pub struct Fixture {
    pub id: String,
    pub cmd: String,
    #[serde(default = "default_cwd")]
    pub cwd: String,
    pub expect: Expect,
}

fn default_cwd() -> String {
    "/repo".to_string()
}

#[derive(Debug, Deserialize)]
pub struct Expect {
    /// Number of top-level actions expected (after flattening compounds is NOT done — a loop or
    /// if/case collapses to exactly one Compound action at the top level).
    pub count: Option<usize>,
    /// op of the first action: read/search/list/exec/edit/write/fetch/unknown/compound
    pub op: Option<String>,
    pub effect: Option<String>,
    pub mutating: Option<bool>,
    pub path: Option<String>,
    pub pattern: Option<String>,
    pub url: Option<String>,
    pub min_confidence: Option<f64>,
    pub max_confidence: Option<f64>,
    pub compound_kind: Option<String>,
    pub argv: Option<Vec<String>>,
}

fn op_name(a: &CanonicalAction) -> &'static str {
    adage_core::keys::op_name(a)
}

#[allow(dead_code)]
fn op_name_unused(a: &CanonicalAction) -> &'static str {
    match a {
        CanonicalAction::Control { .. } => "control",
        CanonicalAction::Read { .. } => "read",
        CanonicalAction::Search { .. } => "search",
        CanonicalAction::List { .. } => "list",
        CanonicalAction::Exec { .. } => "exec",
        CanonicalAction::Edit { .. } => "edit",
        CanonicalAction::Write { .. } => "write",
        CanonicalAction::Fetch { .. } => "fetch",
        CanonicalAction::Unknown { .. } => "unknown",
        CanonicalAction::Compound { .. } => "compound",
    }
}

fn check_one(fx: &Fixture, actual: &[Normalized]) -> Result<(), String> {
    if let Some(n) = fx.expect.count {
        if actual.len() != n {
            return Err(format!("expected {n} action(s), got {}", actual.len()));
        }
    }
    let Some(first) = actual.first() else {
        if fx.expect.op.is_none() {
            return Ok(());
        }
        return Err("expected at least one action, got none".to_string());
    };

    if let Some(op) = &fx.expect.op {
        let got = op_name(&first.action);
        if got != op {
            return Err(format!("expected op {op}, got {got}"));
        }
    }
    if let Some(effect) = &fx.expect.effect {
        if first.effect.as_str() != effect {
            return Err(format!("expected effect {effect}, got {}", first.effect.as_str()));
        }
    }
    if let Some(m) = fx.expect.mutating {
        if first.mutating != m {
            return Err(format!("expected mutating={m}, got {}", first.mutating));
        }
    }
    if let Some(minc) = fx.expect.min_confidence {
        if first.confidence < minc {
            return Err(format!("expected confidence >= {minc}, got {}", first.confidence));
        }
    }
    if let Some(maxc) = fx.expect.max_confidence {
        if first.confidence > maxc {
            return Err(format!("expected confidence <= {maxc}, got {}", first.confidence));
        }
    }
    if let Some(p) = &fx.expect.path {
        let got = match &first.action {
            CanonicalAction::Read { path, .. }
            | CanonicalAction::List { path }
            | CanonicalAction::Edit { path }
            | CanonicalAction::Write { path } => Some(path.clone()),
            _ => None,
        };
        if got.as_deref() != Some(p.as_str()) {
            return Err(format!("expected path {p:?}, got {got:?}"));
        }
    }
    if let Some(p) = &fx.expect.pattern {
        if let CanonicalAction::Search { pattern, .. } = &first.action {
            if pattern != p {
                return Err(format!("expected pattern {p:?}, got {pattern:?}"));
            }
        } else {
            return Err("expected a search action for `pattern` assertion".to_string());
        }
    }
    if let Some(u) = &fx.expect.url {
        if let CanonicalAction::Fetch { url } = &first.action {
            if url != u {
                return Err(format!("expected url {u:?}, got {url:?}"));
            }
        } else {
            return Err("expected a fetch action for `url` assertion".to_string());
        }
    }
    if let Some(expected_argv) = &fx.expect.argv {
        if let CanonicalAction::Exec { argv, .. } = &first.action {
            if argv != expected_argv {
                return Err(format!("expected argv {expected_argv:?}, got {argv:?}"));
            }
        } else {
            return Err("expected an exec action for `argv` assertion".to_string());
        }
    }
    if let Some(k) = &fx.expect.compound_kind {
        if let CanonicalAction::Compound { kind, .. } = &first.action {
            if kind != k {
                return Err(format!("expected compound kind {k:?}, got {kind:?}"));
            }
        } else {
            return Err("expected a compound action for `compound_kind` assertion".to_string());
        }
    }
    Ok(())
}

fn run(path: &str, table: &CommandTable) -> i32 {
    let raw = match std::fs::read_to_string(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("cannot read {path}: {e}");
            return 2;
        }
    };
    let value: Value = serde_json::from_str(&raw).expect("normaliser.json must be valid JSON");
    let fixtures: Vec<Fixture> = serde_json::from_value(value).expect("normaliser.json must match Fixture[] shape");

    let mut passed = 0;
    let mut failed = 0;
    let mut failures: Vec<String> = Vec::new();

    for fx in &fixtures {
        let actual = normalize_shell(&fx.cmd, &fx.cwd, table);
        match check_one(fx, &actual) {
            Ok(()) => passed += 1,
            Err(msg) => {
                failed += 1;
                failures.push(format!(
                    "[{}] {}\n    cmd: {}\n    got: {}",
                    fx.id,
                    msg,
                    fx.cmd,
                    serde_json::to_string(&actual).unwrap_or_default()
                ));
            }
        }
    }

    println!("fixtures: {passed} passed, {failed} failed, {} total", fixtures.len());
    if !failures.is_empty() {
        println!("\nfailures:");
        for f in &failures {
            println!("  {f}");
        }
    }
    if failed == 0 && fixtures.len() >= 300 {
        0
    } else if failed == 0 {
        eprintln!("warning: fewer than 300 fixtures ({} < 300)", fixtures.len());
        1
    } else {
        1
    }
}

#[test]
fn every_normaliser_case_passes() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/cases/normaliser.json");
    assert_eq!(run(path, CommandTable::load()), 0, "normaliser cases failed");
}
