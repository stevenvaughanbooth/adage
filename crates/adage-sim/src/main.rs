//! `adage-sim [dir]`: write the mock session histories (Claude Code transcripts, OpenCode exports,
//! Codex rollouts) for the acme-billing scenario. Default: the repository's `sim/fixtures`.

fn main() {
    let dir = std::env::args().nth(1).map(std::path::PathBuf::from).unwrap_or_else(adage_sim::fixtures_dir);
    match adage_sim::write_all(&dir) {
        Ok(n) => println!("adage-sim: wrote {n} files under {}", dir.display()),
        Err(e) => {
            eprintln!("adage-sim: {e}");
            std::process::exit(1);
        }
    }
}
