//! `conduit` — command-line runner for Conduit Studio projects.

mod edit;
mod gui;
mod run;

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use clap::{Parser, Subcommand};
use conduit_core::{Graph, NullHost, Registry, Value, evaluate, load};

#[derive(Parser)]
#[command(name = "conduit", version, about = "Run Conduit Studio node graphs")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Render a project, show its preview windows, and re-render on every file change.
    Run {
        /// Project folder containing one `<node_id>.toml` per node.
        dir: PathBuf,
    },
    /// Explore a project in a terminal UI while it renders (keyboard-driven).
    Edit {
        /// Project folder containing one `<node_id>.toml` per node.
        dir: PathBuf,
    },
    /// Load and evaluate a project without opening any windows; report problems.
    Check {
        /// Project folder containing one `<node_id>.toml` per node.
        dir: PathBuf,
    },
    /// List available node types and their ports.
    Nodes,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let registry = conduit_nodes::registry();
    match cli.command {
        Command::Run { dir } => match run::run(&dir, Arc::new(registry)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:#}");
                ExitCode::FAILURE
            }
        },
        Command::Edit { dir } => match edit::edit(&dir, Arc::new(registry)) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:#}");
                ExitCode::FAILURE
            }
        },
        Command::Check { dir } => check(&dir, &registry),
        Command::Nodes => {
            print_nodes(&registry);
            ExitCode::SUCCESS
        }
    }
}

fn check(dir: &Path, registry: &Registry) -> ExitCode {
    let graph = match load(dir, registry) {
        Ok(graph) => graph,
        Err(errors) => {
            eprintln!("{errors}");
            eprintln!("{} problem(s) found", errors.0.len());
            return ExitCode::FAILURE;
        }
    };
    match evaluate(&graph, registry, &mut NullHost) {
        Ok(values) => {
            print_outputs(&graph, registry, &values);
            println!("ok: {} node(s)", graph.nodes.len());
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn print_outputs(graph: &Graph, registry: &Registry, values: &conduit_core::Values) {
    for node in &graph.nodes {
        let spec = registry
            .get(&node.kind)
            .expect("loaded with this registry")
            .spec();
        if spec.outputs.is_empty() {
            println!("{:<24} {}", node.id, node.kind);
        }
        for out in &spec.outputs {
            let key = conduit_core::Reference {
                node: node.id.clone(),
                port: out.name.to_string(),
            };
            let summary = values.get(&key).map(describe).unwrap_or_default();
            println!("{:<24} {:<12} {summary}", key.to_string(), node.kind);
        }
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Image(img) => format!("image {}x{}", img.width(), img.height()),
        Value::Layer(l) => {
            format!(
                "layer {}x{} at {},{}",
                l.image.width(),
                l.image.height(),
                l.x,
                l.y
            )
        }
        Value::Number(n) => format!("number {n}"),
        Value::Color(c) => format!("color {:?}", c.0),
    }
}

fn print_nodes(registry: &Registry) {
    for spec in registry.specs() {
        let gui = if spec.gui { "  (gui)" } else { "" };
        println!("{}{gui}", spec.kind);
        for p in &spec.inputs {
            let mut flags = Vec::new();
            if p.multi {
                flags.push("list");
            }
            if p.optional {
                flags.push("optional");
            }
            let flags = if flags.is_empty() {
                String::new()
            } else {
                format!(" ({})", flags.join(", "))
            };
            println!("  in   {:<10} {}{flags}", p.name, p.ty);
        }
        for p in &spec.outputs {
            println!("  out  {:<10} {}", p.name, p.ty);
        }
        for p in &spec.params {
            let need = match p.default {
                _ if p.required => " (required)".to_string(),
                Some(d) => format!(" (default {d})"),
                None => String::new(),
            };
            let doc = if p.doc.is_empty() {
                String::new()
            } else {
                format!("  — {}", p.doc)
            };
            println!("  par  {:<10} {}{need}{doc}", p.name, p.kind);
        }
    }
}
