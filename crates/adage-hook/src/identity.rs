//! The two identity lists a session keeps for Bash calls, and the key each one holds a call by.
//! They are keyed differently, because their readers hold different things, and the keys must
//! stay as they are: the state files of running sessions hold these hashes.
//!
//! - **Decided** (`SessionState::seen_pretool`, through `see_pretool` / `has_seen_pretool`): the
//!   `PreToolUse` calls already decided this session, so the mod's two phases and the command hook
//!   that follows them decide a call once. Keyed by [`decided_key`]: the hash of the adapter's
//!   event identity, for Claude Code the JSON triple `["claude","PreToolUse",["<tool_use_id>"]]`
//!   (`harness::tool_event_identity`). Kept oldest first, at most `SEEN_PRETOOL`; past that the
//!   oldest are dropped.
//! - **Answered** (`SessionState::answered`, through `see_answered`): the calls the mod answered
//!   with a receipt's result, which were observed then, so the `PostToolBatch` that lists the same
//!   call does not observe it twice. Keyed by [`answered_key`]: the hash of the bare `tool_use_id`,
//!   because that batch's reader (`adage_runtime::decide`) holds only the call's `tool_use_id`.
//!   Also at most `SEEN_PRETOOL`, the oldest dropped first.
//!
//! The same call is therefore held under two different hashes, one in each list
//! (`the_two_identity_lists_hold_a_call_by_different_keys` pins it).

use crate::harness;
use crate::phase;
use serde_json::Value;

/// The key the decided list holds a `PreToolUse` call by: the hash of its harness event identity.
/// `None` when the payload carries no usable `tool_use_id`: such a call is decided every time, as
/// before the mod.
pub(crate) fn decided_key(adapter: &dyn harness::Adapter, p: &Value) -> Option<u64> {
    adapter.event_identity("PreToolUse", p).map(|i| phase::identity_hash(&i))
}

/// The key the answered list holds a call by: the hash of its bare, non-empty `tool_use_id`, as
/// the batch that lists the call names it.
pub(crate) fn answered_key(p: &Value) -> Option<u64> {
    p["tool_use_id"].as_str().filter(|id| !id.is_empty()).map(adage_runtime::identity_hash)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::*;
    use adage_runtime::Arm;
    use serde_json::json;

    /// T-A1-03 and T-A1-04: 64 calls with ids of the largest size an identity takes each decide;
    /// the first, sent again, does not; a 65th decides and pushes the first out, which then
    /// decides again; the state stays well inside its slot. A call with no id, or an id too long
    /// to be an identity, is decided every time, as before.
    #[test]
    fn the_identity_ring_is_bounded_and_exact() {
        let w = u11_world(Arm::Treated);
        let same = json!({ "verdict": { "engine": "same" } });
        let id = |i: usize| format!("{i:0>256}");
        for i in 0..64 {
            assert!(bash_call(&w, &NO_EXTENSION, &id(i), PYTEST, same.clone()).is_some(), "call {i}");
        }
        assert!(bash_call(&w, &NO_EXTENSION, &id(0), PYTEST, same.clone()).is_none(), "decided already");
        assert!(bash_call(&w, &NO_EXTENSION, &id(64), PYTEST, same.clone()).is_some(), "a 65th");
        assert!(bash_call(&w, &NO_EXTENSION, &id(0), PYTEST, same.clone()).is_some(), "the oldest was dropped");
        assert_eq!(pretool_records(&w).len(), 66);
        let bytes = serde_json::to_vec(&w.state()).unwrap().len();
        assert!(bytes < adage_store::state::SLOT - 64, "{bytes} bytes");
        let records = pretool_records(&w).len();
        for _ in 0..2 {
            assert!(bash_call(&w, &NO_EXTENSION, "", PYTEST, same.clone()).is_some(), "no id: decided as before");
            assert!(bash_call(&w, &NO_EXTENSION, &"x".repeat(257), PYTEST, same.clone()).is_some(), "not an identity");
        }
        assert_eq!(pretool_records(&w).len(), records + 4);
    }

    /// The two lists hold the same call by different keys (see the module documentation): a call
    /// the mod answered with a receipt is in the decided list by the hash of its event identity,
    /// the triple `["claude","PreToolUse",[id]]`, and in the answered list by the hash of its bare
    /// id. This pins today's keys; changing either one strands what running sessions recorded.
    #[test]
    fn the_two_identity_lists_hold_a_call_by_different_keys() {
        let w = receipt_world(Arm::Treated);
        let answered = json!({ "verdict": { "answered": "receipt" } });
        assert!(bash_call(&w, &FRESH_RECEIPTS, "toolu_1", PYTEST, answered).is_some(), "answered by the receipt");
        let st = w.state();
        let triple = adage_runtime::identity_hash(r#"["claude","PreToolUse",["toolu_1"]]"#);
        let bare = adage_runtime::identity_hash("toolu_1");
        assert_ne!(triple, bare);
        assert_eq!(st.seen_pretool, vec![triple], "decided: by the event identity");
        assert_eq!(st.answered, vec![bare], "answered: by the bare tool_use_id");
        let p = json!({ "tool_use_id": "toolu_1" });
        assert_eq!(decided_key(&harness::claude::Claude, &p), Some(triple));
        assert_eq!(answered_key(&p), Some(bare));
        assert_eq!(answered_key(&json!({ "tool_use_id": "" })), None);
        assert_eq!(decided_key(&harness::claude::Claude, &json!({})), None);
    }
}
