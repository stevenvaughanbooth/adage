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
    subcommands: Vec<RawCommand>,
}

pub struct CommandSpec {
    pub effect: Effect,
    pub value_flags: Vec<String>,
    pub mutating_flags: Vec<String>,
    pub regenerate_flags: Vec<String>,
    pub publish_flags: Vec<String>,
    pub pattern_effects: Vec<(Regex, Effect)>,
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
    let subcommands = raw
        .subcommands
        .into_iter()
        .map(|sc| (sc.name.clone(), build_spec(sc)))
        .collect();
    CommandSpec {
        effect,
        value_flags: raw.value_flags,
        mutating_flags: raw.mutating_flags,
        regenerate_flags: raw.regenerate_flags,
        publish_flags: raw.publish_flags,
        pattern_effects,
        subcommands,
    }
}

impl CommandTable {
    pub fn load() -> CommandTable {
        const RAW: &str = include_str!("../data/commands.toml");
        let raw: RawTable = toml::from_str(RAW).expect("data/commands.toml must parse");
        let commands = raw
            .commands
            .into_iter()
            .map(|c| (c.name.clone(), build_spec(c)))
            .collect();
        CommandTable { commands }
    }

    pub fn get(&self, name: &str) -> Option<&CommandSpec> {
        self.commands.get(name)
    }
}
