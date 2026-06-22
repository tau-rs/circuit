#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::adapters::checkpoints::Checkpoints;
use circuit::adapters::forge::Forge;
use circuit::adapters::git::Git;
use circuit::adapters::probe::SystemDeliveryProbe;
use circuit::adapters::store::Workspace;
use circuit::dag::DagError;

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
    /// Structural comprehension: entry points + reachable function groups (no LLM)
    Comprehend {
        #[arg(default_value = ".")]
        path: PathBuf,
    },
    /// Impact / blast radius: dependents + dependencies of a function (no LLM)
    Impact {
        /// Function name or `module::name` to analyze
        target: String,
        #[arg(default_value = ".")]
        path: PathBuf,
        /// Cap hops in both cones (default: unlimited)
        #[arg(long)]
        max_depth: Option<u32>,
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
        /// Include archived sessions in the no-selector list
        #[arg(long)]
        all: bool,
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
    /// Archive (retire) a session: tear down its worktree, optionally delete
    /// its branch, and flip status to `archived` (the durable agent-stop signal).
    Archive {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        /// Also delete the session's branch (default: keep it)
        #[arg(long)]
        delete_branch: bool,
        /// Remove a dirty/locked worktree and delete an un-merged branch
        #[arg(long)]
        force: bool,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Unarchive (restore) a session: flip status back to `active` and re-add
    /// the worktree from the kept branch.
    Unarchive {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Open a PR for the session's branch (title/body from its DAG node).
    Pr {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Merge the session's approved PR (merge-commit strategy).
    Merge {
        /// Session id (ULID) or unique DAG-node name
        id: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Update the session's branch from its base branch.
    Update {
        /// Session id (ULID) or unique DAG-node name
        id: String,
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
        Command::Comprehend { path } => run_comprehend(&path),
        Command::Impact {
            target,
            path,
            max_depth,
        } => run_impact(&target, &path, max_depth),
        Command::Init { path } => run_init(&path),
        Command::Spec { command } => run_spec(command),
        Command::Dag { command } => run_dag(command),
        Command::Session { command } => run_session(command),
        Command::Flow { session, all, path } => run_flow(session.as_deref(), all, &path),
        Command::Board { spec, path } => run_board(&spec, &path),
    }
}

fn run_analyze(path: &Path) -> Result<()> {
    println!("{}", circuit::app::analyze(path)?);
    Ok(())
}

fn run_comprehend(path: &Path) -> Result<()> {
    println!("{}", circuit::app::comprehend(path)?);
    Ok(())
}

fn run_impact(target: &str, path: &Path, max_depth: Option<u32>) -> Result<()> {
    println!("{}", circuit::app::impact(path, target, max_depth)?);
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
        SessionCommand::Archive {
            id,
            delete_branch,
            force,
            path,
        } => run_session_archive(&id, delete_branch, force, &path),
        SessionCommand::Unarchive { id, path } => run_session_unarchive(&id, &path),
        SessionCommand::Pr { id, path } => run_session_pr(&id, &path),
        SessionCommand::Merge { id, path } => run_session_merge(&id, &path),
        SessionCommand::Update { id, path } => run_session_update(&id, &path),
    }
}

fn run_session_spawn(dag_node: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
    let out =
        circuit::app::session_spawn(&ws, &ws, &ws, &git, dag_node, env.as_deref(), ws.root())?;
    println!(
        "Spawned session {} for node {} (stage: Project)",
        out.session_id, out.dag_node
    );
    println!("  branch:   {}", out.branch);
    println!("  worktree: {}", out.worktree.display());
    Ok(())
}

fn run_flow(selector: Option<&str>, all: bool, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let forge = Forge::new(ws.root());
    let checkpoints = Checkpoints::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::flow(&ws, &ws, &git, &forge, &checkpoints, &probe, selector, all)?;
    println!("{out}");
    Ok(())
}

/// Archive a session: delegates the worktree/branch teardown + status flip to
/// the app layer, then prints the outcome.
fn run_session_archive(id: &str, delete_branch: bool, force: bool, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let out = circuit::app::session_archive(&ws, &ws, &git, id, delete_branch, force)?;
    if out.already_archived {
        println!("Session {} already archived.", out.session_id);
        return Ok(());
    }
    println!(
        "Session {} archived — agent session may now end.",
        out.session_id
    );
    match (&out.branch, delete_branch) {
        (Some(b), true) => println!("  branch {b} deleted"),
        (Some(b), false) => println!("  branch {b} kept (use --delete-branch to remove)"),
        (None, _) => {}
    }
    Ok(())
}

/// Unarchive a session: flips status back to active and rehydrates the worktree
/// from the kept branch (delegated to the app layer).
fn run_session_unarchive(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let git = Git::new(ws.root());
    let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
    let out = circuit::app::session_unarchive(&ws, &ws, &git, id, env.as_deref(), ws.root())?;
    if out.was_not_archived {
        println!("Session {} is not archived.", out.session_id);
        return Ok(());
    }
    println!("Session {} restored to active.", out.session_id);
    match (&out.rehydrated_worktree, &out.branch_missing) {
        (Some(wt), _) => println!("  worktree: {}", wt.display()),
        (None, Some(branch)) => println!(
            "  branch {branch} no longer exists — worktree not recreated; \
             session derives Draft"
        ),
        (None, None) => {}
    }
    Ok(())
}

/// Open a PR for the session's branch via the forge adapter.
fn run_session_pr(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let forge = Forge::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::session_pr(&ws, &ws, &ws, &forge, &probe, id)?;
    println!("Opened PR for session {} (node {})", out.session_id, id);
    println!("  branch: {} → base: {}", out.branch, out.base);
    println!("  title:  {}", out.title);
    Ok(())
}

/// Merge the session's approved PR via the forge adapter.
fn run_session_merge(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let forge = Forge::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::session_merge(&ws, &ws, &forge, &probe, id)?;
    println!(
        "Merged PR for session {} ({} → {})",
        out.session_id, out.branch, out.base
    );
    Ok(())
}

/// Update the session's branch from base via the forge adapter.
fn run_session_update(id: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let forge = Forge::new(ws.root());
    let probe = SystemDeliveryProbe::new(ws.root());
    let out = circuit::app::session_update(&ws, &ws, &forge, &probe, id)?;
    println!("Updated {} from {}", out.branch, out.base);
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
