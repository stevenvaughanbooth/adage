//! Loads `data/commands.toml` — the per-command flag grammar and effect table — as data.
//!
//! Embedded at compile time via `include_str!` (so the binary is self-contained and the spike
//! folder can be zipped up / run from any cwd) but parsed at *runtime*, and structurally it is
//! exactly the kind of external, editable table Design Spec §8.6 and Architecture §4.7 call for:
//! "per-command flag grammars as data (a table)" / "mapping tables per harness version as data".

use regex::Regex;
use serde::Deserialize;
use std::collections::HashMap;

use crate::types::Effect;

#[derive(Debug, Deserialize)]
struct RawTable {
    commands: Vec<RawCommand>,
}

#[derive(Debug, Deserialize, Default)]
struct RawCommand {
    name: String,
    effect: String,
    #[serde(default)]
    value_flags: Vec<String>,
    #[serde(default)]
    mutating_flags: Vec<String>,
    #[serde(default)]
    regenerate_flags: Vec<String>,
    #[serde(default)]
    publish_flags: Vec<String>,
    #[serde(default)]
    pattern_effects: Vec<(String, String)>,
    #[serde(default)]
    inert: bool,
    #[serde(default)]
    subcommands: Vec<RawCommand>,
}

pub struct CommandSpec {
    pub effect: Effect,
    pub value_flags: Vec<String>,
    pub mutating_flags: Vec<String>,
    pub regenerate_flags: Vec<String>,
    pub publish_flags: Vec<String>,
    pub pattern_effects: Vec<(Regex, Effect)>,
    /// True only when repository files cannot make this program run code (Track C §4.2).
    /// Gates automatic approval in `adage-exec`; never inherited by subcommands or parents.
    pub inert: bool,
    pub subcommands: HashMap<String, CommandSpec>,
}

pub struct CommandTable {
    pub commands: HashMap<String, CommandSpec>,
}

fn build_spec(raw: RawCommand) -> CommandSpec {
    let effect = Effect::parse(&raw.effect).unwrap_or(Effect::Environment);
    let pattern_effects = raw
        .pattern_effects
        .into_iter()
        .filter_map(|(pat, eff)| {
            let re = Regex::new(&pat).ok()?;
            let e = Effect::parse(&eff)?;
            Some((re, e))
        })
        .collect();
    let subcommands = raw.subcommands.into_iter().map(|sc| (sc.name.clone(), build_spec(sc))).collect();
    CommandSpec {
        effect,
        value_flags: raw.value_flags,
        mutating_flags: raw.mutating_flags,
        regenerate_flags: raw.regenerate_flags,
        publish_flags: raw.publish_flags,
        pattern_effects,
        inert: raw.inert,
        subcommands,
    }
}

impl CommandTable {
    pub fn load() -> CommandTable {
        const RAW: &str = include_str!("../data/commands.toml");
        let raw: RawTable = toml::from_str(RAW).expect("data/commands.toml must parse");
        let commands = raw.commands.into_iter().map(|c| (c.name.clone(), build_spec(c))).collect();
        CommandTable { commands }
    }

    pub fn get(&self, name: &str) -> Option<&CommandSpec> {
        self.commands.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inert_names(table: &CommandTable) -> Vec<String> {
        let mut out = Vec::new();
        for (name, spec) in &table.commands {
            if spec.inert {
                out.push(name.clone());
            }
            for (sub, s) in &spec.subcommands {
                if s.inert {
                    out.push(format!("{name} {sub}"));
                }
            }
        }
        out.sort();
        out
    }

    #[test]
    fn inert_is_exactly_the_reviewed_set() {
        // Track C §4.2: widening this set widens what can be approved automatically, so it is
        // pinned here and any change must be a deliberate edit to this list.
        let table = CommandTable::load();
        assert_eq!(
            inert_names(&table),
            [
                "cat",
                "git diff",
                "git log",
                "git rev-parse",
                "git show",
                "git status",
                "grep",
                "head",
                "jq",
                "ls",
                "rg",
                "tail",
                "wc"
            ]
        );
        assert!(!table.get("git").unwrap().inert, "git itself is never inert");
        for never in ["make", "npm", "cargo", "pytest", "python", "python3", "node", "sh", "bash"] {
            if let Some(spec) = table.get(never) {
                assert!(!spec.inert, "{never} must never be inert");
            }
        }
    }
}
