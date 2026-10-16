mod corpus;
mod fixtures;
mod shell;
mod table;
mod template;
mod tools;
mod types;

use table::CommandTable;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let table = CommandTable::load();

    match args.get(1).map(|s| s.as_str()) {
        Some("shell") => {
            let cmd = args.get(2).cloned().unwrap_or_default();
            let out = shell::normalize_shell(&cmd, "/repo", &table);
            println!("{}", serde_json::to_string_pretty(&out).unwrap());
        }
        Some("fixtures") => {
            let path = args.get(2).cloned().unwrap_or_else(|| "fixtures/golden.json".to_string());
            std::process::exit(fixtures::run(&path, &table));
        }
        Some("corpus") => {
            corpus::run(&table);
        }
        _ => {
            eprintln!("usage: normaliser shell '<command>' | fixtures [path] | corpus");
            std::process::exit(2);
        }
    }
}
