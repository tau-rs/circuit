#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::adapters::forge::GhForge;
use circuit::dag::{validate, DagError};
use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
use circuit::model::node::DagNode;
use circuit::model::spec::SpecRecord;
use circuit::model::store::Workspace;

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
    /// Pull-request automation via the `gh` CLI
    Pr {
        #[command(subcommand)]
        command: PrCommand,
    },
    /// Record a local review checkpoint (no-remote synthetic PR)
    Checkpoint {
        /// Session id
        session: String,
        #[arg(long, value_enum)]
        state: CheckpointStateArg,
        /// Commit sha this checkpoint snapshots
        #[arg(long)]
        commit: String,
        /// Optional reviewer note
        #[arg(long)]
        note: Option<String>,
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
enum PrCommand {
    /// Open a PR for a session's branch
    Create {
        /// Session id
        session: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        body: Option<String>,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Merge a session's PR
    Merge {
        /// Session id
        session: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
    /// Update a session's PR branch from base
    UpdateFromBase {
        /// Session id
        session: String,
        #[arg(long, default_value = ".")]
        path: PathBuf,
    },
}

/// CLI spelling of the checkpoint states (kebab-case: self-review|accepted|archived).
#[derive(Clone, Copy, clap::ValueEnum)]
enum CheckpointStateArg {
    SelfReview,
    Accepted,
    Archived,
}

impl From<CheckpointStateArg> for circuit::adapters::checkpoints::CheckpointState {
    fn from(a: CheckpointStateArg) -> Self {
        use circuit::adapters::checkpoints::CheckpointState as S;
        match a {
            CheckpointStateArg::SelfReview => S::SelfReview,
            CheckpointStateArg::Accepted => S::Accepted,
            CheckpointStateArg::Archived => S::Archived,
        }
    }
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
        Command::Pr { command } => run_pr(command),
        Command::Checkpoint {
            session,
            state,
            commit,
            note,
            path,
        } => run_checkpoint(session, state, commit, note, path),
    }
}

fn run_analyze(path: &Path) -> Result<()> {
    let graph = circuit::builder::build_graph(path)?;
    let cycles = circuit::indicators::cycles::find_cycles(&graph);
    let violations = circuit::indicators::dependency_rule::violations(&graph);

    println!(
        "Architecture — No-cycles (ADP): {}",
        if cycles.is_empty() {
            "● SOUND".to_string()
        } else {
            format!("⛔ {} cyclic group(s)", cycles.len())
        }
    );
    for c in &cycles {
        println!("  cycle: {}", c.join(" → "));
    }

    println!(
        "Architecture — Dependency rule: {}",
        if violations.is_empty() {
            "● SOUND".to_string()
        } else {
            format!("⛔ {} violation(s)", violations.len())
        }
    );
    for v in &violations {
        println!(
            "  {} ({:?}) → {} ({:?})  VIOLATION",
            v.from, v.from_layer, v.to, v.to_layer
        );
    }

    println!("\n--- mermaid ---");
    println!(
        "{}",
        circuit::render::mermaid::render(&graph, &violations, &cycles)
    );
    Ok(())
}

fn run_init(path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    if ws.is_initialized() {
        println!("Already initialized at {}", ws.circuit_dir().display());
        return Ok(());
    }
    ws.save_config(&Config::default())
        .context("writing config.toml")?;
    ws.save_glossary(&Glossary::default())
        .context("writing glossary.toml")?;
    ensure_gitignored(path, ".circuit/local.toml").context("updating .gitignore")?;
    println!("Initialized .circuit/ at {}", ws.circuit_dir().display());
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
            let mut spec = SpecRecord::new(&id, title, intent);
            spec.bounded_contexts = contexts;
            ws.save_spec(&spec)
                .with_context(|| format!("writing spec {id}"))?;
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
            let mut node = DagNode::new(&id, spec, title, branch);
            node.intent = intent;
            node.depends_on = depends_on;
            ws.save_dag_node(&node)
                .with_context(|| format!("writing dag node {id}"))?;
            println!("Added DAG node: {id}");
            Ok(())
        }
        DagCommand::Link { from, to, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            let mut node = ws
                .load_dag_node(&from)
                .with_context(|| format!("loading dag node {from}"))?;
            if !node.depends_on.contains(&to) {
                node.depends_on.push(to.clone());
            }
            ws.save_dag_node(&node)
                .with_context(|| format!("writing dag node {from}"))?;
            println!("Linked {from} → {to}");
            Ok(())
        }
        DagCommand::Check { path } => {
            let ws = Workspace::new(&path);
            let nodes = ws.list_dag_nodes().context("reading dag nodes")?;
            let errors = validate(&nodes);
            if errors.is_empty() {
                println!("DAG sound — {} node(s), no problems", nodes.len());
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

fn run_pr(command: PrCommand) -> Result<()> {
    match command {
        PrCommand::Create {
            session,
            title,
            body,
            path,
        } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::pr_create(&ws, &GhForge::new(), &session, title, body)
        }
        PrCommand::Merge { session, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::pr_merge(&ws, &GhForge::new(), &session)
        }
        PrCommand::UpdateFromBase { session, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            circuit::app::pr_update_from_base(&ws, &GhForge::new(), &session)
        }
    }
}

fn run_checkpoint(
    session: String,
    state: CheckpointStateArg,
    commit: String,
    note: Option<String>,
    path: PathBuf,
) -> Result<()> {
    let ws = Workspace::new(&path);
    require_initialized(&ws)?;
    circuit::app::write_checkpoint(&ws, &session, state.into(), commit, note)
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
