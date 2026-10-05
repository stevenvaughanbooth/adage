//! Repository keys without `origin`: `<dir>@<root commit>`, read from the object store without
//! running git (loose objects, packs with deltas, packed refs, linked worktrees), and the legacy
//! bare-name key accepted only while nothing supersedes it.
//!
//! The hand-built repositories below use made-up object ids: the reader never re-hashes objects,
//! so what they prove is the format handling. The last test checks the same against real git,
//! when it is installed.

use adage_core::repo::{lookup_key, repo_key, repo_key_cached};
use miniz_oxide::deflate::compress_to_vec_zlib;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn id(n: u8) -> String {
    format!("{n:02x}").repeat(20)
}

fn commit(tree: u8, parent: Option<&str>, msg: &str) -> Vec<u8> {
    let mut s = format!("tree {}\n", id(tree));
    if let Some(p) = parent {
        s.push_str(&format!("parent {p}\n"));
    }
    s.push_str(&format!("author A <a@b> 0 +0000\ncommitter A <a@b> 0 +0000\n\n{msg}\n"));
    s.into_bytes()
}

/// A work tree `root/<name>` whose `.git` has `HEAD` on `refs/heads/main`.
fn repo(root: &Path, name: &str) -> PathBuf {
    let w = root.join(name);
    std::fs::create_dir_all(w.join(".git/objects")).unwrap();
    std::fs::create_dir_all(w.join(".git/refs/heads")).unwrap();
    std::fs::write(w.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(w.join(".git/config"), "[core]\n\tbare = false\n").unwrap();
    w
}

fn loose(git: &Path, oid: &str, body: &[u8]) {
    let mut raw = format!("commit {}\0", body.len()).into_bytes();
    raw.extend_from_slice(body);
    let dir = git.join("objects").join(&oid[..2]);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(&oid[2..]), compress_to_vec_zlib(&raw, 6)).unwrap();
}

/// A three-commit loose history, root `id(root)`; HEAD → `refs/heads/main`.
fn loose_history(w: &Path, root: u8) {
    let git = w.join(".git");
    let (a, b, c) = (id(root), id(root + 1), id(root + 2));
    loose(&git, &a, &commit(1, None, "root"));
    loose(&git, &b, &commit(2, Some(&a), "second"));
    loose(&git, &c, &commit(3, Some(&b), "third"));
    std::fs::write(git.join("refs/heads/main"), format!("{c}\n")).unwrap();
}

fn keys<'a>(v: &[&'a str]) -> BTreeSet<&'a str> {
    v.iter().copied().collect()
}

#[test]
fn unrelated_repositories_with_one_name_get_different_keys() {
    let d = tempfile::tempdir().unwrap();
    let (a, b) = (repo(&d.path().join("x"), "app"), repo(&d.path().join("y"), "app"));
    loose_history(&a, 0x10);
    loose_history(&b, 0x20);
    assert_eq!(repo_key(&a), format!("app@{}", &id(0x10)[..12]));
    assert_eq!(repo_key(&b), format!("app@{}", &id(0x20)[..12]));

    // A copy of `a` elsewhere (same root commit) shares its key.
    let c = repo(&d.path().join("z"), "app");
    loose_history(&c, 0x10);
    assert_eq!(repo_key(&c), repo_key(&a));

    // `origin` still wins.
    std::fs::write(a.join(".git/config"), "[remote \"origin\"]\n\turl = git@github.com:Acme/App.git\n").unwrap();
    assert_eq!(repo_key(&a), "github.com/acme/app");
}

#[test]
fn no_commits_or_no_repository_falls_back_to_the_path() {
    let d = tempfile::tempdir().unwrap();
    let empty = repo(d.path(), "fresh");
    let k = repo_key(&empty);
    assert!(k.starts_with("fresh@path-") && k.len() == "fresh@path-".len() + 12, "{k}");
    let plain = d.path().join("notes");
    std::fs::create_dir_all(&plain).unwrap();
    assert!(repo_key(&plain).starts_with("notes@path-"));
    assert_ne!(repo_key(&plain), repo_key(&d.path().join("x").join("notes").tap_mkdir()));
    // A checkout that no longer exists (old history): the legacy key.
    assert_eq!(repo_key(&d.path().join("gone")), "gone");
}

trait Mkdir {
    fn tap_mkdir(self) -> Self;
}
impl Mkdir for PathBuf {
    fn tap_mkdir(self) -> Self {
        std::fs::create_dir_all(&self).unwrap();
        self
    }
}

#[test]
fn packed_refs_and_linked_worktrees() {
    let d = tempfile::tempdir().unwrap();
    let main = repo(d.path(), "app");
    loose_history(&main, 0x30);
    // The branch moves into packed-refs.
    let tip = std::fs::read_to_string(main.join(".git/refs/heads/main")).unwrap();
    std::fs::remove_file(main.join(".git/refs/heads/main")).unwrap();
    std::fs::write(
        main.join(".git/packed-refs"),
        format!("# pack-refs with: peeled fully-peeled sorted\n{} refs/heads/main\n^{}\n", tip.trim(), id(0x99)),
    )
    .unwrap();
    let want = format!("app@{}", &id(0x30)[..12]);
    assert_eq!(repo_key(&main), want);

    // A linked worktree: `.git` is a file, HEAD lives in the worktree's git dir, objects and refs
    // in the common dir. It has its main checkout's key, so reflexes learned there reach it.
    let wt_git = main.join(".git/worktrees/feature");
    std::fs::create_dir_all(&wt_git).unwrap();
    std::fs::write(wt_git.join("HEAD"), "ref: refs/heads/main\n").unwrap();
    std::fs::write(wt_git.join("commondir"), "../..\n").unwrap();
    let wt = d.path().join("feature");
    std::fs::create_dir_all(&wt).unwrap();
    std::fs::write(wt.join(".git"), format!("gitdir: {}\n", wt_git.display())).unwrap();
    assert_eq!(repo_key(&wt), format!("app@{}", &id(0x30)[..12]));
}

// ---------------------------------------------------------------------------------------------
// A hand-built v2 pack: a full commit, an offset delta and a reference delta.

fn varint(mut n: usize, out: &mut Vec<u8>) {
    loop {
        let b = (n & 0x7f) as u8;
        n >>= 7;
        if n == 0 {
            out.push(b);
            return;
        }
        out.push(b | 0x80);
    }
}

fn entry_header(kind: u8, size: usize) -> Vec<u8> {
    let mut out = Vec::new();
    let mut c = (kind << 4) | (size & 15) as u8;
    let mut s = size >> 4;
    while s != 0 {
        out.push(c | 0x80);
        c = (s & 0x7f) as u8;
        s >>= 7;
    }
    out.push(c);
    out
}

fn ofs_encoding(mut ofs: u64) -> Vec<u8> {
    let mut out = vec![(ofs & 127) as u8];
    ofs >>= 7;
    while ofs != 0 {
        ofs -= 1;
        out.insert(0, 0x80 | (ofs & 127) as u8);
        ofs >>= 7;
    }
    out
}

/// A delta from `base` to `target`: copy `base`'s first line, insert the rest.
fn delta(base: &[u8], target: &[u8]) -> Vec<u8> {
    let mut d = Vec::new();
    varint(base.len(), &mut d);
    varint(target.len(), &mut d);
    let line = base.iter().position(|&b| b == b'\n').unwrap() + 1;
    assert_eq!(&base[..line], &target[..line], "the test shares the tree line");
    d.extend_from_slice(&[0x80 | 0x10, line as u8]); // copy offset 0, `line` bytes
    for chunk in target[line..].chunks(127) {
        d.push(chunk.len() as u8);
        d.extend_from_slice(chunk);
    }
    d
}

fn hex_bytes(h: &str) -> Vec<u8> {
    (0..20).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect()
}

fn write_pack(git: &Path, objects: &[(String, Vec<u8>)]) {
    let mut pack = b"PACK".to_vec();
    pack.extend_from_slice(&2u32.to_be_bytes());
    pack.extend_from_slice(&(objects.len() as u32).to_be_bytes());
    let mut offsets: Vec<(String, u64)> = Vec::new();
    for (oid, entry) in objects {
        offsets.push((oid.clone(), pack.len() as u64));
        pack.extend_from_slice(entry);
    }
    pack.extend_from_slice(&[0u8; 20]);
    offsets.sort();
    let mut idx = vec![0xff, b't', b'O', b'c', 0, 0, 0, 2];
    for b in 0..=255u8 {
        let n = offsets.iter().filter(|(o, _)| hex_bytes(o)[0] <= b).count() as u32;
        idx.extend_from_slice(&n.to_be_bytes());
    }
    for (o, _) in &offsets {
        idx.extend_from_slice(&hex_bytes(o));
    }
    idx.extend(std::iter::repeat_n(0u8, offsets.len() * 4)); // CRCs, unchecked
    for (_, off) in &offsets {
        idx.extend_from_slice(&(*off as u32).to_be_bytes());
    }
    idx.extend_from_slice(&[0u8; 40]);
    let dir = git.join("objects/pack");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("pack-test.pack"), pack).unwrap();
    std::fs::write(dir.join("pack-test.idx"), idx).unwrap();
}

#[test]
fn packed_history_with_offset_and_reference_deltas() {
    let d = tempfile::tempdir().unwrap();
    let w = repo(d.path(), "packed");
    let git = w.join(".git");
    let (a, b, c, e) = (id(0x41), id(0x42), id(0x43), id(0x44));
    let ca = commit(7, None, "root");
    let cb = commit(7, Some(&a), "second");
    let cc = commit(7, Some(&b), &"a longer message, so the delta needs two inserts. ".repeat(4));
    let ce = commit(7, Some(&c), "fourth");
    let full = |kind: u8, body: &[u8]| {
        let mut v = entry_header(kind, body.len());
        v.extend(compress_to_vec_zlib(body, 6));
        v
    };
    let ea = full(1, &ca);
    let eb = full(1, &cb);
    // c: an offset delta against b, which sits right after a.
    let dc = delta(&cb, &cc);
    let b_off = 12 + ea.len() as u64;
    let c_off = b_off + eb.len() as u64;
    let mut ec = entry_header(6, dc.len());
    ec.extend(ofs_encoding(c_off - b_off));
    ec.extend(compress_to_vec_zlib(&dc, 6));
    // e: a reference delta against c.
    let de = delta(&cc, &ce);
    let mut ee = entry_header(7, de.len());
    ee.extend(hex_bytes(&c));
    ee.extend(compress_to_vec_zlib(&de, 6));
    write_pack(&git, &[(a.clone(), ea), (b, eb), (c, ec), (e.clone(), ee)]);
    std::fs::write(git.join("refs/heads/main"), format!("{e}\n")).unwrap();
    assert_eq!(repo_key(&w), format!("packed@{}", &a[..12]));
}

#[test]
fn a_cached_root_is_reused_only_while_the_repository_holds_it() {
    let d = tempfile::tempdir().unwrap();
    let cache = d.path().join("cache");
    let w = repo(d.path(), "cached");
    loose_history(&w, 0x50);
    let want = format!("cached@{}", &id(0x50)[..12]);
    assert_eq!(repo_key_cached(&w, &cache), want);
    assert_eq!(std::fs::read_dir(&cache).unwrap().count(), 1, "one cache entry per repository path");
    // The entry is read back (the history is not walked: break the tip and the key holds).
    let git = w.join(".git");
    std::fs::write(git.join("refs/heads/main"), format!("{}\n", id(0x5f))).unwrap();
    assert_eq!(repo_key_cached(&w, &cache), want);
    // Once the repository no longer holds that root, the entry is ignored and the history walked
    // again (here it cannot be: the tip is broken, so the key falls back to the path).
    let root = id(0x50);
    std::fs::remove_file(git.join("objects").join(&root[..2]).join(&root[2..])).unwrap();
    assert!(repo_key_cached(&w, &cache).starts_with("cached@path-"));
}

#[test]
fn legacy_keys_hold_until_a_repository_of_that_name_is_re_keyed() {
    let d = tempfile::tempdir().unwrap();
    let w = repo(d.path(), "app");
    loose_history(&w, 0x60);
    let new = repo_key(&w);
    let other = format!("app@{}", &id(0x70)[..12]);
    // Only the legacy key: routes learned before the change keep working.
    assert_eq!(lookup_key(&w, None, &keys(&["app", "else"])).as_deref(), Some("app"));
    // This repository re-keyed: its own key.
    assert_eq!(lookup_key(&w, None, &keys(&["app", &new])).as_deref(), Some(new.as_str()));
    // Another repository named `app` re-keyed: the legacy key could be either's; neither applies.
    assert_eq!(lookup_key(&w, None, &keys(&["app", &other])), None);
    assert_eq!(lookup_key(&w, None, &keys(&[&other])), None);
    // Nothing for any `app`: no object store read is needed, and nothing applies.
    assert_eq!(lookup_key(&w, None, &keys(&["application@0123456789ab", "ap"])), None);
}

/// Against real git: loose, then packed with deltas (`git gc --aggressive`), and a linked
/// worktree. Skipped when git is not installed.
#[test]
fn matches_git_rev_list() {
    let git = |dir: &Path, args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_AUTHOR_NAME", "a")
            .env("GIT_AUTHOR_EMAIL", "a@b")
            .env("GIT_COMMITTER_NAME", "a")
            .env("GIT_COMMITTER_EMAIL", "a@b")
            .output()
    };
    let d = tempfile::tempdir().unwrap();
    let w = d.path().join("real");
    std::fs::create_dir_all(&w).unwrap();
    if !git(&w, &["init", "-q", "-b", "main"]).is_ok_and(|o| o.status.success()) {
        eprintln!("git not available: skipping");
        return;
    }
    for i in 0..40 {
        std::fs::write(w.join("f.txt"), format!("line {i}\n").repeat(50)).unwrap();
        git(&w, &["add", "f.txt"]).unwrap();
        git(&w, &["commit", "-q", "-m", &format!("commit {i} with the same long message body ").repeat(3)]).unwrap();
    }
    let out = git(&w, &["rev-list", "--max-parents=0", "HEAD"]).unwrap();
    let root = String::from_utf8(out.stdout).unwrap().trim().to_string();
    let want = format!("real@{}", &root[..12]);
    assert_eq!(repo_key(&w), want, "loose objects");

    let p = d.path().join("packed");
    std::fs::create_dir_all(&p).unwrap();
    std::fs::create_dir_all(p.join("real")).unwrap();
    let pw = p.join("real");
    git(d.path(), &["clone", "-q", "--no-local", w.to_str().unwrap(), pw.to_str().unwrap()]).unwrap();
    git(&pw, &["remote", "remove", "origin"]).unwrap();
    git(&pw, &["gc", "-q", "--aggressive", "--prune=now"]).unwrap();
    assert!(std::fs::read_dir(pw.join(".git/objects/pack")).unwrap().count() >= 2, "packed");
    assert_eq!(repo_key(&pw), want, "a packed copy shares the key");

    let wt = d.path().join("wt");
    git(&w, &["worktree", "add", "-q", "-b", "side", wt.to_str().unwrap()]).unwrap();
    assert_eq!(repo_key(&wt), want, "a linked worktree has its main checkout's key");
}
