//! `adage config`: show and change `$ADAGE_HOME/config.toml` (keys: `adage_store::config::KEYS`).
//!
//! ```text
//! adage config [--json]          every key's effective value, and where it came from
//! adage config get <key>         one key's effective value
//! adage config set <key> <value> validate and store a value (refuses inside an agent session)
//! ```

use adage_store::config::{self, KEYS};
use adage_store::Paths;
use anyhow::{anyhow, bail, Result};
use serde_json::json;

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("--json") | Some("show") => show(paths, args.iter().any(|a| a == "--json")),
        Some("get") => {
            let Some(key) = args.get(1) else { bail!("usage: adage config get <key>") };
            let loaded = config::load(paths);
            let (value, _) = loaded.get(key).ok_or_else(|| anyhow!("unknown key {key}; `adage config` lists them"))?;
            println!("{value}");
            Ok(())
        }
        Some("set") => {
            let (Some(key), Some(value)) = (args.get(1), args.get(2)) else {
                bail!("usage: adage config set <key> <value>")
            };
            // Settings decide what runs in the background and what agents are told: a person's call.
            crate::share::require_person("config set")?;
            let stored = config::set(paths, key, value).map_err(|e| anyhow!(e))?;
            println!("adage config: {key} = {stored} (in {})", paths.config().display());
            let loaded = config::load(paths);
            if let Some((v, src @ config::Source::Env(_))) = loaded.get(key) {
                println!("note: {src} overrides it, so the effective value is {v}");
            }
            Ok(())
        }
        Some(other) => {
            bail!("unknown `adage config {other}`; usage: adage config [get <key> | set <key> <value>] [--json]")
        }
    }
}

fn show(paths: &Paths, as_json: bool) -> Result<()> {
    let loaded = config::load(paths);
    if as_json {
        let keys: Vec<serde_json::Value> = KEYS
            .iter()
            .map(|k| {
                let (value, source) = &loaded.values[k.name];
                json!({ "key": k.name, "value": value, "source": source.to_string(), "default": k.default, "env": k.env, "doc": k.doc })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(
                &json!({ "file": paths.config(), "keys": keys, "problems": loaded.problems })
            )?
        );
        return Ok(());
    }
    println!("adage config ({})", paths.config().display());
    for k in KEYS {
        let (value, source) = &loaded.values[k.name];
        let shown = if value.is_empty() { "\"\"" } else { value };
        println!("  {:<24} = {:<18} ({source})", k.name, shown);
        println!("  {:<24}   {}", "", k.doc);
    }
    for p in &loaded.problems {
        println!("warning: {p}");
    }
    Ok(())
}
