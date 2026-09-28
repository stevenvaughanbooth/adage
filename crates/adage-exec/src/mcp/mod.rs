//! `adage-exec mcp`: E2, routes as MCP tools (Track C §3.1, §3.5).
//!
//! A stdio MCP server, hand-written over JSON-RPC 2.0 ([`server`]). It reads `index.json` and the
//! approvals for the session's repository (from MCP roots, else the working directory, resolved
//! with `repo_key`) and lists one tool per route that is live and approved at E2 on this machine
//! ([`catalog`]), described from a fixed template, with an `inputSchema` from the slots' contracts
//! and an `outputSchema` from the declared output types ([`schema`]). Each call is checked again
//! from scratch and runs through every gate of [`crate::run`] at E2's limits ([`call`]), bounded
//! by a per-session rate limit and a machine-wide concurrency cap ([`limit`]).
//!
//! ```text
//! adage-exec mcp [--repo <dir>]          serve on stdin/stdout
//! adage-exec mcp --list [--repo <dir>]   print the tools/list this repository would get, and exit
//! ```

pub mod call;
pub mod catalog;
pub mod limit;
pub mod schema;
pub mod server;

use std::path::PathBuf;

/// The `mcp` subcommand. Returns the exit code.
pub fn main(args: &[String]) -> i32 {
    let mut repo = None;
    let mut list = false;
    let mut canaries = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--repo" if i + 1 < args.len() => {
                repo = Some(PathBuf::from(&args[i + 1]));
                i += 1;
            }
            "--canary" if i + 1 < args.len() => {
                canaries.push(args[i + 1].clone());
                i += 1;
            }
            "--list" => list = true,
            other => {
                eprintln!("adage-exec mcp: unknown argument {other:?}\nusage: adage-exec mcp [--repo <dir>] [--list]");
                return 2;
            }
        }
        i += 1;
    }
    let home = adage_store::Paths::from_env().root;
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    if list {
        let c = server::list(&home, repo.as_deref().unwrap_or(&cwd));
        let tools: Vec<serde_json::Value> = c.tools.iter().map(|t| t.definition(server::SUPPORTED[0])).collect();
        let v = serde_json::json!({ "repo": c.repo_key, "tools": tools, "note": c.note });
        println!("{}", serde_json::to_string_pretty(&v).unwrap_or_default());
        return 0;
    }
    let opts = server::Options { home, repo, cwd, canaries };
    let stdin = std::io::stdin();
    server::serve(opts, stdin.lock(), Box::new(std::io::stdout()));
    0
}
