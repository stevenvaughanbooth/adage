//! A sample pack (the Track C §1.4 example, extended with a vendored script, a dead end and the
//! finish line its `instead` names) as files in memory.
#![allow(dead_code)]

use adage_pack::digest::sha256_hex;
use std::collections::BTreeMap;

pub const LOOKUP: &str =
    "#!/usr/bin/env python3\nimport json, sys\nprint(json.dumps({'files': [], 'state': 'Active'}))\n";
pub const VENDORED: &str = "import json\nprint(json.dumps({'count': 3}))\n";

pub fn lookup_sha() -> String {
    sha256_hex(LOOKUP.as_bytes())
}

pub fn vendored_sha() -> String {
    sha256_hex(VENDORED.as_bytes())
}

pub fn manifest() -> String {
    format!(
        r#"format = "adage-pack/1"

[pack]
name = "acme/work-items"
version = "1.4.0"
origin = "reflex"
description = "Work item context for the payments repository"

[applicability]
kind = "repo"
repo_keys = ["github.com/acme/payments", "payments@1a2b3c4d5e6f"]

[requires]
os = ["linux", "macos"]
tools = [{{ name = "python3" }}, {{ name = "make" }}]
harness = ["claude-code >=2.1.200"]
templates = ["avoid@1", "obligation@1", "read@1", "run@1"]
command_table = ">=3"
repo_files = [{{ path = "scripts/wi-lookup", sha256 = "{lookup}" }}]

[exec]
requested = "E1"

[provenance]
tool = "adage 0.5.0"
origins = {{ organic = 412, synthetic = 12 }}
"#,
        lookup = lookup_sha()
    )
}

pub fn work_item_context() -> String {
    format!(
        r#"id: work-item-context
summary: Look up the work item named in the prompt
shape: run
trigger: {{ on: prompt, span: ticket }}
inputs:
  item: {{ type: work-item-id, from: {{ prompt: {{ span: ticket }} }} }}
steps:
  - id: lookup
    run: {{ argv: ["./scripts/wi-lookup", "{{item}}", "--json"] }}
    effect: observe
    script: {{ path: scripts/wi-lookup, closure: ref, sha256: "{lookup}" }}
    outputs:
      files: {{ type: repo-path-list, from: {{ json: files }}, max: 5 }}
      state: {{ type: wi-state, from: {{ json: state }} }}
  - id: read-linked
    read: {{ from: lookup.files, max: 5 }}
    effect: observe
strength: fire
exit:
  - {{ when: result_equals, step: lookup, field: state, value: Closed }}
  - {{ when: intent_changes }}
exec: {{ level: E1, timeout_ms: 1500, output: typed }}
evidence:
  opportunities: 10
  support: 9
  days: 9
  rate: 0.9
  lower_bound: 0.5958
  provenance: desire-path
  by_origin: {{ synthetic: 2 }}
"#,
        lookup = lookup_sha()
    )
}

pub const READ_LINKED: &str = r#"id: read-linked-files
shape: read
trigger: { on: after_command, command: "exec ./scripts/wi-lookup {id} --json" }
inputs:
  files: { type: repo-path-list, from: { result: { field: files } } }
steps:
  - id: read
    read: { from: files, max: 5 }
    effect: observe
strength: prime
exit:
  - { when: intent_changes }
"#;

pub fn counter() -> String {
    format!(
        r#"id: count-items
shape: run
trigger: {{ on: session_start }}
steps:
  - id: count
    run: {{ argv: ["python3", "scripts/sha256-{v}"] }}
    effect: observe
    script: {{ path: "scripts/sha256-{v}", closure: vendor, sha256: "{v}" }}
    outputs:
      count: {{ type: int, from: {{ json: count }} }}
strength: prime
"#,
        v = vendored_sha()
    )
}

pub const DEAD_END: &str = r#"id: dead-end-pytest
shape: avoid
trigger: { on: session_start }
steps:
  - id: avoid
    avoid:
      runners: [["python3", "-m", "pytest"]]
      instead: ["make", "test"]
      signature: No module named pytest
    effect: verify
strength: inhibit
"#;

pub const FINISH_LINE: &str = r#"id: finish-line-tests
shape: obligation
trigger: { on: after_edit, paths: "src/*.py" }
steps:
  - id: test
    run: { argv: ["make", "test"] }
    effect: verify
strength: fire
exit:
  - { when: exploratory_change }
"#;

pub const WORK_ITEM_ID: &str = "id: work-item-id\nbase: string\npattern: ^W-\\d{4,9}$\n";
pub const WI_STATE: &str = "id: wi-state\nbase: enum\nvalues: [Active, Closed]\n";

/// The sample pack's files without `CLOSURE`.
pub fn files_without_closure() -> BTreeMap<String, Vec<u8>> {
    let mut f = BTreeMap::new();
    let mut put = |k: &str, v: String| {
        f.insert(k.to_string(), v.into_bytes());
    };
    put("pack.toml", manifest());
    put("routes/work-item-context.yaml", work_item_context());
    put("routes/read-linked-files.yaml", READ_LINKED.into());
    put("routes/count-items.yaml", counter());
    put("routes/dead-end-pytest.yaml", DEAD_END.into());
    put("routes/finish-line-tests.yaml", FINISH_LINE.into());
    put("types/work-item-id.yaml", WORK_ITEM_ID.into());
    put("types/wi-state.yaml", WI_STATE.into());
    put(&format!("scripts/sha256-{}", vendored_sha()), VENDORED.into());
    put("evidence.json", r#"{"work-item-context":{"organic":9,"synthetic":2}}"#.to_string() + "\n");
    f
}

/// Add the `CLOSURE` the files imply (derived by the crate itself).
pub fn with_closure(mut f: BTreeMap<String, Vec<u8>>) -> BTreeMap<String, Vec<u8>> {
    f.remove("CLOSURE");
    let parsed = adage_pack::validate::parse(&f).expect("parses");
    adage_pack::validate::validate(&parsed).expect("valid");
    let c = adage_pack::closure::compute(&parsed).expect("closed");
    f.insert("CLOSURE".into(), c.render().into_bytes());
    f
}

/// The sample pack's complete files.
pub fn files() -> BTreeMap<String, Vec<u8>> {
    with_closure(files_without_closure())
}

pub fn write_dir(dir: &std::path::Path, files: &BTreeMap<String, Vec<u8>>) {
    for (rel, bytes) in files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, bytes).unwrap();
    }
}
