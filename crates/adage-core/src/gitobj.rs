//! Just enough of git's on-disk format to find a repository's root commit without running git:
//! `HEAD` → ref (loose or packed) → commit objects (loose, or in a v2 pack, deltas included),
//! following first parents to the commit that has none.
//!
//! Hooks run this, so it never spawns a process, reads only what the walk needs, and gives up
//! (returns `None`) on anything it does not understand: SHA-256 repositories, shallow clones,
//! alternates, a corrupt object, or a history longer than [`MAX_WALK`] first-parent commits. The
//! caller then falls back to another key.

use miniz_oxide::inflate::decompress_to_vec_zlib_with_limit;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// At most this many first-parent commits are walked.
pub const MAX_WALK: usize = 100_000;
/// The largest commit object read (a commit is its header plus its message).
const MAX_COMMIT: usize = 4 << 20;
/// The longest delta chain followed in a pack (git's default `--depth` is 50).
const MAX_DELTA_DEPTH: usize = 64;

pub type Oid = [u8; 20];

pub fn hex(id: &Oid) -> String {
    id.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_hex(s: &str) -> Option<Oid> {
    let s = s.trim();
    if s.len() != 40 {
        return None;
    }
    let mut out = [0u8; 20];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// A repository's git directories: `git` holds `HEAD` (per worktree), `common` holds objects and
/// shared refs (the same directory, except in a linked worktree).
pub struct Repo {
    git: PathBuf,
    common: PathBuf,
    packs: Option<Vec<Pack>>,
}

struct Pack {
    idx: File,
    pack: File,
    pack_len: u64,
    fanout: Vec<u32>,
}

const OBJ_COMMIT: u8 = 1;
const OBJ_OFS_DELTA: u8 = 6;
const OBJ_REF_DELTA: u8 = 7;

/// The git directories of the work tree at `root`: its own, which holds `HEAD`, and the common one,
/// which holds the config, objects and shared refs. `root/.git` is a directory, or a `gitdir:` file:
/// a linked worktree's git directory names the common one (the main checkout's) in `commondir`; a
/// submodule's has no `commondir`, so it is its own common directory.
pub fn dirs(root: &Path) -> Option<(PathBuf, PathBuf)> {
    let dotgit = root.join(".git");
    let git = if dotgit.is_dir() {
        dotgit
    } else {
        let text = std::fs::read_to_string(&dotgit).ok()?;
        let p = PathBuf::from(text.lines().next()?.strip_prefix("gitdir:")?.trim());
        if p.is_absolute() {
            p
        } else {
            root.join(p)
        }
    };
    let common = match std::fs::read_to_string(git.join("commondir")) {
        Ok(c) => {
            let p = PathBuf::from(c.trim());
            if p.is_absolute() {
                p
            } else {
                git.join(p)
            }
        }
        Err(_) => git.clone(),
    };
    Some((git, common))
}

impl Repo {
    /// The repository whose work tree is `root` ([`dirs`]).
    pub fn open(root: &Path) -> Option<Repo> {
        let (git, common) = dirs(root)?;
        // Objects this walk cannot see.
        if common.join("shallow").exists() || common.join("objects/info/alternates").exists() {
            return None;
        }
        Some(Repo { git, common, packs: None })
    }

    /// The commit `HEAD` names.
    pub fn head(&self) -> Option<Oid> {
        let head = std::fs::read_to_string(self.git.join("HEAD")).ok()?;
        self.resolve(head.trim(), 0)
    }

    fn resolve(&self, value: &str, depth: usize) -> Option<Oid> {
        let Some(name) = value.strip_prefix("ref:") else { return parse_hex(value) };
        let name = name.trim();
        if depth > 5 || !name.starts_with("refs/") || name.split('/').any(|c| c == ".." || c.is_empty()) {
            return None;
        }
        for dir in [&self.git, &self.common] {
            if let Ok(v) = std::fs::read_to_string(dir.join(name)) {
                return self.resolve(v.trim(), depth + 1);
            }
        }
        let packed = std::fs::read_to_string(self.common.join("packed-refs")).ok()?;
        packed.lines().filter(|l| !l.starts_with('#') && !l.starts_with('^')).find_map(|l| {
            let (id, n) = l.split_once(' ')?;
            (n.trim() == name).then(|| parse_hex(id)).flatten()
        })
    }

    fn loose_path(&self, id: &Oid) -> PathBuf {
        let h = hex(id);
        self.common.join("objects").join(&h[..2]).join(&h[2..])
    }

    /// Whether the object store holds `id` (loose, or in a pack index).
    pub fn has(&mut self, id: &Oid) -> bool {
        self.loose_path(id).is_file() || self.packs().iter_mut().any(|p| p.find(id).is_some())
    }

    fn packs(&mut self) -> &mut Vec<Pack> {
        if self.packs.is_none() {
            let mut idx: Vec<PathBuf> = std::fs::read_dir(self.common.join("objects/pack"))
                .map(|d| d.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "idx")).collect())
                .unwrap_or_default();
            idx.sort();
            self.packs = Some(idx.iter().filter_map(|p| Pack::open(p)).collect());
        }
        self.packs.as_mut().unwrap()
    }

    /// An object's type and content.
    pub fn object(&mut self, id: &Oid, max: usize) -> Option<(u8, Vec<u8>)> {
        if let Ok(raw) = std::fs::read(self.loose_path(id)) {
            let data = decompress_to_vec_zlib_with_limit(&raw, max + 64).ok()?;
            let nul = data.iter().position(|&b| b == 0)?;
            let header = std::str::from_utf8(&data[..nul]).ok()?;
            let (kind, len) = header.split_once(' ')?;
            let kind = match kind {
                "commit" => 1,
                "tree" => 2,
                "blob" => 3,
                "tag" => 4,
                _ => return None,
            };
            let body = data[nul + 1..].to_vec();
            return (len.parse::<usize>().ok()? == body.len()).then_some((kind, body));
        }
        let packs = self.packs();
        for i in 0..packs.len() {
            if let Some(off) = packs[i].find(id) {
                return read_packed(packs, i, off, max);
            }
        }
        None
    }

    /// The root of `HEAD`'s first-parent history.
    pub fn root_commit(&mut self) -> Option<Oid> {
        let mut cur = self.head()?;
        for _ in 0..MAX_WALK {
            let (kind, body) = self.object(&cur, MAX_COMMIT)?;
            if kind != OBJ_COMMIT {
                return None;
            }
            match first_parent(&body) {
                Some(p) => cur = p,
                None => return Some(cur),
            }
        }
        None
    }
}

impl Repo {
    /// The bytes of the file at `rel` (a repository-relative path, `/`-separated) in `HEAD`'s
    /// tree: `None` when it is not committed there, or is larger than `max`. Read to tell a file
    /// committed in the repository from one that only sits in the work tree (`adage add --trust`
    /// accepts a press only for a pack committed in the repository, architecture §3.12).
    pub fn committed(&mut self, rel: &str, max: usize) -> Option<Vec<u8>> {
        let head = self.head()?;
        let (kind, body) = self.object(&head, MAX_COMMIT)?;
        if kind != OBJ_COMMIT {
            return None;
        }
        let first = body.split(|&b| b == b'\n').next()?;
        let mut id = parse_hex(std::str::from_utf8(first.strip_prefix(b"tree ")?).ok()?)?;
        let parts: Vec<&str> = rel.split('/').filter(|p| !p.is_empty() && *p != ".").collect();
        for (i, part) in parts.iter().enumerate() {
            let (kind, tree) = self.object(&id, MAX_COMMIT)?;
            if kind != OBJ_TREE {
                return None;
            }
            let (mode, entry) = tree_entry(&tree, part)?;
            let last = i + 1 == parts.len();
            // A file is `100644` or `100755`; a directory `40000`; nothing else is followed.
            if last != mode.starts_with("100") || (!last && mode != "40000") {
                return None;
            }
            id = entry;
        }
        let (kind, blob) = self.object(&id, max)?;
        (kind == OBJ_BLOB && blob.len() <= max).then_some(blob)
    }
}

impl Repo {
    /// The id of the tree or blob at `rel` in `commit`'s tree (`None` when absent).
    fn entry_at(&mut self, commit: &Oid, rel: &str) -> Option<Oid> {
        let (kind, body) = self.object(commit, MAX_COMMIT)?;
        if kind != OBJ_COMMIT {
            return None;
        }
        let first = body.split(|&b| b == b'\n').next()?;
        let mut id = parse_hex(std::str::from_utf8(first.strip_prefix(b"tree ")?).ok()?)?;
        for part in rel.split('/').filter(|p| !p.is_empty() && *p != ".") {
            let (kind, tree) = self.object(&id, MAX_COMMIT)?;
            if kind != OBJ_TREE {
                return None;
            }
            id = tree_entry(&tree, part)?.1;
        }
        Some(id)
    }

    /// When the content at `rel` (a file or a directory) in `HEAD`'s tree was committed: the
    /// committer time, in seconds since the epoch, of the oldest commit on `HEAD`'s first-parent
    /// history that already held exactly this content, the walk stopping at [`MAX_WALK`]. `None`
    /// when `HEAD` does not hold `rel`. The time is what the commit says, which whoever made the
    /// commit chose.
    pub fn committed_at(&mut self, rel: &str) -> Option<u64> {
        let mut cur = self.head()?;
        let want = self.entry_at(&cur, rel)?;
        let mut at = None;
        for _ in 0..MAX_WALK {
            let (kind, body) = self.object(&cur, MAX_COMMIT)?;
            if kind != OBJ_COMMIT {
                return None;
            }
            at = committer_time(&body).or(at);
            match first_parent(&body) {
                Some(p) if self.entry_at(&p, rel) == Some(want) => cur = p,
                _ => break,
            }
        }
        at
    }
}

/// The committer time of a commit (`committer <who> <secs> <zone>`, among the headers).
fn committer_time(body: &[u8]) -> Option<u64> {
    for line in body.split(|&b| b == b'\n') {
        if line.is_empty() {
            break;
        }
        if let Some(c) = line.strip_prefix(b"committer ") {
            let c = std::str::from_utf8(c).ok()?;
            return c.rsplit(' ').nth(1)?.parse().ok();
        }
    }
    None
}

const OBJ_TREE: u8 = 2;
const OBJ_BLOB: u8 = 3;

/// The mode and id of `name` in a tree object (`<mode> <name>\0<20-byte id>`, repeated).
fn tree_entry(tree: &[u8], name: &str) -> Option<(String, Oid)> {
    let mut rest = tree;
    while !rest.is_empty() {
        let sp = rest.iter().position(|&b| b == b' ')?;
        let nul = rest.iter().position(|&b| b == 0)?;
        if nul < sp || rest.len() < nul + 21 {
            return None;
        }
        let mode = std::str::from_utf8(&rest[..sp]).ok()?;
        let entry = &rest[sp + 1..nul];
        let mut id = [0u8; 20];
        id.copy_from_slice(&rest[nul + 1..nul + 21]);
        if entry == name.as_bytes() {
            return Some((mode.to_string(), id));
        }
        rest = &rest[nul + 21..];
    }
    None
}

/// A commit's first `parent` header, if any (headers end at the first blank line).
fn first_parent(body: &[u8]) -> Option<Oid> {
    for line in body.split(|&b| b == b'\n') {
        if line.is_empty() {
            break;
        }
        if let Some(p) = line.strip_prefix(b"parent ") {
            return parse_hex(std::str::from_utf8(p).ok()?);
        }
    }
    None
}

fn be32(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

impl Pack {
    fn open(idx_path: &Path) -> Option<Pack> {
        let mut idx = File::open(idx_path).ok()?;
        let mut head = vec![0u8; 8 + 256 * 4];
        idx.read_exact(&mut head).ok()?;
        // Version 2 only: magic "\xfftOc", version 2.
        if head[..4] != [0xff, b't', b'O', b'c'] || be32(&head[4..8]) != 2 {
            return None;
        }
        let fanout: Vec<u32> = head[8..].chunks(4).map(be32).collect();
        // A sorted fanout, and a file long enough for its names, CRCs and offsets.
        let n = fanout[255] as u64;
        if fanout.windows(2).any(|w| w[0] > w[1]) || idx.metadata().ok()?.len() < 8 + 256 * 4 + n * 28 {
            return None;
        }
        let pack = File::open(idx_path.with_extension("pack")).ok()?;
        let pack_len = pack.metadata().ok()?.len();
        Some(Pack { idx, pack, pack_len, fanout })
    }

    fn n(&self) -> u64 {
        self.fanout[255] as u64
    }

    /// The object's offset in the pack, by binary search of the index's sorted names.
    fn find(&mut self, id: &Oid) -> Option<u64> {
        let lo = if id[0] == 0 { 0 } else { self.fanout[id[0] as usize - 1] } as u64;
        let hi = self.fanout[id[0] as usize] as u64;
        if hi <= lo {
            return None;
        }
        let names_at = 8 + 256 * 4;
        let mut names = vec![0u8; ((hi - lo) * 20) as usize];
        self.idx.seek(SeekFrom::Start(names_at + lo * 20)).ok()?;
        self.idx.read_exact(&mut names).ok()?;
        let k = names.chunks(20).collect::<Vec<_>>().binary_search(&&id[..]).ok()? as u64;
        let n = self.n();
        let offsets_at = names_at + n * 20 + n * 4;
        let mut b = [0u8; 4];
        self.idx.seek(SeekFrom::Start(offsets_at + (lo + k) * 4)).ok()?;
        self.idx.read_exact(&mut b).ok()?;
        let off = be32(&b);
        if off & 0x8000_0000 == 0 {
            return Some(off as u64);
        }
        let mut b8 = [0u8; 8];
        self.idx.seek(SeekFrom::Start(offsets_at + n * 4 + (off & 0x7fff_ffff) as u64 * 8)).ok()?;
        self.idx.read_exact(&mut b8).ok()?;
        Some(u64::from_be_bytes(b8))
    }

    fn read_at(&mut self, off: u64, len: usize) -> Option<Vec<u8>> {
        let len = len.min(self.pack_len.checked_sub(off)? as usize);
        let mut buf = vec![0u8; len];
        self.pack.seek(SeekFrom::Start(off)).ok()?;
        self.pack.read_exact(&mut buf).ok()?;
        Some(buf)
    }

    /// The entry at `off`: (type, inflated size, where its data starts, its delta base).
    fn entry(&mut self, off: u64) -> Option<(u8, usize, u64, Option<Base>)> {
        let h = self.read_at(off, 32)?;
        let mut i = 0;
        let mut c = *h.get(i)?;
        let kind = (c >> 4) & 7;
        let mut size = (c & 15) as u64;
        let mut shift = 4;
        while c & 0x80 != 0 {
            i += 1;
            c = *h.get(i)?;
            if shift > 56 {
                return None;
            }
            size |= ((c & 0x7f) as u64) << shift;
            shift += 7;
        }
        i += 1;
        let base = match kind {
            OBJ_OFS_DELTA => {
                let mut c = *h.get(i)?;
                let mut rel = (c & 0x7f) as u64;
                while c & 0x80 != 0 {
                    i += 1;
                    c = *h.get(i)?;
                    rel = rel.checked_add(1)?.checked_mul(128)? | (c & 0x7f) as u64;
                }
                i += 1;
                Some(Base::Offset(off.checked_sub(rel)?))
            }
            OBJ_REF_DELTA => {
                let id: Oid = h.get(i..i + 20)?.try_into().ok()?;
                i += 20;
                Some(Base::Id(id))
            }
            1..=4 => None,
            _ => return None,
        };
        Some((kind, usize::try_from(size).ok()?, off + i as u64, base))
    }

    /// Inflate `size` bytes of zlib data starting at `at`, reading more of the pack until the
    /// stream is complete.
    fn inflate(&mut self, at: u64, size: usize) -> Option<Vec<u8>> {
        let mut want = size + 256;
        loop {
            let raw = self.read_at(at, want)?;
            match decompress_to_vec_zlib_with_limit(&raw, size) {
                Ok(v) if v.len() == size => return Some(v),
                Ok(_) => return None,
                Err(_) if raw.len() == want => want = want.checked_mul(2)?,
                Err(_) => return None,
            }
        }
    }
}

enum Base {
    Offset(u64),
    Id(Oid),
}

/// A packed object, following its delta chain (within the pack, or through the others).
fn read_packed(packs: &mut [Pack], mut p: usize, mut off: u64, max: usize) -> Option<(u8, Vec<u8>)> {
    let mut deltas: Vec<Vec<u8>> = Vec::new();
    loop {
        if deltas.len() > MAX_DELTA_DEPTH {
            return None;
        }
        let (kind, size, at, base) = packs[p].entry(off)?;
        if size > max.max(MAX_COMMIT) * 4 {
            return None;
        }
        let data = packs[p].inflate(at, size)?;
        match base {
            None => {
                let mut out = data;
                for d in deltas.iter().rev() {
                    out = apply_delta(&out, d, max)?;
                }
                return (out.len() <= max).then_some((kind, out));
            }
            Some(Base::Offset(o)) => {
                deltas.push(data);
                off = o;
            }
            Some(Base::Id(id)) => {
                deltas.push(data);
                let (q, o) = (0..packs.len()).find_map(|q| packs[q].find(&id).map(|o| (q, o)))?;
                p = q;
                off = o;
            }
        }
    }
}

fn varint(d: &[u8], i: &mut usize) -> Option<usize> {
    let mut v = 0usize;
    let mut shift = 0;
    loop {
        let c = *d.get(*i)?;
        *i += 1;
        if shift > 56 {
            return None;
        }
        v |= ((c & 0x7f) as usize) << shift;
        shift += 7;
        if c & 0x80 == 0 {
            return Some(v);
        }
    }
}

/// Apply a git delta (copy/insert instructions) to `base`.
fn apply_delta(base: &[u8], d: &[u8], max: usize) -> Option<Vec<u8>> {
    let mut i = 0;
    if varint(d, &mut i)? != base.len() {
        return None;
    }
    let want = varint(d, &mut i)?;
    if want > max.max(MAX_COMMIT) * 4 {
        return None;
    }
    let mut out = Vec::with_capacity(want);
    while i < d.len() {
        let op = d[i];
        i += 1;
        if op & 0x80 != 0 {
            let mut off = 0usize;
            let mut len = 0usize;
            for k in 0..4 {
                if op & (1 << k) != 0 {
                    off |= (*d.get(i)? as usize) << (8 * k);
                    i += 1;
                }
            }
            for k in 0..3 {
                if op & (0x10 << k) != 0 {
                    len |= (*d.get(i)? as usize) << (8 * k);
                    i += 1;
                }
            }
            if len == 0 {
                len = 0x10000;
            }
            out.extend_from_slice(base.get(off..off.checked_add(len)?)?);
        } else if op != 0 {
            out.extend_from_slice(d.get(i..i + op as usize)?);
            i += op as usize;
        } else {
            return None;
        }
    }
    (out.len() == want).then_some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deltas_copy_and_insert() {
        let base = b"hello, world";
        // src 12, dst 11: copy 0..5, insert "!!", copy 7..11
        let d = [12, 11, 0x80 | 0x10, 5, 2, b'!', b'!', 0x80 | 0x01 | 0x10, 7, 4];
        assert_eq!(apply_delta(base, &d, 64).unwrap(), b"hello!!worl");
        assert!(apply_delta(base, &[11, 1, 1, b'x'], 64).is_none(), "wrong base size");
        assert!(apply_delta(base, &[12, 1, 0], 64).is_none(), "reserved opcode");
        assert!(apply_delta(base, &[12, 4, 0x80 | 0x01 | 0x10, 10, 4], 64).is_none(), "copy past the base");
    }

    #[test]
    fn tree_entries_are_found_by_exact_name() {
        let mut tree = Vec::new();
        for (mode, name, b) in [("40000", "pack", 1u8), ("100644", "pack.toml", 2), ("100755", "run", 3)] {
            tree.extend_from_slice(format!("{mode} {name}\0").as_bytes());
            tree.extend_from_slice(&[b; 20]);
        }
        assert_eq!(tree_entry(&tree, "pack"), Some(("40000".into(), [1; 20])));
        assert_eq!(tree_entry(&tree, "pack.toml"), Some(("100644".into(), [2; 20])));
        assert_eq!(tree_entry(&tree, "run"), Some(("100755".into(), [3; 20])));
        assert_eq!(tree_entry(&tree, "pac"), None);
        assert_eq!(tree_entry(&tree[..30], "run"), None, "a truncated tree");
    }

    #[test]
    fn the_committer_time_is_read_from_the_headers() {
        let body = "tree 1111111111111111111111111111111111111111\nauthor A <a@b> 100 +0000\ncommitter C D <c@d> 1700000000 +0200\n\ncommitter x <y> 5 +0000\n";
        assert_eq!(committer_time(body.as_bytes()), Some(1_700_000_000));
        assert_eq!(committer_time(b"tree 1\n\ncommitter C <c> 5 +0000\n"), None);
    }

    #[test]
    fn first_parent_stops_at_the_headers() {
        let p = "1111111111111111111111111111111111111111";
        let body = format!("tree {p}\nparent {p}\nparent 2222222222222222222222222222222222222222\n\nparent x\n");
        assert_eq!(first_parent(body.as_bytes()).map(|o| hex(&o)).as_deref(), Some(p));
        assert!(first_parent(format!("tree {p}\n\nparent {p}\n").as_bytes()).is_none());
    }
}
