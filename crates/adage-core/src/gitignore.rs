//! Whether git ignores a path in a checkout, read from its `.gitignore` files and
//! `info/exclude`, without running git. Procedure cards never take content from an ignored file
//! (build output, local configuration, secrets kept out of the repository).
//!
//! The rules are git's, for the patterns people write: blank lines and `#` comments; `!` to
//! re-include; a trailing `/` for directories only; a pattern with a `/` before its end is anchored
//! to its `.gitignore`'s directory, one without matches a name at any depth; `*`, `?`, `[…]` and
//! `**`. A file inside an ignored directory is ignored. The global excludes file is not read.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

struct Rule {
    pattern: String,
    negate: bool,
    dir_only: bool,
    anchored: bool,
}

fn rules(text: &str) -> Vec<Rule> {
    text.lines()
        .filter_map(|l| {
            let l = l.trim_end_matches('\r').trim_end_matches(' ');
            if l.is_empty() || l.starts_with('#') {
                return None;
            }
            let (negate, l) = match l.strip_prefix('!') {
                Some(r) => (true, r),
                None => (false, l.strip_prefix('\\').unwrap_or(l)),
            };
            let (dir_only, l) = match l.strip_suffix('/') {
                Some(r) => (true, r),
                None => (false, l),
            };
            let anchored = l.contains('/');
            let pattern = l.trim_start_matches('/').to_string();
            (!pattern.is_empty()).then_some(Rule { pattern, negate, dir_only, anchored })
        })
        .collect()
}

/// A glob over `/`-separated paths: `*` and `?` stay within a component, `**` crosses them.
fn glob(p: &[u8], s: &[u8]) -> bool {
    if p.is_empty() {
        return s.is_empty();
    }
    if p.starts_with(b"**") {
        let rest = &p[2..];
        let rest = rest.strip_prefix(b"/").unwrap_or(rest);
        return (0..=s.len()).any(|i| (i == 0 || s[i - 1] == b'/' || rest.is_empty()) && glob(rest, &s[i..]));
    }
    match p[0] {
        b'*' => {
            for i in 0..=s.len() {
                if glob(&p[1..], &s[i..]) {
                    return true;
                }
                if i < s.len() && s[i] == b'/' {
                    break;
                }
            }
            false
        }
        b'?' => !s.is_empty() && s[0] != b'/' && glob(&p[1..], &s[1..]),
        b'[' => {
            let Some(close) = p.iter().skip(2).position(|&c| c == b']').map(|k| k + 2) else {
                return !s.is_empty() && s[0] == b'[' && glob(&p[1..], &s[1..]);
            };
            if s.is_empty() || s[0] == b'/' {
                return false;
            }
            let class = &p[1..close];
            let (neg, class) = match class.first() {
                Some(b'!' | b'^') => (true, &class[1..]),
                _ => (false, class),
            };
            let mut hit = false;
            let mut k = 0;
            while k < class.len() {
                if k + 2 < class.len() && class[k + 1] == b'-' {
                    hit |= (class[k]..=class[k + 2]).contains(&s[0]);
                    k += 3;
                } else {
                    hit |= class[k] == s[0];
                    k += 1;
                }
            }
            hit != neg && glob(&p[close + 1..], &s[1..])
        }
        b'\\' if p.len() > 1 => !s.is_empty() && s[0] == p[1] && glob(&p[2..], &s[1..]),
        c => !s.is_empty() && s[0] == c && glob(&p[1..], &s[1..]),
    }
}

/// The `.gitignore` rules of one checkout, read as paths are asked about.
pub struct Ignore {
    root: PathBuf,
    exclude: Vec<Rule>,
    /// Per directory (repository-relative, `""` for the root): its `.gitignore`'s rules.
    dirs: RefCell<HashMap<String, std::rc::Rc<Vec<Rule>>>>,
}

impl Ignore {
    /// The checkout at `root` (its `info/exclude` is read now; `.gitignore` files as needed).
    pub fn new(root: &Path) -> Ignore {
        let exclude = crate::gitobj::dirs(root)
            .and_then(|(_, common)| std::fs::read_to_string(common.join("info/exclude")).ok())
            .map(|t| rules(&t))
            .unwrap_or_default();
        Ignore { root: root.to_path_buf(), exclude, dirs: RefCell::new(HashMap::new()) }
    }

    fn at(&self, dir: &str) -> std::rc::Rc<Vec<Rule>> {
        self.dirs
            .borrow_mut()
            .entry(dir.to_string())
            .or_insert_with(|| {
                let p = self.root.join(dir).join(".gitignore");
                let small = std::fs::metadata(&p).is_ok_and(|m| m.len() <= 1 << 20);
                std::rc::Rc::new(
                    small.then(|| std::fs::read_to_string(&p).ok()).flatten().map_or_else(Vec::new, |t| rules(&t)),
                )
            })
            .clone()
    }

    /// The last rule that decides `rel` (a path under `base`, the rules' directory), if any.
    fn decide(rules: &[Rule], rel: &str, is_dir: bool, verdict: &mut Option<bool>) {
        let name = rel.rsplit('/').next().unwrap_or(rel);
        for r in rules {
            if r.dir_only && !is_dir {
                continue;
            }
            let target = if r.anchored { rel } else { name };
            if glob(r.pattern.as_bytes(), target.as_bytes()) {
                *verdict = Some(!r.negate);
            }
        }
    }

    fn ignored_one(&self, rel: &str, is_dir: bool) -> bool {
        let mut verdict = None;
        Self::decide(&self.exclude, rel, is_dir, &mut verdict);
        // Deeper `.gitignore` files decide after shallower ones.
        let comps: Vec<&str> = rel.split('/').collect();
        for depth in 0..comps.len() {
            let dir = comps[..depth].join("/");
            let rules = self.at(&dir);
            if rules.is_empty() {
                continue;
            }
            let sub = comps[depth..].join("/");
            Self::decide(&rules, &sub, is_dir, &mut verdict);
        }
        verdict == Some(true)
    }

    /// Whether git ignores the file at `rel` (repository-relative, `/`-separated): it, or a
    /// directory above it, is ignored.
    pub fn ignored(&self, rel: &str) -> bool {
        let rel = rel.trim_start_matches("./");
        if rel.is_empty() || rel.split('/').any(|c| c == "..") {
            return false;
        }
        let comps: Vec<&str> = rel.split('/').filter(|c| !c.is_empty()).collect();
        (1..=comps.len()).any(|n| self.ignored_one(&comps[..n].join("/"), n < comps.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patterns_as_git_reads_them() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join(".git/info")).unwrap();
        std::fs::create_dir_all(d.path().join("web")).unwrap();
        std::fs::write(
            d.path().join(".gitignore"),
            "# local\n.env*\n!.env.example\n/build/\n*.log\nconfig/local.*\n**/cache/**\nsecret[0-9].txt\n",
        )
        .unwrap();
        std::fs::write(d.path().join("web/.gitignore"), "dist\n").unwrap();
        std::fs::write(d.path().join(".git/info/exclude"), "scratch.py\n").unwrap();
        let ig = Ignore::new(d.path());
        for p in [
            ".env",
            ".env.local",
            "sub/.env.production",
            "build/out.js",
            "app.log",
            "logs/app.log",
            "config/local.toml",
            "a/cache/b/c.txt",
            "secret1.txt",
            "web/dist/app.js",
            "scratch.py",
        ] {
            assert!(ig.ignored(p), "{p}");
        }
        for p in [".env.example", "src/build/x.rs", "config/prod.toml", "dist/app.js", "src/main.rs", "secretX.txt"] {
            assert!(!ig.ignored(p), "{p}");
        }
    }
}
