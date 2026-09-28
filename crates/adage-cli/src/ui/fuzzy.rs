//! A small fuzzy matcher for `/` filters and the command palette: the query's characters in
//! order, case-insensitively, scored so that contiguous runs, word starts and early matches rank
//! first. Enough for a few hundred reflexes and actions; no dependency.

/// A match: its score (higher is better) and the matched character positions in `text`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Match {
    pub score: i64,
    pub positions: Vec<usize>,
}

fn boundary(prev: Option<char>) -> bool {
    prev.is_none_or(|p| !p.is_alphanumeric())
}

/// Match `query` against `text`. An empty query matches everything with score 0.
pub fn score(query: &str, text: &str) -> Option<Match> {
    let q: Vec<char> = query.chars().filter(|c| !c.is_whitespace()).flat_map(char::to_lowercase).collect();
    if q.is_empty() {
        return Some(Match { score: 0, positions: vec![] });
    }
    let t: Vec<char> = text.chars().collect();
    let lower: Vec<char> = t.iter().map(|c| c.to_lowercase().next().unwrap_or(*c)).collect();
    // A contiguous occurrence beats any scattered one; the earliest at a word start wins.
    let mut best: Option<Match> = None;
    for start in 0..lower.len() {
        if lower.len() - start < q.len() {
            break;
        }
        if lower[start..start + q.len()] == q[..] {
            let at_word = boundary(start.checked_sub(1).map(|i| t[i]));
            let s = 1000 + q.len() as i64 * 20 + if at_word { 200 } else { 0 } - start as i64;
            if best.as_ref().is_none_or(|b| s > b.score) {
                best = Some(Match { score: s, positions: (start..start + q.len()).collect() });
            }
        }
    }
    if best.is_some() {
        return best;
    }
    // Scattered, greedily: each character at the next word start if there is one before the
    // following plain occurrence, else the next occurrence.
    let mut positions = Vec::with_capacity(q.len());
    let mut i = 0;
    for &c in &q {
        let next = (i..lower.len()).find(|&j| lower[j] == c)?;
        let word = (next..lower.len()).find(|&j| lower[j] == c && boundary(j.checked_sub(1).map(|k| t[k])));
        let pick = match word {
            Some(w) if w - next <= 8 => w,
            _ => next,
        };
        positions.push(pick);
        i = pick + 1;
    }
    let mut s = 0i64;
    for (k, &p) in positions.iter().enumerate() {
        s += 10;
        if boundary(p.checked_sub(1).map(|i| t[i])) {
            s += 15;
        }
        if k > 0 && positions[k - 1] + 1 == p {
            s += 12;
        }
    }
    s -= (positions[0] as i64).min(20);
    s -= (positions[positions.len() - 1] - positions[0]) as i64 / 2;
    Some(Match { score: s, positions })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contiguous_and_word_starts_rank_first() {
        assert!(score("xyz", "lookup-work-item").is_none());
        let a = score("str", "Strengthen reflex…").unwrap();
        assert_eq!(a.positions, vec![0, 1, 2]);
        let b = score("str", "Show control-arm traces").unwrap();
        assert!(a.score > b.score, "{a:?} {b:?}");
        let c = score("lwi", "lookup-work-item").unwrap();
        assert_eq!(c.positions, vec![0, 7, 12]);
        assert_eq!(score("", "anything").unwrap().score, 0);
        assert!(score("WORK", "lookup-work-item").is_some(), "case-insensitive");
    }
}
