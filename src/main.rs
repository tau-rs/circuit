use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::adapters::checkpoints::Checkpoints;
use circuit::adapters::delivery::{self, DeliveryMode};
use circuit::adapters::forge::Forge;
use circuit::adapters::git::Git;
use circuit::cockpit::health::Health;
use circuit::dag::{validate, DagError};
use circuit::flow::facts::DeliveryFacts;
use circuit::flow::rail::render_rail;
use circuit::flow::stage::derive_stage;
use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
use circuit::model::local::resolve_worktree_dir;
use circuit::model::node::DagNode;
use circuit::model::spec::SpecRecord;
use circuit::model::store::Workspace;
use circuit::ports::{CheckpointStore, ForgePort, GitPort};
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

fn run_session(command: SessionCommand) -> Result<()> {
    match command {
        SessionCommand::Spawn { dag_node, path } => run_session_spawn(&dag_node, &path),
    }
}

fn run_session_spawn(dag_node: &str, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let node = ws
        .load_dag_node(dag_node)
        .with_context(|| format!("loading dag node {dag_node}"))?;
    let config = ws.load_config().context("loading config.toml")?;
    let base = &config.base_branch;

    let git = Git::new(ws.root());

    // Refuse to clobber an existing branch (a session may already own it).
    if git
        .branch_facts(&node.branch, base)
        .with_context(|| format!("checking branch {}", node.branch))?
        .exists
    {
        anyhow::bail!(
            "branch {} already exists — refusing to spawn over it",
            node.branch
        );
    }

    // 1. Allocate identity and write the authored record (parent = node.spec).
    //    Record-first per §4/§7.1 (identity precedes the branch). If a later
    //    git step fails, the branch-less record persists; delete the .toml (or
    //    clean up the branch) and re-run — no automatic rollback in M2.
    let id = SessionId::generate();
    let record = SessionRecord::impl_(id, node.spec.clone(), node.id.clone(), node.branch.clone());
    ws.save_session(&record)
        .with_context(|| format!("writing session {id}"))?;

    // 2. Resolve the (machine-local, never-stored) worktree path.
    let local = ws.load_local().context("loading local.toml")?;
    let env = std::env::var("CIRCUIT_WORKTREES_DIR").ok();
    let worktree = resolve_worktree_dir(env.as_deref(), &local, ws.root(), &id.to_string());

    // 3. Create the branch + worktree.
    git.create_branch(&node.branch, base)
        .with_context(|| format!("creating branch {}", node.branch))?;
    git.add_worktree(&node.branch, &worktree)
        .with_context(|| format!("adding worktree at {}", worktree.display()))?;

    println!("Spawned session {id} for node {} (stage: Project)", node.id);
    println!("  branch:   {}", node.branch);
    println!("  worktree: {}", worktree.display());
    Ok(())
}

/// Is the `gh` CLI installed and runnable? (Auth is checked per-call via the
/// review_state error path; this only gates which source we consult.)
fn gh_available() -> bool {
    std::process::Command::new("gh")
        .arg("--version")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Does the repo at `root` have a remote pointing at github.com?
fn has_github_remote(root: &Path) -> bool {
    std::process::Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["remote", "-v"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("github.com"))
        .unwrap_or(false)
}

/// Render the rail for one session (by ULID, else by unique DAG-node name) or,
/// when `selector` is `None`, every session in the workspace.
fn run_flow(selector: Option<&str>, path: &Path) -> Result<()> {
    let ws = Workspace::new(path);
    require_initialized(&ws)?;

    let sessions = match selector {
        Some(sel) => vec![resolve_session(&ws, sel)?],
        None => ws.list_sessions().context("listing sessions")?,
    };

    if sessions.is_empty() {
        println!("No sessions yet.");
        return Ok(());
    }

    let config = ws.load_config().context("loading config.toml")?;
    let git = Git::new(ws.root());
    let mode = delivery::resolve(gh_available(), has_github_remote(ws.root()));
    let forge = Forge::new(ws.root());
    let checkpoints = Checkpoints::new(ws.root());

    let mut blocks = Vec::new();
    for s in &sessions {
        let branch_facts = match &s.branch {
            Some(b) => git
                .branch_facts(b, &config.base_branch)
                .with_context(|| format!("deriving facts for {b}"))?,
            None => Default::default(),
        };
        // Resolve real review state from the selected source. Any adapter Err
        // (forge unreachable, unreadable checkpoint) degrades to `None` —
        // the honest "undeterminable" path that renders `PR ?` (§7.2).
        let review = match (&s.branch, mode) {
            (Some(b), DeliveryMode::Forge) => forge.review_state(b).ok(),
            (Some(_), DeliveryMode::Local) => {
                checkpoints.review_state(&s.id.to_string()).ok()
            }
            (None, _) => None,
        };
        let facts = DeliveryFacts {
            branch: branch_facts,
            review,
        };
        let view = derive_stage(s, &facts);
        // Label by DAG node when present (impl/fix), else by session id (spec).
        let label = s.dag_node.clone().unwrap_or_else(|| s.id.to_string());
        blocks.push(render_rail(
            &label,
            s.kind,
            view,
            s.branch.as_deref(),
            &facts.branch,
            facts.review,
            Health::Unknown,
            s.is_archived(),
        ));
    }
    println!("{}", blocks.join("\n\n"));
    Ok(())
}

/// Resolve a session selector: first as a ULID, then as a unique DAG-node name.
fn resolve_session(ws: &Workspace, selector: &str) -> Result<SessionRecord> {
    // Exact ULID match.
    if selector.parse::<SessionId>().is_ok() {
        if let Ok(s) = ws.load_session(selector) {
            return Ok(s);
        }
    }
    // Fall back to a unique DAG-node-name match.
    let all = ws.list_sessions().context("listing sessions")?;
    let mut matches: Vec<SessionRecord> = all
        .into_iter()
        .filter(|s| s.dag_node.as_deref() == Some(selector))
        .collect();
    match matches.len() {
        1 => Ok(matches.pop().unwrap()),
        0 => anyhow::bail!(
            "no session matches `{selector}` (not a known session id or DAG-node name)"
        ),
        n => anyhow::bail!(
            "`{selector}` matches {n} sessions — pass the session id (ULID) to disambiguate"
        ),
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

    // The real git adapter now backs the board, replacing the no-op stub the
    // board slice shipped before this adapter landed. NOTE: node_health and
    // traceability each query git per node, so this shells out per node — fine
    // for M2 board sizes, batchable later if it becomes a cost.
    let git = Git::new(ws.root());

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
                let facts = DeliveryFacts {
                    branch,
                    review: None,
                };
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
    let m = trace
        .merged
        .map(|count| count.to_string())
        .unwrap_or_else(|| "?".to_string());
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
