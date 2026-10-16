use super::*;
use adage_core::Effect;
use adage_ingest::format::Decode;
use serde_json::{json, Value as Json};

const COMMAND: &str = "node tools/gate.mjs --strict";
const WORKFLOW: &str = "name: CI\njobs:\n  checks:\n    steps:\n      - name: Validate contracts\n        run: node tools/gate.mjs --strict\n";

struct Tool {
    name: &'static str,
    input: Json,
    result: Option<(String, bool)>,
}

fn bash(command: &str, output: &str) -> Tool {
    Tool { name: "Bash", input: json!({"command":command}), result: Some((output.into(), false)) }
}

fn authored(path: &str, content: &str) -> Tool {
    Tool {
        name: "Write",
        input: json!({"file_path":path,"content":content}),
        result: Some((format!("File created successfully at: {path}"), false)),
    }
}

fn package(command: &str) -> String {
    json!({"scripts":{"validate-contracts":command}}).to_string()
}

fn worker(id: &str, calls: Vec<Tool>) -> Session {
    let mut records = vec![json!({"type":"user","sessionId":"parent","agentId":id,
        "isSidechain":true,"cwd":"/repo","message":{"content":"Inspect repository validation"}})];
    for (i, call) in calls.into_iter().enumerate() {
        records.push(json!({"type":"assistant","cwd":"/repo","message":{
            "id":format!("m{i}"),"content":[{"type":"tool_use","id":format!("call-{i}"),
            "name":call.name,"input":call.input}]}}));
        if let Some((output, failed)) = call.result {
            records.push(json!({"type":"user","message":{"content":[{"type":"tool_result",
                "tool_use_id":format!("call-{i}"),"content":output,"is_error":failed}]}}));
        }
    }
    let raw = records.iter().map(Json::to_string).collect::<Vec<_>>().join("\n");
    let cx = Decode {
        table: adage_core::CommandTable::load(),
        locator: &adage_ingest::locate::AsRecorded,
        repo_override: Some("example/contracts"),
        subagent: true,
    };
    adage_ingest::claude::decode(raw.as_bytes(), &cx).unwrap()
}

fn before() -> Session {
    worker("worker", vec![bash("cat package.json", &package(COMMAND)), bash(COMMAND, "validation complete")])
}

#[test]
fn a_later_authored_workflow_is_a_review_lead_not_prior_configuration_or_success() {
    let s = worker(
        "worker",
        vec![bash(COMMAND, "PASS: all contracts"), authored("/repo/.github/workflows/ci.yml", WORKFLOW)],
    );
    let original = serde_json::to_vec(&s).unwrap();
    let found = learn(&[&s]);
    assert_eq!(found.len(), 1);
    let p = &found[0];
    assert_eq!(p.status, "needs_evidence_review");
    assert_eq!(p.declaration_path, ".github/workflows/ci.yml");
    assert_eq!(p.declaration_field, "/jobs/checks/steps/0/run");
    assert_eq!(p.script, "tools/gate.mjs");
    assert_eq!(p.working_directory, ".");
    assert_eq!(p.declared_argv, ["node", "tools/gate.mjs", "--strict"]);
    assert_eq!(p.observed_argv, p.declared_argv);
    assert_eq!((p.sessions, p.evidence_groups), (1, 1));
    let o = &p.observations[0];
    assert_eq!(o.declaration_origin, DeclarationOrigin::Authored);
    assert_eq!(o.declaration_timing, DeclarationTiming::AfterInvocation);
    assert_eq!(o.invocation_shape, "single_command_request");
    assert_eq!(o.outcome, "unknown");
    assert_eq!(o.invocation.call_id, "call-0");
    assert_eq!(o.declaration.call_id, "call-1");
    assert_eq!(o.document_sha256, hash(WORKFLOW));
    assert_eq!(o.declared_role, "Validate contracts");
    assert_eq!(o.declared_role_basis, "ci_step_name");
    assert_eq!(o.invocation.output_sha256, hash("PASS: all contracts"));
    assert!(p.requirements.iter().any(|r| r.contains("later declaration")));
    assert!(p.requirements.iter().any(|r| r.contains("no execution authority")));
    assert_eq!(s.steps[0].norm.effect, Effect::Environment);
    assert_eq!(s.outcome, None);
    assert_eq!(serde_json::to_vec(&s).unwrap(), original);
    let integrated = super::super::learn(&[&s]);
    assert_eq!(integrated.declared_validators, found);
    assert_eq!(integrated.checks, 0);
    assert!(integrated.proposals.is_empty());
}

#[test]
fn a_prior_complete_cat_preserves_exact_interpreter_spelling_and_native_anchors() {
    let command = "/opt/tools/bin/node tools/gate.mjs --strict";
    let s = worker("worker", vec![bash("cat package.json", &package(COMMAND)), bash(command, "not a runner report")]);
    let found = learn(&[&s]);
    assert_eq!(found.len(), 1);
    let p = &found[0];
    assert_eq!(p.declared_argv[0], "node");
    assert_eq!(p.observed_argv[0], "/opt/tools/bin/node");
    assert_eq!(p.declaration_field, "/scripts/validate-contracts");
    let o = &p.observations[0];
    assert_eq!(o.declaration_origin, DeclarationOrigin::Read);
    assert_eq!(o.declaration_timing, DeclarationTiming::BeforeInvocation);
    assert_eq!(o.declaration.call_id, "call-0");
    assert_eq!(o.invocation.call_id, "call-1");
    assert_eq!(o.declaration.input_sha256, hash(serde_json::to_vec(&s.steps[0].call.input).unwrap()));
    assert_eq!(o.declared_role, "validate-contracts");
    assert_eq!(o.declared_role_basis, "package_script_name");
    assert_eq!(o.outcome, "unknown");
}

#[test]
fn a_checker_filename_is_an_explicit_discovery_clue_under_neutral_declarative_labels() {
    let command = "node tools/check-plugin.mjs --strict";
    let workflow = "jobs:\n  build:\n    name: Build artifacts\n    steps:\n      - name: Structure and versions\n        run: node tools/check-plugin.mjs --strict\n";
    let s = worker("worker", vec![bash(command, "finished"), authored("/repo/.github/workflows/ci.yml", workflow)]);
    let found = learn(&[&s]);
    assert_eq!(found.len(), 1);
    let candidate = &found[0];
    let observation = &candidate.observations[0];
    assert_eq!(candidate.script, "tools/check-plugin.mjs");
    assert_eq!(candidate.declaration_field, "/jobs/build/steps/0/run");
    assert_eq!(candidate.status, "needs_evidence_review");
    assert_eq!(observation.declared_role, "check-plugin.mjs");
    assert_eq!(observation.declared_role_basis, "script_filename");
    assert_eq!(observation.document_sha256, hash(workflow));
    assert_eq!(observation.declaration_timing, DeclarationTiming::AfterInvocation);
    assert_eq!(observation.outcome, "unknown");
    assert_eq!(s.steps[0].norm.effect, Effect::Environment);
    assert_eq!(s.outcome, None);
}

#[test]
fn workflow_role_precedence_retains_the_qualifying_label_and_its_exact_basis() {
    for (job_id, job_name, step_name, expected_role, expected_basis) in [
        ("checks", "Test contracts", "Audit versions", "Audit versions", "ci_step_name"),
        ("checks", "Test contracts", "Structure and versions", "Test contracts", "ci_job_name"),
        ("checks", "Build artifacts", "Structure and versions", "checks", "ci_job_id"),
    ] {
        let body = format!(
            "jobs:\n  {job_id}:\n    name: {job_name}\n    steps:\n      - name: {step_name}\n        run: {COMMAND}\n"
        );
        let s = worker("worker", vec![bash("cat .github/workflows/ci.yml", &body), bash(COMMAND, "finished")]);
        let found = learn(&[&s]);
        assert_eq!(found.len(), 1, "qualifying {expected_basis}");
        let observation = &found[0].observations[0];
        assert_eq!(observation.declared_role, expected_role);
        assert_eq!(observation.declared_role_basis, expected_basis);
        assert_eq!(observation.declaration_origin, DeclarationOrigin::Read);
        assert_eq!(observation.declaration_timing, DeclarationTiming::BeforeInvocation);
        assert_eq!(observation.outcome, "unknown");
    }
}

#[test]
fn package_alias_precedes_filename_and_directory_tokens_never_supply_a_role() {
    for (alias, command, expected_role, expected_basis) in [
        ("validate-contracts", "node tools/check-plugin.mjs", "validate-contracts", "package_script_name"),
        ("structure", "node tools/check-plugin.mjs", "check-plugin.mjs", "script_filename"),
    ] {
        let body = json!({"scripts":{alias:command}}).to_string();
        let s = worker("worker", vec![bash("cat package.json", &body), bash(command, "finished")]);
        let found = learn(&[&s]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].declaration_field, format!("/scripts/{alias}"));
        assert_eq!(found[0].observations[0].declared_role, expected_role);
        assert_eq!(found[0].observations[0].declared_role_basis, expected_basis);
        assert_eq!(found[0].observations[0].outcome, "unknown");
    }
    for command in ["node tools/gate.mjs", "node checks/gate.mjs", "node tools/checker.mjs"] {
        let package = json!({"scripts":{"structure":command}}).to_string();
        let workflow = format!(
            "jobs:\n  build:\n    name: Build artifacts\n    steps:\n      - name: Structure and versions\n        run: {command}\n"
        );
        for (path, body) in [("package.json", package), (".github/workflows/ci.yml", workflow)] {
            let s = worker("worker", vec![bash(&format!("cat {path}"), &body), bash(command, "finished")]);
            assert!(learn(&[&s]).is_empty(), "neither a role label nor a basename token: {command} in {path}");
        }
    }
}

#[test]
fn a_qualifying_role_retains_source_text_beyond_the_display_name_limit() {
    let alias = format!("{} test contracts", "a".repeat(121));
    let body = json!({"scripts":{&alias:COMMAND}}).to_string();
    let s = worker("worker", vec![bash("cat package.json", &body), bash(COMMAND, "finished")]);
    let found = learn(&[&s]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].observations[0].declared_role, alias);
    assert_eq!(found[0].observations[0].declared_role_basis, "package_script_name");
    assert_eq!(found[0].observations[0].document_sha256, hash(&body));
}

#[test]
fn failed_missing_denied_and_unacknowledged_writes_are_not_declarations() {
    for result in [
        None,
        Some((String::new(), false)),
        Some(("done".into(), false)),
        Some(("File created successfully at: /repo/.github/workflows/other.yml".into(), false)),
        Some(("File created successfully at: /repo/.github/workflows/ci.yml".into(), true)),
        Some(("<tool_use_error>Permission denied</tool_use_error>".into(), true)),
    ] {
        let mut write = authored("/repo/.github/workflows/ci.yml", WORKFLOW);
        write.result = result.clone();
        let s = worker("worker", vec![bash(COMMAND, "PASS"), write]);
        assert!(learn(&[&s]).is_empty(), "unconfirmed native write: {result:?}");
    }
    let mut write = authored("/repo/.github/workflows/ci.yml", WORKFLOW);
    write.result.as_mut().unwrap().0.push_str(" (file state is current in your context — no need to Read it back)");
    assert_eq!(learn(&[&worker("worker", vec![bash(COMMAND, ""), write])]).len(), 1);
}

#[test]
fn quoted_partial_compound_and_transformed_reads_do_not_supply_declarations() {
    let body = package(COMMAND);
    for command in [
        "echo package.json",
        "cat package.json | head -20",
        "head -20 package.json",
        "sed -n '1,20p' package.json",
        "cat package.json other.json",
        "cat package.json; echo done",
        "cat package.json > copy.json",
        "cat < package.json",
        "node -p 'JSON.stringify(require(\"./package.json\"))'",
    ] {
        let s = worker("worker", vec![bash(command, &body), bash(COMMAND, "PASS")]);
        assert!(learn(&[&s]).is_empty(), "not a sole literal file read: {command}");
    }
    for input in
        [json!({"file_path":"/repo/package.json"}), json!({"file_path":"/repo/package.json","offset":1,"limit":20})]
    {
        let read = Tool { name: "Read", input, result: Some((body.clone(), false)) };
        assert!(learn(&[&worker("worker", vec![read, bash(COMMAND, "PASS")])]).is_empty());
    }
    for output in [
        "{\"scripts\":",
        "1\t{\"scripts\":{\"validate-contracts\":\"node tools/gate.mjs --strict\"}}",
        "Output truncated\n{}",
    ] {
        let s = worker("worker", vec![bash("cat package.json", output), bash(COMMAND, "PASS")]);
        assert!(learn(&[&s]).is_empty(), "not a complete manifest: {output}");
    }
}

#[test]
fn only_declarative_script_fields_and_validation_role_claims_are_candidates() {
    for body in [
        json!({"description":"validate node tools/gate.mjs --strict"}).to_string(),
        json!({"scripts":{"deploy":COMMAND}}).to_string(),
        json!({"scripts":{"validate-contracts":"echo node tools/gate.mjs --strict"}}).to_string(),
        json!({"scripts":{"validate-contracts":"node tools/gate.mjs --strict && echo passed"}}).to_string(),
        "{\"scripts\":{\"validate-contracts\":\"node tools/gate.mjs --strict\",\"validate-contracts\":\"node other/gate.mjs\"}}".into(),
    ] {
        assert!(learn(&[&worker("worker", vec![bash("cat package.json", &body), bash(COMMAND, "PASS")])]).is_empty());
    }
    for body in [
        "name: Validate\nrun: node tools/gate.mjs --strict\n",
        "jobs:\n  deploy:\n    steps:\n      - run: node tools/gate.mjs --strict\n",
        "jobs:\n  checks:\n    steps:\n      - run: node tools/gate.mjs --strict\n        working-directory: ${{ matrix.directory }}\n",
    ] {
        let s = worker("worker", vec![bash("cat .github/workflows/ci.yml", body), bash(COMMAND, "PASS")]);
        assert!(learn(&[&s]).is_empty(), "unsupported declaration: {body}");
    }
}

#[test]
fn same_turn_prompt_actor_cwd_and_argument_mismatches_never_join() {
    for mutation in 0..5 {
        let mut s = before();
        match mutation {
            0 => s.steps[1].turn = s.steps[0].turn,
            1 => s.steps[1].prompt += 1,
            2 => s.steps[1].call.agent = Some("other-worker".into()),
            3 => {
                let CanonicalAction::Exec { cwd, .. } = &mut s.steps[1].norm.action else { panic!("exec") };
                *cwd = "/repo/subdir".into();
            }
            4 => {
                let CanonicalAction::Exec { cwd, .. } = &mut s.steps[1].norm.action else { panic!("exec") };
                *cwd = "/another/repo".into();
            }
            _ => unreachable!(),
        }
        assert!(learn(&[&s]).is_empty(), "scope mutation {mutation}");
    }
    for command in [
        "node tools/gate.mjs",
        "node tools/gate.mjs --other",
        "node other/gate.mjs --strict",
        "python3 tools/gate.mjs --strict",
    ] {
        let s = worker("worker", vec![bash("cat package.json", &package(COMMAND)), bash(command, "PASS")]);
        assert!(learn(&[&s]).is_empty(), "different invocation: {command}");
    }
}

#[test]
fn traversal_dynamic_and_shell_list_invocations_abstain() {
    for command in [
        "node ../tools/gate.mjs --strict",
        "node tools/../gate.mjs --strict",
        "node $SCRIPT --strict",
        "node $(echo tools/gate.mjs) --strict",
        "NODE_ENV=test node tools/gate.mjs --strict",
        "node tools/gate.mjs --strict; echo done",
        "true && node tools/gate.mjs --strict",
        "f() { node tools/gate.mjs --strict; }; f",
        "bash -c 'node tools/gate.mjs --strict'",
    ] {
        let s = worker("worker", vec![bash("cat package.json", &package(command)), bash(command, "PASS")]);
        assert!(learn(&[&s]).is_empty(), "unsupported command: {command}");
    }
    let s = worker("worker", vec![authored("/outside/package.json", &package(COMMAND)), bash(COMMAND, "PASS")]);
    assert!(learn(&[&s]).is_empty());
}

#[test]
fn a_first_pipeline_segment_stays_a_request_with_unknown_outcome() {
    let s = worker(
        "worker",
        vec![bash("cat package.json", &package(COMMAND)), bash("node tools/gate.mjs --strict | tail -20", "PASS")],
    );
    let found = learn(&[&s]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].observations.len(), 1, "normalized copies are one native invocation");
    assert_eq!(found[0].observations[0].invocation_shape, "pipeline_segment_request");
    assert_eq!(found[0].observations[0].outcome, "unknown");
    let second = worker(
        "worker",
        vec![bash("cat package.json", &package(COMMAND)), bash("cat input | node tools/gate.mjs --strict", "PASS")],
    );
    assert!(learn(&[&second]).is_empty(), "do not infer a later pipeline stage");
}

#[test]
fn reused_conflicting_missing_and_nonagent_native_identities_do_not_qualify() {
    for mutation in 0..8 {
        let mut s = before();
        match mutation {
            0 => s.steps[1].call.tool_use_id = None,
            1 => s.steps[1].call.tool_use_id = s.steps[0].call.tool_use_id.clone(),
            2 => {
                let mut copy = s.steps[1].clone();
                copy.turn += 1;
                s.steps.push(copy);
            }
            3 => {
                let mut copy = s.steps[0].clone();
                copy.turn = 2;
                s.steps.push(copy);
            }
            4 => s.steps[1].call.by = By::Adage,
            5 => {
                s.steps[1].call.failed = true;
                s.steps[1].call.output = "<tool_use_error>denied</tool_use_error>".into();
            }
            6 => {
                let mut copy = s.steps[1].clone();
                copy.call.output = "conflicting result".into();
                s.steps.push(copy);
            }
            7 => {
                let mut copy = s.steps[1].clone();
                copy.call.input = json!({"command":"node tools/other.mjs"});
                s.steps.push(copy);
            }
            _ => unreachable!(),
        }
        assert!(learn(&[&s]).is_empty(), "native identity mutation {mutation}");
    }
    let mut s = before();
    s.steered = true;
    assert!(learn(&[&s]).is_empty());
}

#[test]
fn a_custom_command_failure_is_still_only_a_reviewable_request() {
    let mut s = before();
    s.steps[1].call.failed = true;
    s.steps[1].call.output = "2 failed, 20 passed in 0.1s".into();
    let found = learn(&[&s]);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].observations[0].outcome, "unknown");
    assert_eq!(s.steps[1].norm.effect, Effect::Environment);
    assert_eq!(s.outcome, None);
}

#[test]
fn siblings_duplicates_and_input_order_do_not_inflate_independent_support() {
    let a = before();
    let mut b = before();
    b.id = "parent/other-worker".into();
    b.delegation.as_mut().unwrap().agent_id = "other-worker".into();
    for st in &mut b.steps {
        st.call.agent = Some("other-worker".into());
    }
    let forward = learn(&[&a, &b, &a]);
    let reverse = learn(&[&b, &a]);
    assert_eq!(forward, reverse);
    assert_eq!(forward.len(), 1);
    assert_eq!((forward[0].sessions, forward[0].evidence_groups), (2, 1));
    assert_eq!(forward[0].observations.len(), 2);
    let mut other_parent = b.clone();
    other_parent.id = "another-parent/other-worker".into();
    other_parent.delegation.as_mut().unwrap().parent_id = "another-parent".into();
    let independent = learn(&[&a, &other_parent]);
    assert_eq!((independent[0].sessions, independent[0].evidence_groups), (2, 2));
}

#[test]
fn declarations_never_join_across_sessions() {
    let read = worker("reader", vec![bash("cat package.json", &package(COMMAND))]);
    let call = worker("caller", vec![bash(COMMAND, "PASS")]);
    assert!(learn(&[&read, &call]).is_empty());
}
