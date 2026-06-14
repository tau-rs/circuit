use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "circuit", about = "Architecture derivation & visualization")]
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
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Analyze { path } => {
            let graph = circuit::builder::build_graph(&path)?;
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
        }
    }
    Ok(())
}
