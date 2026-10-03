use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output};

fn run(home: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_adage"))
        .env("ADAGE_HOME", home)
        .env("ADAGE_COLOR", "never")
        .env("ADAGE_EMBED_MODEL", "none")
        .args(args)
        .output()
        .unwrap()
}

fn ok_json(output: Output) -> Value {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn export_import_inspect_and_learn_share_the_canonical_contract() {
    let d = tempfile::tempdir().unwrap();
    let home = d.path().join("home");
    let raw = d.path().join("trace.jsonl");
    let lines = [
        json!({"type":"user","sessionId":"s1","cwd":"/recorded/repo","message":{"role":"user","content":"Run the tests"}}),
        json!({"type":"assistant","sessionId":"s1","cwd":"/recorded/repo","message":{"id":"m1","content":[{"type":"tool_use","id":"t1","name":"Bash","input":{"command":"make test"}}]}}),
        json!({"type":"user","sessionId":"s1","cwd":"/recorded/repo","message":{"content":[{"type":"tool_result","tool_use_id":"t1","content":"1 passed","is_error":false}]}}),
    ];
    std::fs::write(&raw, lines.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
    let identity = d.path().join("identities.json");
    std::fs::write(&identity, br#"{"s1":{"task_id":"TASK-7","attempt_id":"1"}}"#).unwrap();
    let output = run(
        &home,
        &["corpus", "export", raw.to_str().unwrap(), "--repo", "acme/repo", "--identities", identity.to_str().unwrap()],
    );
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let canonical: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(canonical["schema"], 1);
    assert_eq!(canonical["session"]["identity"]["task_id"], "TASK-7");
    let input = d.path().join("corpus.jsonl");
    std::fs::write(&input, output.stdout).unwrap();
    let args = ["corpus", "import", input.to_str().unwrap(), "--repo", "acme/repo"];
    let imported = ok_json(run(&home, &args));
    assert_eq!(imported["added"], 1);
    assert_eq!(ok_json(run(&home, &args))["duplicates"], 1);
    let inspected = ok_json(run(&home, &["corpus", "inspect", "--repo", "acme/repo"]));
    assert_eq!(inspected["generation"], imported["generation"]);
    let learned = ok_json(run(&home, &["corpus", "learn", "--repo", "acme/repo"]));
    assert_eq!(learned["sessions_used"], 1);
    assert!(home.join("index.json").is_file());
    std::fs::write(&input, b"{broken\n").unwrap();
    let rejected = run(&home, &args);
    assert!(!rejected.status.success());
    let report: Value = serde_json::from_slice(&rejected.stdout).unwrap();
    assert_eq!(report["committed"], false);
    assert_eq!(ok_json(run(&home, &["corpus", "inspect", "--repo", "acme/repo"])), inspected);
}
