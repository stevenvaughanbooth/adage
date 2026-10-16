//! The repositories Adage can condition on, found in Claude Code's own history without parsing a
//! transcript (docs/design/everyday-ux.md §5.5), and the choice of them (`learn.repos`): what
//! `adage init` asks.
//!
//! Each project directory under `~/.claude/projects` is one working directory
//! ([`adage_learn::scope::claude_workdirs`]). Working directories are grouped by repository key:
//! a linked worktree's sessions count for its repository (the main checkout is shown), a
//! subdirectory's for the repository around it, and a removed Claude Code worktree's for the
//! repository it was in. A directory that is not in a repository, no longer exists, or is under
//! a temporary directory ([`temporary`]) is left out.

use adage_core::repo;
use adage_learn::scope::{self, Workdir};
use adage_store::config::{self, Source};
use adage_store::Paths;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// One repository with Claude Code sessions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub key: String,
    /// Its main checkout (where its linked worktrees' sessions count too); for the current
    /// repository ([`rows`]), the checkout this runs in, where it is conditioned and its instincts
    /// bind.
    pub root: PathBuf,
    pub sessions: usize,
    /// When its newest session was last written (milliseconds since the Unix epoch).
    pub last_ms: u64,
}

impl Found {
    /// How a person knows it: the key when it names a remote (`github.com/acme/billing`), else
    /// the checkout's path (`~/scratch/notes`), since `notes@1a2b3c4d5e6f` means nothing to them.
    pub fn label(&self) -> String {
        if self.key.contains('@') {
            crate::detect::tilde(&self.root)
        } else {
            self.key.clone()
        }
    }
}

/// The repositories with Claude Code sessions on this machine, most recently used first.
pub fn found(paths: &Paths) -> Vec<Found> {
    let cfg = config::load(paths).config;
    let Some(projects) = crate::history::claude_projects(&cfg) else { return vec![] };
    group(scope::claude_workdirs(&projects), &paths.repo_keys(), &temporary(adage_store::home_dir().as_deref()))
}

/// The temporary directories: this process's, and the system's (`/tmp`, `/var/tmp`, macOS's
/// per-user `/var/folders`), as written and as resolved. One that holds the home directory is
/// not, there (a sandbox, or a test, where everything is under it).
fn temporary(home: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = vec![std::env::temp_dir()];
    dirs.extend(["/tmp", "/var/tmp", "/var/folders"].map(PathBuf::from));
    let resolved: Vec<PathBuf> = dirs.iter().filter_map(|d| d.canonicalize().ok()).collect();
    dirs.extend(resolved);
    dirs.retain(|d| !home.is_some_and(|h| scope::inside(h, std::slice::from_ref(d))));
    dirs.sort();
    dirs.dedup();
    dirs
}

/// Working directories → repositories, by key. `skip`: directories whose repositories are left
/// out (the temporary directory).
fn group(dirs: Vec<Workdir>, keys: &Path, skip: &[PathBuf]) -> Vec<Found> {
    let mut by_key: BTreeMap<String, Found> = BTreeMap::new();
    let mut of_root: BTreeMap<PathBuf, (String, PathBuf)> = BTreeMap::new();
    for w in dirs {
        // A removed worktree's repository, else the repository around the directory, if it still
        // exists.
        let Some(root) = repo::removed_worktree(&w.cwd).or_else(|| repo::find_root(&w.cwd)) else {
            continue;
        };
        if !root.is_dir() || scope::inside(&root, skip) {
            continue;
        }
        let (key, main) = of_root
            .entry(root.clone())
            .or_insert_with(|| {
                let main = scope::checkouts(&root).into_iter().find(|c| c.join(".git").is_dir());
                (repo::repo_key_cached(&root, keys), main.unwrap_or_else(|| root.clone()))
            })
            .clone();
        let f = by_key.entry(key.clone()).or_insert(Found { key, root: main, sessions: 0, last_ms: 0 });
        f.sessions += w.sessions;
        f.last_ms = f.last_ms.max(w.last_ms);
    }
    let mut out: Vec<Found> = by_key.into_values().collect();
    out.sort_by(|a, b| b.last_ms.cmp(&a.last_ms).then_with(|| a.key.cmp(&b.key)));
    out
}

/// The repositories to tick when asking: those `learn.repos` names, and the current one. With
/// `learn.repos` unset (every repository, the default), only the current one: a person opts in
/// to the rest. Set to `all` on purpose, every one found.
pub fn preselected(paths: &Paths, found: &[Found], here: Option<&str>) -> Vec<String> {
    let loaded = config::load(paths);
    let mut keys: Vec<String> = match (&loaded.config.learn_repos, loaded.get("learn.repos").map(|(_, s)| s)) {
        (Some(list), _) => list.clone(),
        (None, Some(Source::Default) | None) => vec![],
        (None, Some(_)) => found.iter().map(|f| f.key.clone()).collect(),
    };
    if let Some(h) = here.filter(|h| !keys.iter().any(|k| k == h)) {
        keys.push(h.to_string());
    }
    keys
}

/// The rows to ask about: every repository found, the current one first, then any `learn.repos`
/// names that has no Claude Code session here (so a choice is never dropped unseen).
pub fn rows(found: Vec<Found>, chosen: &[String], here: Option<(&str, &Path)>) -> Vec<Found> {
    let mut rows = found;
    if let Some((key, root)) = here {
        match rows.iter().position(|f| f.key == key) {
            Some(i) => {
                let f = Found { root: root.to_path_buf(), ..rows.remove(i) };
                rows.insert(0, f);
            }
            None => rows.insert(0, Found { key: key.to_string(), root: root.to_path_buf(), sessions: 0, last_ms: 0 }),
        }
    }
    for k in chosen {
        if !rows.iter().any(|f| &f.key == k) {
            rows.push(Found { key: k.clone(), root: PathBuf::new(), sessions: 0, last_ms: 0 });
        }
    }
    rows
}

/// Store the choice as `learn.repos` (a person's decision: `config set`'s guard applies).
pub fn choose(paths: &Paths, keys: &[String]) -> anyhow::Result<()> {
    crate::share::require_person("config set learn.repos")?;
    config::set(paths, "learn.repos", &keys.join(",")).map_err(|e| anyhow::anyhow!(e))?;
    Ok(())
}

/// `12 sessions · last 3 days ago`, or `no Claude Code sessions yet`.
pub fn usage(f: &Found, now_ms: u64) -> String {
    if f.sessions == 0 {
        return "no Claude Code sessions yet".into();
    }
    let n = crate::words::count(f.sessions, "session");
    format!("{n} · last {}", when(now_ms, f.last_ms))
}

/// When something last happened, in days: `today`, `yesterday`, `5 days ago`, `3 weeks ago`,
/// `4 months ago`.
pub fn when(now_ms: u64, at_ms: u64) -> String {
    let days = now_ms.saturating_sub(at_ms) / 86_400_000;
    match days {
        0 => "today".into(),
        1 => "yesterday".into(),
        2..=13 => format!("{days} days ago"),
        14..=59 => format!("{} weeks ago", days / 7),
        _ => format!("{} months ago", days / 30),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(dir: &Path, origin: Option<&str>) {
        std::fs::create_dir_all(dir.join(".git")).unwrap();
        let cfg = origin.map(|o| format!("[remote \"origin\"]\n\turl = {o}\n")).unwrap_or_default();
        std::fs::write(dir.join(".git/config"), cfg).unwrap();
    }

    fn wd(cwd: &Path, sessions: usize, last_ms: u64) -> Workdir {
        Workdir { cwd: cwd.to_path_buf(), sessions, last_ms }
    }

    #[test]
    fn working_directories_group_into_repositories() {
        let d = tempfile::tempdir().unwrap();
        let base = d.path().canonicalize().unwrap();
        let app = base.join("app");
        git(&app, Some("git@github.com:acme/app.git"));
        std::fs::create_dir_all(app.join("src")).unwrap();
        // A linked worktree of app: `.git` is a file naming app's git directory.
        let wt = app.join(".claude/worktrees/w1");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::create_dir_all(app.join(".git/worktrees/w1")).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", app.join(".git/worktrees/w1").display())).unwrap();
        std::fs::write(app.join(".git/worktrees/w1/commondir"), "../..\n").unwrap();
        std::fs::write(app.join(".git/worktrees/w1/gitdir"), format!("{}\n", wt.join(".git").display())).unwrap();
        let notes = base.join("notes");
        git(&notes, None);
        let tmp = base.join("tmp");
        git(&tmp.join("scratch"), Some("https://github.com/acme/scratch"));
        let keys = base.join("keys");
        let dirs = vec![
            wd(&app.join("src"), 3, 500),
            wd(&wt, 2, 900),
            wd(&app.join(".claude/worktrees/gone"), 4, 100), // removed since
            wd(&notes, 1, 700),
            wd(&base.join("deleted"), 5, 800), // not a repository, and gone
            wd(&tmp.join("scratch"), 6, 950),  // under the temporary directory
        ];
        let got = group(dirs, &keys, &[tmp]);
        let shown: Vec<(&str, usize, u64)> = got.iter().map(|f| (f.key.as_str(), f.sessions, f.last_ms)).collect();
        assert_eq!(shown[0], ("github.com/acme/app", 9, 900), "worktrees, removed or not, count for app: {got:?}");
        assert_eq!(got[0].root, app, "the main checkout is shown");
        assert_eq!(got[0].label(), "github.com/acme/app");
        assert_eq!(got.len(), 2, "{got:?}");
        assert!(got[1].key.starts_with("notes@"), "{got:?}");
        assert_eq!(got[1].label(), crate::detect::tilde(&notes), "a key without a remote reads as its path");
    }

    #[test]
    fn temporary_directories_are_left_out_unless_home_is_in_one() {
        let tmp = temporary(Some(Path::new("/Users/me")));
        assert!(tmp.contains(&PathBuf::from("/tmp")) && tmp.contains(&std::env::temp_dir()), "{tmp:?}");
        let sandboxed = temporary(Some(Path::new("/tmp/sandbox/home")));
        assert!(!sandboxed.contains(&PathBuf::from("/tmp")), "{sandboxed:?}");
        assert!(sandboxed.contains(&PathBuf::from("/var/tmp")), "{sandboxed:?}");
    }

    #[test]
    fn the_current_repository_comes_first_and_no_choice_is_dropped() {
        let f = |k: &str, n: usize| Found {
            key: k.into(),
            root: PathBuf::from(format!("/r/{k}")),
            sessions: n,
            last_ms: 0,
        };
        let found = vec![f("a", 1), f("b", 2)];
        let got = rows(found.clone(), &["gone".into()], Some(("b", Path::new("/r/b-worktree"))));
        let keys: Vec<&str> = got.iter().map(|f| f.key.as_str()).collect();
        assert_eq!(keys, vec!["b", "a", "gone"]);
        assert_eq!(got[0].root, Path::new("/r/b-worktree"), "the checkout this runs in");
        let got = rows(found, &[], Some(("new", Path::new("/r/new"))));
        assert_eq!((got[0].key.as_str(), got[0].sessions), ("new", 0), "here, with no history yet");
        assert_eq!(usage(&got[0], 0), "no Claude Code sessions yet");
    }

    #[test]
    fn last_use_reads_in_days() {
        let day = 86_400_000;
        let now = 400 * day;
        assert_eq!(when(now, now - 1000), "today");
        assert_eq!(when(now, now - day), "yesterday");
        assert_eq!(when(now, now - 3 * day), "3 days ago");
        assert_eq!(when(now, now - 15 * day), "2 weeks ago");
        assert_eq!(when(now, now - 90 * day), "3 months ago");
        let f = Found { key: "k".into(), root: PathBuf::new(), sessions: 12, last_ms: now - 3 * day };
        assert_eq!(usage(&f, now), "12 sessions · last 3 days ago");
    }

    #[test]
    fn only_the_current_repository_is_ticked_until_a_person_chooses() {
        let d = tempfile::tempdir().unwrap();
        let paths = Paths::at(d.path());
        let found = vec![Found { key: "a".into(), root: PathBuf::new(), sessions: 1, last_ms: 0 }];
        assert_eq!(preselected(&paths, &found, Some("here")), vec!["here".to_string()]);
        config::set(&paths, "learn.repos", "b").unwrap();
        assert_eq!(preselected(&paths, &found, Some("here")), vec!["b".to_string(), "here".to_string()]);
        config::set(&paths, "learn.repos", "all").unwrap();
        assert_eq!(preselected(&paths, &found, None), vec!["a".to_string()], "all, chosen on purpose");
    }
}
