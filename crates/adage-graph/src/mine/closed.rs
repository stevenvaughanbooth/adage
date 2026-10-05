//! Closure (Track G §3.2 step 6, BIDE-style): a frequent pattern is **closed** when no
//! super-pattern (one more item anywhere, one more constraint, an item refined to a finer label
//! or a specific polarity) has the same supporting sessions. Only closed patterns are scored.
//!
//! One deviation for reflexes: only a super-pattern with the same number of context items
//! absorbs a pattern. A longer context with the same supporting sessions is not more evidence,
//! only a narrower trigger that costs more to state; the two compete in MDL selection instead.
//!
//! The check runs over the frequent set: patterns are grouped by their session set (equal sets
//! only can absorb each other), and inside a group a pattern is dropped when another pattern of
//! the group contains it ([`contains`]). Super-patterns of a frequent pattern are frequent, and
//! target-rooted growth reaches every super-pattern with the same target within the depth
//! bounds, so the check sees them.

use super::{Db, Item, MinedPattern, Pattern, Pol};
use crate::events::LabelTable;
use std::collections::BTreeMap;

fn item_covers(t: &LabelTable, p: &Item, q: &Item) -> bool {
    t.is_ancestor_or_self(p.label, q.label)
        && (p.pol == Pol::Any || p.pol == q.pol)
        && (!p.last || (q.last && q.label == p.label))
}

/// Whether pattern `q` contains `p`: an order-preserving map of `p`'s items onto `q`'s with the
/// targets aligned, each `p` item covering its image (an ancestor-or-self label, polarity, `last`),
/// and every relation and edge constraint of `p` held by `q` (edge constraints need adjacent
/// images; a later-batch relation needs a later batch somewhere between the images).
pub fn contains(t: &LabelTable, q: &Pattern, p: &Pattern) -> bool {
    if p.items.len() > q.items.len() || p.cut > q.cut || p.items.len() - p.cut > q.items.len() - q.cut {
        return false;
    }
    fn go(t: &LabelTable, q: &Pattern, p: &Pattern, i: usize, from: usize, prev: Option<usize>) -> bool {
        if i == p.items.len() {
            return true;
        }
        let pi = &p.items[i];
        // Target alignment: p's cut item maps to q's cut item; context stays context.
        let range: Vec<usize> = if i == p.cut {
            if q.cut >= from {
                vec![q.cut]
            } else {
                vec![]
            }
        } else if i < p.cut {
            (from..q.cut).collect()
        } else {
            (from.max(q.cut + 1)..q.items.len()).collect()
        };
        for j in range {
            let qj = &q.items[j];
            if !item_covers(t, pi, qj) {
                continue;
            }
            if let Some(pj) = prev {
                if i > 0 {
                    let adjacent = j == pj + 1;
                    if (pi.flow && !(adjacent && qj.flow)) || (pi.same_art && !(adjacent && qj.same_art)) {
                        continue;
                    }
                    if pi.same && !(adjacent && qj.same) {
                        continue;
                    }
                    if !pi.same && !(pj + 1..=j).any(|k| !q.items[k].same) {
                        continue;
                    }
                }
            }
            if go(t, q, p, i + 1, j + 1, Some(j)) {
                return true;
            }
        }
        false
    }
    go(t, q, p, 0, 0, None)
}

/// Mark the closed patterns of a frequent set.
pub fn mark(db: &Db, patterns: &mut [MinedPattern]) {
    let mut groups: BTreeMap<&[u32], Vec<usize>> = BTreeMap::new();
    for (i, m) in patterns.iter().enumerate() {
        groups.entry(m.sessions.as_slice()).or_default().push(i);
    }
    let mut closed = vec![true; patterns.len()];
    for idx in groups.values() {
        if idx.len() < 2 {
            continue;
        }
        for &a in idx {
            for &b in idx {
                if a != b
                    && patterns[a].pattern != patterns[b].pattern
                    && patterns[b].pattern.cut == patterns[a].pattern.cut
                    && patterns[b].pattern.items.len() >= patterns[a].pattern.items.len()
                    && contains(&db.labels, &patterns[b].pattern, &patterns[a].pattern)
                {
                    closed[a] = false;
                    break;
                }
            }
        }
    }
    for (m, c) in patterns.iter_mut().zip(closed) {
        m.closed = c;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::{Level, NodeType, NONE};

    fn table() -> LabelTable {
        // 0 act/x:a → 1 act/h:A ; 2 act/x:b ; 3 act/x:y
        LabelTable {
            names: vec!["act/x:a".into(), "act/h:A".into(), "act/x:b".into(), "act/x:y".into()],
            ty: vec![NodeType::Act; 4],
            level: vec![Level::Exact, Level::Head, Level::Exact, Level::Exact],
            parent: vec![1, NONE, NONE, NONE],
            effect: vec![None; 4],
        }
    }

    fn it(l: u32) -> Item {
        Item::new(l, Pol::Any)
    }

    #[test]
    fn containment_respects_targets_levels_and_edges() {
        let t = table();
        let p = Pattern { items: vec![it(1), it(3)], cut: 1 };
        let q = Pattern { items: vec![it(0), it(2), it(3)], cut: 2 };
        assert!(contains(&t, &q, &p), "A → y is inside a → b → y");
        assert!(!contains(&t, &p, &q));
        let mut flowed = p.clone();
        flowed.items[1].flow = true;
        assert!(!contains(&t, &q, &flowed), "a flow edge needs adjacent images");
        let mut qf = Pattern { items: vec![it(0), it(3)], cut: 1 };
        qf.items[1].flow = true;
        assert!(contains(&t, &qf, &flowed));
        // The target must align: a → y (target y) is not inside y → b (target y at 0, ctx none).
        let r = Pattern { items: vec![it(3), it(2)], cut: 1 };
        assert!(!contains(&t, &r, &Pattern { items: vec![it(0), it(3)], cut: 1 }));
    }
}
