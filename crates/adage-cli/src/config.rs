//! `adage config`: show and change `$ADAGE_HOME/config.toml` (keys: `adage_store::config::KEYS`).
//!
//! ```text
//! adage config [--json]          every key's effective value, and where it came from
//! adage config get <key>         one key's effective value
//! adage config set <key> <value> validate and store a value (refuses inside an agent session)
//! ```

use adage_store::config::{self, KEYS};
use adage_store::Paths;
use adage_term::{Line, Out, Role};
use anyhow::{anyhow, bail, Result};
use serde_json::json;

pub fn run(args: &[String], paths: &Paths) -> Result<()> {
    match args.first().map(String::as_str) {
        None | Some("--json") | Some("show") => show(paths, args.iter().any(|a| a == "--json")),
        Some("get") => {
            let Some(key) = args.get(1) else { bail!("usage: adage config get <key>") };
            let loaded = config::load(paths);
            let (value, _) = loaded
                .get(key)
                .ok_or_else(|| crate::hint(format!("unknown key {key}"), "`adage config` lists every key"))?;
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
            let mut o = Out::stdout();
            o.header("config set", &[key])?;
            o.blank()?;
            o.kv("value", Line::new().text(stored))?;
            o.kv("stored in", Line::new().text(crate::detect::tilde(&paths.config())))?;
            let loaded = config::load(paths);
            if let Some((v, src @ config::Source::Env(_))) = loaded.get(key) {
                o.blank()?;
                o.warn(&format!("{src} overrides it, so the effective value is {v}"))?;
            }
            o.blank()?;
            o.next("adage config", "every setting and where it comes from")?;
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
    let mut o = Out::stdout();
    o.header("config", &[&crate::detect::tilde(&paths.config())])?;
    o.blank()?;
    let w = KEYS.iter().map(|k| k.name.len()).max().unwrap_or(24) + 2;
    for k in KEYS {
        let (value, source) = &loaded.values[k.name];
        let shown = if value.is_empty() { "\"\"" } else { value };
        let src = source.to_string();
        let src = if src == "default" { src } else { format!("from {src}") };
        o.line(&Line::new().text(k.name).pad_to(w).bold(Role::Ink, shown).pad_to(w + 20).faint(src))?;
        o.line(&Line::new().pad_to(w).quiet(doc(k.doc)))?;
    }
    for p in &loaded.problems {
        o.blank()?;
        o.warn(p)?;
    }
    o.blank()?;
    o.next("adage config set <key> <value>", "change one")?;
    Ok(())
}

/// A key's description in the lexicon's words (the stored description, as `--json` prints it, is
/// unchanged).
fn doc(d: &str) -> String {
    crate::words::prose(d)
        .replace("learn automatically", "condition automatically")
        .replace("automatic learn ", "automatic conditioning run ")
        .replace("to learn from", "to condition from")
}
