//! OpenCode session import, from `opencode export <sessionID>` output (opencode.ai/docs/cli).
//!
//! OpenCode's own storage (`~/.local/share/opencode`, a SQLite database since 1.2 whose schema is
//! changing) is not a documented interface, so Adage reads the documented export instead:
//! `{ "info": Session, "messages": [{ "info": Message, "parts": Part[] }] }`, with the `Session`,
//! `Message`, and `Part` types of the published SDK (opencode.ai/docs/sdk). Each assistant message
//! that calls tools is one model turn; each `ToolPart` is one call, translated into the canonical
//! tool vocabulary by `adage_core::tools::from_opencode`.
//!
//! Steering: the OpenCode plugin delivers directives as synthetic user text parts (prompt time) and
//! appended to tool output (after a tool); both are recorded in the export. Sub-agent sessions
//! (`info.parentID` set) are skipped, like Claude Code sidechains.

use crate::format::{Decode, Format, HEAD_MIN};
use crate::locate::Located;
use crate::signals::{call_steps, Prompts, Signals};
use adage_core::tools::{from_opencode, normalize_tool};
use adage_core::CommandTable;
use adage_trace::session::OPENCODE;
use adage_trace::{wire, Call, Session};
use serde_json::Value;
use std::io::BufRead;
use std::path::Path;

/// Is this parsed file an OpenCode export?
pub fn is_export(v: &Value) -> bool {
    v.get("info").is_some_and(|i| i.get("id").is_some()) && v.get("messages").is_some_and(|m| m.is_array())
}

/// Read a file and return it if it is an OpenCode export. `opencode export` writes `info` as the
/// first key, so a Claude Code transcript is rejected from its first few bytes, unread.
pub fn read_export(path: &Path) -> Option<Value> {
    use std::io::Read;
    let mut head = [0u8; HEAD_MIN];
    let n = std::fs::File::open(path).ok()?.read(&mut head).ok()?;
    if !opens_export(&head[..n]) {
        return None;
    }
    let v: Value = serde_json::from_reader(std::io::BufReader::new(std::fs::File::open(path).ok()?)).ok()?;
    is_export(&v).then_some(v)
}

/// Civil date (UTC) of a millisecond Unix timestamp, `YYYY-MM-DD`.
fn day_of(ms: i64) -> String {
    let z = ms.div_euclid(86_400_000) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn s<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(*k))?.as_str()
}

/// Text Adage wrote (a directive, or a Stop-hook continuation: `render::is_finish_line`), never a
/// prompt the person typed.
fn own_text(t: &str) -> bool {
    t.trim_start().starts_with("<adage-reflex ") || wire::is_finish_line(t)
}

/// Whether a file's first bytes open an export: `{` then the `"info"` key.
fn opens_export(head: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&head[..head.len().min(HEAD_MIN)]);
    head.trim_start().strip_prefix('{').is_some_and(|rest| rest.trim_start().starts_with("\"info\""))
}

/// OpenCode exports as a [`Format`]: recognised by their opening `"info"` key.
pub struct Export;

impl Format for Export {
    fn harness(&self) -> adage_trace::Harness {
        adage_trace::Harness::OpenCode
    }

    fn sniff(&self, head: &[u8]) -> bool {
        opens_export(head)
    }

    fn decode(&self, trace: &mut dyn BufRead, cx: &Decode<'_>) -> Option<Session> {
        let v: Value = serde_json::from_reader(trace).ok()?;
        decode(is_export(&v).then_some(&v)?, cx)
    }
}

/// Parse one export. `None` for sub-agent sessions and sessions without a usable prompt.
pub fn parse_export(v: &Value, table: &CommandTable, repo_override: Option<&str>) -> Option<Session> {
    decode(v, &Decode::local(table, repo_override))
}

/// Decode one parsed export. The repository is found by `cx.locator`; nothing else here touches
/// the filesystem.
pub fn decode(v: &Value, cx: &Decode<'_>) -> Option<Session> {
    let (table, repo_override) = (cx.table, cx.repo_override);
    let info = v.get("info")?;
    if info.get("parentID").is_some_and(|p| !p.is_null()) {
        return None;
    }
    let id = s(info, &["id"])?.to_string();
    let mut cwd: Option<String> = s(info, &["directory"]).map(str::to_string);
    let day = info.pointer("/time/created").and_then(|t| t.as_i64()).map(day_of).unwrap_or_default();
    let mut model: Option<String> = None;
    let mut prompt: Option<String> = None;
    let mut signals = Signals::default();
    // (turn, prompt index, tool, input, output, failed)
    let mut uses: Vec<(u32, u16, String, Value, String, bool)> = Vec::new();
    let mut turns = 0u32;
    let mut prompts = Prompts::default();
    for m in v.get("messages")?.as_array()? {
        let mi = m.get("info").unwrap_or(&Value::Null);
        let parts = m.get("parts").and_then(|p| p.as_array()).map(Vec::as_slice).unwrap_or(&[]);
        match s(mi, &["role"]) {
            Some("user") => {
                let mut typed: Option<&str> = None;
                for p in parts.iter().filter(|p| s(p, &["type"]) == Some("text")) {
                    let text = s(p, &["text"]).unwrap_or("");
                    let synthetic = p.get("synthetic").and_then(|x| x.as_bool()) == Some(true);
                    if synthetic {
                        signals.at(uses.len());
                        signals.note(text);
                    } else if p.get("ignored").and_then(|x| x.as_bool()) != Some(true)
                        && !text.trim().is_empty()
                        && !own_text(text)
                    {
                        typed.get_or_insert(text);
                        if prompt.is_none() {
                            prompt = Some(text.to_string());
                        }
                    }
                }
                if let Some(t) = typed {
                    prompts.push(t);
                }
            }
            Some("assistant") => {
                if model.is_none() {
                    model = s(mi, &["modelID"]).map(str::to_string);
                }
                if cwd.is_none() {
                    cwd = s(mi, &["path", "cwd"]).map(str::to_string);
                }
                let mut any = false;
                for p in parts.iter().filter(|p| s(p, &["type"]) == Some("tool")) {
                    let st = p.get("state").unwrap_or(&Value::Null);
                    let (output, failed) = match s(st, &["status"]) {
                        Some("completed") => {
                            let out = s(st, &["output"]).unwrap_or("").to_string();
                            let exit = st.pointer("/metadata/exit").and_then(|x| x.as_i64()).is_some_and(|c| c != 0);
                            (out, exit)
                        }
                        Some("error") => (s(st, &["error"]).unwrap_or("").to_string(), true),
                        _ => continue, // pending or running: the call never finished
                    };
                    // Delivered with this call's result: it steers the calls after it.
                    signals.at(uses.len() + 1);
                    signals.note(&output);
                    any = true;
                    uses.push((
                        turns,
                        prompts.current(),
                        s(p, &["tool"]).unwrap_or("").to_string(),
                        st.get("input").cloned().unwrap_or(Value::Null),
                        output,
                        failed,
                    ));
                }
                if any {
                    turns += 1;
                }
            }
            _ => {}
        }
    }

    let cwd = cwd?;
    let prompt = prompt?;
    let Located { root, repo: repo_key } = cx.locator.locate(Path::new(&cwd), repo_override);
    let root_str = root.to_string_lossy().to_string();
    let mut steps = Vec::new();
    let mut first_step: Vec<u32> = Vec::with_capacity(uses.len());
    for (turn, at, native, input, output, failed) in uses {
        first_step.push(steps.len() as u32);
        for (tool, mut input) in from_opencode(&native, &input) {
            if let Some(p) = input.get("file_path").and_then(|v| v.as_str()) {
                if !p.is_empty() && !p.starts_with('/') {
                    input["file_path"] = Value::String(adage_core::shell::resolve_path(&cwd, p));
                }
            }
            let call = Call { tool: tool.clone(), input: input.clone(), output: output.clone(), failed };
            steps.extend(call_steps(turn, at, &call, normalize_tool(&tool, &input, &root_str, table), &root));
        }
    }
    let prompts = prompts.anchor(&steps);
    let influence = signals.anchor(&first_step, steps.len());
    Some(Session {
        id,
        root,
        repo: repo_key,
        model,
        day,
        prompt,
        steps,
        steered: signals.steered,
        directed: signals.directed,
        influence,
        outcome: None,
        harness: OPENCODE.into(),
        prompts,
        identity: Default::default(),
        ending: Default::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::day_of;

    #[test]
    fn civil_dates() {
        assert_eq!(day_of(0), "1970-01-01");
        assert_eq!(day_of(1_790_000_000_000), "2026-09-21");
        assert_eq!(day_of(951_782_400_000), "2000-02-29");
    }
}
