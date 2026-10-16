use adage_core::CommandTable;
use adage_train::{compile, procedure};

#[test]
fn edited_file_template_keeps_text_without_runner_target_evidence() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(root.path().join("src")).unwrap();
    std::fs::create_dir_all(root.path().join("tests")).unwrap();
    std::fs::write(root.path().join("src/core.py"), "pass\n").unwrap();
    std::fs::write(root.path().join("tests/test_core.py"), "def test_core(): assert True\n").unwrap();
    let source = b"# Check changes\n\n1. Edit files in `src/*.py`.\n2. Run `.venv/bin/python -m pytest tests/test_{module}.py -q`.\n3. Before you finish, run `make test`.\n";
    let table = CommandTable::load();
    let graph = procedure::parse(source, "SKILL.md", "fixture", table);
    assert!(graph.slots.values().any(|s| s.from == procedure::SlotFrom::Edit));
    let result = compile::compile(graph, root.path(), table);
    assert!(result.text.values().flatten().any(|s| s.contains("runner-qualified target execution evidence")));
    assert!(result.candidates.iter().all(|c| c.route.inputs.values().all(|i| i.from.edit_tests.is_none())));
    // A concrete authored check retains its ordinary compile path.
    assert!(result.candidates.iter().any(|c| c
        .route
        .steps
        .iter()
        .any(|s| { s.run.as_ref().is_some_and(|r| r.argv == ["make", "test"]) })));
}
