//! The instinct catalogue, as data (`data/catalogue.toml`, embedded at compile time).

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

const RAW: &str = include_str!("../data/catalogue.toml");

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Pack {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Instinct {
    pub id: String,
    pub priority: String,
    pub enabled: bool,
    pub title: String,
    pub trigger: String,
    pub directive: String,
    pub cap: String,
    pub detection: String,
    pub fp_risk: String,
    pub kind: String,
    #[serde(default)]
    pub needs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Probes {
    pub python: Vec<String>,
    pub node: Vec<String>,
    pub pytest: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Generators {
    pub known: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Catalogue {
    pub pack: Pack,
    #[serde(rename = "instinct")]
    pub instincts: Vec<Instinct>,
    pub probes: Probes,
    pub generators: Generators,
    /// Root lockfile name → the tool that maintains it.
    pub lockfiles: BTreeMap<String, String>,
}

static CATALOGUE: Lazy<Catalogue> = Lazy::new(|| toml::from_str(RAW).expect("data/catalogue.toml must parse"));

/// The embedded catalogue.
pub fn catalogue() -> &'static Catalogue {
    &CATALOGUE
}

impl Catalogue {
    pub fn get(&self, id: &str) -> Option<&Instinct> {
        self.instincts.iter().find(|i| i.id == id)
    }

    /// `instinct:<pack>@<version>`: the provenance every bound route and advisory carries.
    pub fn provenance(&self) -> String {
        format!("instinct:{}@{}", self.pack.name, self.pack.version)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalogue_parses_and_is_complete() {
        let c = catalogue();
        let ids: Vec<&str> = c.instincts.iter().map(|i| i.id.as_str()).collect();
        for want in [
            "U1", "U2", "U3", "U4", "U5", "U6", "N1", "P1", "U7", "U8", "U9", "U10", "U11", "N2", "N3", "R1", "R2",
            "G1", "X1",
        ] {
            assert!(ids.contains(&want), "{want} missing");
        }
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "duplicate instinct ids");
        // Only the P0 set binds now; every instinct is capped at prime.
        for i in &c.instincts {
            assert_eq!(i.cap, "prime", "{}", i.id);
            if i.priority == "P1" {
                assert!(!i.enabled, "{} is P1 and must ship disabled", i.id);
            }
            assert!(["route", "advisory"].contains(&i.kind.as_str()), "{}", i.id);
        }
        let enabled: Vec<&str> = c.instincts.iter().filter(|i| i.enabled).map(|i| i.id.as_str()).collect();
        assert_eq!(enabled, ["U1", "U3", "U4", "U5", "U6", "N1", "P1", "U11"]);
        assert_eq!(c.provenance(), "instinct:core@0.1.0");
    }
}
