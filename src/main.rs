use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use circuit::model::config::Config;
use circuit::model::glossary::Glossary;
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

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { path } => run_analyze(&path),
        Command::Init { path } => run_init(&path),
        Command::Spec { command } => run_spec(command),
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

fn run_spec(command: SpecCommand) -> Result<()> {
    match command {
        SpecCommand::New { id, title, intent, contexts, path } => {
            let ws = Workspace::new(&path);
            let mut spec = SpecRecord::new(&id, title, intent);
            spec.bounded_contexts = contexts;
            ws.save_spec(&spec).with_context(|| format!("writing spec {id}"))?;
            println!("Created spec session: {id}");
            Ok(())
        }
    }
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
