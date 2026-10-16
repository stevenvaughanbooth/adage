//! A deliberately small, non-evaluating decoder for one Codex orchestration idiom.
//!
//! Only an entire `text(await tools.exec_command({literal options}))` invocation with one
//! completed, untruncated result is transparent. Other JavaScript stays an opaque native call.
//! This is not a JavaScript interpreter, a shell executor, or a general wrapper recognizer.

use serde::Deserialize;
use serde_json::{json, Map, Value};

const MAX_INPUT: usize = 128 << 10;
const MAX_RESULT: usize = 4 << 20;

pub(crate) struct Forwarded {
    pub input: Value,
    pub cwd: String,
    pub output: String,
    pub failed: bool,
}

struct Literal<'a>(&'a str);

impl Literal<'_> {
    fn token(&mut self, token: &str) -> Option<()> {
        self.0 = self.0.trim_start_matches(|c: char| c.is_ascii_whitespace());
        self.0 = self.0.strip_prefix(token)?;
        Some(())
    }

    fn value(&mut self) -> Option<Value> {
        self.0 = self.0.trim_start_matches(|c: char| c.is_ascii_whitespace());
        let mut stream = serde_json::Deserializer::from_str(self.0).into_iter::<Value>();
        let value = stream.next()?.ok()?;
        self.0 = &self.0[stream.byte_offset()..];
        Some(value)
    }

    fn object(&mut self) -> Option<Map<String, Value>> {
        self.token("{")?;
        let mut options = Map::new();
        loop {
            self.0 = self.0.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if self.0.starts_with('}') {
                self.token("}")?;
                return Some(options);
            }
            if options.len() >= 10 {
                return None;
            }
            let key = if self.0.starts_with('"') {
                self.value()?.as_str()?.to_string()
            } else {
                let end = self.0.find(|c: char| !c.is_ascii_alphabetic() && c != '_').unwrap_or(self.0.len());
                if end == 0 {
                    return None;
                }
                let key = self.0[..end].to_string();
                self.0 = &self.0[end..];
                key
            };
            self.token(":")?;
            let value = self.value()?;
            if options.insert(key, value).is_some() {
                return None;
            }
            self.0 = self.0.trim_start_matches(|c: char| c.is_ascii_whitespace());
            if !self.0.starts_with('}') {
                self.token(",")?;
            }
        }
    }
}

fn options(source: &str) -> Option<Map<String, Value>> {
    if source.len() > MAX_INPUT {
        return None;
    }
    let mut p = Literal(source);
    p.token("text")?;
    p.token("(")?;
    p.token("await")?;
    // `awaittools` is one identifier, not an await expression.
    if !p.0.starts_with(|c: char| c.is_ascii_whitespace()) {
        return None;
    }
    p.token("tools")?;
    p.token(".")?;
    p.token("exec_command")?;
    p.token("(")?;
    let args = p.object()?;
    p.token(")")?;
    p.token(")")?;
    let tail = p.0.trim_matches(|c: char| c.is_ascii_whitespace());
    if !tail.is_empty() && tail != ";" {
        return None;
    }
    for (key, value) in &args {
        let valid = match key.as_str() {
            "cmd" => value.as_str().is_some_and(|s| !s.trim().is_empty() && s.len() <= 64 << 10 && !s.contains('\0')),
            "workdir" => value.as_str().is_some_and(|s| s.starts_with('/') && !s.contains('\0')),
            "max_output_tokens" | "yield_time_ms" => value.as_u64().is_some(),
            // Recovery attribution does not model execution-context changes yet. Keeping their
            // metadata alone would not stop the learner from crediting a privilege change to
            // an interpreter fix, so these calls must remain opaque boundaries for now.
            "sandbox_permissions" => value.as_str() == Some("use_default"),
            "login" => value.as_bool() == Some(true),
            "tty" => value.as_bool() == Some(false),
            "justification" => value.is_string(),
            "prefix_rule" => value.as_array().is_some_and(|v| v.iter().all(Value::is_string)),
            _ => false,
        };
        if !valid {
            return None;
        }
    }
    args.get("cmd")?.as_str()?;
    args.get("workdir")?.as_str()?;
    Some(args)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Completed {
    output: String,
    exit_code: i32,
    original_token_count: Option<u64>,
    #[serde(rename = "chunk_id")]
    _chunk_id: Option<String>,
    #[serde(rename = "wall_time_seconds")]
    _wall_time_seconds: Option<f64>,
}

/// Native freeform tools also record a transport header and one separately emitted text block.
/// Inspect their structure before flattening: extra blocks, nontext content and yielded scripts
/// are not transparent. The header is never interpreted as a shell exit status.
fn result_body(result: &Value) -> Option<&str> {
    if let Some(s) = result.as_str() {
        return Some(s);
    }
    let blocks = result.as_array()?;
    if blocks.len() != 2 {
        return None;
    }
    let text = |block: &Value| {
        let object = block.as_object()?;
        (object.len() == 2 && object.get("type")?.as_str()? == "input_text").then_some(())
    };
    text(&blocks[0])?;
    text(&blocks[1])?;
    let header = blocks[0].get("text")?.as_str()?;
    let duration = header.strip_prefix("Script completed\nWall time ")?.strip_suffix(" seconds\nOutput:\n")?;
    if !duration.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        || !duration.parse::<f64>().is_ok_and(|n| n.is_finite() && n >= 0.0)
    {
        return None;
    }
    blocks[1].get("text")?.as_str()
}

pub(crate) fn decode(tool: &str, source: &str, native_result: &Value) -> Option<Forwarded> {
    let result = result_body(native_result)?;
    if !matches!(tool, "exec" | "functions.exec") || result.len() > MAX_RESULT {
        return None;
    }
    let args = options(source)?;
    // A strict struct rejects duplicate and unknown fields, including session_id even when
    // null. A yielded, transformed or multiple-result wrapper is never a completed shell call.
    let completed: Completed = serde_json::from_str(result).ok()?;
    let lower = completed.output.to_ascii_lowercase();
    if ["output truncated", "truncated output", "tokens truncated", "script running with cell id"]
        .iter()
        .any(|marker| lower.contains(marker))
        || completed
            .original_token_count
            .is_some_and(|n| n > args.get("max_output_tokens").and_then(Value::as_u64).unwrap_or(10_000))
    {
        return None;
    }
    let cwd = args.get("workdir")?.as_str()?.to_string();
    Some(Forwarded {
        input: json!({
            "command": args.get("cmd")?,
            "workdir": cwd,
            "native_wrapper": {"tool": tool, "input": source, "output": native_result, "arguments": args},
        }),
        cwd,
        output: completed.output,
        failed: completed.exit_code != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SOURCE: &str =
        r#"text(await tools.exec_command({cmd:"cargo test", "workdir":"/repo", max_output_tokens:4000}));"#;
    const RESULT: &str = r#"{"chunk_id":"a","wall_time_seconds":0.2,"exit_code":0,"original_token_count":5,"output":"test result: ok. 1 passed; 0 failed"}"#;

    fn decode(tool: &str, source: &str, result: &str) -> Option<Forwarded> {
        super::decode(tool, source, &json!(result))
    }

    #[test]
    fn native_completed_transport_has_exactly_one_emitted_result() {
        let result = json!([
            {"type":"input_text", "text":"Script completed\nWall time 0.3 seconds\nOutput:\n"},
            {"type":"input_text", "text":RESULT},
        ]);
        let call = super::decode("exec", SOURCE, &result).unwrap();
        assert_eq!(call.input["native_wrapper"]["output"], result);
        assert!(!call.failed);
        for mutation in 0..6 {
            let mut bad = result.clone();
            match mutation {
                0 => bad.as_array_mut().unwrap().push(json!({"type":"input_text","text":"extra"})),
                1 => bad[0]["text"] = json!("Script running with cell ID 1\nOutput:\n"),
                2 => bad[0]["text"] = json!("Script completed\nOutput:\n"),
                3 => bad[1]["type"] = json!("image"),
                4 => bad[1]["extra"] = json!("metadata"),
                5 => bad[0]["text"] = json!("Script completed\nWall time NaN seconds\nOutput:\n"),
                _ => unreachable!(),
            };
            assert!(super::decode("exec", SOURCE, &bad).is_none(), "mutation {mutation}");
        }
    }

    #[test]
    fn only_the_literal_forwarder_is_transparent() {
        let call = decode("exec", SOURCE, RESULT).unwrap();
        assert_eq!(call.cwd, "/repo");
        assert_eq!(call.input["command"], "cargo test");
        assert_eq!(call.input["native_wrapper"]["input"], SOURCE);
        assert_eq!(call.input["native_wrapper"]["output"], RESULT);
        assert!(!call.failed);
        assert!(
            decode("functions.exec", SOURCE, &RESULT.replace("\"exit_code\":0", "\"exit_code\":101")).unwrap().failed
        );
        assert!(decode(
            "exec",
            " text ( await tools . exec_command ( {cmd: \"echo \\\"hi\\\"\", workdir: \"/repo\",} ) ) ; ",
            RESULT
        )
        .is_some());
        assert!(decode("exec", &SOURCE.replace("await tools", "awaittools"), RESULT).is_none());
    }

    #[test]
    fn opaque_code_context_changes_and_ambiguous_results_abstain() {
        for source in [
            SOURCE.replace("\"cargo test\"", "command"),
            SOURCE.replace("\"cargo test\"", "`cargo ${task}`"),
            SOURCE.replace("\"/repo\"", "\"relative\""),
            SOURCE.replace("\"workdir\":\"/repo", "\"cwd\":\"/repo"),
            SOURCE.replace("cmd:", "...extra, cmd:"),
            SOURCE.replace("cmd:", "cmd: \"echo unrelated\", cmd:"),
            SOURCE.replace("cmd:", "get cmd() { return 'true'; }, other:"),
            SOURCE.replace("cmd:", "sandbox_permissions: \"require_escalated\", cmd:"),
            SOURCE.replace("cmd:", "login: false, cmd:"),
            SOURCE.replace("cmd:", "tty: true, cmd:"),
            SOURCE.replace("cmd:", "shell: \"/bin/zsh\", cmd:"),
            SOURCE.replace("4000", "\"4000\""),
            format!("{SOURCE} text('extra');"),
            format!("const x = 1; {SOURCE}"),
            SOURCE.replace("text(await", "text(await Promise.all([await"),
            SOURCE.replace("exec_command", "other_tool"),
        ] {
            assert!(decode("exec", &source, RESULT).is_none(), "{source}");
        }
        for result in [
            RESULT.replace("\"exit_code\":0,", ""),
            RESULT.replace("\"exit_code\":0", "\"exit_code\":null"),
            RESULT.replace("\"exit_code\":0", "\"exit_code\":0,\"exit_code\":1"),
            RESULT.replace("\"exit_code\":0", "\"exit_code\":0,\"session_id\":null"),
            RESULT.replace("\"exit_code\":0", "\"exit_code\":0,\"unexpected\":true"),
            RESULT.replace("\"original_token_count\":5", "\"original_token_count\":5000"),
            RESULT.replace("test result:", "Warning: truncated output. test result:"),
            RESULT.replace("test result:", "Output truncated. test result:"),
            format!("{RESULT}\n{RESULT}"),
            format!("Script completed\n{RESULT}"),
            "Exit code: 0\nOutput:\npassed".into(),
            "User rejected the tool call".into(),
        ] {
            assert!(decode("exec", SOURCE, &result).is_none(), "{result}");
        }
        assert!(decode("unrelated", SOURCE, RESULT).is_none());
    }
}
