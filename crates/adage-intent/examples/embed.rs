//! Normalise and embed prompts with an installed model, one JSON line per input line.
//!
//! ```text
//! cargo run --release -p adage-intent --example embed -- <installed model dir> < prompts.txt
//! ```
//! Input lines are JSON strings (so prompts may contain newlines). Used by spike S7 to check the
//! Rust tokenizer and pooling against the Python reference (model2vec).

use adage_intent::model::StaticModel;
use adage_intent::normalize::normalize;
use std::io::{BufRead, Write};

fn main() {
    let dir = std::env::args().nth(1).expect("usage: embed <installed model dir>");
    let m = StaticModel::load(std::path::Path::new(&dir)).expect("open model");
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    for line in std::io::stdin().lock().lines() {
        let raw: String = serde_json::from_str(&line.unwrap()).expect("a JSON string per line");
        let norm = normalize(&raw);
        let vec = m.embed(&norm);
        writeln!(out, "{}", serde_json::json!({ "norm": norm, "vec": vec })).unwrap();
    }
}
