//! A trained pack (compiled from a skill by `adage train`): sequences and an edited file's tests
//! bind to runtime forms, and `procedure.json` must agree with its pack.

mod common;

use adage_pack::bind::{bind, BindEnv, Status, Target};
use adage_pack::{Os, Pack};
use std::collections::BTreeMap;
use std::path::Path;

fn env(dir: &Path, tools: &[&str]) -> BindEnv {
    let bin = dir.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    for t in tools {
        let p = bin.join(t);
        std::fs::write(&p, "#!/bin/sh\nexit 1\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
    }
    BindEnv { os: Os::Linux, path: Some(bin.into_os_string()), cas: None }
}

fn skill() -> String {
    "b".repeat(64)
}

const SEQUENCE: &str = r#"id: test-then-ci
shape: sequence
trigger: { on: after_edit, paths: "src/*.py" }
steps:
  - { id: test, run: { argv: ["make", "test"] }, effect: verify }
  - { id: ci, run: { argv: ["make", "ci"] }, effect: verify }
strength: fire
exit:
  - { when: exploratory_change }
"#;

const MODULE_TESTS: &str = r#"id: module-tests
shape: obligation
trigger: { on: after_edit, paths: "src/*.py" }
inputs:
  tests:
    type: repo-path-list
    from: { edit_tests: { k: 2, map: { "src/core.py": ["tests/test_core.py", "tests/test_basic.py"] } } }
steps:
  - { id: run, run: { argv: ["python3", "-m", "pytest", "{tests}", "-q"] }, effect: verify }
strength: prime
"#;

fn procedure(route: &str, sha: &str) -> String {
    serde_json::json!({
        "schema": "adage-procedure/1",
        "skill": { "name": "billing", "sha256": sha, "bytes": 100 },
        "steps": [
            { "id": "s1", "title": "Test", "role": "verify", "when": "after_edit src/*.py",
              "commands": ["make test", "make ci"], "outcome": { "status": "compiled", "routes": [route] } },
            { "id": "s2", "title": "Fix it", "role": "edit", "when": "always",
              "outcome": { "status": "text", "reason": "needs judgment" } }
        ]
    })
    .to_string()
}

fn files_without_closure() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        f.insert(k.to_string(), v.into_bytes());
    };
    put(
        "pack.toml",
        format!(
            r#"format = "adage-pack/1"

[pack]
name = "acme/skill-billing"
version = "0.1.0"
origin = "trained"

[applicability]
kind = "repo"
repo_keys = ["github.com/acme/billing"]

[requires]
tools = [{{ name = "make" }}, {{ name = "python3" }}]
templates = ["obligation@1", "sequence@1"]

[provenance]
skill = "sha256:{}"
"#,
            skill()
        ),
    );
    put("routes/test-then-ci.yaml", SEQUENCE.into());
    put("routes/module-tests.yaml", MODULE_TESTS.into());
    put("procedure.json", procedure("test-then-ci", &skill()));
    f
}

fn closed(mut f: BTreeMap<String, Vec<u8>>) -> Result<BTreeMap<String, Vec<u8>>, String> {
    f.remove("CLOSURE");
    let parsed = adage_pack::validate::parse(&f).map_err(|e| format!("{e:#}"))?;
    adage_pack::validate::validate(&parsed).map_err(|e| format!("{e:#}"))?;
    let c = adage_pack::closure::compute(&parsed).map_err(|e| format!("{e:#}"))?;
    f.insert("CLOSURE".into(), c.render().into_bytes());
    Ok(f)
}

#[test]
fn sequences_and_edited_file_tests_bind_live() {
    let d = tempfile::tempdir().unwrap();
    let pack = Pack::from_files(&closed(files_without_closure()).unwrap()).unwrap();
    assert_eq!(pack.procedure.as_ref().unwrap().counts(), (1, 1, 0));
    let t = Target { repo: "github.com/acme/billing".into(), root: None };
    let b = bind(&pack, &t, &env(d.path(), &["make", "python3"]));
    let seq = b.routes.iter().find(|r| r.id == "test-then-ci").unwrap();
    assert!(seq.status.is_live(), "{:?}", seq.status);
    let rt = seq.route.clone().unwrap();
    assert_eq!(rt.step, adage_tracks::Step::Sequence { commands: vec!["make test".into(), "make ci".into()] });
    assert_eq!(rt.effect, adage_core::Effect::Verify);
    assert_eq!(rt.strength, adage_tracks::Strength::Fire);
    let mt = b.routes.iter().find(|r| r.id == "module-tests").unwrap();
    assert!(mt.status.is_live(), "{:?}", mt.status);
    let rt = mt.route.clone().unwrap();
    assert!(matches!(
        &rt.slots[0].source,
        adage_tracks::SlotSource::EditTests { glob, k: 2, map } if glob == "src/*.py" && map.len() == 1
    ));
    assert_eq!(rt.step, adage_tracks::Step::Run { command: "python3 -m pytest {tests} -q".into() });
    // It survives writing and reading back, procedure included.
    let out = d.path().join("pack");
    pack.write(&out).unwrap();
    assert!(out.join("procedure.json").is_file());
    assert_eq!(Pack::load(&out).unwrap(), pack);
    // A sequence of four commands has no runtime form: inert, with the reason.
    let mut f = files_without_closure();
    let four = SEQUENCE.replace(
        "  - { id: ci, run: { argv: [\"make\", \"ci\"] }, effect: verify }\n",
        "  - { id: ci, run: { argv: [\"make\", \"ci\"] }, effect: verify }\n  - { id: c, run: { argv: [\"make\", \"check\"] }, effect: verify }\n  - { id: d, run: { argv: [\"make\", \"test\"] }, effect: verify }\n",
    );
    f.insert("routes/test-then-ci.yaml".into(), four.into_bytes());
    let pack = Pack::from_files(&closed(f).unwrap()).unwrap();
    let b = bind(&pack, &t, &env(d.path(), &["make", "python3"]));
    match &b.routes.iter().find(|r| r.id == "test-then-ci").unwrap().status {
        Status::Inert { reason } => assert!(reason.contains("2 to 3 commands"), "{reason}"),
        s => panic!("{s:?}"),
    }
}

#[test]
fn a_procedure_must_match_its_pack() {
    // A step naming a route the pack does not have.
    let mut f = files_without_closure();
    f.insert("procedure.json".into(), procedure("nope", &skill()).into_bytes());
    assert!(closed(f).unwrap_err().contains("dangling"));
    // A skill hash other than the manifest's.
    let mut f = files_without_closure();
    f.insert("procedure.json".into(), procedure("test-then-ci", &"c".repeat(64)).into_bytes());
    assert!(closed(f).unwrap_err().contains("provenance"));
    // Only a trained pack carries one.
    let mut f = files_without_closure();
    let m = String::from_utf8(f["pack.toml"].clone()).unwrap().replace("origin = \"trained\"", "origin = \"reflex\"");
    f.insert("pack.toml".into(), m.into_bytes());
    assert!(closed(f).is_err());
    // An unknown field in the procedure.
    let mut f = files_without_closure();
    let p = procedure("test-then-ci", &skill()).replacen("{", "{\"extra\":1,", 1);
    f.insert("procedure.json".into(), p.into_bytes());
    assert!(closed(f).is_err());
}

#[test]
fn an_edited_file_s_tests_are_checked() {
    for (from, to) in [
        // On a trigger that is not an edit.
        ("on: after_edit, paths: \"src/*.py\" }\ninputs", "on: session_start }\ninputs"),
        // Bound into part of a token.
        ("\"{tests}\"", "\"--files={tests}\""),
        // A path out of the repository in the map.
        ("tests/test_basic.py", "../x.py"),
        // More tests than k.
        ("k: 2", "k: 1"),
    ] {
        let mut f = files_without_closure();
        let r = MODULE_TESTS.replace(from, to);
        assert_ne!(r, MODULE_TESTS, "{from}");
        f.insert("routes/module-tests.yaml".into(), r.into_bytes());
        assert!(closed(f).is_err(), "{to}");
    }
}
