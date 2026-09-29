//! The pinned price table (`pricing.toml`) that turns a trace's token counts into an
//! **API-list-equivalent** dollar figure for the savings estimate (docs/zero-config.md §4.1).
//!
//! A model the table does not list has no price: its sessions are shown as tokens and turns only.
//! The table is compiled in, so every surface (card, watch, map) prices with the same numbers, and
//! it cites the date its prices were read ([`Table::as_of`]).

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

const TABLE: &str = include_str!("pricing.toml");

/// Dollars per million tokens, per token class.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Rates {
    pub input: f64,
    pub output: f64,
    /// Cache writes with the 5-minute TTL (1.25x input).
    pub cache_write: f64,
    /// Cache writes with the 1-hour TTL (2x input), which Claude Code also writes.
    pub cache_write_1h: f64,
    pub cache_read: f64,
}

#[derive(Debug, Clone, Deserialize)]
struct Entry {
    prefix: String,
    input: f64,
    output: f64,
    cache_write: f64,
    /// Defaults to 2x input, Anthropic's 1-hour cache write multiplier.
    #[serde(default)]
    cache_write_1h: Option<f64>,
    cache_read: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Table {
    /// The date the list prices were read (`YYYY-MM-DD`).
    pub as_of: String,
    pub source: String,
    #[serde(rename = "model")]
    models: Vec<Entry>,
}

/// Token counts of one trace, by class. `input` excludes cached tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tokens {
    pub input: u64,
    pub output: u64,
    /// Every cache write, whatever its TTL.
    pub cache_write: u64,
    pub cache_read: u64,
    /// Of `cache_write`, the tokens written with the 1-hour TTL (Claude Code's
    /// `usage.cache_creation.ephemeral_1h_input_tokens`), priced at their own rate.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_write_1h: u64,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Tokens {
    pub fn add(&mut self, o: &Tokens) {
        self.input += o.input;
        self.output += o.output;
        self.cache_write += o.cache_write;
        self.cache_read += o.cache_read;
        self.cache_write_1h += o.cache_write_1h;
    }
    pub fn total(&self) -> u64 {
        self.input + self.output + self.cache_write + self.cache_read
    }
}

static PARSED: Lazy<Table> = Lazy::new(|| toml::from_str(TABLE).expect("pricing.toml parses (tested)"));

/// The compiled-in table.
pub fn table() -> &'static Table {
    &PARSED
}

impl Table {
    /// The rates for a model id, by the longest listed prefix it starts with; `None` for a model
    /// the table does not list (never guessed).
    pub fn rates(&self, model: &str) -> Option<Rates> {
        let m = model.trim().to_ascii_lowercase();
        // Bedrock and Vertex spellings: `anthropic.claude-…`, `claude-…@date`.
        let m = m.strip_prefix("anthropic.").unwrap_or(&m).to_string();
        self.models.iter().filter(|e| m.starts_with(&e.prefix)).max_by_key(|e| e.prefix.len()).map(|e| Rates {
            input: e.input,
            output: e.output,
            cache_write: e.cache_write,
            cache_write_1h: e.cache_write_1h.unwrap_or(2.0 * e.input),
            cache_read: e.cache_read,
        })
    }

    /// API-list-equivalent dollars for these tokens on this model; `None` if it is not listed.
    pub fn cost(&self, model: &str, t: &Tokens) -> Option<f64> {
        let r = self.rates(model)?;
        Some(
            (t.input as f64 * r.input
                + t.output as f64 * r.output
                + t.cache_write.saturating_sub(t.cache_write_1h) as f64 * r.cache_write
                + t.cache_write_1h.min(t.cache_write) as f64 * r.cache_write_1h
                + t.cache_read as f64 * r.cache_read)
                / 1e6,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_table_parses_cites_its_date_and_matches_by_longest_prefix() {
        let t = table();
        assert_eq!(t.as_of.len(), 10, "a YYYY-MM-DD date");
        assert_eq!(t.rates("claude-haiku-4-5-20251001").unwrap().input, 1.0);
        assert_eq!(t.rates("claude-opus-5-5").unwrap().input, 4.0, "not claude-opus-5's");
        assert_eq!(t.rates("claude-opus-5").unwrap().input, 5.0);
        assert_eq!(t.rates("claude-sonnet-4-6").unwrap().output, 15.0);
        assert_eq!(t.rates("anthropic.claude-sonnet-5").unwrap().input, 2.0);
        assert_eq!(t.rates("claude-opus-4-20250514").unwrap().input, 15.0);
        assert!(t.rates("gpt-5-codex").is_none());
        assert!(t.rates("claude-test").is_none());
        assert!(t.rates("").is_none());
        let tok = Tokens { input: 1_000_000, output: 1_000_000, cache_read: 1_000_000, ..Default::default() };
        assert!((t.cost("claude-sonnet-4-6", &tok).unwrap() - 18.3).abs() < 1e-9);
        assert!(t.cost("mystery", &tok).is_none());
        // Cache writes by TTL: 5 minutes at 1.25x input, 1 hour at 2x (Haiku 4.5: $1.25, $2).
        let w = |cache_write, cache_write_1h| Tokens { cache_write, cache_write_1h, ..Default::default() };
        assert!((t.cost("claude-haiku-4-5", &w(1_000_000, 0)).unwrap() - 1.25).abs() < 1e-9);
        assert!((t.cost("claude-haiku-4-5", &w(1_000_000, 1_000_000)).unwrap() - 2.0).abs() < 1e-9);
        assert!((t.cost("claude-haiku-4-5", &w(2_000_000, 1_000_000)).unwrap() - 3.25).abs() < 1e-9);
    }
}
