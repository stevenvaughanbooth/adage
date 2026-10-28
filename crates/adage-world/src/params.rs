//! The resolver's, classifier's and scheduler's parameters: fields, never hidden globals, so a
//! replay and the evaluation can vary them (the `Policy` pattern of `adage-runtime`).

use serde::{Deserialize, Serialize};

/// Every tunable, with the design's initial values. The evaluation set (`adage world eval`)
/// revisits the weights, the threshold and `tau_min`; nothing else reads them.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Params {
    /// The weight of the embedding similarity between the prompt and the artifact's text.
    pub w_sim: f32,
    /// The weight of recency, `exp(-minutes since last touch / tau_min)`.
    pub w_recency: f32,
    /// The weight of an entity match: a title, a ticket key, a pull request number or a person
    /// named in the prompt, or the one artifact of the kind the prompt names.
    pub w_entity: f32,
    /// The weight of the kind the prompt's verb implies.
    pub w_kind: f32,
    /// Recency's half-life scale, in minutes.
    pub tau_min: f32,
    /// A candidate at or above this score may be attached.
    pub threshold: f32,
    /// The lead the top candidate needs over the second to be attached alone; closer than this,
    /// the top contenders are listed for the agent to confirm.
    pub margin: f32,
    /// More contenders than this within `margin` of the top: nothing is attached.
    pub max_contenders: usize,
    /// How far back an artifact's last touch may be to be a candidate, in milliseconds (4 h).
    pub window_ms: u64,
    /// How far back a meeting may have ended to be a candidate with its attachments (2 h).
    pub meeting_window_ms: u64,
    /// The most candidates scored for one prompt.
    pub max_candidates: usize,
    /// Token budgets (characters / 4) for each injection class.
    pub hint_tokens: u32,
    pub snapshot_tokens: u32,
    pub delta_tokens: u32,
    pub interrupt_tokens: u32,
    /// Interrupts delivered per session, at most.
    pub max_interrupts: u32,
    /// A calendar block starting within this many milliseconds of an idle `Stop` makes the gap
    /// predictable (10 min).
    pub gap_lookahead_ms: u64,
    /// The cached tokens a session needs before a longer TTL pays for itself.
    pub ttl_min_cached: u64,
    /// A block longer than this leaves idle compaction to do its work (55 min).
    pub long_block_ms: u64,
    /// The finalize nudge goes out this long before a block ends (3 min).
    pub finalize_before_ms: u64,
    /// A free slot at least this long may hold a review block (15 min), within this lookahead (2 h).
    pub review_slot_ms: u64,
    pub review_lookahead_ms: u64,
    /// Briefs pending before a review block is proposed.
    pub review_min_briefs: u32,
}

const MIN: u64 = 60_000;

impl Default for Params {
    fn default() -> Params {
        Params {
            w_sim: 0.35,
            w_recency: 0.25,
            w_entity: 0.30,
            w_kind: 0.10,
            tau_min: 45.0,
            // The design's 0.72 assumed a sentence-transformer's similarity scale; the bundled
            // static model and the hashed fallback score lower on a short anaphoric prompt, so
            // the floor starts here (an entity match with a fresh touch clears it, nothing else
            // does alone) and the evaluation set moves it.
            threshold: 0.5,
            margin: 0.12,
            max_contenders: 3,
            window_ms: 4 * 60 * MIN,
            meeting_window_ms: 2 * 60 * MIN,
            max_candidates: 200,
            hint_tokens: 300,
            snapshot_tokens: 1_500,
            delta_tokens: 400,
            interrupt_tokens: 200,
            max_interrupts: 3,
            gap_lookahead_ms: 10 * MIN,
            ttl_min_cached: 50_000,
            long_block_ms: 55 * MIN,
            finalize_before_ms: 3 * MIN,
            review_slot_ms: 15 * MIN,
            review_lookahead_ms: 2 * 60 * MIN,
            review_min_briefs: 2,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Params;

    #[test]
    fn the_weights_sum_to_one_so_a_perfect_candidate_scores_one() {
        let p = Params::default();
        let sum = p.w_sim + p.w_recency + p.w_entity + p.w_kind;
        assert!((sum - 1.0).abs() < 1e-6, "{sum}");
        assert!(p.threshold < 1.0 && p.margin < p.threshold);
    }

    #[test]
    fn a_partial_json_keeps_the_defaults_for_the_rest() {
        let p: Params = serde_json::from_str(r#"{"threshold":0.8}"#).unwrap();
        assert_eq!(p.threshold, 0.8);
        assert_eq!(p.tau_min, Params::default().tau_min);
    }
}
