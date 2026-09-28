//! Fixed id forms and shared rules (Track G §6.3). Some shapes have always had an id of a fixed,
//! content-derived form, and keep it, so `review.yaml` decisions, proposals, and the instinct ids
//! that line up with learned ids all survive re-learning. The rules here (the runner of a command,
//! a failure's signature, the field an output lists files in) are shared with `adage-learn`:
//!
//! | Shape | Id |
//! | --- | --- |
//! | lookup (`prompt` → `run`, a value slot) | `lookup-work-item` |
//! | linked files (`after_command` → `read_batch`) | `read-linked-files` |
//! | dead end (`session_start` → `avoid`, or `after_failure` → `run`) | `dead-end-<stable(W)>` for the working command W's most common signature, `dead-end-<stable(W, S)>` for each further signature S |
//! | finish line (`after_edit` → a regenerate `run`) | `finish-line-<stable(glob, command)>` |
//! | intent (`intent` → `run`) | `intent-<stable(group)>`, `group` the command's head and effect |
//!
//! `stable` is a 24-bit FNV-1a suffix; W is the working command's canonical key.

use adage_core::spans::{self, SpanKind};

/// The stable id suffix (FNV-1a over the parts, each followed by a NUL; 6 hex digits).
pub fn stable(parts: &[&str]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x100_0000_01b3);
        }
    }
    format!("{:06x}", h & 0xff_ffff)
}

pub const LOOKUP_ID: &str = "lookup-work-item";
pub const LINKED_FILES_ID: &str = "read-linked-files";

/// A dead end's id: the most common signature of working command `wkey` keeps the id of W alone.
pub fn dead_end_id(wkey: &str, signature: &str, rank: usize) -> String {
    if rank == 0 {
        format!("dead-end-{}", stable(&[wkey]))
    } else {
        format!("dead-end-{}", stable(&[wkey, signature]))
    }
}

/// A finish line's id.
pub fn finish_line_id(glob: &str, command: &str) -> String {
    format!("finish-line-{}", stable(&[glob, command]))
}

/// An argv as a directed command: plain words as they are, anything else single-quoted (which
/// validation then refuses, so a quoted argument never becomes a route).
pub fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.chars().all(|c| c.is_ascii_alphanumeric() || "-_./={}:,@+".contains(c)) {
                a.clone()
            } else {
                format!("'{}'", a.replace('\'', ""))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// A command's runner, without its targets (`python3 -m pytest tests/a.py -v` →
/// `python3 -m pytest`, `node --test src/` → `node --test`, `cargo test --workspace` →
/// `cargo test`), as the dead-end miner names it.
pub fn runner(argv: &[String]) -> String {
    use adage_core::pattern::{is_slot, Shape};
    match Shape::of_argv(argv) {
        Some(s) if !s.identity.iter().any(|t| is_slot(t)) => {
            if s.identity.first() == Some(&s.program) {
                s.identity.join(" ")
            } else {
                format!("{} {}", s.program, s.identity.join(" "))
            }
        }
        _ => fallback_runner(argv),
    }
}

fn fallback_runner(argv: &[String]) -> String {
    let mut out: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < argv.len() {
        let t = argv[i].as_str();
        if i > 0 && (t.contains('/') || t.contains('.') || (t.starts_with('-') && t != "-m")) {
            break;
        }
        out.push(t);
        if t == "-m" {
            if let Some(m) = argv.get(i + 1) {
                out.push(m);
            }
            break;
        }
        i += 1;
        if out.len() >= 3 {
            break;
        }
    }
    out.join(" ")
}

/// A failure signature safe to store and share: no absolute paths, no machine details.
fn signature_of(line: &str) -> String {
    let parts: Vec<&str> = line.split(": ").collect();
    let kept: Vec<&str> =
        parts.iter().copied().skip_while(|p| p.trim_start().starts_with('/') || p.contains("/bin/")).collect();
    let s = if kept.is_empty() { line } else { &kept.join(": ") };
    s.split_whitespace().filter(|w| !w.starts_with('/')).collect::<Vec<_>>().join(" ").chars().take(120).collect()
}

/// The failure signature of a failed run's output: its last error line that renders safely.
pub fn failure_signature(output: &str) -> Option<String> {
    output.lines().rev().map(|l| signature_of(l.trim())).find(|l| {
        let lc = l.to_ascii_lowercase();
        (lc.contains("error") || lc.contains("no module named") || lc.contains("not found") || lc.contains("not set"))
            && adage_tracks::io::safe_text(l)
    })
}

/// Pseudo-field meaning "repository paths mentioned in the command's text output".
pub const PATHS_FIELD: &str = "*paths";

/// The field of a command's output that lists repository files: a JSON array of path-like
/// strings, else the paths the text mentions ([`PATHS_FIELD`]); `None` when it lists none.
pub fn listed_field(output: &str) -> Option<String> {
    let json = {
        let (a, b) = (output.find('{'), output.rfind('}'));
        match (a, b) {
            (Some(a), Some(b)) if b > a => serde_json::from_str::<serde_json::Value>(&output[a..=b]).ok(),
            _ => None,
        }
    };
    if let Some(obj) = json.as_ref().and_then(|j| j.as_object()) {
        for (k, v) in obj {
            let arr: Vec<&str> =
                v.as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            if !arr.is_empty() && arr.iter().all(|p| p.contains('/') || p.contains('.')) {
                return Some(k.clone());
            }
        }
    }
    spans::extract(output).iter().any(|x| x.kind == SpanKind::Path).then(|| PATHS_FIELD.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_stable() {
        // The demo's dead end (adage-instincts' safety tests pin the same value).
        assert_eq!(dead_end_id("exec make test", "x", 0), "dead-end-d41b1e");
        assert_ne!(dead_end_id("exec make test", "x", 1), "dead-end-d41b1e");
        assert_eq!(
            finish_line_id("schema/*.json", "make gen"),
            format!("finish-line-{}", stable(&["schema/*.json", "make gen"]))
        );
    }

    #[test]
    fn runners_and_signatures() {
        let v = |x: &[&str]| x.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(runner(&v(&["python3", "-m", "pytest", "tests/a.py", "-v"])), "python3 -m pytest");
        assert_eq!(runner(&v(&["cargo", "test", "--workspace"])), "cargo test");
        assert_eq!(
            failure_signature("x\n/usr/bin/python3: No module named pytest\n").as_deref(),
            Some("No module named pytest")
        );
        assert_eq!(listed_field("{\"files\": [\"a/b.py\"]}").as_deref(), Some("files"));
        assert_eq!(listed_field("Files:\n  tally/a.py\n").as_deref(), Some(PATHS_FIELD));
        assert_eq!(shell_join(&v(&["make", "gen"])), "make gen");
    }
}
