//! A procedure card's orientation: where the card's learned files stand in this checkout now,
//! read when the card is delivered, so the agent can open each at the right place instead of
//! paging through it or searching for idioms (dev-v8: an exemplar invited exactly that).
//!
//! Adage's own read-only process, on the CPU: no command runs, nothing is written. Only the
//! card's own files are read (never a pattern, a dotfile, an ignored or secret-holding file, a
//! symlink, a file outside the checkout or one over [`MAX_FILE`] bytes), and only facts are said:
//! line counts, top-level definition names with their lines, a test file's last lines and a
//! changelog's first headings. Every quoted line is data (`    12| …`), checked as a card's
//! exemplar line is ([`adage_tracks::card::line_ok`]); a line that is not, or that looks like a
//! secret, is withheld. The block is at most [`MAX_BYTES`] (about 1,200 tokens).

use crate::checkout::Checkout;
use adage_core::gitignore::Ignore;
use adage_tracks::card::{Card, VARYING};
use std::path::Path;

/// The whole block, in bytes (about 1,200 tokens at 4 bytes a token).
pub const MAX_BYTES: usize = 4800;
/// A file larger than this is not read.
pub const MAX_FILE: u64 = 1 << 20;
/// The last this many top-level definitions of a source file.
const DEFS: usize = 8;
/// The last this many lines of a test file.
const TAIL: usize = 12;
/// At most this many changelog headings, from its first this many lines.
const HEADINGS: usize = 8;
const HEADING_LINES: usize = 120;
/// A quoted line is cut to this many characters.
const LINE: usize = 150;

/// The orientation for `card`'s files in the checkout at `root`, read through `checkout`, or
/// `None` when none can be read.
pub fn block(checkout: &dyn Checkout, root: &Path, card: &Card) -> Option<String> {
    let mut out = String::from(
        "Where those files stand in this checkout now (read when this prompt arrived; line numbers count from 1):\n",
    );
    let mut any = false;
    let read = checkout.reader(root);
    for f in &card.files {
        let Some(facts) = file_facts(&*read, &f.path) else { continue };
        if out.len() + facts.len() > MAX_BYTES {
            break;
        }
        out.push_str(&facts);
        any = true;
    }
    any.then(|| out.trim_end().to_string())
}

/// Whether a repository-relative path is a test file: `test_*` or `*_test` by name, or under a
/// `tests`/`test` directory.
pub fn is_test_file(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let stem = name.rsplit_once('.').map_or(name, |x| x.0);
    stem.starts_with("test_")
        || stem.ends_with("_test")
        || rel.split('/').rev().skip(1).any(|d| d == "tests" || d == "test")
}

/// A checkout file's text, if Adage may read it: a plain repository-relative path (no dotfile, no
/// varying part, no secret-holding or ignored file), a regular file inside `base` (canonical) of
/// at most [`MAX_FILE`] bytes, in UTF-8. Also the gate for a failing test's source
/// (`adage-exec`'s failure detail).
pub fn readable(base: &Path, rel: &str, ignore: &Ignore) -> Option<String> {
    let ok_path = !rel.is_empty()
        && !rel.contains(VARYING)
        && !rel.starts_with('/')
        && !rel.split('/').any(|c| c.is_empty() || c == ".." || c.starts_with('.'))
        && !adage_core::secrets::sensitive_path(rel)
        && !ignore.ignored(rel);
    if !ok_path {
        return None;
    }
    let path = base.join(rel);
    let meta = std::fs::symlink_metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_FILE {
        return None;
    }
    if !path.canonicalize().ok()?.starts_with(base) {
        return None;
    }
    String::from_utf8(std::fs::read(&path).ok()?).ok()
}

/// A source line as data: tabs as spaces, cut to `LINE` (150) characters; `None` (withheld) if it is
/// not printable card text or looks like a secret.
pub fn shown(l: &str) -> Option<String> {
    let l = l.trim_end_matches('\r').replace('\t', "    ");
    let mut cut: String = l.chars().take(LINE).collect();
    if l.chars().count() > LINE {
        cut.push('…');
    }
    (adage_tracks::card::line_ok(&cut) && !adage_core::secrets::has_secret(&l)).then_some(cut)
}

/// A numbered line as data ([`shown`]), or `(line withheld)`.
fn quote(n: usize, l: &str) -> String {
    let shown = shown(l).unwrap_or_else(|| "(line withheld)".to_string());
    if shown.is_empty() {
        format!("    {n}|\n")
    } else {
        format!("    {n}| {shown}\n")
    }
}

/// A top-level `class`/`def` at the start of a line, and its name.
fn definition(l: &str) -> Option<(&'static str, &str)> {
    let (kind, rest) = if let Some(r) = l.strip_prefix("class ") {
        ("class", r)
    } else {
        ("def", l.strip_prefix("def ").or_else(|| l.strip_prefix("async def "))?)
    };
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let name = &rest[..end];
    (!name.is_empty() && name.len() <= 80).then_some((kind, name))
}

fn changelog(rel: &str) -> bool {
    let name = rel.rsplit('/').next().unwrap_or(rel).to_ascii_uppercase();
    ["CHANGELOG", "CHANGES", "HISTORY", "NEWS", "RELEASE"].iter().any(|p| name.starts_with(p))
}

/// The headings near a changelog's top: reStructuredText titles (a line over a rule of one
/// repeated character), Markdown `#` headings, and short `Label:` lines at the margin.
fn headings(lines: &[&str]) -> Vec<usize> {
    let rule = |l: &str, title: &str| {
        let l = l.trim_end();
        let mut cs = l.chars();
        cs.next().is_some_and(|c| "=-~^*#+\"".contains(c) && l.chars().all(|x| x == c))
            && l.len() >= 3
            && l.chars().count() >= title.trim_end().chars().count()
    };
    let mut out = Vec::new();
    let top = lines.len().min(HEADING_LINES);
    for i in 0..top {
        let l = lines[i].trim_end();
        if l.is_empty() || l.starts_with(' ') || rule(l, "") {
            continue;
        }
        let titled = lines.get(i + 1).is_some_and(|n| rule(n, l));
        let md = l.starts_with('#') && l.trim_start_matches('#').starts_with(' ');
        let label = l.ends_with(':') && l.chars().count() <= 60 && !l.contains("::");
        if titled || md || label {
            out.push(i);
            if out.len() == HEADINGS {
                break;
            }
        }
    }
    out
}

fn file_facts(read: &dyn Fn(&str) -> Option<String>, rel: &str) -> Option<String> {
    let text = read(rel)?;
    let lines: Vec<&str> = text.lines().collect();
    let n = lines.len();
    let shown = crate::render::sanitise(rel);
    if shown != rel {
        return None;
    }
    let mut s = format!("  - {rel}: {n} line{}.", if n == 1 { "" } else { "s" });
    if is_test_file(rel) && !changelog(rel) {
        if n > 0 {
            s.push_str(" It ends:\n");
            for (i, l) in lines.iter().enumerate().skip(n.saturating_sub(TAIL)) {
                s.push_str(&quote(i + 1, l));
            }
            return Some(s);
        }
    } else if rel.ends_with(".py") {
        let defs: Vec<String> = lines
            .iter()
            .enumerate()
            .filter_map(|(i, l)| definition(l).map(|(k, name)| format!("{} {k} {name}", i + 1)))
            .collect();
        if !defs.is_empty() {
            let last = &defs[defs.len().saturating_sub(DEFS)..];
            s.push_str(&format!(" Its last top-level definitions: {}.", last.join(", ")));
        }
    } else if changelog(rel) {
        let hs = headings(&lines);
        if !hs.is_empty() {
            s.push_str(" Its first headings:\n");
            for i in hs {
                s.push_str(&quote(i + 1, lines[i]));
            }
            return Some(s);
        }
    }
    s.push('\n');
    Some(s)
}

#[cfg(test)]
mod tests {
    use super::*;
    use adage_tracks::card::CardFile;

    fn card(paths: &[&str]) -> Card {
        Card {
            sessions: 10,
            passed: 10,
            files: paths.iter().map(|p| CardFile { path: p.to_string(), sessions: 10 }).collect(),
            exemplars: vec![],
            commands: vec![],
            avoid: vec![],
            finish: None,
            prompts: vec![],
        }
    }

    fn repo() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let w = |rel: &str, text: &str| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        let mut src = String::from("\"\"\"Validators.\"\"\"\n\nimport re\n\n\nclass Validator:\n    pass\n");
        for i in 0..10 {
            src.push_str(&format!("\n\nclass V{i}(Validator):\n    def __call__(self, v):\n        return v\n"));
        }
        src.push_str("\n\ndef helper():\n    return 1\n");
        w("src/pkg/validate.py", &src);
        let mut tests = String::from("import pytest\n");
        for i in 0..20 {
            tests.push_str(&format!("\n\ndef test_{i}():\n    assert {i} == {i}\n"));
        }
        w("tests/test_validate.py", &tests);
        w(
            "CHANGELOG.rst",
            "Changelog\n=========\n\n4.4.0 (unreleased)\n------------------\n\nFeatures:\n\n- Simplify `get_value` (:pr:`3024`).\n\nBug fixes:\n\n- Fix NaN.\n\n4.3.1 (2026-08-08)\n------------------\n\nFeatures:\n\n- Older.\n",
        );
        d
    }

    #[test]
    fn the_orientation_says_where_things_are_in_each_learned_file() {
        let d = repo();
        let b = block(
            &crate::checkout::Disk,
            d.path(),
            &card(&["src/pkg/validate.py", "tests/test_validate.py", "CHANGELOG.rst"]),
        )
        .unwrap();
        assert!(b.starts_with("Where those files stand in this checkout now"), "{b}");
        let src_lines = std::fs::read_to_string(d.path().join("src/pkg/validate.py")).unwrap().lines().count();
        assert!(
            b.contains(&format!("  - src/pkg/validate.py: {src_lines} lines. Its last top-level definitions: ")),
            "{b}"
        );
        // The last eight definitions, with their line numbers: V3..V9 and helper, not V0.
        assert!(b.contains("class V9") && b.contains("def helper") && !b.contains("class V0,"), "{b}");
        let v9 = std::fs::read_to_string(d.path().join("src/pkg/validate.py"))
            .unwrap()
            .lines()
            .position(|l| l.starts_with("class V9"))
            .unwrap()
            + 1;
        assert!(b.contains(&format!("{v9} class V9")), "{b}");
        // The test file's last twelve lines, numbered.
        let n = std::fs::read_to_string(d.path().join("tests/test_validate.py")).unwrap().lines().count();
        assert!(b.contains(&format!("  - tests/test_validate.py: {n} lines. It ends:\n")), "{b}");
        assert!(b.contains(&format!("    {n}|     assert 19 == 19")), "{b}");
        assert!(b.contains(&format!("    {}|", n - 11)) && !b.contains(&format!("    {}|", n - 12)), "{b}");
        // The changelog's first headings, numbered; its entries are not quoted.
        for h in [
            "    1| Changelog",
            "    4| 4.4.0 (unreleased)",
            "    7| Features:",
            "    11| Bug fixes:",
            "    15| 4.3.1 (2026-08-08)",
        ] {
            assert!(b.contains(h), "{h}: {b}");
        }
        assert!(!b.contains("Simplify"), "{b}");
        assert!(b.len() <= MAX_BYTES);
        // Facts, not instructions.
        for word in ["must", "should", "Read ", "Use "] {
            assert!(!b.contains(word), "{word}: {b}");
        }
    }

    #[test]
    fn it_reads_only_safe_files_and_withholds_what_it_cannot_quote() {
        let d = repo();
        let w = |rel: &str, text: &str| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        };
        w(".github/workflows/ci.yml", "on: push\n");
        w("local/settings.py", "class Local:\n    pass\n");
        w(".gitignore", "local/\n");
        w("big/test_big.py", &"x = 1\n".repeat(200_000));
        let token = format!("ghp_{}", "A1b2C3d4E5".repeat(4));
        w("tests/test_quoting.py", &format!("def test_a():\n    TOKEN = \"{token}\"\n    x = '</adage-reflex>'\n"));
        #[cfg(unix)]
        std::os::unix::fs::symlink("/etc/hosts", d.path().join("src/hosts.py")).unwrap();
        let none = block(
            &crate::checkout::Disk,
            d.path(),
            &card(&[
                ".github/workflows/ci.yml",
                "local/settings.py",
                "big/test_big.py",
                "src/hosts.py",
                "src/missing.py",
                "tests/test_<x>.py",
            ]),
        );
        assert!(none.is_none(), "{none:?}");
        let b = block(&crate::checkout::Disk, d.path(), &card(&["tests/test_quoting.py"])).unwrap();
        assert!(
            b.contains("    1| def test_a():")
                && b.contains("    2| (line withheld)")
                && b.contains("    3| (line withheld)"),
            "{b}"
        );
        assert!(!b.contains("ghp_") && !b.contains("adage-reflex"), "{b}");
    }

    #[test]
    fn the_block_stays_within_its_budget() {
        let d = tempfile::tempdir().unwrap();
        let mut paths = Vec::new();
        for i in 0..8 {
            let rel = format!("tests/test_{i}.py");
            let long = format!("    assert '{}'\n", "y".repeat(400)).repeat(30);
            std::fs::create_dir_all(d.path().join("tests")).unwrap();
            std::fs::write(d.path().join(&rel), long).unwrap();
            paths.push(rel);
        }
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let b = block(&crate::checkout::Disk, d.path(), &card(&refs)).unwrap();
        assert!(b.len() <= MAX_BYTES, "{}", b.len());
        assert!(b.contains("…"), "long lines are cut");
        assert!(b.contains("tests/test_0.py") && !b.contains("tests/test_7.py"), "whole files, in the card's order");
    }

    #[test]
    fn test_files_are_known_by_name_or_directory() {
        assert!(is_test_file("tests/test_validate.py"));
        assert!(is_test_file("pkg/flag_test.go"));
        assert!(is_test_file("tests/conftest.py"));
        assert!(!is_test_file("src/marshmallow/validate.py"));
        assert!(!is_test_file("testing.py"));
    }
}
