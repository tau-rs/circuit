use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::dag::{validate, DagError};
use circuit::flow::facts::{BranchFacts, DeliveryFacts};
use circuit::flow::stage::derive_stage;
use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
use circuit::model::node::DagNode;
use circuit::model::spec::SpecRecord;
use circuit::model::store::Workspace;
use circuit::ports::{GitPort, Worktree};
use circuit::render::dag_board::{self, Board, BoardNode};
use circuit::session::{SessionId, SessionRecord};

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
        Command::Board { spec, path } => run_board(&spec, &path),
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
    println!("{}", circuit::render::mermaid::render(&graph, &violations, &cycles));
    Ok(())
}

fn run_init(path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    if ws.is_initialized() {
        println!("Already initialized at {}", ws.circuit_dir().display());
        return Ok(());
    }
    ws.save_config(&Config::default()).context("writing config.toml")?;
    ws.save_glossary(&Glossary::default()).context("writing glossary.toml")?;
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
        SpecCommand::New { id, title, intent, contexts, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            let mut spec = SpecRecord::new(&id, title, intent);
            spec.bounded_contexts = contexts;
            ws.save_spec(&spec).with_context(|| format!("writing spec {id}"))?;
            println!("Created spec session: {id}");
            Ok(())
        }
    }
}

fn run_dag(command: DagCommand) -> Result<()> {
    match command {
        DagCommand::AddNode { id, spec, title, branch, intent, depends_on, path } => {
            let ws = Workspace::new(&path);
            require_initialized(&ws)?;
            let mut node = DagNode::new(&id, spec, title, branch);
            node.intent = intent;
            node.depends_on = depends_on;
            ws.save_dag_node(&node).with_context(|| format!("writing dag node {id}"))?;
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
            ws.save_dag_node(&node).with_context(|| format!("writing dag node {from}"))?;
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

/// No-op `GitPort` for the period before the git-adapter slice merges: it answers
/// honestly that it knows nothing. `branch_facts` errors (=> `?` stage, `?/n`
/// tasks); `list_worktrees` is empty (=> `Unknown` health). PR NOTE: when the git
/// adapter lands, swap this for the real adapter at the one `run_board` wiring
/// point — `cockpit`/`render` are already generic over `GitPort`.
struct UnknownGit;

#[derive(Debug)]
struct GitUnavailable;
impl std::fmt::Display for GitUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "git adapter not yet available")
    }
}
impl std::error::Error for GitUnavailable {}

impl GitPort for UnknownGit {
    type Error = GitUnavailable;
    fn branch_facts(&self, _branch: &str, _base: &str) -> Result<BranchFacts, Self::Error> {
        Err(GitUnavailable)
    }
    fn create_branch(&self, _branch: &str, _base: &str) -> Result<(), Self::Error> {
        Err(GitUnavailable)
    }
    fn add_worktree(&self, _branch: &str, _path: &Path) -> Result<(), Self::Error> {
        Err(GitUnavailable)
    }
    fn list_worktrees(&self) -> Result<Vec<Worktree>, Self::Error> {
        Ok(vec![])
    }
}

fn run_board(spec: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;
    let base = ws.load_config().context("reading config.toml")?.base_branch;
    let nodes: Vec<DagNode> = ws
        .list_dag_nodes()
        .context("reading dag nodes")?
        .into_iter()
        .filter(|n| n.spec == spec)
        .collect();
    let sessions = ws.list_sessions().context("reading sessions")?;

    let git = UnknownGit;

    let mut board_nodes = Vec::new();
    for n in &nodes {
        let stage = match git.branch_facts(&n.branch, &base) {
            Ok(branch) => {
                let session = sessions
                    .iter()
                    .find(|s| s.dag_node.as_deref() == Some(n.id.as_str()))
                    .cloned()
                    // derive_stage ignores the session in M2, so a synthesized
                    // record (with a throwaway id, never rendered) is sound here.
                    .unwrap_or_else(|| {
                        SessionRecord::impl_(SessionId::generate(), &n.spec, &n.id, &n.branch)
                    });
                let facts = DeliveryFacts { branch, review: None };
                Some(derive_stage(&session, &facts))
            }
            Err(_) => None,
        };
        let health = circuit::cockpit::rollup::node_health(&git, &n.branch);
        board_nodes.push(BoardNode {
            id: n.id.clone(),
            depends_on: n.depends_on.clone(),
            stage,
            health,
        });
    }

    let board = Board { nodes: board_nodes };
    print!("{}", dag_board::render(&board));

    // Per-node readout, sorted by id (matches the board's node order).
    println!("\n--- nodes ---");
    let mut sorted: Vec<&BoardNode> = board.nodes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));
    let healths: Vec<_> = sorted.iter().map(|n| n.health).collect();
    for n in &sorted {
        println!(
            "  {}  {}  {}",
            n.id,
            dag_board::stage_cell(&n.stage),
            dag_board::glyph(n.health)
        );
    }

    let spec_health = circuit::cockpit::health::rollup_children(&healths);
    let trace = circuit::cockpit::rollup::traceability(&git, &nodes, &base);
    let m = trace.merged.map(|m| m.to_string()).unwrap_or_else(|| "?".to_string());
    println!("\nSpec health: {}", dag_board::glyph(spec_health));
    println!("Tasks: {}/{} done", m, trace.total);
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
