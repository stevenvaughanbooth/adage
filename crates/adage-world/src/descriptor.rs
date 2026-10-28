//! Source descriptors: a source is a TOML file, not code (`packs/world/*.toml`, embedded here, and
//! any the person adds under `$ADAGE_HOME/packs/world/`). It names the artifact kind the source
//! yields, how often to ask, and either an ordered list of providers (a connected MCP server, a
//! tool, its arguments and the fields to keep: the Claude Code mod runs these) or what to watch
//! on disk (`adage world follow` runs these). The binary validates every ingested event against
//! its descriptor, so a source cannot write a kind or a field it did not declare.

use adage_trace::ArtifactKind;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Where a source runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Needs {
    /// Through a connected MCP server, in the mod (`world_pull`).
    Connector,
    /// On this machine, in the follow child.
    Local,
}

/// One provider of a connector source: the first whose server the session has connected wins.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    /// The server's name as `/mcp` lists it; a glob (`github*`, `*linear*`) is allowed.
    pub server: String,
    /// The tool's name on that server.
    pub tool: String,
    /// The tool's arguments, with `{{now-lookback}}`, `{{now+lookahead}}` and `{{now}}` filled in
    /// by the mod as RFC 3339 times.
    #[serde(default)]
    pub args: serde_json::Value,
    /// JSON pointers into the tool's result, `*` walking a list: the fields kept and nothing else.
    /// `busy_when` and `accepted_when` list the values of the pointed field that mean so.
    #[serde(default)]
    pub map: BTreeMap<String, serde_json::Value>,
}

/// What a local source watches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Watch {
    /// The configuration key whose directories are watched (`world.transcripts`, `world.roots`),
    /// or empty for a source that watches no directory (the desktop).
    #[serde(default)]
    pub dirs: String,
    #[serde(default)]
    pub glob: String,
    #[serde(default)]
    pub debounce: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Source {
    pub name: String,
    pub kind: ArtifactKind,
    pub needs: Needs,
    #[serde(default)]
    pub every: String,
    #[serde(default)]
    pub lookback: String,
    #[serde(default)]
    pub lookahead: String,
}

/// One descriptor file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Descriptor {
    pub source: Source,
    #[serde(default)]
    pub provider: Vec<Provider>,
    #[serde(default)]
    pub watch: Option<Watch>,
}

impl Descriptor {
    pub fn name(&self) -> &str {
        &self.source.name
    }

    pub fn kind(&self) -> ArtifactKind {
        self.source.kind
    }

    /// The poll period in seconds (`every`), 300 when unspelled.
    pub fn every_secs(&self) -> u64 {
        secs(&self.source.every).unwrap_or(300)
    }

    pub fn lookback_secs(&self) -> u64 {
        secs(&self.source.lookback).unwrap_or(0)
    }

    pub fn lookahead_secs(&self) -> u64 {
        secs(&self.source.lookahead).unwrap_or(0)
    }

    /// Everything wrong with a descriptor, in words; empty when it is sound.
    pub fn problems(&self) -> Vec<String> {
        let mut out = Vec::new();
        let n = &self.source.name;
        if n.is_empty() || n.len() > 32 || !n.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            out.push(format!("name {n:?}: lowercase letters, digits and hyphens, at most 32"));
        }
        for (field, text) in
            [("every", &self.source.every), ("lookback", &self.source.lookback), ("lookahead", &self.source.lookahead)]
        {
            if !text.is_empty() && secs(text).is_none() {
                out.push(format!("{n}: {field} {text:?} is not a duration (`300s`, `2h`, `15m`)"));
            }
        }
        if self.every_secs() < 5 {
            out.push(format!("{n}: every {:?} is under 5 seconds", self.source.every));
        }
        match self.source.needs {
            Needs::Connector => {
                if self.provider.is_empty() {
                    out.push(format!("{n}: a connector source needs at least one provider"));
                }
                for p in &self.provider {
                    if p.server.is_empty() || p.tool.is_empty() {
                        out.push(format!("{n}: a provider needs a server and a tool"));
                    }
                    if !p.map.contains_key("id") || !p.map.contains_key("title") {
                        out.push(format!("{n}: provider {} maps no id or title", p.server));
                    }
                    for (k, v) in &p.map {
                        let ok = match v {
                            serde_json::Value::String(s) => s.starts_with('/'),
                            serde_json::Value::Array(a) => k.ends_with("_when") && a.iter().all(|x| x.is_string()),
                            _ => false,
                        };
                        if !ok {
                            out.push(format!("{n}: map {k} is not a JSON pointer or a `*_when` list"));
                        }
                    }
                }
            }
            Needs::Local => {
                if self.watch.is_none() {
                    out.push(format!("{n}: a local source needs a [watch] table"));
                }
                if !self.provider.is_empty() {
                    out.push(format!("{n}: a local source has no providers"));
                }
            }
        }
        out
    }
}

/// `300s`, `15m`, `2h`, `1d` as seconds.
pub fn secs(s: &str) -> Option<u64> {
    let s = s.trim();
    let (num, unit) = s.split_at(s.find(|c: char| !c.is_ascii_digit())?);
    let n: u64 = num.parse().ok()?;
    match unit {
        "s" => Some(n),
        "m" => Some(n * 60),
        "h" => Some(n * 3_600),
        "d" => Some(n * 86_400),
        _ => None,
    }
}

/// Parse one descriptor's TOML.
pub fn parse(text: &str) -> Result<Descriptor, String> {
    let d: Descriptor = toml::from_str(text).map_err(|e| e.to_string())?;
    let problems = d.problems();
    if problems.is_empty() {
        Ok(d)
    } else {
        Err(problems.join("; "))
    }
}

/// The descriptors shipped with Adage (`packs/world/`), in name order.
pub const BUILTIN: &[(&str, &str)] = &[
    ("calendar", include_str!("../../../packs/world/calendar.toml")),
    ("desktop", include_str!("../../../packs/world/desktop.toml")),
    ("files", include_str!("../../../packs/world/files.toml")),
    ("github", include_str!("../../../packs/world/github.toml")),
    ("meetings", include_str!("../../../packs/world/meetings.toml")),
    ("tickets", include_str!("../../../packs/world/tickets.toml")),
];

/// The built-in descriptors, parsed.
pub fn builtin() -> Vec<Descriptor> {
    BUILTIN.iter().map(|(n, t)| parse(t).unwrap_or_else(|e| panic!("packs/world/{n}.toml: {e}"))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_descriptor_parses_and_is_named_after_its_file() {
        let all = builtin();
        assert_eq!(all.len(), BUILTIN.len());
        for ((file, _), d) in BUILTIN.iter().zip(&all) {
            assert_eq!(d.name(), *file);
        }
        let connectors: Vec<&str> =
            all.iter().filter(|d| d.source.needs == Needs::Connector).map(|d| d.name()).collect();
        assert_eq!(connectors, ["calendar", "github", "tickets"]);
        let cal = all.iter().find(|d| d.name() == "calendar").unwrap();
        assert_eq!(cal.every_secs(), 300);
        assert_eq!((cal.lookback_secs(), cal.lookahead_secs()), (7_200, 28_800));
        assert_eq!(cal.provider[0].server, "claude.ai Google Calendar");
        assert_eq!(cal.provider[0].map["start"], serde_json::json!("/items/*/start/dateTime"));
        let desk = all.iter().find(|d| d.name() == "desktop").unwrap();
        assert_eq!(desk.watch.as_ref().unwrap().dirs, "");
    }

    #[test]
    fn a_bad_descriptor_says_what_is_wrong() {
        let e =
            parse("[source]\nname = \"X Y\"\nkind = \"pr\"\nneeds = \"connector\"\nevery = \"soon\"\n").unwrap_err();
        assert!(e.contains("name") && e.contains("every") && e.contains("provider"), "{e}");
        let e = parse("[source]\nname = \"ok\"\nkind = \"pr\"\nneeds = \"local\"\n").unwrap_err();
        assert!(e.contains("[watch]"), "{e}");
        assert_eq!(secs("2h"), Some(7_200));
        assert_eq!(secs("x"), None);
    }
}
