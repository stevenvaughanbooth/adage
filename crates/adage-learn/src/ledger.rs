//! The ledger (docs/zero-config.md §4.2): one [`SessionSummary`] per trace of a repository, what
//! the savings estimate is computed from.
//!
//! `$ADAGE_HOME/ledger/<slug>.jsonl` holds one line per session, replaced by `session_id`. A line
//! holds counts, model ids and times only: no prompt, command or path. It is joined to the hook
//! spools by session id (a spool is named by the Claude Code or OpenCode session id, or the Codex
//! thread id). `<slug>.seen.json` remembers, per transcript (by a hash of its path), the size and
//! modification time it was summarised at, so a trace is read again only when it grows.
//!
//! What each harness gives:
//!
//! | Harness | Tokens | Cost |
//! | --- | --- | --- |
//! | Claude Code | assistant `message.usage`, deduplicated by `message.id` (the last record of an id wins) | priced from the pinned table ([`adage_core::pricing`]), or none for an unlisted model |
//! | Codex | the last non-null `token_count.info.total_token_usage` | none (not an Anthropic model; tokens and turns only) |
//! | OpenCode | each assistant message's `tokens` | the harness's own `cost` |
//!
//! Sub-agent transcripts and sidechain records are left out in v0.1: the parent session's own
//! turns and tokens only.

use adage_core::pricing::{self, Tokens};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Where a session's dollar figure came from.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cost {
    /// API-list-equivalent US dollars; `None` when the source is `none`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usd: Option<f64>,
    /// `harness` (reported by the harness), `priced@<date>` (the pinned table of that date), or
    /// `none` (a model the table does not list: tokens and turns only).
    pub source: String,
}

/// One trace, summarised.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    /// `claude-code`, `codex` or `opencode`.
    pub harness: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub harness_version: String,
    pub models: Vec<String>,
    /// Milliseconds since the Unix epoch of the first and last records (0 if unknown).
    pub started_at: u64,
    pub ended_at: u64,
    /// Model responses (Claude Code: distinct assistant message ids).
    pub model_turns: u32,
    pub tool_calls: u32,
    /// The model turn (0-based) of the first file edit, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub first_edit_turn: Option<u32>,
    pub tokens: Tokens,
    pub cost: Cost,
    /// The agent's last message asked the person for help (the outcome proxy of
    /// [`crate::import::asks_for_help`]): the quality guardrail shown beside savings.
    pub asked_for_help: bool,
}

/// Parse `YYYY-MM-DDTHH:MM:SS[.fff]Z` into milliseconds since the Unix epoch.
pub fn parse_ts(s: &str) -> Option<u64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[10] != b'T' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, mo, d, h, mi, se) = (n(0..4)?, n(5..7)?, n(8..10)?, n(11..13)?, n(14..16)?, n(17..19)?);
    let ms = if b.get(19) == Some(&b'.') {
        let frac: String = s[20..].chars().take_while(|c| c.is_ascii_digit()).collect();
        format!("{frac:0<3}")[..3].parse::<i64>().unwrap_or(0)
    } else {
        0
    };
    // Days from the civil date (Howard Hinnant's algorithm).
    let (y2, m2) = if mo <= 2 { (y - 1, mo + 9) } else { (y, mo - 3) };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * m2 + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let t = ((days * 24 + h) * 60 + mi) * 60 + se;
    u64::try_from(t * 1000 + ms).ok()
}

fn is_edit_tool(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "edit" | "write" | "multiedit" | "notebookedit" | "apply_patch" | "patch" | "str_replace_based_edit_tool"
    )
}

fn priced(models: &[String], by_model: &BTreeMap<String, Tokens>) -> Cost {
    let table = pricing::table();
    let mut usd = 0.0;
    for (m, t) in by_model {
        match table.cost(m, t) {
            Some(c) => usd += c,
            // One unpriced model makes the whole session unpriced: never a partial figure.
            None if t.total() > 0 => return Cost { usd: None, source: "none".into() },
            None => {}
        }
    }
    if models.is_empty() || by_model.values().all(|t| t.total() == 0) {
        return Cost { usd: None, source: "none".into() };
    }
    Cost { usd: Some(usd), source: format!("priced@{}", table.as_of) }
}

/// Summarise a Claude Code transcript. `None` for a sub-agent's transcript or a file with no
/// session.
pub fn claude(path: &Path) -> Option<SessionSummary> {
    use std::io::BufRead;
    if path.parent().and_then(|p| p.file_name()).is_some_and(|n| n == "subagents") {
        return None;
    }
    let reader = std::io::BufReader::new(std::fs::File::open(path).ok()?);
    let mut id = String::new();
    let mut version = String::new();
    let (mut started, mut ended) = (0u64, 0u64);
    // message id → (model, usage): the last record of an id carries its final usage.
    let mut usage: BTreeMap<String, (String, Tokens)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut tools: BTreeSet<String> = BTreeSet::new();
    let mut first_edit: Option<u32> = None;
    let mut last_reply: Option<String> = None;
    for line in reader.lines() {
        let Ok(line) = line else { break };
        let Ok(rec) = serde_json::from_str::<Value>(&line) else { continue };
        if rec.get("isSidechain").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if id.is_empty() {
            if let Some(s) = rec.get("sessionId").and_then(Value::as_str) {
                id = s.to_string();
            }
        }
        if let Some(v) = rec.get("version").and_then(Value::as_str) {
            version = v.to_string();
        }
        if let Some(t) = rec.get("timestamp").and_then(Value::as_str).and_then(parse_ts) {
            if started == 0 {
                started = t;
            }
            ended = ended.max(t);
        }
        if rec.get("type").and_then(Value::as_str) != Some("assistant") {
            if rec.get("type").and_then(Value::as_str) == Some("user")
                && rec["message"]["content"].as_array().is_some_and(|a| a.iter().any(|i| i["type"] == "tool_result"))
            {
                last_reply = None;
            }
            continue;
        }
        let msg = &rec["message"];
        let model = msg.get("model").and_then(Value::as_str).unwrap_or("").to_string();
        if model == "<synthetic>" {
            continue;
        }
        let mid = msg.get("id").and_then(Value::as_str).unwrap_or("").to_string();
        if mid.is_empty() {
            continue;
        }
        if !usage.contains_key(&mid) {
            order.push(mid.clone());
        }
        let turn = order.iter().position(|m| m == &mid).unwrap_or(0) as u32;
        let u = &msg["usage"];
        let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
        let t = Tokens {
            input: n("input_tokens"),
            output: n("output_tokens"),
            cache_write: n("cache_creation_input_tokens"),
            cache_read: n("cache_read_input_tokens"),
            // Written with the 1-hour TTL, priced at its own rate (2x input, not 1.25x).
            cache_write_1h: u["cache_creation"]["ephemeral_1h_input_tokens"].as_u64().unwrap_or(0),
        };
        let entry = usage.entry(mid).or_insert_with(|| (model.clone(), Tokens::default()));
        if t.total() > 0 {
            entry.1 = t;
        }
        for it in msg.get("content").and_then(Value::as_array).into_iter().flatten() {
            match it.get("type").and_then(Value::as_str) {
                Some("tool_use") => {
                    let tid = it.get("id").and_then(Value::as_str).unwrap_or("").to_string();
                    tools.insert(if tid.is_empty() { format!("{turn}/{}", tools.len()) } else { tid });
                    if first_edit.is_none() && it.get("name").and_then(Value::as_str).is_some_and(is_edit_tool) {
                        first_edit = Some(turn);
                    }
                    last_reply = None;
                }
                Some("text") => {
                    if let Some(t) = it.get("text").and_then(Value::as_str).filter(|t| !t.trim().is_empty()) {
                        last_reply = Some(adage_core::tokens::bounded(t, 4096).to_string());
                    }
                }
                _ => {}
            }
        }
    }
    if id.is_empty() || order.is_empty() {
        return None;
    }
    let mut by_model: BTreeMap<String, Tokens> = BTreeMap::new();
    let mut total = Tokens::default();
    for (m, t) in usage.values() {
        by_model.entry(m.clone()).or_default().add(t);
        total.add(t);
    }
    let models: Vec<String> = by_model.keys().filter(|m| !m.is_empty()).cloned().collect();
    Some(SessionSummary {
        session_id: id,
        harness: crate::import::CLAUDE_CODE.into(),
        harness_version: version,
        cost: priced(&models, &by_model),
        models,
        started_at: started,
        ended_at: ended,
        model_turns: order.len() as u32,
        tool_calls: tools.len() as u32,
        first_edit_turn: first_edit,
        tokens: total,
        asked_for_help: last_reply.as_deref().is_some_and(crate::import::asks_for_help),
    })
}

/// Summarise a Codex rollout (tokens and turns; Codex models are not priced).
pub fn codex(path: &Path) -> Option<SessionSummary> {
    let lines = crate::codex::read_rollout(path)?;
    let mut s = SessionSummary {
        session_id: String::new(),
        harness: "codex".into(),
        harness_version: String::new(),
        models: vec![],
        started_at: 0,
        ended_at: 0,
        model_turns: 0,
        tool_calls: 0,
        first_edit_turn: None,
        tokens: Tokens::default(),
        cost: Cost { usd: None, source: "none".into() },
        asked_for_help: false,
    };
    let mut last_reply: Option<String> = None;
    for v in &lines {
        if let Some(t) = v.get("timestamp").and_then(Value::as_str).and_then(parse_ts) {
            if s.started_at == 0 {
                s.started_at = t;
            }
            s.ended_at = s.ended_at.max(t);
        }
        let p = &v["payload"];
        match (v["type"].as_str(), p["type"].as_str()) {
            (Some("session_meta"), _) => {
                s.session_id = p["id"].as_str().unwrap_or("").to_string();
                s.harness_version = p["cli_version"].as_str().unwrap_or("").to_string();
            }
            (Some("turn_context"), _) => {
                if let Some(m) = p["model"].as_str() {
                    if !s.models.iter().any(|x| x == m) {
                        s.models.push(m.to_string());
                    }
                }
            }
            (Some("event_msg"), Some("token_count")) => {
                s.model_turns += 1;
                let u = &p["info"]["total_token_usage"];
                if u.is_object() {
                    let n = |k: &str| u.get(k).and_then(Value::as_u64).unwrap_or(0);
                    let cached = n("cached_input_tokens");
                    s.tokens = Tokens {
                        input: n("input_tokens").saturating_sub(cached),
                        output: n("output_tokens"),
                        cache_read: cached,
                        ..Default::default()
                    };
                }
            }
            (Some("event_msg"), Some("agent_message")) => last_reply = p["message"].as_str().map(str::to_string),
            (Some("response_item"), Some(k)) if k.ends_with("_call") && !k.ends_with("_output") => {
                s.tool_calls += 1;
                let name = p["name"].as_str().unwrap_or("");
                let patch = name == "apply_patch" || p["arguments"].as_str().is_some_and(|a| a.contains("apply_patch"));
                if patch && s.first_edit_turn.is_none() {
                    s.first_edit_turn = Some(s.model_turns);
                }
            }
            _ => {}
        }
    }
    if s.session_id.is_empty() {
        return None;
    }
    s.asked_for_help = last_reply.as_deref().is_some_and(crate::import::asks_for_help);
    Some(s)
}

/// Summarise an `opencode export` file (the harness reports its own cost).
pub fn opencode(path: &Path) -> Option<SessionSummary> {
    let v = crate::opencode::read_export(path)?;
    let info = &v["info"];
    let id = info["id"].as_str()?.to_string();
    let mut s = SessionSummary {
        session_id: id,
        harness: crate::import::OPENCODE.into(),
        harness_version: info["version"].as_str().unwrap_or("").to_string(),
        models: vec![],
        started_at: info["time"]["created"].as_u64().unwrap_or(0),
        ended_at: info["time"]["updated"].as_u64().unwrap_or(0),
        model_turns: 0,
        tool_calls: 0,
        first_edit_turn: None,
        tokens: Tokens::default(),
        cost: Cost { usd: None, source: "none".into() },
        asked_for_help: false,
    };
    let mut cost = 0.0;
    let mut costed = false;
    let mut last_reply: Option<String> = None;
    for m in v["messages"].as_array().into_iter().flatten() {
        let mi = &m["info"];
        if mi["role"] != "assistant" {
            continue;
        }
        let turn = s.model_turns;
        s.model_turns += 1;
        if let Some(model) = mi["modelID"].as_str() {
            if !s.models.iter().any(|x| x == model) {
                s.models.push(model.to_string());
            }
        }
        if let Some(c) = mi["cost"].as_f64() {
            cost += c;
            costed = true;
        }
        let t = &mi["tokens"];
        let n = |v: &Value| v.as_u64().unwrap_or(0);
        s.tokens.add(&Tokens {
            input: n(&t["input"]),
            output: n(&t["output"]) + n(&t["reasoning"]),
            cache_write: n(&t["cache"]["write"]),
            cache_read: n(&t["cache"]["read"]),
            ..Default::default()
        });
        for p in m["parts"].as_array().into_iter().flatten() {
            match p["type"].as_str() {
                Some("tool") => {
                    s.tool_calls += 1;
                    if s.first_edit_turn.is_none() && p["tool"].as_str().is_some_and(is_edit_tool) {
                        s.first_edit_turn = Some(turn);
                    }
                    last_reply = None;
                }
                Some("text") => last_reply = p["text"].as_str().map(str::to_string),
                _ => {}
            }
        }
    }
    if costed {
        s.cost = Cost { usd: Some(cost), source: "harness".into() };
    }
    s.asked_for_help = last_reply.as_deref().is_some_and(crate::import::asks_for_help);
    Some(s)
}

/// Summarise any supported trace file (recognised by content, as `crate::read_session` does).
pub fn summarise(path: &Path) -> Option<SessionSummary> {
    if crate::codex::read_rollout(path).is_some() {
        return codex(path);
    }
    if crate::opencode::read_export(path).is_some() {
        return opencode(path);
    }
    claude(path)
}

/// The ledger files of one repository.
pub struct Ledger {
    dir: PathBuf,
    slug: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Seen {
    /// Hash of the transcript's path → `[size, mtime_ns]` and the session id summarised.
    files: BTreeMap<String, (u64, u128, String)>,
    /// The [`SUMMARY_REVISION`] the files were summarised with: an older one summarises them all
    /// again (so a pricing fix reaches sessions already in the ledger).
    #[serde(default)]
    revision: u32,
}

/// How sessions are summarised and priced; bumped when that changes. 1: 1-hour cache writes
/// priced at their own rate.
pub const SUMMARY_REVISION: u32 = 1;

fn path_hash(p: &Path) -> String {
    let d = Sha256::digest(p.to_string_lossy().as_bytes());
    d.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn stamp(p: &Path) -> Option<(u64, u128)> {
    let m = std::fs::metadata(p).ok()?;
    let t = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_nanos();
    Some((m.len(), t))
}

impl Ledger {
    /// `dir` is `$ADAGE_HOME/ledger`; `repo` the repository key.
    pub fn new(dir: &Path, repo: &str) -> Ledger {
        Ledger { dir: dir.to_path_buf(), slug: crate::slug(repo) }
    }
    pub fn path(&self) -> PathBuf {
        self.dir.join(format!("{}.jsonl", self.slug))
    }
    fn seen_path(&self) -> PathBuf {
        self.dir.join(format!("{}.seen.json", self.slug))
    }
    /// The summary written beside the ledger (`<slug>.summary.json`), which every surface reads.
    pub fn summary_path(&self) -> PathBuf {
        self.dir.join(format!("{}.summary.json", self.slug))
    }

    /// Every session in the ledger, by id.
    pub fn read(&self) -> BTreeMap<String, SessionSummary> {
        read_file(&self.path())
    }

    /// Summarise `transcripts` (this repository's, already scoped) into the ledger: a transcript
    /// unchanged since it was last summarised is not read again. Returns the ledger. Best effort:
    /// a ledger that cannot be written is recomputed next time.
    pub fn update(&self, transcripts: &[PathBuf]) -> BTreeMap<String, SessionSummary> {
        let mut all = self.read();
        let mut seen: Seen =
            std::fs::read(self.seen_path()).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if seen.revision != SUMMARY_REVISION {
            seen = Seen { revision: SUMMARY_REVISION, ..Seen::default() };
        }
        let mut changed = false;
        for p in transcripts {
            let Some((size, mtime)) = stamp(p) else { continue };
            let h = path_hash(p);
            if seen.files.get(&h).is_some_and(|(s, m, id)| *s == size && *m == mtime && all.contains_key(id)) {
                continue;
            }
            if let Some(s) = summarise(p) {
                seen.files.insert(h, (size, mtime, s.session_id.clone()));
                all.insert(s.session_id.clone(), s);
                changed = true;
            }
        }
        if changed {
            let _ = std::fs::create_dir_all(&self.dir);
            let body: String = all.values().filter_map(|s| serde_json::to_string(s).ok()).map(|l| l + "\n").collect();
            let _ = adage_store::config::write_atomic(&self.path(), body.as_bytes());
            if let Ok(b) = serde_json::to_vec(&seen) {
                let _ = adage_store::config::write_atomic(&self.seen_path(), &b);
            }
        }
        all
    }
}

/// A ledger file, by session id (a later line for an id replaces an earlier one).
pub fn read_file(p: &Path) -> BTreeMap<String, SessionSummary> {
    std::fs::read_to_string(p)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<SessionSummary>(l).ok())
        .map(|s| (s.session_id.clone(), s))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn timestamps_parse() {
        assert_eq!(parse_ts("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_ts("2026-09-14T19:06:07.551Z"), Some(1_789_412_767_551));
        assert_eq!(parse_ts("nope"), None);
    }

    fn write(d: &Path, name: &str, lines: &[Value]) -> PathBuf {
        let p = d.join(name);
        std::fs::write(&p, lines.iter().map(|l| format!("{l}\n")).collect::<String>()).unwrap();
        p
    }

    #[test]
    fn claude_usage_is_deduplicated_by_message_and_priced() {
        let d = tempfile::tempdir().unwrap();
        let usage = json!({"input_tokens": 10, "output_tokens": 100, "cache_read_input_tokens": 1000, "cache_creation_input_tokens": 0});
        let a = |mid: &str, content: Value, u: &Value| json!({"type":"assistant","sessionId":"s1","version":"2.1.87","timestamp":"2026-09-14T19:06:07.000Z","message":{"id":mid,"model":"claude-sonnet-4-6","content":content,"usage":u}});
        let p = write(
            d.path(),
            "s1.jsonl",
            &[
                json!({"type":"user","sessionId":"s1","timestamp":"2026-09-14T19:06:00.000Z","message":{"content":"hi"}}),
                // Two records of one response (a thinking block, then the tool call).
                a("m1", json!([{"type":"text","text":"Looking."}]), &usage),
                a("m1", json!([{"type":"tool_use","id":"t1","name":"Read","input":{}}]), &usage),
                a("m2", json!([{"type":"tool_use","id":"t2","name":"Edit","input":{}}]), &usage),
                a("m3", json!([{"type":"text","text":"Could you tell me which customer this is for?"}]), &usage),
                json!({"type":"assistant","isSidechain":true,"sessionId":"s1","message":{"id":"side","model":"claude-sonnet-4-6","usage":usage}}),
            ],
        );
        let s = claude(&p).unwrap();
        assert_eq!((s.model_turns, s.tool_calls, s.first_edit_turn), (3, 2, Some(1)));
        assert_eq!(s.tokens, Tokens { input: 30, output: 300, cache_read: 3000, ..Default::default() });
        assert_eq!(s.harness_version, "2.1.87");
        assert!(s.cost.source.starts_with("priced@"));
        let want = (30.0 * 3.0 + 300.0 * 15.0 + 3000.0 * 0.3) / 1e6;
        assert!((s.cost.usd.unwrap() - want).abs() < 1e-12);
        assert!(s.asked_for_help);
        let line = serde_json::to_string(&s).unwrap();
        assert!(!line.contains("Could you") && !line.contains(&d.path().to_string_lossy().to_string()), "{line}");
    }

    /// Launch report, bug 10: the ledger read about 10% under Claude Code's own `total_cost_usd`.
    /// Claude Code writes 1-hour cache entries, which cost 2x input, not the 5-minute 1.25x. The
    /// fixture is RepoBench v2's `eval-click-0cd28d65-r2-adage` (Claude Code 2.1.284, Haiku 4.5)
    /// with its content stripped: usage, message ids, models and tool names only. Claude Code
    /// reported `total_cost_usd` 0.2402909 for it; the old pricing gave 0.2191 (-8.8%).
    #[test]
    fn a_real_session_prices_to_what_claude_code_reported() {
        let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/ledger/click-0cd28d65-r2.jsonl");
        let s = claude(&p).unwrap();
        assert!(s.tokens.cache_write_1h > 0 && s.tokens.cache_write_1h <= s.tokens.cache_write, "{:?}", s.tokens);
        let usd = s.cost.usd.unwrap();
        assert!((usd - 0.2402909).abs() < 0.0005, "{usd} vs Claude Code's 0.2402909");
    }

    /// Both cache TTLs in one session: each part at its own rate (Haiku 4.5: 5-minute writes
    /// $1.25/M, 1-hour writes $2/M); without the split, every write at the 5-minute rate.
    #[test]
    fn cache_writes_are_priced_by_their_ttl() {
        let d = tempfile::tempdir().unwrap();
        let rec = |mid: &str, u: Value| json!({"type":"assistant","sessionId":"s2","timestamp":"2026-09-14T19:06:07.000Z","message":{"id":mid,"model":"claude-haiku-4-5-20251001","content":[],"usage":u}});
        let split = json!({"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 3_000_000,
                           "cache_creation": {"ephemeral_5m_input_tokens": 1_000_000, "ephemeral_1h_input_tokens": 2_000_000}});
        let plain = json!({"input_tokens": 0, "output_tokens": 0, "cache_read_input_tokens": 0, "cache_creation_input_tokens": 1_000_000});
        let p = write(d.path(), "s2.jsonl", &[rec("m1", split), rec("m2", plain)]);
        let s = claude(&p).unwrap();
        assert_eq!((s.tokens.cache_write, s.tokens.cache_write_1h), (4_000_000, 2_000_000));
        assert!((s.cost.usd.unwrap() - (1.25 + 2.0 * 2.0 + 1.25)).abs() < 1e-9, "{:?}", s.cost);
    }

    #[test]
    fn an_unknown_model_is_tokens_only() {
        let d = tempfile::tempdir().unwrap();
        let p = write(
            d.path(),
            "s.jsonl",
            &[
                json!({"type":"assistant","sessionId":"s2","message":{"id":"m1","model":"mystery-model-9","content":[],"usage":{"input_tokens":5,"output_tokens":7}}}),
            ],
        );
        let s = claude(&p).unwrap();
        assert_eq!(s.cost, Cost { usd: None, source: "none".into() });
        assert_eq!(s.tokens.total(), 12);
        assert_eq!(s.model_turns, 1);
    }

    #[test]
    fn fixtures_of_every_harness_summarise() {
        let root = adage_sim::fixtures_dir();
        let claude_files = crate::import::discover(&root.join("claude/projects"));
        let s = summarise(&claude_files[0]).unwrap();
        assert_eq!(s.harness, "claude-code");
        assert!(s.model_turns > 0 && s.cost.usd.is_some());
        let codex_files = crate::codex::discover(&root.join("codex/sessions")).0;
        let s = summarise(&codex_files[0]).unwrap();
        assert_eq!((s.harness.as_str(), s.cost.source.as_str()), ("codex", "none"));
        assert!(s.model_turns > 0 && s.tool_calls > 0 && s.models == vec!["gpt-5.3-codex".to_string()]);
        let oc = adage_sim::fixture_files("opencode");
        let s = summarise(&oc[0]).unwrap();
        assert_eq!(s.cost.source, "harness");
        assert!(s.cost.usd.unwrap() > 0.0 && s.tokens.cache_read > 0);
    }

    #[test]
    fn the_ledger_replaces_by_session_and_skips_unchanged_files() {
        let d = tempfile::tempdir().unwrap();
        let rec = |out: u64| json!({"type":"assistant","sessionId":"s1","message":{"id":"m1","model":"claude-haiku-4-5","content":[],"usage":{"input_tokens":1,"output_tokens":out}}});
        let p = write(d.path(), "s1.jsonl", &[rec(1)]);
        let l = Ledger::new(&d.path().join("ledger"), "github.com/acme/billing");
        assert_eq!(l.update(std::slice::from_ref(&p))["s1"].tokens.output, 1);
        write(d.path(), "s1.jsonl", &[rec(1), rec(9)]);
        let got = l.update(std::slice::from_ref(&p));
        assert_eq!((got.len(), got["s1"].tokens.output), (1, 9));
        assert_eq!(l.read().len(), 1);
        assert!(l.path().ends_with("ledger/github-com-acme-billing.jsonl"));
    }
}
