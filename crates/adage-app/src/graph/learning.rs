//! Conditioning input counts are distinct from recorded hook activity. Keep the most recent
//! retained successful run for each repository; adding runs would count the same history again.

use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn summary(runs: &[Value]) -> Value {
    let mut repositories = BTreeMap::<String, Value>::new();
    let mut batches = BTreeMap::<u64, BTreeSet<&str>>::new();
    for run in runs.iter().filter(|r| r["kind"] == "condition") {
        if let (Some(ts), Some(repo)) = (run["ts"].as_u64(), run["repo"].as_str()) {
            batches.entry(ts).or_default().insert(repo);
        }
    }
    for run in runs {
        if !matches!(run["kind"].as_str(), None | Some("condition" | "learn")) {
            continue;
        }
        let explicit = run["repo"].as_str().filter(|repo| !repo.is_empty());
        let reports = run["repos"].as_object();
        let mut names: Vec<&str> = reports.into_iter().flat_map(|repos| repos.keys().map(String::as_str)).collect();
        names.extend(explicit);
        names.sort_unstable();
        names.dedup();
        for repo in names {
            let ts = run["ts"].as_u64().unwrap_or(0);
            if repositories.get(repo).is_some_and(|old| old["last_conditioned_at"].as_u64().unwrap_or(0) > ts) {
                continue;
            }
            // Global input counts cannot be attributed to each repository of an --all run.
            // A per-repository report does carry the count used for that repository.
            let one_repo = explicit == Some(repo) || (explicit.is_none() && reports.is_some_and(|r| r.len() == 1));
            let scoped = one_repo
                && match run["input_scope"].as_str() {
                    Some("repository") => true,
                    Some(_) => false,
                    None => !batches.get(&ts).is_some_and(|repos| repos.len() > 1),
                };
            let count = |key: &str| scoped.then(|| run[key].as_u64()).flatten();
            let used = run["repos"][repo]["sessions"].as_u64().or_else(|| count("sessions_used"));
            repositories.insert(
                repo.into(),
                json!({
                    "repo": repo,
                    "last_conditioned_at": ts,
                    "sessions_used": used,
                    "files_read": count("sessions_read"),
                    "sessions_steered": count("sessions_steered"),
                    "sessions_duplicates": count("sessions_duplicates"),
                }),
            );
        }
    }
    json!({
        "source": "recent_conditioning_runs",
        "description": "The latest successful conditioning run retained in the recent log for each repository. These counts describe learning input, including supported subagent histories; recorded activity only contains sessions seen by Adage's hooks. Runs overlap and must not be added. Missing counts were not recorded.",
        "repositories": repositories.into_values().collect::<Vec<_>>(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latest_run_counts_are_not_added_or_erased_by_errors() {
        let history = summary(&[
            json!({"kind":"condition", "repo":"acme", "ts":20, "sessions_read":100, "sessions_used":80}),
            json!({"kind":"condition", "repo":"acme", "ts":10, "sessions_read":90, "sessions_used":70}),
            json!({"kind":"condition_error", "repo":"acme", "ts":30}),
        ]);
        assert_eq!(history["repositories"].as_array().unwrap().len(), 1);
        let row = &history["repositories"][0];
        assert_eq!(row["sessions_used"], 80);
        assert_eq!(row["files_read"], 100);
        assert_eq!(row["last_conditioned_at"], 20);
        assert!(row["sessions_duplicates"].is_null());
    }

    #[test]
    fn global_input_counts_are_not_repeated_for_each_repository() {
        let history = summary(&[json!({"ts":10, "sessions_read":100, "sessions_used":90,
            "sessions_steered":3, "repos":{"acme":{"sessions":60}, "billing":{"sessions":30}}})]);
        let rows = history["repositories"].as_array().unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0]["sessions_used"], 60);
        assert_eq!(rows[1]["sessions_used"], 30);
        assert!(rows.iter().all(|row| row["files_read"].is_null() && row["sessions_steered"].is_null()));
    }

    #[test]
    fn manual_batches_and_partial_run_logs_keep_global_counts_unattributed() {
        let history = summary(&[
            json!({"kind":"condition", "repo":"acme", "ts":10, "sessions_read":100,
                "sessions_used":60, "repos":{"acme":{"sessions":60}}}),
            json!({"kind":"condition", "repo":"billing", "ts":10, "sessions_read":100,
                "sessions_used":30, "repos":{"billing":{"sessions":30}}}),
            // New logs retain their scope even when other lines of the batch have rotated out.
            json!({"kind":"condition", "repo":"partial", "ts":20, "input_scope":"run",
                "sessions_read":200, "sessions_used":70, "repos":{"partial":{"sessions":70}}}),
        ]);
        let rows = history["repositories"].as_array().unwrap();
        assert_eq!(rows.iter().map(|r| r["sessions_used"].as_u64().unwrap()).collect::<Vec<_>>(), vec![60, 30, 70]);
        assert!(rows.iter().all(|row| row["files_read"].is_null()));
    }

    #[test]
    fn zero_and_unknown_are_distinct_and_legacy_single_repo_counts_work() {
        let history = summary(&[
            json!({"ts":10, "sessions_read":8, "sessions_used":6, "repos":{"acme":{"sessions":6}}}),
            json!({"kind":"learn", "repo":"empty", "ts":20, "sessions_read":0, "sessions_used":0}),
            json!({"kind":"condition", "repo":"unknown", "ts":20}),
        ]);
        let rows = history["repositories"].as_array().unwrap();
        assert_eq!(rows[0]["files_read"], 8);
        assert_eq!(rows[1]["sessions_used"], 0);
        assert!(rows[2]["sessions_used"].is_null());
    }
}
