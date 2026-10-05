//! Errors that tell a person what to do instead.

/// `what` failed; `help` says what to run instead. Front ends show `help` beside the error.
#[derive(Debug)]
pub struct Hint {
    pub what: String,
    pub help: String,
}

impl std::fmt::Display for Hint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.what)
    }
}

impl std::error::Error for Hint {}

/// `what` failed; `help` says what to run instead.
pub fn hint(what: impl Into<String>, help: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(Hint { what: what.into(), help: help.into() })
}
