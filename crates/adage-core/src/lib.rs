//! Adage core: the canonical action vocabulary, the shell normaliser (tree-sitter-bash plus a
//! data-driven command table), effect classes, typed spans, and repository identity.

pub mod action;
pub mod cloud;
pub mod execution_equivalence;
pub mod gitignore;
pub mod gitobj;
pub mod json;
pub mod judge;
pub mod keys;
pub mod outcome;
pub mod pattern;
pub mod pricing;
pub mod repo;
pub mod secrets;
pub mod shell;
pub mod spans;
pub mod table;
pub mod template;
pub mod time;
pub mod tokens;
pub mod tools;
pub mod types;
pub mod vtype;

pub use table::CommandTable;
pub use types::{CanonicalAction, Effect, Normalized};

#[cfg(test)]
mod classification_tests {
    use super::*;

    fn effect(cmd: &str) -> Effect {
        shell::normalize_shell(cmd, "/repo", CommandTable::load())[0].effect
    }

    #[test]
    fn repo_scripts_with_read_verbs_are_observe() {
        assert_eq!(effect("./scripts/wi-lookup W-1 --json"), Effect::Observe);
        assert_eq!(effect("scripts/show-config"), Effect::Observe);
        assert_eq!(effect("./scripts/deploy-prod"), Effect::Environment);
    }

    #[test]
    fn interpreters_are_classified_by_script() {
        assert_eq!(effect("python3 scripts/wi-lookup W-1"), Effect::Observe);
        assert_eq!(effect("python3 tools/gen.py"), Effect::Environment);
    }

    #[test]
    fn make_targets() {
        assert_eq!(effect("make gen"), Effect::Regenerate);
        assert_eq!(effect("make test"), Effect::Verify);
        assert_eq!(effect("make ci"), Effect::Verify);
    }
}
