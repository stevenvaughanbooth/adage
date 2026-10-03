//! The command-line grammar: every command, flag and argument, with help text, in one place.
//!
//! clap validates the command line, prints help (styled from the design tokens), suggests the
//! nearest command for a typo, and generates shell completions. Each command module then reads
//! its flags from the raw arguments as it always has; a test ([`tests`]) checks that every flag a
//! module reads is declared here, so the two cannot drift apart. `adage hook` never reaches clap:
//! `main` handles it first, on the hook's fast path.
//!
//! Words follow the lexicon (docs/lexicon.md): traces, reflexes, instincts; `sense`,
//! `condition`, `map`, `delegate`; `review strengthen | weaken | extinguish | delegate | revoke`.

use adage_term::{Role, Theme};
use clap::builder::styling::Styles;
use clap::builder::StyledStr;
use clap::{Args, Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "adage",
    version,
    about = "Agent traces become reflexes.",
    long_about = None,
    disable_help_subcommand = true,
    subcommand_required = false,
    arg_required_else_help = false,
)]
pub struct Cli {
    /// Plain output for screen readers and logs: no colour, no glyphs beyond ASCII.
    #[arg(long, global = true)]
    pub plain: bool,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    // ------------------------------------------------------------------ start
    /// Set Adage up in one go: find your harnesses, register hooks, condition from past traces.
    #[command(
        after_help = "At a terminal this is a guided flow. `--print` shows every step without asking; `--yes` runs them."
    )]
    Init(InitArgs),
    /// Check that this machine's Adage is set up and healthy.
    Doctor(DoctorArgs),

    // ------------------------------------------------------------------ every day
    /// Reflexes and instincts in effect, with their strength and evidence.
    Status(JsonArgs),
    /// A live pane for a split terminal: what fired, what was conditioned, health.
    Watch(WatchArgs),
    /// The full-screen app: live feed, reflexes and review, the map, health, settings.
    Ui(UiArgs),
    /// Open the map in your browser: every stimulus, reflex and response, live.
    Map(MapArgs),
    /// Reflexes waiting for a person, and the decisions you make about them.
    #[command(args_conflicts_with_subcommands = true)]
    Review(ReviewArgs),
    /// What Adage saves in this repository, measured against control sessions.
    Savings(SavingsArgs),

    // ------------------------------------------------------------------ learning
    /// What your agents actually do, read from past traces (read-only).
    #[command(
        after_help = "With no source named, reads configured Claude/Codex history and OpenCode exports. Explicit sources replace these defaults."
    )]
    Sense(SenseArgs),
    /// Condition reflexes from traces and compile the index.
    #[command(
        after_help = "With no source named, reads configured Claude/Codex history and OpenCode exports. Explicit sources replace these defaults. With no default history, binds applicable instincts in the current repository."
    )]
    Condition(ConditionArgs),
    /// Import, inspect and learn from versioned canonical trace corpora.
    Corpus(CorpusArgs),
    /// What Adage told agents, and whether they followed it.
    Report(JsonArgs),
    /// Adage's live state as a graph (JSON for tools; the map draws it).
    Graph(GraphArgs),
    /// Compile a skill into one reviewed pack of reflexes (runs adage-train).
    #[command(disable_help_flag = true)]
    Train(TrainArgs),

    // ------------------------------------------------------------------ sharing
    /// The packs Adage binds: its base instincts and the packs you added.
    Packs(PacksArgs),
    /// Where packs come from: the adage marketplace, built in.
    Marketplace(MarketplaceArgs),
    /// Build and sign a pack from a track, to share it.
    Share(ShareArgs),
    /// Verify, install and bind a shared pack.
    Add(AddArgs),

    // ------------------------------------------------------------------ execution
    /// Let hooks run delegated reflexes (E1 prefetch, E2 actions), or stop them.
    #[command(args_conflicts_with_subcommands = true)]
    Delegate(DelegateArgs),

    // ------------------------------------------------------------------ setup
    /// Print or write one harness's hook registration.
    Install(InstallArgs),
    /// The optional embedding model for intent families.
    Models(ModelsArgs),
    /// Show and change settings in $ADAGE_HOME/config.toml.
    Config(ConfigArgs),
    /// Print a Claude Code hooks settings block.
    #[command(name = "hooks-config")]
    HooksConfig(HooksConfigArgs),
    /// Print a shell completion script.
    Completions(CompletionsArgs),
    /// Update Adage to the latest version, the same way it was installed.
    #[command(
        after_help = "Until the first release is published, this builds the main branch from source with cargo."
    )]
    Update(UpdateArgs),

    // ------------------------------------------------------------------ evaluation
    /// Offline replay evaluation of what Adage would have said.
    #[command(disable_help_flag = true)]
    Eval(EvalArgs),

    /// Print the version.
    Version,
}

#[derive(Args, Debug)]
pub struct JsonArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct InitArgs {
    /// The repository to set up (default: the current directory).
    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,
    #[arg(long, value_name = "PATH", hide = true)]
    pub repo_path: Option<PathBuf>,
    /// Run every step without asking (fetches the model).
    #[arg(long, conflicts_with = "print")]
    pub yes: bool,
    /// Show every step and its command without doing anything.
    #[arg(long)]
    pub print: bool,
}

#[derive(Args, Debug)]
pub struct DoctorArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
    /// The same checks, live, with hook activity (`adage watch`).
    #[arg(long)]
    pub watch: bool,
    /// With --watch: `adage watch`'s flags.
    #[arg(long, requires = "watch", hide = true)]
    pub once: bool,
    #[arg(long, value_name = "WxH", requires = "watch", hide = true)]
    pub size: Option<String>,
    #[arg(long, value_name = "PATH", requires = "watch", hide = true)]
    pub repo: Option<PathBuf>,
    #[arg(long, requires = "watch", hide = true)]
    pub demo: bool,
}

#[derive(Args, Debug)]
pub struct WatchArgs {
    /// Follow this repository (default: the current one).
    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,
    /// Replay a sample scenario through real hook decisions.
    #[arg(long)]
    pub demo: bool,
    /// Print one frame and exit.
    #[arg(long)]
    pub once: bool,
    /// Frame size for --once, as WIDTHxHEIGHT.
    #[arg(long, value_name = "WxH")]
    pub size: Option<String>,
}

#[derive(Args, Debug)]
pub struct UiArgs {
    /// Open on this repository (default: the current one).
    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,
    /// Replay a sample scenario.
    #[arg(long)]
    pub demo: bool,
}

#[derive(Args, Debug)]
pub struct MapArgs {
    /// Port on 127.0.0.1 (0 picks a free one).
    #[arg(long, value_name = "N")]
    pub port: Option<u16>,
    /// Print the address instead of opening a browser.
    #[arg(long)]
    pub no_open: bool,
    /// Open the map focused on this reflex.
    #[arg(long, value_name = "REFLEX")]
    pub focus: Option<String>,
    /// Serve a sample scenario.
    #[arg(long)]
    pub demo: bool,
}

#[derive(Args, Debug)]
pub struct ReviewArgs {
    /// Machine-readable queue (agents may read it).
    #[arg(long)]
    pub json: bool,
    /// The statistics behind each reflex too (bounds, uplift, the graph engine's reasons).
    #[arg(long, short = 'v')]
    pub verbose: bool,
    #[command(subcommand)]
    pub decision: Option<ReviewDecision>,
}

#[derive(Subcommand, Debug)]
pub enum ReviewDecision {
    /// Accept a reflex as it is (a nascent one goes into service).
    Approve(Decide),
    /// Raise a reflex from prime to fire, within its effect ceiling.
    Strengthen(Decide),
    /// Lower a reflex from fire to prime; held across conditioning.
    Weaken(Decide),
    /// Retire a reflex; conditioning keeps it retired unless its evidence grows materially.
    Extinguish(Decide),
    /// Edit-lock a reflex: conditioning never overwrites it.
    Pin(Decide),
    /// Keep a pinned reflex and silence its proposed change.
    Dismiss(Decide),
    /// Approve a reflex for delegated execution (E1 prefetch or E2 action).
    Delegate(DelegateRoute),
    /// Withdraw a reflex's delegation.
    Revoke(RevokeRoute),
}

#[derive(Args, Debug)]
pub struct Decide {
    /// The track (name, or path).
    pub track: String,
    /// The reflex id.
    pub reflex: String,
    /// Who decided (recorded with the decision).
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
}

#[derive(Args, Debug)]
pub struct DelegateRoute {
    /// The track (name, or path).
    pub track: String,
    /// The reflex id.
    pub reflex: String,
    /// Execution level: `e1` (prefetch) or `e2` (an action).
    #[arg(long, value_name = "LEVEL")]
    pub level: String,
    /// E2: files the action may write, as globs (repeatable).
    #[arg(long, value_name = "GLOB", action = clap::ArgAction::Append)]
    pub outputs: Vec<String>,
    /// E2: files the action reads that key its cache, as globs (repeatable).
    #[arg(long, value_name = "GLOB", action = clap::ArgAction::Append)]
    pub cache: Vec<String>,
    /// E2: what the action does to the repository.
    #[arg(long, value_name = "EFFECT", value_parser = ["verify", "regenerate"])]
    pub effect: Option<String>,
    /// Pass the raw output through instead of selected fields.
    #[arg(long)]
    pub raw: bool,
    /// Fields to keep from the output, as JSON.
    #[arg(long, value_name = "JSON")]
    pub fields: Option<String>,
    /// Stop when a field has a value (repeatable).
    #[arg(long = "exit-when", value_name = "FIELD=VALUE")]
    pub exit_when: Vec<String>,
    /// Time limit in milliseconds.
    #[arg(long = "timeout-ms", value_name = "MS")]
    pub timeout_ms: Option<u64>,
    /// The repository (default: the current one).
    #[arg(long, value_name = "DIR")]
    pub repo: Option<PathBuf>,
    /// Who decided (recorded with the delegation).
    #[arg(long, value_name = "NAME")]
    pub by: Option<String>,
}

#[derive(Args, Debug)]
pub struct RevokeRoute {
    /// The track (name, or path).
    pub track: String,
    /// The reflex id.
    pub reflex: String,
    /// The repository (default: the current one).
    #[arg(long, value_name = "DIR")]
    pub repo: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct Sources {
    /// Transcript files to read (Claude Code JSONL, `opencode export` output).
    #[arg(long, value_name = "FILE", num_args = 1..)]
    pub transcripts: Vec<PathBuf>,
    /// Claude Code history (otherwise uses configured history when no source is named).
    #[arg(long, value_name = "DIR")]
    pub claude_projects: Option<PathBuf>,
    /// Codex rollouts (otherwise $CODEX_HOME/sessions once Codex hooks are registered).
    #[arg(long, value_name = "DIR")]
    pub codex_sessions: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct SenseArgs {
    #[command(flatten)]
    pub sources: Sources,
    /// Read every repository's traces (default: those recorded in this repository).
    #[arg(long, conflicts_with = "repo")]
    pub all: bool,
    /// Keep one repository key.
    #[arg(long, value_name = "KEY")]
    pub repo: Option<String>,
    /// Keep traces from the last N days.
    #[arg(long, value_name = "DAYS")]
    pub since: Option<u32>,
    /// Use the sample scenario's thresholds.
    #[arg(long)]
    pub demo: bool,
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct ConditionArgs {
    #[command(flatten)]
    pub sources: Sources,
    /// Keep traces whose recorded working directory is inside this path.
    #[arg(long, value_name = "PATH")]
    pub cwd_prefix: Option<String>,
    /// Session outcomes, as a JSON file of {session: bool}.
    #[arg(long, value_name = "FILE")]
    pub outcomes: Option<PathBuf>,
    /// Evidence identity JSON: {session: {task_id, group_id, attempt_id}}.
    #[arg(long, value_name = "FILE")]
    pub identities: Option<PathBuf>,
    /// Force every trace into this repository key (with --instincts-only: the key to bind under).
    #[arg(long, value_name = "KEY")]
    pub repo: Option<String>,
    /// Low thresholds, for the sample scenario.
    #[arg(long)]
    pub demo: bool,
    /// Re-parse every trace instead of using the parse cache.
    #[arg(long)]
    pub no_cache: bool,
    /// Do not bind instincts.
    #[arg(long)]
    pub no_instincts: bool,
    /// Bind only instincts, from the repository's files, with no traces (day 0).
    #[arg(long)]
    pub instincts_only: bool,
    /// The repository for --instincts-only (default: the current directory).
    #[arg(long, value_name = "DIR")]
    pub repo_path: Option<PathBuf>,
    /// Condition one repository quietly, as hooks do after a session.
    #[arg(long, hide = true)]
    pub auto: bool,
    /// Condition only this repository: traces recorded in its checkouts, and its instincts
    /// (default: the repository this runs in).
    #[arg(long, value_name = "DIR")]
    pub repo_root: Option<PathBuf>,
    /// Condition every repository in the history sources (default: the one this runs in).
    #[arg(long, conflicts_with_all = ["repo_root", "auto"])]
    pub all: bool,
    #[arg(long, value_name = "MS", hide = true)]
    pub after: Option<u64>,
    /// With --auto: the first run in a repository (hooks pass it).
    #[arg(long, hide = true, requires = "auto")]
    pub bootstrap: bool,
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct CorpusArgs {
    #[command(subcommand)]
    pub action: CorpusAction,
}

#[derive(Subcommand, Debug)]
pub enum CorpusAction {
    /// Normalize native trace files to canonical JSONL on stdout.
    Export {
        #[arg(required = true, num_args = 1..)]
        transcripts: Vec<PathBuf>,
        #[arg(long)]
        repo: Option<String>,
        #[arg(long, value_name = "FILE")]
        identities: Option<PathBuf>,
    },
    /// Atomically import one repository's canonical JSONL; print the audit report as JSON.
    Import {
        input: PathBuf,
        #[arg(long)]
        repo: String,
        /// Replace the snapshot, including corrections and removals; default merges new ids.
        #[arg(long)]
        replace: bool,
    },
    /// Show the repository's committed evidence manifest as JSON.
    Inspect {
        #[arg(long)]
        repo: String,
    },
    /// Learn the committed snapshot and compile its reflexes; print the report as JSON.
    Learn {
        #[arg(long)]
        repo: String,
        /// Low thresholds for synthetic fixtures.
        #[arg(long)]
        demo: bool,
    },
}

#[derive(Args, Debug)]
pub struct GraphArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
    /// Recent decisions to include.
    #[arg(long, value_name = "N")]
    pub events: Option<usize>,
    /// The graph engine's facts for one repository.
    #[arg(long)]
    pub learned: bool,
    /// The repository key whose facts --learned prints.
    #[arg(long, value_name = "REPO", requires = "learned")]
    pub dump: Option<String>,
    /// Transcript files for --learned.
    #[arg(long, value_name = "FILE", num_args = 1..)]
    pub transcripts: Vec<PathBuf>,
    /// Claude Code history for --learned.
    #[arg(long, value_name = "DIR")]
    pub claude_projects: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ShareArgs {
    /// A learned track, an installed pack, or a pack or track directory.
    pub source: String,
    /// Where to write the pack.
    #[arg(long, value_name = "DIR")]
    pub out: PathBuf,
    /// Signing key (default: your SSH key).
    #[arg(long, value_name = "PATH")]
    pub key: Option<PathBuf>,
    /// Acknowledge vendored scripts that reach the network or read secrets.
    #[arg(long)]
    pub ack_scripts: bool,
    /// Vendor the repository scripts reflexes run into the pack.
    #[arg(long)]
    pub vendor: bool,
    /// Attach self-reported test results (JSONL; repeatable).
    #[arg(long, value_name = "FILE", action = clap::ArgAction::Append)]
    pub attest: Vec<PathBuf>,
    /// The repository checkout scripts are pinned against (default: the current one).
    #[arg(long, value_name = "DIR")]
    pub repo_root: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct AddArgs {
    /// The pack directory.
    pub dir: PathBuf,
    /// Trust the pack's signer.
    #[arg(long)]
    pub trust: bool,
    /// The repository to bind the pack in (default: the one `add` runs in).
    #[arg(long, value_name = "DIR")]
    pub repo_path: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct PacksArgs {
    #[command(subcommand)]
    pub action: Option<PacksAction>,
    /// The repository to report on, or to bind again (default: the current one).
    #[arg(long, value_name = "DIR", global = true)]
    pub repo_path: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum PacksAction {
    /// Bind an instinct pack wherever its detectors find what it needs.
    On {
        /// The pack (`adage packs` lists them).
        name: String,
    },
    /// Stop binding an instinct pack, here and everywhere.
    Off {
        /// The pack (`adage packs` lists them).
        name: String,
    },
    /// A pack's files, as installed: what a packaged reflex is.
    Show {
        /// The pack (`adage packs` lists them).
        name: String,
    },
}

#[derive(Args, Debug)]
pub struct MarketplaceArgs {
    #[command(subcommand)]
    pub action: Option<MarketplaceAction>,
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Subcommand, Debug)]
pub enum MarketplaceAction {
    /// A marketplace's manifest, as published: every pack it offers, pinned by digest.
    Show {
        /// The marketplace (`adage marketplace` lists them).
        name: String,
    },
    /// Install a marketplace's packs now, as learning does: each at its pin; ones it no longer lists removed.
    Update {
        /// The marketplace (default: every one).
        name: Option<String>,
    },
}

#[derive(Args, Debug)]
pub struct DelegateArgs {
    /// With no subcommand: the status, as machine-readable output.
    #[arg(long)]
    pub json: bool,
    /// With no subcommand: the repository whose status to show (default: the current one).
    #[arg(long, value_name = "DIR")]
    pub repo: Option<PathBuf>,
    /// With no subcommand, `status`.
    #[command(subcommand)]
    pub action: Option<DelegateAction>,
}

#[derive(Subcommand, Debug)]
pub enum DelegateAction {
    /// Hooks call adage-exec, which runs delegated reflexes (grants are per repository).
    On {
        /// The adage-exec binary.
        #[arg(long, value_name = "PATH")]
        bin: Option<PathBuf>,
        /// The repository whose hook registrations change (default: the current one).
        #[arg(long, value_name = "DIR")]
        repo: Option<PathBuf>,
    },
    /// Hooks call adage again (delegations are kept).
    Off {
        /// The repository whose hook registrations change (default: the current one).
        #[arg(long, value_name = "DIR")]
        repo: Option<PathBuf>,
    },
    /// Registrations and delegated reflexes; changes nothing.
    Status {
        /// Machine-readable output.
        #[arg(long)]
        json: bool,
        /// The repository (default: the current one).
        #[arg(long, value_name = "DIR")]
        repo: Option<PathBuf>,
    },
    /// Rewrites to the project's environment (PreToolUse), on their own switch.
    Rewrites {
        /// `on` or `off`.
        #[arg(value_parser = ["on", "off"])]
        state: String,
    },
    /// This machine's execution policy, and where it is kept; --max-level e2 opts in to actions.
    Policy {
        /// The highest level reflexes may run at: e1 (read-only lookups) or e2 (approved actions
        /// from hooks; a person's decision).
        #[arg(long, value_name = "LEVEL", value_parser = ["e1", "e2", "E1", "E2"])]
        max_level: Option<String>,
    },
}

#[derive(ValueEnum, Clone, Debug)]
pub enum Harness {
    #[value(name = "claude-code", alias = "claude")]
    ClaudeCode,
    Opencode,
    Codex,
}

#[derive(Args, Debug)]
#[command(
    group(clap::ArgGroup::new("change").args(["write", "remove"])),
    after_help = "Writing skips a harness already registered elsewhere for this repository (the other scope, or the Claude Code plugin): a second registration runs every hook twice. `--remove` takes Adage's hooks out again; your own hooks and settings stay as they are."
)]
pub struct InstallArgs {
    /// The agent harness to register Adage's hooks with.
    pub harness: Harness,
    /// Print the registration (the default).
    #[arg(long, conflicts_with_all = ["write", "remove"])]
    pub print: bool,
    /// Write it (a diff is shown first, and a terminal asks before writing).
    #[arg(long)]
    pub write: bool,
    /// Remove Adage's hooks from the scope's file (a diff is shown first; a backup is kept).
    #[arg(long)]
    pub remove: bool,
    /// With --write or --remove: do it without asking.
    #[arg(long, short = 'y', requires = "change")]
    pub yes: bool,
    /// This repository only (project, the default) or every repository (user).
    #[arg(long, value_parser = ["project", "user"])]
    pub scope: Option<String>,
    /// The adage binary the hooks run (default: this one).
    #[arg(long, value_name = "PATH")]
    pub bin: Option<PathBuf>,
    /// The repository for --scope project (default: the current one).
    #[arg(long, value_name = "DIR")]
    pub repo: Option<PathBuf>,
}

#[derive(Args, Debug)]
pub struct ModelsArgs {
    #[command(subcommand)]
    pub action: Option<ModelsAction>,
}

#[derive(Subcommand, Debug)]
pub enum ModelsAction {
    /// Download a pinned model and verify it (default: the one selected).
    Fetch {
        /// The model (`adage models status` lists them).
        key: Option<String>,
    },
    /// Install from a directory you downloaded yourself.
    Install {
        /// Holds model.safetensors, tokenizer.json and config.json, or an unpacked npm package.
        dir: PathBuf,
        /// The model it is (default: detected).
        key: Option<String>,
    },
    /// Choose what hooks embed prompts with: auto, hashed, or a model key.
    Use {
        /// auto, hashed, potion-base-8m or potion-code-16m-v2.
        which: String,
    },
    /// What hooks embed with now, and every known model.
    Status,
}

#[derive(Args, Debug)]
pub struct ConfigArgs {
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
    #[command(subcommand)]
    pub action: Option<ConfigAction>,
}

#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// One key's effective value.
    Get {
        /// A key, e.g. learn.auto (`adage config` lists them).
        key: String,
    },
    /// Validate and store a value.
    Set {
        /// A key, e.g. learn.auto (`adage config` lists them).
        key: String,
        /// The new value.
        #[arg(allow_hyphen_values = true)]
        value: String,
    },
}

#[derive(Args, Debug)]
pub struct HooksConfigArgs {
    /// The adage binary the hooks run (default: adage).
    pub bin: Option<String>,
}

#[derive(Args, Debug)]
pub struct UpdateArgs {
    /// Where to update from (default: how this adage was installed).
    #[arg(long, value_parser = ["source", "npm", "brew"])]
    pub from: Option<String>,
    /// The branch a source update builds.
    #[arg(long, value_name = "NAME", default_value = "main")]
    pub branch: String,
    /// Print the command without running it.
    #[arg(long)]
    pub print: bool,
}

#[derive(Args, Debug)]
pub struct CompletionsArgs {
    /// The shell to complete for.
    pub shell: clap_complete::Shell,
}

#[derive(Args, Debug)]
pub struct SavingsArgs {
    /// The repository (default: the current one).
    #[arg(long, value_name = "PATH")]
    pub repo: Option<PathBuf>,
    /// Machine-readable output.
    #[arg(long)]
    pub json: bool,
}

#[derive(Args, Debug)]
pub struct TrainArgs {
    /// `<SKILL.md>`, `accept <name>`, `plan`, `--plan <file>` or `report <run-dir>`
    /// (`adage train --help` lists every option).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

#[derive(Args, Debug)]
pub struct EvalArgs {
    /// Arguments for the evaluator (`adage eval --help`).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    pub args: Vec<String>,
}

/// Commands the lexicon renamed (docs/lexicon.md). They are not aliases: using one is an error
/// that names its replacement.
pub fn renamed(args: &[String]) -> Option<&'static str> {
    let first = args.first().map(String::as_str)?;
    let second = args.get(1).map(String::as_str);
    Some(match (first, second) {
        ("learn", _) => "adage condition",
        ("survey", _) => "adage sense",
        ("exec", Some("disable")) => "adage delegate off",
        ("exec", Some("status")) => "adage delegate status",
        ("exec", _) => "adage delegate on",
        ("delegate", _) if args.iter().any(|a| a == "--off") => "adage delegate off",
        ("delegate", _) if args.iter().any(|a| a == "--status") => "adage delegate status",
        ("graph", _) if args.iter().any(|a| a == "--serve") => "adage map",
        ("review", Some("promote")) => "adage review strengthen",
        ("review", Some("demote")) => "adage review weaken",
        ("review", Some("reject" | "retire")) => "adage review extinguish",
        ("review", Some("allow-exec")) => "adage review delegate",
        ("review", Some("revoke-exec" | "undelegate")) => "adage review revoke",
        _ => return None,
    })
}

/// The old spelling [`renamed`] matched, as the person wrote it (`graph --serve`, `review promote`).
pub fn renamed_from(args: &[String]) -> String {
    match (args[0].as_str(), args.get(1)) {
        ("graph", _) => "graph --serve".to_string(),
        ("delegate", _) => {
            let old = args.iter().find(|a| *a == "--off" || *a == "--status").map_or("", String::as_str);
            format!("delegate {old}")
        }
        (v @ ("review" | "exec"), Some(s)) => format!("{v} {s}"),
        (v, _) => v.to_string(),
    }
}

/// A clap usage error (unknown flag, missing argument, bad value) in the output grammar: the
/// message; clap's tips (a similar argument, the possible values); then help with the usage and
/// the command's `--help`. Returns the message and its labelled branches for `Out::error_items`.
pub fn usage_error(e: &clap::Error, args: &[String]) -> (String, Vec<(&'static str, String)>) {
    // The plain rendering (StyledStr's Display drops styling), which carries the tailored usage.
    let text = e.render().to_string();
    let (mut message, mut extra, mut tips, mut usage) = (String::new(), Vec::new(), Vec::new(), None);
    let mut in_message = false;
    for line in text.lines() {
        let t = line.trim();
        if let Some(m) = line.strip_prefix("error: ") {
            message = m.trim().to_string();
            in_message = true;
        } else if let Some(tip) = t.strip_prefix("tip: ") {
            tips.push(tip.to_string());
        } else if let Some(u) = line.strip_prefix("Usage: ") {
            usage.get_or_insert_with(|| u.trim().to_string());
        } else if t.is_empty() || t.starts_with("For more information") {
            in_message = false;
        } else if in_message {
            // `[possible values: a, b]` is a tip; anything else (the missing arguments) is part
            // of the message.
            match t.strip_prefix('[').and_then(|v| v.strip_suffix(']')) {
                Some(v) => tips.push(v.to_string()),
                None => extra.push(t.to_string()),
            }
        }
    }
    if message.is_empty() {
        message = e.kind().to_string();
    }
    if !extra.is_empty() {
        message = format!("{} {}", message, extra.join(", "));
    }
    // The subcommand the error is about, from the words on the command line.
    let mut cmd = command(&Theme::plain());
    let mut path = vec!["adage".to_string()];
    for a in args {
        if a.starts_with('-') {
            continue;
        }
        let Some(sub) = cmd.find_subcommand(a).cloned() else { break };
        path.push(sub.get_name().to_string());
        cmd = sub;
    }
    let usage = usage.unwrap_or_else(|| {
        let u = cmd.bin_name(path.join(" ")).render_usage().to_string();
        u.trim().trim_start_matches("Usage:").trim().lines().next().unwrap_or_default().to_string()
    });
    let mut items: Vec<(&'static str, String)> = tips.into_iter().map(|t| ("tip", t)).collect();
    items.push(("help", format!("usage: {usage} · see `{} --help`", path.join(" "))));
    (message, items)
}

/// clap's help and error colours, from the design tokens.
pub fn styles(t: &Theme) -> Styles {
    Styles::styled()
        .header(t.style(Role::Accent).bold())
        .usage(t.style(Role::Accent).bold())
        .literal(t.style(Role::Command))
        .placeholder(t.style(Role::Ink3))
        .valid(t.style(Role::Fire))
        .invalid(t.style(Role::Prime))
        .error(t.style(Role::Inhibit).bold())
}

/// The commands, grouped by the question a person has (docs/design/everyday-ux.md §5.6): the
/// everyday three first, then what to run when something is off, then setting up; everything else
/// after. Every command stays.
fn overview(t: &Theme) -> StyledStr {
    const GROUPS: &[(&str, &[(&str, &str)])] = &[
        (
            "Every day",
            &[
                ("watch", "what Adage sees and says, live (a pane for a split terminal)"),
                ("review", "the reflexes Adage formed: look them over, approve, extinguish"),
                ("map", "explore reflexes and their evidence in your browser"),
            ],
        ),
        ("When something is off", &[("doctor", "what is wrong, and the fix for each problem")]),
        (
            "Set up",
            &[
                ("init", "set Adage up: harnesses, hooks, past traces"),
                ("install", "register Adage's hooks with one agent"),
                ("packs", "base instincts and added packs: list, turn on, turn off"),
                ("config", "settings"),
                ("update", "update Adage itself"),
            ],
        ),
        (
            "Conditioning",
            &[
                ("sense", "what your agents actually do (read-only)"),
                ("condition", "condition reflexes from traces now"),
                ("corpus", "import, inspect and learn canonical trace collections"),
                ("report", "what Adage said, and what agents did"),
                ("graph", "Adage's state as a graph"),
                ("train", "compile a skill into reflexes"),
            ],
        ),
        (
            "Sharing",
            &[
                ("marketplace", "where packs come from"),
                ("share", "build and sign a pack"),
                ("add", "verify and install a pack"),
            ],
        ),
        (
            "More",
            &[
                ("ui", "the full-screen app: live, reflexes, map, health, settings"),
                ("status", "reflexes and instincts in effect, with evidence"),
                ("savings", "what Adage saves here"),
                ("delegate", "let hooks run delegated reflexes, and rewrites"),
                ("models", "the optional embedding model"),
                ("completions", "shell completions"),
                ("eval", "offline replay evaluation"),
            ],
        ),
    ];
    let head = t.style(Role::Accent).bold();
    let cmd = t.style(Role::Command);
    let mut s = String::new();
    for (i, (group, cmds)) in GROUPS.iter().enumerate() {
        if i > 0 {
            s.push('\n');
        }
        s.push_str(&format!("{head}{group}:{head:#}\n"));
        for (name, what) in *cmds {
            s.push_str(&format!("  {cmd}{name:<13}{cmd:#}{what}\n"));
        }
    }
    StyledStr::from(s.trim_end().to_string() + "\n")
}

/// The full command, with styles and the grouped overview for this terminal.
pub fn command(t: &Theme) -> clap::Command {
    use clap::CommandFactory;
    let tip = t.style(Role::Ink3);
    let head = t.style(Role::Accent).bold();
    Cli::command()
        .styles(styles(t))
        .color(if t.color() { clap::ColorChoice::Always } else { clap::ColorChoice::Never })
        .help_template(format!(
            "{{about}}\n\n{{usage-heading}} {{usage}}\n\n{{before-help}}\n{head}Options:{head:#}\n{{options}}\n{{after-help}}"
        ))
        .before_help(overview(t))
        .after_help(StyledStr::from(format!(
            "{tip}Bare{tip:#} adage {tip}answers \"is it working here?\": the status card.{tip:#}\n\
             {tip}New here? Start with{tip:#} adage init{tip}. `adage <command> --help` explains each command.{tip:#}\n\
             {tip}Environment: ADAGE_HOME (data directory), ADAGE_DISABLE=1 (kill switch),\n\
             ADAGE_ARM=treated|control, ADAGE_CONTROL (control share), ADAGE_AUTO_LEARN=0|1,\n\
             ADAGE_ALLOW_NONINTERACTIVE=1 (share and add in CI).{tip:#}"
        )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_grammar_is_consistent() {
        command(&Theme::plain()).debug_assert();
    }

    /// Long flags declared anywhere under the named subcommands.
    fn declared(names: &[&str]) -> BTreeSet<String> {
        fn walk(c: &clap::Command, out: &mut BTreeSet<String>) {
            for a in c.get_arguments() {
                if let Some(l) = a.get_long() {
                    out.insert(format!("--{l}"));
                }
            }
            for s in c.get_subcommands() {
                walk(s, out);
            }
        }
        let root = command(&Theme::plain());
        let mut out = BTreeSet::new();
        // Global flags (`--plain`) are declared on the root.
        for a in root.get_arguments().filter(|a| a.is_global_set()) {
            if let Some(l) = a.get_long() {
                out.insert(format!("--{l}"));
            }
        }
        for n in names {
            walk(root.find_subcommand(n).unwrap_or_else(|| panic!("no subcommand {n}")), &mut out);
        }
        out
    }

    /// Flag literals a module reads (`"--name"`), from its source.
    fn read_by(file: &str) -> BTreeSet<String> {
        let src =
            std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src").join(file)).unwrap();
        let mut out = BTreeSet::new();
        let b = src.as_bytes();
        let mut i = 0;
        while let Some(p) = src[i..].find("\"--") {
            let start = i + p + 1;
            let mut end = start + 2;
            while end < b.len() && (b[end].is_ascii_lowercase() || b[end].is_ascii_digit() || b[end] == b'-') {
                end += 1;
            }
            if end < b.len() && b[end] == b'"' && end > start + 2 {
                out.insert(src[start..end].to_string());
            }
            i = end;
        }
        out
    }

    /// Every flag a command module reads is declared for the commands that reach it, so clap
    /// never refuses a flag the module understands.
    #[test]
    fn every_flag_a_module_reads_is_declared() {
        // Flags a module reads that are not command-line flags (arguments it passes to curl,
        // cargo, git or npm, or the top-level --version it handles for scripts).
        let not_flags: BTreeSet<&str> = [
            "--max-filesize",
            "--proto",
            "--connect-timeout",
            "--version",
            "--help",
            "--serve",
            "--locked",
            "--git",
            "--global",
            "--config",
            "--ff-only",
            "--manifest-path",
            "--profile",
            "--release",
        ]
        .into();
        let table: &[(&str, &[&str])] = &[
            ("add.rs", &["add"]),
            ("packs_cmd.rs", &["packs"]),
            ("marketplace_cmd.rs", &["marketplace"]),
            ("autolearn.rs", &["condition"]),
            ("config.rs", &["config"]),
            ("doctor.rs", &["doctor"]),
            ("exec.rs", &["delegate", "review"]),
            ("graph.rs", &["graph"]),
            ("init.rs", &["init"]),
            ("install.rs", &["install"]),
            ("review.rs", &["review"]),
            ("share.rs", &["share"]),
            ("share_pack.rs", &["share"]),
            ("sense.rs", &["sense"]),
            ("savings.rs", &["savings"]),
            ("models.rs", &["models"]),
            ("update.rs", &["update"]),
            ("main.rs", &["condition", "status", "report", "delegate"]),
            ("ui/mod.rs", &["watch", "ui"]),
        ];
        let mut missing = Vec::new();
        for (file, cmds) in table {
            let have = declared(cmds);
            for f in read_by(file) {
                if !have.contains(&f) && !not_flags.contains(f.as_str()) {
                    missing.push(format!("{file} reads {f}, not declared for {cmds:?}"));
                }
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    #[test]
    fn typos_get_a_suggestion() {
        let e = command(&Theme::plain()).try_get_matches_from(["adage", "condtion"]).unwrap_err();
        assert!(e.to_string().contains("condition"), "{e}");
    }

    fn usage_of(args: &[&str]) -> (String, Vec<(&'static str, String)>) {
        let e = command(&Theme::plain()).try_get_matches_from(std::iter::once(&"adage").chain(args)).unwrap_err();
        assert_eq!(e.exit_code(), 2);
        usage_error(&e, &args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn usage_errors_keep_the_suggestion_and_end_with_help() {
        let (msg, items) = usage_of(&["status", "--jsn"]);
        assert_eq!(msg, "unexpected argument '--jsn' found");
        assert_eq!(items[0], ("tip", "a similar argument exists: '--json'".to_string()));
        assert_eq!(items.last().unwrap(), &("help", "usage: adage status --json · see `adage status --help`".into()));

        let (msg, items) = usage_of(&["condtion"]);
        assert!(msg.contains("'condtion'"), "{msg}");
        assert!(items.iter().any(|(l, t)| *l == "tip" && t.contains("'condition'")), "{items:?}");
        assert!(items.last().unwrap().1.ends_with("see `adage --help`"), "{items:?}");
    }

    #[test]
    fn missing_arguments_are_named_in_the_message() {
        let (msg, items) = usage_of(&["share"]);
        assert_eq!(msg, "the following required arguments were not provided: --out <DIR>, <SOURCE>");
        assert_eq!(items, vec![("help", "usage: adage share --out <DIR> <SOURCE> · see `adage share --help`".into())]);

        let (msg, items) = usage_of(&["review", "approve", "t"]);
        assert!(msg.ends_with("<REFLEX>"), "{msg}");
        assert!(items[0].1.ends_with("see `adage review approve --help`"), "{items:?}");
    }

    #[test]
    fn bad_values_list_the_possible_ones() {
        let (msg, items) = usage_of(&["install", "foo"]);
        assert_eq!(msg, "invalid value 'foo' for '<HARNESS>'");
        assert_eq!(items[0], ("tip", "possible values: claude-code, opencode, codex".to_string()));
        // clap prints no usage for a bad value; the command's own usage stands in.
        assert!(items[1].1.starts_with("usage: adage install "), "{items:?}");
        assert!(items[1].1.ends_with("see `adage install --help`"), "{items:?}");
    }

    #[test]
    fn old_verbs_are_gone() {
        for old in ["learn", "survey", "exec"] {
            assert!(command(&Theme::plain()).try_get_matches_from(["adage", old]).is_err(), "{old}");
        }
        for old in ["promote", "demote", "reject", "allow-exec", "undelegate"] {
            assert!(
                command(&Theme::plain()).try_get_matches_from(["adage", "review", old, "t", "r"]).is_err(),
                "{old}"
            );
        }
    }

    #[test]
    fn renamed_verbs_name_their_replacement() {
        let a = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(renamed(&a(&["learn", "--demo"])), Some("adage condition"));
        assert_eq!(renamed(&a(&["review", "promote", "t", "r"])), Some("adage review strengthen"));
        assert_eq!(renamed(&a(&["graph", "--serve"])), Some("adage map"));
        assert_eq!(renamed(&a(&["graph", "--json"])), None);
        assert_eq!(renamed(&a(&["review", "undelegate", "t", "r"])), Some("adage review revoke"));
        assert_eq!(renamed(&a(&["delegate", "--off"])), Some("adage delegate off"));
        assert_eq!(renamed(&a(&["delegate", "off"])), None);
        assert_eq!(renamed_from(&a(&["delegate", "--bin", "x", "--off"])), "delegate --off");
        assert_eq!(renamed_from(&a(&["review", "promote", "t", "r"])), "review promote");
        assert_eq!(renamed(&a(&["condition"])), None);
    }

    #[test]
    fn hook_spawned_conditioning_parses() {
        // adage-hook starts `adage condition --auto --repo-root <r> [--bootstrap] [--after ms]`.
        let m = command(&Theme::plain()).try_get_matches_from([
            "adage",
            "condition",
            "--auto",
            "--repo-root",
            "/r",
            "--bootstrap",
            "--after",
            "1500",
        ]);
        assert!(m.is_ok(), "{m:?}");
        assert!(command(&Theme::plain()).try_get_matches_from(["adage", "condition", "--engine", "graph"]).is_err());
    }

    #[test]
    fn delegate_takes_subcommands() {
        for args in [
            &["delegate"][..],
            &["delegate", "on", "--bin", "/x"],
            &["delegate", "off"],
            &["delegate", "status", "--json"],
            &["delegate", "--json"],
            &["delegate", "rewrites", "off"],
            &["delegate", "policy"],
            &["delegate", "policy", "--max-level", "e2"],
        ] {
            let m = command(&Theme::plain()).try_get_matches_from(std::iter::once(&"adage").chain(args));
            assert!(m.is_ok(), "{args:?}: {m:?}");
        }
        assert!(command(&Theme::plain()).try_get_matches_from(["adage", "delegate", "rewrites", "maybe"]).is_err());
        assert!(command(&Theme::plain())
            .try_get_matches_from(["adage", "delegate", "policy", "--max-level", "e3"])
            .is_err());
    }

    #[test]
    fn top_level_help() {
        let help = command(&Theme::plain()).render_help().to_string();
        insta::assert_snapshot!(help);
    }
}
