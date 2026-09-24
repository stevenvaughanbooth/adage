//! A line diff for the confirmation before a file is written (hook registrations are small).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Keep,
    Add,
    Remove,
}

/// Files longer than this are shown whole, as added and removed, rather than diffed.
const MAX_LINES: usize = 2000;

/// `before` → `after`, line by line (longest common subsequence).
pub fn lines(before: &str, after: &str) -> Vec<(Op, String)> {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    if a.len() > MAX_LINES || b.len() > MAX_LINES {
        return a
            .iter()
            .map(|l| (Op::Remove, l.to_string()))
            .chain(b.iter().map(|l| (Op::Add, l.to_string())))
            .collect();
    }
    let (n, m) = (a.len(), b.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j, mut out) = (0, 0, Vec::new());
    while i < n || j < m {
        if i < n && j < m && a[i] == b[j] {
            out.push((Op::Keep, a[i].to_string()));
            i += 1;
            j += 1;
        } else if j < m && (i == n || lcs[i][j + 1] >= lcs[i + 1][j]) {
            out.push((Op::Add, b[j].to_string()));
            j += 1;
        } else {
            out.push((Op::Remove, a[i].to_string()));
            i += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diffs_by_line() {
        let d = lines("a\nb\nc\n", "a\nx\nc\nd\n");
        let ops: Vec<(Op, &str)> = d.iter().map(|(o, l)| (*o, l.as_str())).collect();
        assert_eq!(ops, vec![(Op::Keep, "a"), (Op::Add, "x"), (Op::Remove, "b"), (Op::Keep, "c"), (Op::Add, "d")]);
        assert!(lines("", "a\n").iter().all(|(o, _)| *o == Op::Add));
    }
}
