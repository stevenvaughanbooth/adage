//! Harness simulator. One scenario (the acme-billing sandbox, `demo/SANDBOX.md`), rendered as the
//! session history each harness writes to disk and as the hook payloads each harness sends:
//!
//! | Harness | History | Hooks |
//! | --- | --- | --- |
//! | Claude Code | `projects/<project>/<session>.jsonl` transcripts | command hooks, `PostToolBatch` per turn |
//! | OpenCode | `opencode export` JSON | Adage's plugin → `adage hook opencode`, and the plugin itself |
//! | Codex | `sessions/YYYY/MM/DD/rollout-*.jsonl` | command hooks, `PostToolUse` per call |
//!
//! Everything is deterministic: `adage-sim <dir>` rewrites the committed fixtures in
//! `sim/fixtures/` byte for byte, and a test fails if they are stale. Formats and their sources
//! are recorded in `docs/harnesses.md`.

pub mod claude;
pub mod codex;
pub mod opencode;
pub mod scenario;

use std::path::{Path, PathBuf};

/// Seeds for each harness's history: the same scenario, phrased and paced differently.
pub const CLAUDE_SEED: u64 = 11;
pub const OPENCODE_SEED: u64 = 22;
pub const CODEX_SEED: u64 = 33;

/// The committed fixture directory, `sim/fixtures`.
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../sim/fixtures")
}

/// Every fixture file: (path relative to the fixture directory, contents).
pub fn render_all() -> Vec<(String, String)> {
    let cwd = scenario::CWD;
    let mut out = Vec::new();
    for s in scenario::history(CLAUDE_SEED) {
        out.push((
            format!("claude/projects/{}/{}.jsonl", claude::project_dir(cwd), claude::session_id(&s)),
            claude::transcript(&s, cwd),
        ));
    }
    for s in scenario::history(OPENCODE_SEED) {
        let v = opencode::export(&s, cwd);
        out.push((format!("opencode/{}", opencode::file_name(&s)), serde_json::to_string_pretty(&v).unwrap() + "\n"));
    }
    for s in scenario::history(CODEX_SEED) {
        out.push((format!("codex/{}", codex::rel_path(&s)), codex::rollout(&s, cwd)));
    }
    out.sort();
    out
}

/// Write every fixture under `dir`.
pub fn write_all(dir: &Path) -> std::io::Result<usize> {
    let files = render_all();
    for (rel, body) in &files {
        let p = dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap())?;
        std::fs::write(p, body)?;
    }
    Ok(files.len())
}

/// Fixture files for one harness (`claude`, `opencode`, `codex`), absolute, sorted.
pub fn fixture_files(harness: &str) -> Vec<PathBuf> {
    let root = fixtures_dir();
    render_all()
        .into_iter()
        .filter(|(rel, _)| rel.starts_with(&format!("{harness}/")))
        .map(|(rel, _)| root.join(rel))
        .collect()
}
