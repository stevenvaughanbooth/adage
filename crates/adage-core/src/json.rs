//! JSON found in free text.

use serde_json::Value;

/// The first JSON object in a tool's text output.
pub fn extract_json(s: &str) -> Option<Value> {
    let start = s.find('{')?;
    let end = s.rfind('}')?;
    if end <= start {
        return None;
    }
    serde_json::from_str(&s[start..=end]).ok()
}
