//! Native delegation is one leakage family, not independent task demonstrations merely because
//! its workers have separate session ids. This pass retains each worker's own calls and identity.

use adage_trace::{EvidenceIdentity, Session};
use std::collections::{BTreeMap, BTreeSet};

const PARENT: &str = "codex-parent:";
const FAMILY: &str = "codex-lineage:";

fn family_prefix(s: &Session) -> Option<&'static str> {
    match s.harness.as_str() {
        adage_trace::session::CODEX => Some(FAMILY),
        adage_trace::session::CLAUDE_CODE => Some("claude-lineage:"),
        _ => None,
    }
}

fn delegated_parent(s: &Session) -> Option<&str> {
    let d = s.delegation.as_ref()?;
    let own = match s.harness.as_str() {
        adage_trace::session::CODEX => d.agent_id == s.id,
        adage_trace::session::CLAUDE_CODE => s.id == format!("{}/{}", d.parent_id, d.agent_id),
        _ => false,
    };
    (own && !d.parent_id.is_empty() && !d.agent_id.is_empty()).then_some(d.parent_id.as_str())
}

/// Only our native identity shape may be recomputed. Explicit task/group/attempt identities win.
/// A resolved root has no delegation; its own attempt and self-anchored family identify it.
fn generated_family(s: &Session) -> Option<&str> {
    if s.identity.task_id.is_some() || s.identity.attempt_id.as_deref() != Some(s.id.as_str()) {
        return None;
    }
    let root = s.identity.group_id.as_deref()?.strip_prefix(family_prefix(s)?)?;
    (!root.is_empty() && (root == s.id || delegated_parent(s).is_some())).then_some(root)
}

fn native_parent(s: &Session) -> Option<&str> {
    if s.harness == adage_trace::session::CLAUDE_CODE && s.identity.is_empty() {
        return delegated_parent(s);
    }
    let parent = delegated_parent(s)?;
    if generated_family(s).is_some() {
        return Some(parent);
    }
    (s.harness == adage_trace::session::CODEX
        && s.identity.task_id.is_none()
        && s.identity.attempt_id.as_deref() == Some(s.id.as_str())
        && s.identity.group_id.as_deref()?.strip_prefix(PARENT) == Some(parent))
    .then_some(parent)
}

/// Resolve native child→parent links inside each repository, including siblings and nested
/// delegations. Missing ancestors are still family anchors. Explicit caller identities are never
/// replaced. Call after parsing all files, before applying optional caller overrides.
///
/// A source parent's whole session is conservatively one leakage family; this is not a claim
/// that every task in a long session was the same task, or that children had independent success.
pub fn group_native_lineages(sessions: &mut [Session]) {
    // Harness is part of the namespace: equal native ids from different harnesses do not join.
    type Node = (String, String, String);
    let mut edges: BTreeMap<Node, BTreeSet<Node>> = BTreeMap::new();
    let mut children = BTreeSet::new();
    let mut known_roots = BTreeSet::new();
    for s in sessions.iter() {
        let own = (s.repo.clone(), s.harness.clone(), s.id.clone());
        if let Some(parent) = native_parent(s).filter(|p| !p.is_empty()) {
            let parent = (s.repo.clone(), s.harness.clone(), parent.to_string());
            children.insert(own.clone());
            edges.entry(own.clone()).or_default().insert(parent.clone());
            edges.entry(parent).or_default().insert(own.clone());
        }
        // A prior export may already know an ancestor missing from this subset. Preserve that
        // link, while allowing newly imported parent records to join formerly separate exports.
        if let Some(root) = generated_family(s) {
            let root = (s.repo.clone(), s.harness.clone(), root.to_string());
            known_roots.insert(root.clone());
            edges.entry(own.clone()).or_default().insert(root.clone());
            edges.entry(root).or_default().insert(own);
        }
    }
    let mut families = BTreeMap::new();
    for node in edges.keys() {
        if families.contains_key(node) {
            continue;
        }
        let mut pending = vec![node.clone()];
        let mut component = BTreeSet::new();
        while let Some(at) = pending.pop() {
            if component.insert(at.clone()) {
                pending.extend(edges[&at].iter().cloned());
            }
        }
        // Normally the sole ancestor with no parent link; malformed cycles or conflicting
        // native histories remain one conservative group with a deterministic anchor.
        let root = component
            .iter()
            .find(|n| known_roots.contains(*n) && !children.contains(*n))
            .or_else(|| component.iter().find(|n| !children.contains(*n)))
            .unwrap_or_else(|| component.first().unwrap());
        let family = if root.1 == adage_trace::session::CODEX {
            format!("{FAMILY}{}", root.2)
        } else {
            format!("claude-lineage:{}", root.2)
        };
        for member in component {
            families.insert(member, family.clone());
        }
    }
    for s in sessions {
        if !matches!(s.harness.as_str(), adage_trace::session::CODEX | adage_trace::session::CLAUDE_CODE)
            || (!s.identity.is_empty() && native_parent(s).is_none() && generated_family(s).is_none())
        {
            continue;
        }
        if let Some(group) = families.get(&(s.repo.clone(), s.harness.clone(), s.id.clone())) {
            s.identity =
                EvidenceIdentity { task_id: None, group_id: Some(group.clone()), attempt_id: Some(s.id.clone()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn session(repo: &str, id: &str, parent: Option<&str>) -> Session {
        let mut s: Session = serde_json::from_value(json!({
            "id":id,"root":"/repo","repo":repo,"model":null,"day":"2026-10-15","prompt":"task",
            "steps":[],"steered":false,"directed":[],"outcome":null,"harness":"codex"
        }))
        .unwrap();
        if let Some(p) = parent {
            s.delegation = Some(adage_trace::session::Delegation {
                parent_id: p.into(),
                agent_id: id.into(),
                ..Default::default()
            });
            s.identity = EvidenceIdentity {
                group_id: Some(format!("{PARENT}{p}")),
                attempt_id: Some(id.into()),
                ..Default::default()
            };
        }
        s
    }

    #[test]
    fn parent_siblings_grandchildren_and_order_share_one_family() {
        let rows = vec![
            session("r", "root", None),
            session("r", "a", Some("root")),
            session("r", "b", Some("root")),
            session("r", "c", Some("a")),
        ];
        let mut forward = rows.clone();
        let mut reverse: Vec<_> = rows.into_iter().rev().collect();
        group_native_lineages(&mut forward);
        group_native_lineages(&mut reverse);
        reverse.reverse();
        assert_eq!(forward, reverse);
        assert!(forward.iter().all(|s| s.identity.group_id.as_deref() == Some("codex-lineage:root")));
        assert_eq!(forward.iter().filter_map(|s| s.evidence_group().map(|g| g.key)).collect::<BTreeSet<_>>().len(), 1);
        let before = forward.clone();
        group_native_lineages(&mut forward);
        assert_eq!(forward, before, "a resolved batch is stable");
    }

    #[test]
    fn missing_parent_repo_boundaries_and_explicit_identity_are_preserved() {
        let mut rows = vec![
            session("r", "a", Some("missing")),
            session("r", "b", Some("missing")),
            session("other", "missing", None),
            session("r", "explicit", None),
        ];
        let explicit = EvidenceIdentity {
            task_id: Some("known-task".into()),
            group_id: Some("known-group".into()),
            attempt_id: Some("known-attempt".into()),
        };
        rows[3].identity = explicit.clone();
        rows.push(session("r", "child", Some("explicit")));
        group_native_lineages(&mut rows);
        assert_eq!(rows[0].identity.group_id, rows[1].identity.group_id);
        assert_eq!(rows[0].identity.group_id.as_deref(), Some("codex-lineage:missing"));
        assert!(rows[2].identity.is_empty(), "equal ids in another repository do not join");
        assert_eq!(rows[3].identity, explicit, "caller evidence identity wins");
    }

    #[test]
    fn cycles_do_not_manufacture_independent_groups() {
        let mut rows = vec![session("r", "a", Some("b")), session("r", "b", Some("a"))];
        group_native_lineages(&mut rows);
        assert_eq!(rows[0].identity.group_id, rows[1].identity.group_id);
    }

    #[test]
    fn separately_resolved_nested_exports_rejoin_and_subsets_keep_known_ancestors() {
        let mut first = vec![session("r", "root", None), session("r", "a", Some("root"))];
        let mut second = vec![session("r", "b", Some("a"))];
        group_native_lineages(&mut first);
        group_native_lineages(&mut second);
        assert_ne!(first[1].identity.group_id, second[0].identity.group_id);
        first.extend(second);
        group_native_lineages(&mut first);
        assert!(first.iter().all(|s| s.identity.group_id.as_deref() == Some("codex-lineage:root")));
        let mut subset = vec![first[2].clone()];
        group_native_lineages(&mut subset);
        assert_eq!(subset[0], first[2], "a missing ancestor does not erase the previous native link");
    }

    #[test]
    fn resolved_looking_names_without_native_identity_are_not_overridden() {
        let mut rows = vec![session("r", "root", None), session("r", "a", Some("root"))];
        rows[1].identity.group_id = Some("codex-lineage:caller-group".into());
        rows[1].identity.attempt_id = Some("caller-attempt".into());
        let declared = rows[1].identity.clone();
        group_native_lineages(&mut rows);
        assert_eq!(rows[1].identity, declared);
        assert!(rows[0].identity.is_empty());
    }

    #[test]
    fn claude_workers_share_parent_evidence_but_not_other_harness_ids() {
        let mut parent = session("r", "root", None);
        parent.harness = adage_trace::session::CLAUDE_CODE.into();
        let child = |id: &str| {
            let mut s = parent.clone();
            s.id = format!("root/{id}");
            s.delegation = Some(adage_trace::session::Delegation {
                parent_id: "root".into(),
                agent_id: id.into(),
                ..Default::default()
            });
            s
        };
        let mut rows = vec![parent.clone(), child("a"), child("b"), session("r", "root", None)];
        group_native_lineages(&mut rows);
        assert_eq!(rows[0].identity.group_id.as_deref(), Some("claude-lineage:root"));
        assert_eq!(rows[0].identity.group_id, rows[1].identity.group_id);
        assert_eq!(rows[1].identity.group_id, rows[2].identity.group_id);
        assert!(rows[3].identity.is_empty());
        let once = rows.clone();
        group_native_lineages(&mut rows);
        assert_eq!(rows, once);
    }
}
