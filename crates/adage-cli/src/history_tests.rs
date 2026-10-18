use super::*;
use serde_json::json;

fn fixture(instincts: bool) -> (tempfile::TempDir, adage_store::Paths, PathBuf, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("repo");
    std::fs::create_dir_all(repo.join(".git")).unwrap();
    let projects = root.join("projects");
    let project = projects.join(adage_learn::scope::claude_slug(&repo));
    std::fs::create_dir_all(&project).unwrap();
    let paths = adage_store::Paths::at(root.join("data"));
    std::fs::create_dir_all(&paths.root).unwrap();
    std::fs::write(
        paths.config(),
        format!(
            "[learn]\nclaude_projects = {:?}\ncodex_sessions = \"\"\nopencode_exports = \"\"\n[instincts]\nenabled = {instincts}\n",
            projects.to_str().unwrap()
        ),
    )
    .unwrap();
    (temp, paths, repo, project)
}

fn transcript(path: &Path, repo: &Path, id: &str, steered: bool, worker: Option<&str>, work: bool) {
    let mut first = json!({"type":"user", "sessionId":id, "cwd":repo,
        "message":{"role":"user", "content":"Inspect the repository and run its checks."}});
    if let Some(agent) = worker {
        first["agentId"] = json!(agent);
        first["isSidechain"] = json!(true);
    }
    let mut rows = vec![first];
    if work {
        rows.push(json!({"type":"assistant", "message":{"content":[
            {"type":"tool_use", "id":"status", "name":"Bash", "input":{"command":"git status --short"}}
        ]}}));
    }
    if steered {
        rows.push(json!({"type":"attachment", "attachment":{"type":"hook_additional_context", "content":
            "<adage-reflex track=\"checks@1\" route=\"test\" strength=\"prime\" kind=\"run\">Run make test.</adage-reflex>"}}));
    }
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, rows.iter().map(|v| format!("{v}\n")).collect::<String>()).unwrap();
}

#[test]
fn steered_history_is_not_rejected_when_instincts_are_disabled() {
    let (_temp, paths, repo, project) = fixture(false);
    let file = project.join("session.jsonl");
    transcript(&file, &repo, "session", true, None, true);
    let session = adage_learn::read_session(&file, adage_core::CommandTable::load(), None).unwrap();
    assert!(session.steered && !session.steps.is_empty(), "work before advice remains in the canonical session");
    let found = preview(&paths, &repo).expect("the learner, not discovery, judges per-candidate eligibility");
    assert_eq!((found.files, found.usable, found.steered, found.unreadable), (1, 0, 1, 0));
    assert!(!found.instincts_only);
    assert!(found.lines().iter().any(|l| l == STEERED_NOTE));
    assert!(!paths.index().exists(), "preview does not condition or publish an index");
}

#[test]
fn inventory_counts_files_including_duplicates_prompt_only_and_worker_history() {
    let (_temp, paths, repo, project) = fixture(true);
    let parent = project.join("parent.jsonl");
    transcript(&parent, &repo, "parent", false, None, true);
    std::fs::copy(&parent, project.join("copy.jsonl")).unwrap();
    transcript(&project.join("prompt-only.jsonl"), &repo, "prompt-only", false, None, false);
    transcript(
        &project.join("parent/subagents/workflows/unit/agent-worker.jsonl"),
        &repo,
        "parent",
        false,
        Some("worker"),
        true,
    );
    let found = preview(&paths, &repo).unwrap();
    assert_eq!((found.files, found.usable, found.steered), (4, 4, 0));
    let text = found.lines().join("\n");
    assert!(text.contains("4 trace files found"), "{text}");
    assert!(text.contains(COVERAGE_NOTE) && text.contains(INVENTORY_NOTE) && text.contains(ELIGIBILITY_NOTE));
    assert!(!text.contains("4 sessions") && !text.contains("to learn from"), "not a learned-session total: {text}");
}

#[test]
fn empty_inventory_only_offers_instincts_when_they_are_enabled() {
    for enabled in [true, false] {
        let (_temp, paths, repo, _) = fixture(enabled);
        let found = preview(&paths, &repo).expect("empty inventory is reported; conditioning owns eligibility");
        assert_eq!((found.files, found.usable, found.steered), (0, 0, 0));
        assert_eq!(found.instincts_only, enabled);
        assert_eq!(found.lines().iter().any(|l| l.contains("can bind instincts")), enabled);
    }
}
