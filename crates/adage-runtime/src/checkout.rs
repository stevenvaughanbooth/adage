//! What a decision reads of the repository's checkout, as a port: [`Disk`] reads the checkout on
//! this machine (what the hook uses); [`Unread`] reads nothing, for a decision made where the
//! checkout is not (a replay of recorded sessions, a remote service). Everything [`crate::decide`]
//! needs from the working tree goes through [`Checkout`]; nothing else in the runtime touches the
//! filesystem.

use std::path::Path;

/// The checkout a decision may read.
pub trait Checkout: Sync {
    /// The text of the file at `rel` under `root`, if Adage may read it: the gate is
    /// [`crate::orient::readable`] (a plain repository-relative path, no dotfile, ignored or
    /// secret-holding file, a regular file inside the checkout of bounded size, in UTF-8).
    fn text(&self, root: &Path, rel: &str) -> Option<String>;
    /// Whether `rel` names a regular file inside the checkout at `root`, symlinks resolved (a
    /// repository-path slot's contract). `rel` is already a safe relative path.
    fn is_file(&self, root: &Path, rel: &str) -> bool;
}

/// The checkout on this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Disk;

impl Checkout for Disk {
    fn text(&self, root: &Path, rel: &str) -> Option<String> {
        let base = root.canonicalize().ok()?;
        crate::orient::readable(&base, rel, &adage_core::gitignore::Ignore::new(root))
    }

    fn is_file(&self, root: &Path, rel: &str) -> bool {
        match (root.canonicalize(), root.join(rel).canonicalize()) {
            (Ok(r), Ok(p)) => p.starts_with(&r) && p.is_file(),
            _ => false,
        }
    }
}

/// No checkout: nothing is readable and no path names a file. A card is then delivered without
/// its orientation, and a repository-path slot never binds.
#[derive(Debug, Clone, Copy, Default)]
pub struct Unread;

impl Checkout for Unread {
    fn text(&self, _: &Path, _: &str) -> Option<String> {
        None
    }

    fn is_file(&self, _: &Path, _: &str) -> bool {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_reads_inside_the_checkout_only_and_unread_reads_nothing() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/a.py"), "x = 1\n").unwrap();
        std::fs::write(root.join(".env"), "SECRET=1\n").unwrap();
        assert_eq!(Disk.text(root, "src/a.py").as_deref(), Some("x = 1\n"));
        assert!(Disk.is_file(root, "src/a.py"));
        assert_eq!(Disk.text(root, ".env"), None, "a dotfile is never read");
        assert!(!Disk.is_file(root, "src"), "a directory is not a file");
        assert!(!Disk.is_file(root, "src/missing.py"));
        assert_eq!(Unread.text(root, "src/a.py"), None);
        assert!(!Unread.is_file(root, "src/a.py"));
    }
}
