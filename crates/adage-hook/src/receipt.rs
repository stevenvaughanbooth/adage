//! First and recent confirmed hook delivery per repository and harness, even before reflexes exist.
//! A tiny receipt contains no prompts, transcript paths, commands or session identifiers.
use adage_store::Paths;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub first_seen_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_ms: Option<u64>,
}

impl Receipt {
    /// Receipts from the first-delivery-only format remain useful evidence.
    pub fn last_seen(&self) -> u64 {
        self.last_seen_ms.unwrap_or(self.first_seen_ms)
    }
}

fn key(root: &Path, harness: &str) -> String {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    format!("{}-{}", adage_core::repo::short_hash(&root.to_string_lossy()), harness)
}

pub fn read(paths: &Paths, root: &Path, harness: &str) -> Option<Receipt> {
    serde_json::from_slice(&std::fs::read(paths.hook_receipt(&key(root, harness))).ok()?).ok()
}

/// Best effort: an existing receipt costs a metadata lookup; its timestamp is refreshed at
/// most once per minute in the normal case. No hook waits for another writer or opens a spool.
/// Concurrent hooks may race at a minute boundary; atomic replacement keeps the receipt whole.
pub(crate) fn record(paths: &Paths, root: &Path, harness: &str) -> std::io::Result<()> {
    record_at(paths, root, harness, adage_core::time::now_ms())
}

fn record_at(paths: &Paths, root: &Path, harness: &str, now: u64) -> std::io::Result<()> {
    const INTERVAL_MS: u64 = 60_000;
    let path = paths.hook_receipt(&key(root, harness));
    // This is the common path, including when no reflex or session state exists yet.
    if let Ok(metadata) = std::fs::symlink_metadata(&path) {
        if !metadata.file_type().is_file() {
            return Ok(());
        }
        if metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .is_some_and(|t| now.saturating_sub(t.as_millis() as u64) < INTERVAL_MS)
        {
            return Ok(());
        }
    }
    let old = read(paths, root, harness);
    // Also throttle by the persisted timestamp across clock changes or a concurrent refresh.
    if old.as_ref().is_some_and(|r| now.saturating_sub(r.last_seen()) < INTERVAL_MS) {
        return Ok(());
    }
    let receipt = Receipt { first_seen_ms: old.as_ref().map_or(now, |r| r.first_seen_ms), last_seen_ms: Some(now) };
    adage_store::config::write_atomic(&path, &serde_json::to_vec(&receipt)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn an_empty_index_confirms_delivery_and_preserves_unrouted_bookkeeping() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("repo");
        std::fs::create_dir_all(root.join(".git")).unwrap();
        let paths = Paths::at(temp.path().join("data"));
        let payload = serde_json::json!({"session_id":"new-session", "cwd": root, "prompt":"hello"}).to_string();
        let output = crate::handle(
            "codex",
            "UserPromptSubmit",
            &payload,
            crate::Options::default(),
            &paths,
            std::time::Instant::now(),
        )
        .unwrap();
        assert!(output.is_none());
        assert!(read(&paths, &root, "codex").is_some());
        let records: Vec<serde_json::Value> = adage_store::spool::read_all(&paths.spool("new-session")).unwrap();
        assert_eq!(records.len(), 1, "the newer startup bookkeeping is preserved");
        assert_eq!(records[0]["unrouted"], true);
        assert!(records[0].get("decision").is_none());
        assert!(!paths.state("new-session").exists());
        let disabled = Paths::at(temp.path().join("disabled-data"));
        std::fs::create_dir(root.join(".adage")).unwrap();
        std::fs::write(root.join(".adage/disabled"), "").unwrap();
        crate::handle(
            "codex",
            "UserPromptSubmit",
            &payload,
            crate::Options::default(),
            &disabled,
            std::time::Instant::now(),
        )
        .unwrap();
        assert!(read(&disabled, &root, "codex").is_none());
    }

    #[test]
    fn refresh_is_throttled_atomic_and_preserves_the_first_delivery() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::at(tmp.path());
        let root = tmp.path().join("repo");
        let now = adage_core::time::now_ms();
        record_at(&paths, &root, "codex", now).unwrap();
        let path = paths.hook_receipt(&key(&root, "codex"));
        let before = std::fs::read(&path).unwrap();
        record_at(&paths, &root, "codex", now + 30_000).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), before);
        record_at(&paths, &root, "codex", now + 61_000).unwrap();
        let updated = read(&paths, &root, "codex").unwrap();
        assert_eq!(updated.first_seen_ms, now);
        assert_eq!(updated.last_seen(), now + 61_000);
        assert_eq!(std::fs::read_dir(path.parent().unwrap()).unwrap().count(), 1, "no temporary file remains");
        let legacy: Receipt = serde_json::from_str("{\"first_seen_ms\":7}").unwrap();
        assert_eq!(legacy.last_seen(), 7);
    }

    #[test]
    fn first_delivery_is_stable_and_repository_scoped() {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths::at(tmp.path());
        let root = tmp.path().join("repo");
        record(&paths, &root, "codex").unwrap();
        let first = read(&paths, &root, "codex").unwrap().first_seen_ms;
        record(&paths, &root, "codex").unwrap();
        assert_eq!(read(&paths, &root, "codex").unwrap().first_seen_ms, first);
        assert!(read(&paths, &root, "claude-code").is_none());
        assert!(read(&paths, &tmp.path().join("other"), "codex").is_none());
        let bytes = std::fs::read_to_string(paths.hook_receipt(&key(&root, "codex"))).unwrap();
        assert!(!bytes.contains("repo"));
    }
}
