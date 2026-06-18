use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::adapters::checkpoints::Checkpoints;
use circuit::adapters::forge::Forge;
use circuit::adapters::git::Git;
use circuit::adapters::probe::SystemDeliveryProbe;
use circuit::dag::DagError;
use circuit::adapters::store::Workspace;

#[derive(Parser)]
#[command(name = "circuit", about = "Architecture derivation, sessions & flow")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Analyze a Rust repo: indicators + mermaid diagram
    Analyze {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Scaffold the `.circuit/` authored data model in the current repo
    Init {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Spec-session commands
    Spec {
        #[command(subcommand)]
        command: SpecCommand,
    },
    /// Task-DAG commands
    Dag {
        #[command(subcommand)]
        command: DagCommand,
    },
    /// Session lifecycle commands
    Session {
        #[command(subcommand)]
        command: SessionCommand,
    },
    /// Render the per-session flow rail
    Flow {
        /// Session id (ULID) or unique DAG-node name; omit to show all sessions
        session: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Spec-level DAG board (mermaid) with stage + health
    Board {
        /// Spec id whose DAG to render
        spec: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum SpecCommand {
    /// Create a new spec session
    New {
        /// Spec id (used as the filename)
        id: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        intent: String,
        /// Bounded context (repeatable)
        #[arg(long = "context")]
        contexts: Vec<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum SessionCommand {
    /// Spawn an impl session for a DAG node: write the record, create the
    /// branch, and add a worktree (the session derives to Project).
    Spawn {
        /// The DAG node id to execute
        dag_node: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum DagCommand {
    /// Add a DAG node (one vertical slice)
    AddNode {
        /// Node id (used as the filename)
        id: String,
        #[arg(long)]
        spec: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        branch: String,
        #[arg(long, default_value = "")]
        intent: String,
        /// Dependency node id (repeatable)
        #[arg(long = "depends-on")]
        depends_on: Vec<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Add a dependency edge from one existing node to another
    Link {
        /// The dependent node
        from: String,
        /// The node it depends on
        to: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Validate the DAG (acyclic, refs resolve, unique branches)
    Check {
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { path } => run_analyze(&path),
        Command::Init { path } => run_init(&path),
        Command::Spec { command } => run_spec(command),
        Command::Dag { command } => run_dag(command),
        Command::Session { command } => run_session(command),
        Command::Flow { session, path } => run_flow(session.as_deref(), &path),
        Command::Board { spec, path } => run_board(&spec, &path),
    }
}

fn run_analyze(path: &Path) -> Result<()> {
    println!("{}", circuit::app::analyze(path)?);
    Ok(())
}

fn run_init(path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    match circuit::app::init(&ws)? {
        circuit::app::InitOutcome::AlreadyInitialized => {
            println!("Already initialized at {}", ws.circuit_dir().display());
        }
        circuit::app::InitOutcome::Initialized => {
            ensure_gitignored(path, ".circuit/local.toml").context("updating .gitignore")?;
            println!("Initialized .circuit/ at {}", ws.circuit_dir().display());
        }
    }
    Ok(())
}

/// Fail fast if `.circuit/` was never initialized, so authoring commands never
/// create a half-formed workspace with no config.toml.
fn require_initialized(ws: &Workspace) -> Result<()> {
    if !ws.is_initialized() {
        anyhow::bail!(
            "no .circuit/ workspace at {} — run `circuit init` first",
            ws.root().display()
        );
    }
    Ok(())
}

fn run_spec(command: SpecCommand) -> Result<()> {
    match command {
        SpecCommand::New {
            id,
            title,
            intent,
            contexts,
            path,
        } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::spec_new(&ws, &ws, &id, title, intent, contexts)?;
            println!("Created spec session: {id}");
            Ok(())
        }
    }
}

fn run_dag(command: DagCommand) -> Result<()> {
    match command {
        DagCommand::AddNode {
            id,
            spec,
            title,
            branch,
            intent,
            depends_on,
            path,
        } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::dag_add_node(&ws, &ws, &id, spec, title, branch, intent, depends_on)?;
            println!("Added DAG node: {id}");
            Ok(())
        }
        DagCommand::Link { from, to, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::dag_link(&ws, &ws, &from, &to)?;
            println!("Linked {from} → {to}");
            Ok(())
        }
        DagCommand::Check { path } => {
            let ws = Workspace::new(&path);
            let (errors, count) = circuit::app::dag_check(&ws)?;
            if errors.is_empty() {
                println!("DAG sound — {count} node(s), no problems");
                return Ok(());
            }
            for e in &errors {
                match e {
                    DagError::Cycle(c) => println!("  cycle: {}", c.join(" → ")),
                    DagError::DanglingRef { node, missing } => {
                        println!("  dangling ref: {node} → {missing} (no such node)")
                    }
                    DagError::DuplicateBranch { branch, nodes } => {
                        println!("  duplicate branch {branch}: {}", nodes.join(", "))
                    }
                }
            }
            std::process::exit(1);
        }
    }
}

fn run_session(command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Spawn { dag_node, path } => run_session_spawn(&dag_node, &path),
    }
}

fn run_session_spawn(dag_node: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
    let out = circuit::app::session_spawn(&ws, &ws, &ws, &git, dag_node, env.as_deref(), ws.root())?;
    println!("Spawned session {} for node {} (stage: Project)", out.session_id, out.dag_node);
    println!("  branch:   {}", out.branch);
    println!("  worktree: {}", out.worktree.display());
    Ok(())
}

fn run_flow(selector: Option<&str>, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let forge = Forge::new(ws.root());
    let checkpoints = Checkpoints::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::flow(&ws, &ws, &git, &forge, &checkpoints, &probe, selector)?;
    println!("{out}");
    Ok(())
}

fn run_board(spec: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let out = circuit::app::board(&ws, &ws, &ws, &git, spec)?;
    println!("{out}");
    Ok(())
}

/// Append a line to `.gitignore` if not already present (idempotent).
fn ensure_gitignored(root: &Path, entry: &str) -> Result<()> {
    let path = root.join(".gitignore");
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == entry) {
        return Ok(());
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str(entry);
    content.push('\n');
    std::fs::write(&path, content)?;
    Ok(())
}
