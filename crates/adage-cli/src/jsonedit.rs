//! Edits to a JSON settings file that keep the rest of it as the person wrote it: key order,
//! indentation, spacing, and every value Adage does not touch, byte for byte (`adage install`,
//! `adage install --remove`, `adage delegate on|off`).
//!
//! A file is first parsed with serde_json, which decides whether it is valid; this module then
//! only finds where each value sits in the text (its byte span) and splices new text in, indented
//! to match the file. Only the `hooks` arrays Adage changes are rewritten, and within them every
//! entry that is not Adage's is kept verbatim.

use anyhow::{bail, Context, Result};
use serde_json::{Map, Value};

/// A value and its byte span (`start..end`, `end` exclusive) in the source text.
#[derive(Debug)]
enum Node {
    Object { start: usize, end: usize, members: Vec<Member> },
    Array { start: usize, end: usize, items: Vec<Node> },
    Scalar { start: usize, end: usize },
}

#[derive(Debug)]
struct Member {
    key: String,
    /// Where the key's opening quote is.
    key_start: usize,
    value: Node,
}

impl Node {
    fn span(&self) -> (usize, usize) {
        match self {
            Node::Object { start, end, .. } | Node::Array { start, end, .. } | Node::Scalar { start, end } => {
                (*start, *end)
            }
        }
    }
}

struct Parser<'a> {
    s: &'a [u8],
    i: usize,
}

impl Parser<'_> {
    fn ws(&mut self) {
        while self.s.get(self.i).is_some_and(|c| matches!(c, b' ' | b'\t' | b'\n' | b'\r')) {
            self.i += 1;
        }
    }

    fn expect(&mut self, c: u8) -> Result<()> {
        self.ws();
        if self.s.get(self.i) != Some(&c) {
            bail!("expected `{}` at byte {}", c as char, self.i);
        }
        self.i += 1;
        Ok(())
    }

    /// A string's span, quotes included (the parser stands on its opening quote).
    fn string(&mut self) -> Result<(usize, usize)> {
        let start = self.i;
        self.i += 1;
        while let Some(&c) = self.s.get(self.i) {
            match c {
                b'\\' => self.i += 2,
                b'"' => {
                    self.i += 1;
                    return Ok((start, self.i));
                }
                _ => self.i += 1,
            }
        }
        bail!("unterminated string at byte {start}")
    }

    fn value(&mut self) -> Result<Node> {
        self.ws();
        let start = self.i;
        match self.s.get(self.i) {
            Some(b'{') => {
                self.i += 1;
                let mut members = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b'}') {
                    self.i += 1;
                    return Ok(Node::Object { start, end: self.i, members });
                }
                loop {
                    self.ws();
                    if self.s.get(self.i) != Some(&b'"') {
                        bail!("expected a key at byte {}", self.i);
                    }
                    let (ks, ke) = self.string()?;
                    let key: String = serde_json::from_slice(&self.s[ks..ke]).context("a key")?;
                    self.expect(b':')?;
                    let value = self.value()?;
                    members.push(Member { key, key_start: ks, value });
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b'}') => {
                            self.i += 1;
                            return Ok(Node::Object { start, end: self.i, members });
                        }
                        _ => bail!("expected `,` or `}}` at byte {}", self.i),
                    }
                }
            }
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                self.ws();
                if self.s.get(self.i) == Some(&b']') {
                    self.i += 1;
                    return Ok(Node::Array { start, end: self.i, items });
                }
                loop {
                    items.push(self.value()?);
                    self.ws();
                    match self.s.get(self.i) {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Node::Array { start, end: self.i, items });
                        }
                        _ => bail!("expected `,` or `]` at byte {}", self.i),
                    }
                }
            }
            Some(b'"') => {
                let (s, e) = self.string()?;
                Ok(Node::Scalar { start: s, end: e })
            }
            Some(_) => {
                while self
                    .s
                    .get(self.i)
                    .is_some_and(|c| !matches!(c, b',' | b']' | b'}' | b' ' | b'\t' | b'\n' | b'\r'))
                {
                    self.i += 1;
                }
                Ok(Node::Scalar { start, end: self.i })
            }
            None => bail!("unexpected end of the document"),
        }
    }
}

fn parse(src: &str) -> Result<Node> {
    let mut p = Parser { s: src.as_bytes(), i: 0 };
    p.value()
}

/// The leading whitespace of the line `pos` is on.
fn line_indent(src: &str, pos: usize) -> &str {
    let line_start = src[..pos].rfind('\n').map_or(0, |n| n + 1);
    let line = &src[line_start..];
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

/// One level of indentation in this file: how far the root's first member is indented past the
/// root; two spaces when that cannot be told (an empty or one-line root).
fn indent_unit(src: &str, root: &Node) -> String {
    if let Node::Object { start, members, .. } = root {
        if let Some(m) = members.first() {
            if src[*start..m.key_start].contains('\n') {
                let (outer, inner) = (line_indent(src, *start), line_indent(src, m.key_start));
                if let Some(unit) = inner.strip_prefix(outer).filter(|u| !u.is_empty()) {
                    return unit.to_string();
                }
            }
        }
    }
    "  ".into()
}

/// `v` pretty-printed with `unit` per level, every line after the first starting at `base`.
fn pretty(v: &Value, unit: &str, base: &str) -> String {
    use serde::Serialize;
    let mut buf = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(
        &mut buf,
        serde_json::ser::PrettyFormatter::with_indent(unit.as_bytes()),
    );
    v.serialize(&mut ser).expect("a JSON value serializes");
    String::from_utf8(buf).expect("serde_json writes UTF-8").replace('\n', &format!("\n{base}"))
}

/// A container from item texts: one item per line at `inner`, closed at `outer`.
fn container(open: char, close: char, items: &[String], inner: &str, outer: &str) -> String {
    if items.is_empty() {
        return format!("{open}{close}");
    }
    format!("{open}\n{inner}{}\n{outer}{close}", items.join(&format!(",\n{inner}")))
}

/// Text replacements, applied from the end so earlier offsets stay valid. They never overlap.
fn apply(src: &str, mut edits: Vec<(usize, usize, String)>) -> String {
    edits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.cmp(&a.1)));
    let mut out = src.to_string();
    for (s, e, text) in edits {
        out.replace_range(s..e, &text);
    }
    out
}

/// The root object's `hooks` member, if it has one: its index and value.
fn hooks_of(members: &[Member]) -> Option<(usize, &Node)> {
    members.iter().position(|m| m.key == "hooks").map(|i| (i, &members[i].value))
}

/// An array's items without the ones `drop` picks, verbatim, and whether any was dropped.
fn kept_items(src: &str, items: &[Node], drop: &dyn Fn(&Value) -> bool) -> Result<(Vec<String>, bool)> {
    let mut kept = Vec::new();
    let mut dropped = false;
    for it in items {
        let (s, e) = it.span();
        let v: Value = serde_json::from_str(&src[s..e])?;
        if drop(&v) {
            dropped = true;
        } else {
            kept.push(src[s..e].to_string());
        }
    }
    Ok((kept, dropped))
}

/// `src` (a JSON object) with each `hooks.<event>` array in `ours` holding its entries that
/// `is_ours` does not pick, verbatim, then `ours`'s entries; events `src` lacks are added at the
/// end of `hooks`, and `hooks` itself at the end of the document if it has none. Everything else
/// keeps its text. The caller has checked that `src` is an object whose `hooks` (if any) is an
/// object of arrays.
pub(crate) fn merge_hooks(src: &str, ours: &Map<String, Value>, is_ours: &dyn Fn(&Value) -> bool) -> Result<String> {
    let root = parse(src)?;
    let Node::Object { start: root_start, members, .. } = &root else { bail!("not a JSON object") };
    let unit = indent_unit(src, &root);
    let root_inner = format!("{}{unit}", line_indent(src, *root_start));
    let Some((h, hooks)) = hooks_of(members) else {
        let member = format!("\"hooks\": {}", pretty(&Value::Object(ours.clone()), &unit, &root_inner));
        return Ok(insert_member(src, &root, &root_inner, member));
    };
    let Node::Object { start: hs, end: he, members: events } = hooks else { bail!("\"hooks\" is not an object") };
    let outer = line_indent(src, members[h].key_start).to_string();
    let inner = format!("{outer}{unit}");
    let entry_indent = format!("{inner}{unit}");
    let mut texts: Vec<String> = Vec::new();
    for m in events {
        let (vs, ve) = m.value.span();
        let Some(entries) = ours.get(&m.key) else {
            texts.push(src[m.key_start..ve].to_string());
            continue;
        };
        let Node::Array { items, .. } = &m.value else { bail!("hooks.{} is not an array", m.key) };
        let (mut kept, _) = kept_items(src, items, is_ours)?;
        kept.extend(entries.as_array().into_iter().flatten().map(|e| pretty(e, &unit, &entry_indent)));
        let key_text = &src[m.key_start..vs];
        texts.push(format!("{key_text}{}", container('[', ']', &kept, &entry_indent, &inner)));
    }
    for (event, entries) in ours.iter().filter(|(e, _)| !events.iter().any(|m| &m.key == *e)) {
        texts.push(format!("{}: {}", Value::String(event.clone()), pretty(entries, &unit, &inner)));
    }
    Ok(apply(src, vec![(*hs, *he, container('{', '}', &texts, &inner, &outer))]))
}

/// `src` without the `hooks` entries `is_ours` picks: an event left with none is removed, and
/// `hooks` too when nothing is left in it. `None` when there is nothing of Adage's to remove.
pub(crate) fn remove_hooks(src: &str, is_ours: &dyn Fn(&Value) -> bool) -> Result<Option<String>> {
    let root = parse(src)?;
    let Node::Object { members, .. } = &root else { bail!("not a JSON object") };
    let Some((h, Node::Object { start: hs, end: he, members: events })) = hooks_of(members) else { return Ok(None) };
    let unit = indent_unit(src, &root);
    let outer = line_indent(src, members[h].key_start).to_string();
    let inner = format!("{outer}{unit}");
    let entry_indent = format!("{inner}{unit}");
    let (mut texts, mut changed) = (Vec::new(), false);
    for m in events {
        let (vs, ve) = m.value.span();
        let Node::Array { items, .. } = &m.value else {
            texts.push(src[m.key_start..ve].to_string());
            continue;
        };
        let (kept, dropped) = kept_items(src, items, is_ours)?;
        if !dropped {
            texts.push(src[m.key_start..ve].to_string());
            continue;
        }
        changed = true;
        if !kept.is_empty() {
            texts.push(format!("{}{}", &src[m.key_start..vs], container('[', ']', &kept, &entry_indent, &inner)));
        }
    }
    if !changed {
        return Ok(None);
    }
    if !texts.is_empty() || events.is_empty() {
        return Ok(Some(apply(src, vec![(*hs, *he, container('{', '}', &texts, &inner, &outer))])));
    }
    // Nothing but Adage's was in `hooks`: remove the member, and the comma that went with it.
    let end = members[h].value.span().1;
    let (s, e) = match h {
        0 if members.len() == 1 => {
            let (rs, re) = root.span();
            return Ok(Some(apply(src, vec![(rs, re, "{}".into())])));
        }
        0 => (members[0].key_start, members[1].key_start),
        i => (members[i - 1].value.span().1, end),
    };
    Ok(Some(apply(src, vec![(s, e, String::new())])))
}

/// `obj` with `member` (its text, key included) added after its last member.
fn insert_member(src: &str, obj: &Node, inner: &str, member: String) -> String {
    let Node::Object { start, end, members } = obj else { return src.to_string() };
    match members.last() {
        Some(last) => {
            let at = last.value.span().1;
            apply(src, vec![(at, at, format!(",\n{inner}{member}"))])
        }
        None => {
            let outer = line_indent(src, *start);
            apply(src, vec![(*start, *end, format!("{{\n{inner}{member}\n{outer}}}"))])
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ours_entry(v: &Value) -> bool {
        v.to_string().contains("\"hook\"")
    }

    fn ours() -> Map<String, Value> {
        json!({
            "Stop": [{ "hooks": [{ "type": "command", "command": "/bin/adage", "args": ["hook", "claude", "Stop"] }] }],
            "SessionStart": [{ "hooks": [{ "type": "command", "command": "/bin/adage", "args": ["hook", "claude", "SessionStart"] }] }],
        })
        .as_object()
        .unwrap()
        .clone()
    }

    #[test]
    fn a_merge_keeps_the_persons_order_and_formatting() {
        let src = "{\n  \"model\": \"sonnet\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"echo mine\"\n          }\n        ]\n      }\n    ]\n  },\n  \"env\": {\"A\": \"1\"}\n}\n";
        let out = merge_hooks(src, &ours(), &ours_entry).unwrap();
        // Still the same document, with Adage's entries added.
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "sonnet");
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 2);
        assert!(v["hooks"]["SessionStart"].is_array());
        // Every line the person wrote is still there, in order: the diff only adds, apart from
        // the comma a line gains when something follows it now.
        let diff = crate::install::line_diff(src, &out);
        for l in diff.lines().filter_map(|l| l.strip_prefix("- ")) {
            assert!(diff.contains(&format!("+ {l},")), "{l:?} changed beyond a comma:\n{diff}");
        }
        assert!(out.starts_with("{\n  \"model\": \"sonnet\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"echo mine\""), "{out}");
        assert!(out.ends_with("  },\n  \"env\": {\"A\": \"1\"}\n}\n"), "{out}");
        // Merging again changes nothing.
        assert_eq!(merge_hooks(&out, &ours(), &ours_entry).unwrap(), out);
    }

    #[test]
    fn a_merge_adds_hooks_at_the_end_in_the_files_indentation() {
        let src = "{\n    \"model\": \"opus\"\n}\n";
        let out = merge_hooks(src, &ours(), &ours_entry).unwrap();
        assert!(
            out.starts_with(
                "{\n    \"model\": \"opus\",\n    \"hooks\": {\n        \"SessionStart\": [\n            {"
            ),
            "{out}"
        );
        assert!(out.ends_with("        ]\n    }\n}\n"), "{out}");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["model"], "opus");
        assert_eq!(merge_hooks("{}", &ours(), &ours_entry).unwrap().matches("\"hook\"").count(), 2);
    }

    #[test]
    fn removal_restores_what_was_there() {
        let src = "{\n  \"model\": \"sonnet\",\n  \"hooks\": {\n    \"Stop\": [\n      {\n        \"hooks\": [\n          {\n            \"type\": \"command\",\n            \"command\": \"echo mine\"\n          }\n        ]\n      }\n    ]\n  }\n}\n";
        let merged = merge_hooks(src, &ours(), &ours_entry).unwrap();
        assert_eq!(remove_hooks(&merged, &ours_entry).unwrap().as_deref(), Some(src));
        assert_eq!(remove_hooks(src, &ours_entry).unwrap(), None, "nothing of Adage's");
        // A file that held only Adage's hooks keeps its other members, or becomes `{}`.
        let only = "{\n  \"model\": \"opus\"\n}\n";
        let merged = merge_hooks(only, &ours(), &ours_entry).unwrap();
        assert_eq!(remove_hooks(&merged, &ours_entry).unwrap().as_deref(), Some(only));
        let first = "{\n  \"hooks\": {},\n  \"model\": \"opus\"\n}\n";
        let merged = merge_hooks(first, &ours(), &ours_entry).unwrap();
        assert_eq!(remove_hooks(&merged, &ours_entry).unwrap().as_deref(), Some("{\n  \"model\": \"opus\"\n}\n"));
        let merged = merge_hooks("{}\n", &ours(), &ours_entry).unwrap();
        assert_eq!(remove_hooks(&merged, &ours_entry).unwrap().as_deref(), Some("{}\n"));
    }

    #[test]
    fn spans_survive_escapes_and_nesting() {
        let src = r#"{"a\"b": [1, {"c": "]}"}], "hooks": {"Stop": []}}"#;
        let out = merge_hooks(src, &ours(), &ours_entry).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["a\"b"][1]["c"], "]}");
        assert_eq!(v["hooks"]["Stop"].as_array().unwrap().len(), 1);
    }
}
