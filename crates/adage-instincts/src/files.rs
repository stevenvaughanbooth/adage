//! Bounded, read-only access to a repository's files.
//!
//! A repository is untrusted input. The binder only ever *reads*: it never executes anything, never
//! follows a symlink out of the repository, never reads more than [`MAX_FILE_BYTES`] of a file
//! (whole-file reads skip larger files; marker scans read at most [`HEAD_BYTES`] from the start),
//! and stops after [`MAX_READS`] reads and [`MAX_WALK_ENTRIES`] directory entries. Everything it
//! skips is recorded, with the reason, in [`RepoFiles::notes`].

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A whole-file read skips any file larger than this.
pub const MAX_FILE_BYTES: u64 = 64 * 1024;
/// A marker scan reads at most this many bytes from the start of a file.
pub const HEAD_BYTES: usize = 4 * 1024;
/// Reads (whole-file and head) per bind.
pub const MAX_READS: usize = 512;
/// Directory entries visited per walk.
pub const MAX_WALK_ENTRIES: usize = 5_000;
/// Directory depth below the root a walk descends to.
pub const MAX_WALK_DEPTH: usize = 8;

/// Directories a walk never enters: dependencies, build outputs, virtual environments, VCS data,
/// test fixtures.
const SKIP_DIRS: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    "node_modules",
    "target",
    "vendor",
    "dist",
    "build",
    "out",
    ".venv",
    "venv",
    "env",
    "__pycache__",
    ".tox",
    ".nox",
    ".mypy_cache",
    ".pytest_cache",
    ".next",
    ".cache",
    "coverage",
    // Test fixtures and test data: sample repositories and files a test suite reads, not this
    // repository's own conventions (a generated file in a fixture is no advice for its authors).
    "fixtures",
    "__fixtures__",
    "testdata",
];

pub struct RepoFiles {
    root: PathBuf,
    reads: Cell<usize>,
    notes: RefCell<BTreeMap<String, String>>,
}

/// A repository-relative path Adage itself may form: no absolute path, no `..`, nothing that is not
/// plain printable ASCII.
pub fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && rel.len() <= 512
        && !rel.starts_with('/')
        && !rel.contains('\\')
        && rel.split('/').all(|s| !s.is_empty() && s != "..")
        && rel.chars().all(|c| c.is_ascii_graphic())
}

impl RepoFiles {
    /// `None` if `root` is not a directory.
    pub fn open(root: &Path) -> Option<RepoFiles> {
        let root = std::fs::canonicalize(root).ok()?;
        root.is_dir().then(|| RepoFiles { root, reads: Cell::new(0), notes: RefCell::new(BTreeMap::new()) })
    }

    /// Files skipped, and why: `rel → reason`.
    pub fn notes(&self) -> BTreeMap<String, String> {
        self.notes.borrow().clone()
    }

    fn note(&self, rel: &str, why: &str) {
        self.notes.borrow_mut().entry(rel.to_string()).or_insert_with(|| why.to_string());
    }

    /// The real path of `rel`, if it is inside the repository (after resolving every symlink).
    fn resolve(&self, rel: &str) -> Result<PathBuf, &'static str> {
        if !safe_rel(rel) {
            return Err("path is not a plain repository-relative path");
        }
        let p = self.root.join(rel);
        let md = std::fs::symlink_metadata(&p).map_err(|_| "missing")?;
        let real = std::fs::canonicalize(&p).map_err(|_| "cannot be resolved")?;
        if !real.starts_with(&self.root) {
            return Err(if md.file_type().is_symlink() {
                "symlink escapes the repository"
            } else {
                "resolves outside the repository"
            });
        }
        Ok(real)
    }

    /// Whether `rel` is a regular file inside the repository. Reads nothing.
    pub fn is_file(&self, rel: &str) -> bool {
        self.resolve(rel).ok().and_then(|p| std::fs::metadata(p).ok()).is_some_and(|m| m.is_file())
    }

    /// Whether `rel` is a directory inside the repository. Reads nothing.
    pub fn is_dir(&self, rel: &str) -> bool {
        self.resolve(rel).ok().and_then(|p| std::fs::metadata(p).ok()).is_some_and(|m| m.is_dir())
    }

    fn take_read(&self, rel: &str) -> bool {
        if self.reads.get() >= MAX_READS {
            self.note(rel, "read budget spent");
            return false;
        }
        self.reads.set(self.reads.get() + 1);
        true
    }

    /// The whole of `rel` as UTF-8, or `None` (recorded) if it is missing, outside the repository,
    /// not a regular file, larger than [`MAX_FILE_BYTES`], or not UTF-8.
    pub fn read(&self, rel: &str) -> Option<String> {
        let real = match self.resolve(rel) {
            Ok(p) => p,
            Err("missing") => return None,
            Err(why) => {
                self.note(rel, why);
                return None;
            }
        };
        let md = std::fs::metadata(&real).ok()?;
        if !md.is_file() {
            self.note(rel, "not a regular file");
            return None;
        }
        if md.len() > MAX_FILE_BYTES {
            self.note(rel, "larger than 64 KiB");
            return None;
        }
        if !self.take_read(rel) {
            return None;
        }
        let mut buf = Vec::new();
        let f = std::fs::File::open(&real).ok()?;
        // The size can change between the check and the read: bound the read itself.
        f.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf).ok()?;
        if buf.len() as u64 > MAX_FILE_BYTES {
            self.note(rel, "larger than 64 KiB");
            return None;
        }
        match String::from_utf8(buf) {
            Ok(s) => Some(s),
            Err(_) => {
                self.note(rel, "not UTF-8");
                None
            }
        }
    }

    /// The whole of `rel`, or, for a file larger than [`MAX_FILE_BYTES`], its first
    /// [`MAX_FILE_BYTES`] cut back to the last complete line, with `true` for "partial". Only for
    /// scans that read line by line and lose nothing but what lies past the cut (`//go:generate`
    /// directives); a whole-file parse (a Makefile, JSON, TOML) must use [`RepoFiles::read`].
    pub fn read_lines_prefix(&self, rel: &str) -> Option<(String, bool)> {
        let real = match self.resolve(rel) {
            Ok(p) => p,
            Err("missing") => return None,
            Err(why) => {
                self.note(rel, why);
                return None;
            }
        };
        if !std::fs::metadata(&real).ok()?.is_file() {
            self.note(rel, "not a regular file");
            return None;
        }
        if !self.take_read(rel) {
            return None;
        }
        let mut buf = Vec::new();
        std::fs::File::open(&real).ok()?.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf).ok()?;
        let partial = buf.len() as u64 > MAX_FILE_BYTES;
        if partial {
            buf.truncate(MAX_FILE_BYTES as usize);
            let end = buf.iter().rposition(|b| *b == b'\n').map_or(0, |i| i + 1);
            buf.truncate(end);
        }
        match String::from_utf8(buf) {
            Ok(s) => Some((s, partial)),
            Err(_) => {
                self.note(rel, "not UTF-8");
                None
            }
        }
    }

    /// At most the first [`HEAD_BYTES`] of `rel` (lossy UTF-8), for marker scans.
    pub fn head(&self, rel: &str) -> Option<String> {
        let real = self.resolve(rel).ok()?;
        if !std::fs::metadata(&real).ok()?.is_file() || !self.take_read(rel) {
            return None;
        }
        let mut buf = Vec::new();
        std::fs::File::open(&real).ok()?.take(HEAD_BYTES as u64).read_to_end(&mut buf).ok()?;
        Some(String::from_utf8_lossy(&buf).into_owned())
    }

    /// Regular files under the repository, sorted, never following a symlink, never entering
    /// [`SKIP_DIRS`] or hidden directories other than `.github`, bounded by [`MAX_WALK_ENTRIES`]
    /// and [`MAX_WALK_DEPTH`].
    pub fn walk(&self) -> Vec<String> {
        let mut out = Vec::new();
        let mut seen = 0usize;
        let mut stack: Vec<(PathBuf, String, usize)> = vec![(self.root.clone(), String::new(), 0)];
        while let Some((dir, rel, depth)) = stack.pop() {
            let Ok(rd) = std::fs::read_dir(&dir) else { continue };
            let mut entries: Vec<(String, std::fs::FileType)> = rd
                .filter_map(|e| e.ok())
                .filter_map(|e| Some((e.file_name().into_string().ok()?, e.file_type().ok()?)))
                .collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            for (name, ft) in entries.into_iter().rev() {
                seen += 1;
                if seen > MAX_WALK_ENTRIES {
                    self.note(if rel.is_empty() { "." } else { &rel }, "walk budget spent");
                    out.sort();
                    return out;
                }
                let child = if rel.is_empty() { name.clone() } else { format!("{rel}/{name}") };
                if !safe_rel(&child) || ft.is_symlink() {
                    continue;
                }
                if ft.is_dir() {
                    let hidden = name.starts_with('.') && name != ".github";
                    if depth < MAX_WALK_DEPTH && !hidden && !SKIP_DIRS.contains(&name.as_str()) {
                        stack.push((dir.join(&name), child, depth + 1));
                    }
                } else if ft.is_file() {
                    out.push(child);
                }
            }
        }
        out.sort();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths_are_plain() {
        assert!(safe_rel("schema/a.json"));
        for bad in ["", "/etc/passwd", "../x", "a/../../b", "a//b", "a\\b", "a b", "te\u{200b}st"] {
            assert!(!safe_rel(bad), "{bad:?}");
        }
    }

    #[test]
    fn a_line_scan_reads_the_first_64_kib_of_a_large_file_up_to_a_whole_line() {
        let d = tempfile::tempdir().unwrap();
        let mut big = String::from("//go:generate stringer -type=Color\n");
        while big.len() < 3 * MAX_FILE_BYTES as usize {
            big.push_str("func TestSomething(t *testing.T) { t.Log(\"x\") }\n");
        }
        std::fs::write(d.path().join("big_test.go"), &big).unwrap();
        std::fs::write(d.path().join("small.go"), "package x\n").unwrap();
        let f = RepoFiles::open(d.path()).unwrap();
        let (text, partial) = f.read_lines_prefix("big_test.go").unwrap();
        assert!(partial && text.len() as u64 <= MAX_FILE_BYTES && text.ends_with('\n'));
        assert!(text.starts_with("//go:generate stringer"));
        assert_eq!(f.read_lines_prefix("small.go").unwrap(), ("package x\n".to_string(), false));
        // A whole-file read still refuses it, and says so.
        assert!(f.read("big_test.go").is_none());
        assert_eq!(f.notes().get("big_test.go").map(String::as_str), Some("larger than 64 KiB"));
    }
}
