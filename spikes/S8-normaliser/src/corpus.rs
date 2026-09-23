//! Runs the normaliser over the real corpus (`~/.claude/projects/**/*.jsonl`, read-only).
//!
//! Privacy per SPIKE.md: this module only ever *reads* the corpus. Aggregate statistics and
//! shapes with every literal value replaced by a placeholder go to `results/` (committed).
//! Anything with real paths/commands/text goes to `local/` (git-ignored) — used only for this
//! spike's own debugging, never committed.

use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::table::CommandTable;
use crate::template::template_argv;
use crate::tools::normalize_tool;
use crate::types::{CanonicalAction, Normalized};

fn discover(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(entries) = fs::read_dir(dir) else { return };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else if p.extension().map(|e| e == "jsonl").unwrap_or(false) {
                out.push(p);
            }
        }
    }
    walk(root, &mut out);
    out
}

fn project_to_cwd(project: &str) -> String {
    let stripped = project.strip_prefix('-').unwrap_or(project);
    format!("/{}", stripped.replace('-', "/"))
}

struct Stats {
    tool_calls: u64,
    non_unknown: u64,
    low_confidence: u64,
    by_op: HashMap<String, u64>,
    by_effect: HashMap<String, u64>,
    exact_templates: HashMap<String, u64>,
    template_templates: HashMap<String, u64>,
    unhandled_shapes: HashMap<String, u64>, // "cmd (no flag-grammar entry)" style, scrubbed
    unknown_tool_names: HashMap<String, u64>,
}

impl Stats {
    fn new() -> Self {
        Stats {
            tool_calls: 0,
            non_unknown: 0,
            low_confidence: 0,
            by_op: HashMap::new(),
            by_effect: HashMap::new(),
            exact_templates: HashMap::new(),
            template_templates: HashMap::new(),
            unhandled_shapes: HashMap::new(),
            unknown_tool_names: HashMap::new(),
        }
    }
}

fn op_name(a: &CanonicalAction) -> &'static str {
    match a {
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

/// The FULL literal key — real path/url/argv text included. Used ONLY to compute the count of
/// distinct exact commands (a number) for the "repeat rate, literal" statistic. Never printed,
/// never serialized: a list of these would be a list of the user's real commands, which must
/// stay in `local/`, not `results/`.
fn literal_key(a: &CanonicalAction) -> String {
    match a {
        CanonicalAction::Read { path, range } => format!("read {path} {range:?}"),
        CanonicalAction::Search { kind, pattern, paths } => format!("search:{kind} {pattern} {}", paths.join(",")),
        CanonicalAction::List { path } => format!("list {path}"),
        CanonicalAction::Exec { argv, cwd } => format!("exec {} @{cwd}", argv.join(" ")),
        CanonicalAction::Edit { path } => format!("edit {path}"),
        CanonicalAction::Write { path } => format!("write {path}"),
        CanonicalAction::Fetch { url } => format!("fetch {url}"),
        CanonicalAction::Unknown { raw } => format!("unknown {raw}"),
        CanonicalAction::Compound { kind, actions } => {
            format!("compound:{kind} [{}]", actions.iter().map(|a| literal_key(&a.action)).collect::<Vec<_>>().join(";"))
        }
    }
}

/// A generalised template key: typed placeholders in place of every literal argument (Design
/// Spec §11: "Arguments are generalised into typed slots"). Safe to print and commit — no
/// literal path, URL, id, or pattern text ever appears in it.
fn template_key(a: &CanonicalAction) -> String {
    match a {
        CanonicalAction::Read { range, .. } => format!("read {{path}}{}", if range.is_some() { " [range]" } else { "" }),
        CanonicalAction::Search { kind, .. } => format!("search:{kind} {{pattern}}"),
        CanonicalAction::List { .. } => "list {path}".to_string(),
        CanonicalAction::Exec { argv, .. } => {
            let templated = template_argv(argv);
            format!("exec {}", templated.join(" "))
        }
        CanonicalAction::Edit { .. } => "edit {path}".to_string(),
        CanonicalAction::Write { .. } => "write {path}".to_string(),
        CanonicalAction::Fetch { url } => {
            if url.starts_with("search:") {
                "fetch {query}".to_string()
            } else {
                "fetch {url}".to_string()
            }
        }
        CanonicalAction::Unknown { .. } => "unknown".to_string(),
        CanonicalAction::Compound { kind, actions } => {
            format!("compound:{kind} [{}]", actions.iter().map(|a| template_key(&a.action)).collect::<Vec<_>>().join(";"))
        }
    }
}

fn record_action(n: &Normalized, stats: &mut Stats) {
    stats.tool_calls += 1;
    if !n.contains_unknown() {
        stats.non_unknown += 1;
    }
    if n.low_confidence() {
        stats.low_confidence += 1;
    }
    *stats.by_op.entry(op_name(&n.action).to_string()).or_insert(0) += 1;
    *stats.by_effect.entry(n.effect.as_str().to_string()).or_insert(0) += 1;
    *stats.exact_templates.entry(literal_key(&n.action)).or_insert(0) += 1;
    *stats.template_templates.entry(template_key(&n.action)).or_insert(0) += 1;

    if let CanonicalAction::Exec { argv, .. } = &n.action {
        if !n.note.is_empty() && n.note.contains("no flag-grammar entry") {
            let head = argv.first().cloned().unwrap_or_default();
            *stats.unhandled_shapes.entry(head).or_insert(0) += 1;
        }
    }
    // Note: a Compound's nested actions are folded into its own literal/template key (see
    // `literal_key`/`template_key`) but are *not* separately re-counted here — each native tool
    // call contributes exactly one entry to these stats, whether or not it contained a loop or
    // branch, so `tool_calls_normalized` stays comparable to a native tool-call count.
}

pub fn run(table: &CommandTable) {
    let home = std::env::var("HOME").expect("HOME must be set");
    let root = Path::new(&home).join(".claude").join("projects");
    let files = discover(&root);

    let mut stats = Stats::new();
    let mut sessions = 0u64;
    let mut lines_parsed = 0u64;
    let mut lines_quarantined = 0u64;

    let mut local_log = fs::File::create("local/corpus-detail.log").ok();

    let start = Instant::now();

    for f in &files {
        sessions += 1;
        let Ok(content) = fs::read_to_string(f) else { continue };
        let project = f
            .parent()
            .and_then(|p| p.file_name())
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        // walk up to the top-level project dir name (first component under projects/)
        let rel = f.strip_prefix(&root).ok();
        let top_project = rel
            .and_then(|r| r.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or(project);
        let default_cwd = project_to_cwd(&top_project);

        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let v: Value = match serde_json::from_str(line) {
                Ok(v) => {
                    lines_parsed += 1;
                    v
                }
                Err(_) => {
                    lines_quarantined += 1;
                    continue;
                }
            };
            if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
                continue;
            }
            let cwd = v.get("cwd").and_then(|c| c.as_str()).unwrap_or(&default_cwd).to_string();
            let Some(content) = v.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) else {
                continue;
            };
            for block in content {
                if block.get("type").and_then(|t| t.as_str()) != Some("tool_use") {
                    continue;
                }
                let name = block.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let empty = Value::Null;
                let input = block.get("input").unwrap_or(&empty);
                let normalized = normalize_tool(name, input, &cwd, table);
                if normalized.is_empty() {
                    // e.g. a Bash call that was pure `cd` — not a coverage failure, just no
                    // action. Still counts toward throughput below.
                    continue;
                }
                for n in &normalized {
                    record_action(n, &mut stats);
                    if n.is_unknown() {
                        *stats.unknown_tool_names.entry(name.to_string()).or_insert(0) += 1;
                    }
                }
                if let Some(f) = &mut local_log {
                    let _ = writeln!(f, "{name}\t{}", serde_json::to_string(&normalized).unwrap_or_default());
                }
            }
        }
    }

    let elapsed = start.elapsed();
    let throughput = stats.tool_calls as f64 / elapsed.as_secs_f64().max(1e-6);

    // ---- write aggregate results (safe to commit: counts and scrubbed shapes only) ----
    let mut by_op: Vec<_> = stats.by_op.iter().collect();
    by_op.sort_by(|a, b| b.1.cmp(a.1));
    let mut by_effect: Vec<_> = stats.by_effect.iter().collect();
    by_effect.sort_by(|a, b| b.1.cmp(a.1));
    let mut top_exact: Vec<_> = stats.exact_templates.iter().collect();
    top_exact.sort_by(|a, b| b.1.cmp(a.1));
    let mut top_template: Vec<_> = stats.template_templates.iter().collect();
    top_template.sort_by(|a, b| b.1.cmp(a.1));
    let mut top_unhandled: Vec<_> = stats.unhandled_shapes.iter().collect();
    top_unhandled.sort_by(|a, b| b.1.cmp(a.1));
    let mut top_unknown_tools: Vec<_> = stats.unknown_tool_names.iter().collect();
    top_unknown_tools.sort_by(|a, b| b.1.cmp(a.1));

    let coverage = stats.non_unknown as f64 / stats.tool_calls.max(1) as f64;
    let low_conf_rate = stats.low_confidence as f64 / stats.tool_calls.max(1) as f64;

    // Repeat rate, matching the PoC's definition exactly (research/poc/src/analyze.ts
    // `repeated()`): the share of actions whose key was seen more than once — not "1 -
    // distinct/total", which is a different (and here, differently-biased) statistic.
    let repeat_rate = |m: &HashMap<String, u64>| -> f64 {
        let repeated: u64 = m.values().filter(|&&v| v > 1).sum();
        repeated as f64 / stats.tool_calls.max(1) as f64
    };
    let repeat_exact = repeat_rate(&stats.exact_templates);
    let repeat_template = repeat_rate(&stats.template_templates);
    let _ = top_exact; // literal top list intentionally never serialized (privacy)

    let summary = serde_json::json!({
        "sessions_scanned": sessions,
        "lines_parsed": lines_parsed,
        "lines_quarantined": lines_quarantined,
        "tool_calls_normalized": stats.tool_calls,
        "non_unknown_rate": coverage,
        "low_confidence_rate": low_conf_rate,
        "by_op": by_op,
        "by_effect": by_effect,
        "distinct_exact_commands": stats.exact_templates.len(),
        "distinct_generalised_templates": stats.template_templates.len(),
        "repeat_rate_exact": repeat_exact,
        "repeat_rate_template": repeat_template,
        "top_generalised_templates": top_template.into_iter().take(25).collect::<Vec<_>>(),
        "top_unhandled_commands": top_unhandled.into_iter().take(25).collect::<Vec<_>>(),
        "unrecognised_native_tools": top_unknown_tools,
        "throughput_calls_per_sec": throughput,
        "elapsed_secs": elapsed.as_secs_f64(),
    });

    fs::create_dir_all("results").ok();
    fs::write("results/corpus-summary.json", serde_json::to_string_pretty(&summary).unwrap()).unwrap();

    println!("{}", serde_json::to_string_pretty(&summary).unwrap());
    println!("\n(full per-call detail, from real transcripts, written to local/corpus-detail.log — git-ignored)");
}
