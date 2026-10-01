//! Repository-scoped reading (docs/zero-config.md §1 "Other repos' traces", P0-5): the traces
//! automatic learning reads for one repository are picked **before** anything is parsed, so other
//! repositories' transcripts are never opened by the learner or entered in the parse cache.
//!
//! A trace belongs to repository R when its recorded working directory is inside one of R's
//! checkouts ([`checkouts`]: the root, its main checkout when the root is a linked worktree, and
//! every linked worktree its git directory lists). Two cheap tests, in order:
//!
//! 1. **Claude Code**: the project directory's name. Claude Code names it after the session's
//!    working directory with every character but ASCII letters and digits replaced by `-`
//!    ([`claude_slug`]), so a session in R or below has a directory name starting with R's slug.
//!    Directories that fail this are never listed, let alone opened.
//! 2. **Every harness**: the first working directory the file records (Claude Code's first `cwd`,
//!    Codex's `session_meta.payload.cwd`), read from at most the first [`HEAD_BYTES`] of the file,
//!    must be inside a checkout (compared by path components, never by string prefix).

use std::path::{Path, PathBuf};

/// How much of a transcript's head is read to find its working directory.
pub const HEAD_BYTES: u64 = 256 * 1024;

/// A Claude Code project directory name for `dir` (`/home/a/my.repo` → `-home-a-my-repo`).
pub fn claude_slug(dir: &Path) -> String {
    dir.to_string_lossy().chars().map(|c| if c.is_ascii_alphanumeric() { c } else { '-' }).collect()
}

/// Every checkout of the repository at `root`: `root`; for a linked worktree, the main checkout;
/// and every linked worktree the common git directory lists (`<common>/worktrees/*/gitdir`).
/// Deduplicated, `root` first.
pub fn checkouts(root: &Path) -> Vec<PathBuf> {
    let mut out = vec![root.to_path_buf()];
    if let Some((_, common)) = adage_core::gitobj::dirs(root) {
        let common = adage_core::repo::lexical_normalise(&common);
        if common.file_name().is_some_and(|n| n == ".git") {
            if let Some(main) = common.parent() {
                out.push(main.to_path_buf());
            }
        }
        for e in std::fs::read_dir(common.join("worktrees")).into_iter().flatten().flatten() {
            // `gitdir` names the worktree's `.git` file.
            if let Ok(text) = std::fs::read_to_string(e.path().join("gitdir")) {
                if let Some(wt) = Path::new(text.trim()).parent() {
                    out.push(wt.to_path_buf());
                }
            }
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// Is this transcript verifiably gone: its directory can be listed and it is not there? A file
/// this process cannot see (a sandboxed session's learner, a directory not mounted here, a
/// permission error) is *not* gone, so evidence is never forgotten for being out of sight.
pub fn gone(path: &Path) -> bool {
    match std::fs::symlink_metadata(path) {
        Ok(_) => false,
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => false,
        Err(_) => {
            let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else { return false };
            match std::fs::read_dir(dir) {
                Ok(rd) => !rd.flatten().any(|e| e.file_name() == name),
                Err(_) => false,
            }
        }
    }
}

/// Is `cwd` one of `roots` or inside one (by path components)?
pub fn inside(cwd: &Path, roots: &[PathBuf]) -> bool {
    let cwd = adage_core::repo::lexical_normalise(cwd);
    roots.iter().any(|r| cwd.starts_with(adage_core::repo::lexical_normalise(r)))
}

/// The first working directory a transcript records: a Claude Code record's `cwd`, a Codex
/// `session_meta`'s `payload.cwd`, or an OpenCode export's `info.directory`. Reads at most
/// [`HEAD_BYTES`].
pub fn recorded_cwd(path: &Path) -> Option<PathBuf> {
    use std::io::{BufRead, Read};
    let f = std::fs::File::open(path).ok()?;
    let mut r = std::io::BufReader::new(f.take(HEAD_BYTES));
    let mut line = String::new();
    for _ in 0..64 {
        line.clear();
        if r.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else { continue };
        let cwd = v
            .get("cwd")
            .or_else(|| v.get("payload").and_then(|p| p.get("cwd")))
            .or_else(|| v.get("info").and_then(|i| i.get("directory")))
            .and_then(|c| c.as_str());
        if let Some(c) = cwd.filter(|c| !c.is_empty()) {
            return Some(PathBuf::from(c));
        }
    }
    None
}

/// A project directory named for a checkout, or for a directory below one.
fn slug_matches(name: &str, slugs: &[String]) -> bool {
    slugs.iter().any(|s| name == s || name.strip_prefix(s.as_str()).is_some_and(|rest| rest.starts_with('-')))
}

/// The Claude Code transcripts under `projects` (`~/.claude/projects`) recorded inside one of
/// `roots`: project directories are filtered by name first, then each file by its recorded cwd.
/// Sub-agent transcripts (`<session>/subagents/*.jsonl`) are included.
pub fn claude_transcripts(projects: &Path, roots: &[PathBuf]) -> Vec<PathBuf> {
    let slugs: Vec<String> = roots.iter().map(|r| claude_slug(r)).collect();
    let mut out = Vec::new();
    for d in std::fs::read_dir(projects).into_iter().flatten().flatten() {
        if !slug_matches(&d.file_name().to_string_lossy(), &slugs) {
            continue;
        }
        for p in crate::import::discover_project(&d.path()) {
            if recorded_cwd(&p).is_some_and(|c| inside(&c, roots)) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// How many Claude Code traces under `projects` were recorded inside `roots`, from directory
/// names alone (no file is opened): what the first-session notice counts. Sub-agent transcripts
/// are not counted.
pub fn count_claude_sessions(projects: &Path, roots: &[PathBuf]) -> usize {
    let slugs: Vec<String> = roots.iter().map(|r| claude_slug(r)).collect();
    std::fs::read_dir(projects)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|d| slug_matches(&d.file_name().to_string_lossy(), &slugs))
        .map(|d| {
            std::fs::read_dir(d.path())
                .into_iter()
                .flatten()
                .flatten()
                .filter(|f| f.path().extension().is_some_and(|x| x == "jsonl"))
                .count()
        })
        .sum()
}

/// A working directory Claude Code recorded sessions in: one project directory under
/// `~/.claude/projects`, its sessions (top-level transcripts; sub-agents' are not counted), and
/// when the newest was last written (milliseconds since the Unix epoch).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workdir {
    pub cwd: PathBuf,
    pub sessions: usize,
    pub last_ms: u64,
}

/// Every working directory Claude Code recorded sessions in, newest first, without parsing a
/// transcript: per project directory, its transcripts are listed and only the newest one's head is
/// read for its recorded `cwd` ([`recorded_cwd`]; the directory's name is lossy). A directory
/// whose transcripts record no `cwd` is left out.
pub fn claude_workdirs(projects: &Path) -> Vec<Workdir> {
    let mut out = Vec::new();
    for d in std::fs::read_dir(projects).into_iter().flatten().flatten() {
        let mut files: Vec<(u64, PathBuf)> = std::fs::read_dir(d.path())
            .into_iter()
            .flatten()
            .flatten()
            .map(|f| f.path())
            .filter(|p| p.extension().is_some_and(|x| x == "jsonl"))
            .map(|p| (modified_ms(&p), p))
            .collect();
        files.sort_by(|a, b| b.cmp(a));
        // The newest transcript names the directory; an empty or unreadable one defers to the next.
        let Some(cwd) = files.iter().take(3).find_map(|(_, p)| recorded_cwd(p)) else { continue };
        out.push(Workdir { cwd, sessions: files.len(), last_ms: files[0].0 });
    }
    out.sort_by(|a, b| b.last_ms.cmp(&a.last_ms).then_with(|| a.cwd.cmp(&b.cwd)));
    out
}

fn modified_ms(p: &Path) -> u64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_millis() as u64)
}

/// Of these transcripts (Codex rollouts, OpenCode exports), those recorded inside `roots`.
pub fn recorded_inside(files: Vec<PathBuf>, roots: &[PathBuf]) -> Vec<PathBuf> {
    files.into_iter().filter(|p| recorded_cwd(p).is_some_and(|c| inside(&c, roots))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_file_whose_directory_lists_without_it_is_gone() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("a.jsonl");
        std::fs::write(&f, "{}").unwrap();
        assert!(!gone(&f));
        std::fs::remove_file(&f).unwrap();
        assert!(gone(&f), "its directory lists, without it");
        assert!(!gone(&d.path().join("hidden/b.jsonl")), "a directory this process cannot see: not gone");
    }

    #[test]
    fn slugs_and_containment() {
        assert_eq!(claude_slug(Path::new("/home/a/my.repo")), "-home-a-my-repo");
        assert_eq!(claude_slug(Path::new("/r/app/.claude/worktrees/x")), "-r-app--claude-worktrees-x");
        let roots = vec![PathBuf::from("/r/app")];
        assert!(inside(Path::new("/r/app"), &roots));
        assert!(inside(Path::new("/r/app/src/../lib"), &roots));
        assert!(!inside(Path::new("/r/app2"), &roots), "by component, not by string prefix");
        assert!(!inside(Path::new("/r"), &roots));
    }

    #[test]
    fn only_this_repositorys_transcripts_are_listed() {
        let d = tempfile::tempdir().unwrap();
        let projects = d.path().join("projects");
        let write = |dir: &str, name: &str, cwd: &str| {
            let p = projects.join(dir);
            std::fs::create_dir_all(&p).unwrap();
            let f = p.join(name);
            std::fs::write(&f, format!("{}\n", serde_json::json!({"type":"user","cwd":cwd}))).unwrap();
            f
        };
        let a = write("-r-app", "a.jsonl", "/r/app");
        let sub = write("-r-app-src", "b.jsonl", "/r/app/src");
        let _other = write("-r-app2", "c.jsonl", "/r/app2");
        let _lying = write("-r-app", "d.jsonl", "/elsewhere");
        let got = claude_transcripts(&projects, &[PathBuf::from("/r/app")]);
        assert_eq!(got, vec![a, sub]);
        assert_eq!(count_claude_sessions(&projects, &[PathBuf::from("/r/app")]), 3, "names only: d counts");
    }

    #[test]
    fn working_directories_come_from_the_newest_transcripts_head() {
        let d = tempfile::tempdir().unwrap();
        let projects = d.path().join("projects");
        let write = |dir: &str, name: &str, line: &str, age_s: u64| {
            let p = projects.join(dir);
            std::fs::create_dir_all(&p).unwrap();
            let f = p.join(name);
            std::fs::write(&f, format!("{line}\n")).unwrap();
            let t = std::time::SystemTime::now() - std::time::Duration::from_secs(age_s);
            std::fs::File::options().write(true).open(&f).unwrap().set_modified(t).unwrap();
        };
        let cwd = |c: &str| serde_json::json!({"type":"user","cwd":c}).to_string();
        write("-r-app", "a.jsonl", &cwd("/r/app"), 300);
        write("-r-app", "b.jsonl", &cwd("/r/app"), 60);
        std::fs::create_dir_all(projects.join("-r-app/b/subagents")).unwrap();
        write("-r-app/b/subagents", "s.jsonl", &cwd("/r/app"), 10);
        write("-r-old", "c.jsonl", &cwd("/r/old"), 86_400);
        // The newest transcript records nothing yet: the next one names the directory.
        write("-r-new", "d.jsonl", &cwd("/r/new"), 7_200);
        write("-r-new", "e.jsonl", r#"{"type":"summary"}"#, 0);
        write("-r-none", "f.jsonl", r#"{"type":"summary"}"#, 0);
        let got = claude_workdirs(&projects);
        let names: Vec<(&str, usize)> = got.iter().map(|w| (w.cwd.to_str().unwrap(), w.sessions)).collect();
        assert_eq!(names, vec![("/r/new", 2), ("/r/app", 2), ("/r/old", 1)], "newest first; sub-agents not counted");
        assert!(got[0].last_ms > got[1].last_ms && got[1].last_ms > got[2].last_ms);
    }
}
