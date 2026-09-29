//! Dry-bind the instincts for a repository and print what binds and what is skipped, one line each.
//!
//! ```sh
//! cargo run -p adage-instincts --example dry_run -- <repo-dir> [<repo-dir> ...]
//! ```
//!
//! Nothing is written anywhere; this is the binder alone, as `adage init` runs it.

use adage_tracks::{Step, Trigger};
use std::collections::BTreeSet;
use std::path::Path;

fn main() {
    for dir in std::env::args().skip(1) {
        let r = adage_instincts::bind(Path::new(&dir), &[], &BTreeSet::new());
        println!("== {dir}");
        for x in &r.routes {
            let who = r.origins.get(&x.id).map(String::as_str).unwrap_or("?");
            let when = match &x.trigger {
                Trigger::AfterFailure { command, signature } => format!("after {command} fails with {signature:?}"),
                Trigger::AfterEdit { paths } => format!("after editing {paths}"),
                Trigger::AfterCommand { command } => format!("after {command}"),
                other => format!("{other:?}"),
            };
            let what = match &x.step {
                Step::Run { command } => format!("run {command}"),
                Step::Advise { template, slots } => format!("advise {template} {slots:?}"),
                other => format!("{other:?}"),
            };
            println!("  bound {who} {} [{}, {:?}]: {when} -> {what}", x.id, x.effect.as_str(), x.state);
        }
        for (who, why) in &r.skipped {
            println!("  skipped {who}: {why}");
        }
    }
}
