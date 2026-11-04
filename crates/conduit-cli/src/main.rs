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
    /// List the installed font families, for the `text` node's `font` param.
    Fonts,
    /// Work with palette nodes.
    #[command(subcommand)]
    Palette(PaletteCommand),
}

#[derive(Subcommand)]
enum PaletteCommand {
    /// Create (or replace the colors of) a palette node from a Coolors link, hex codes,
    /// a Coolors CSS/JSON export, or a .hex / .gpl file.
    Import {
        /// Project folder.
        dir: PathBuf,
        /// Id of the palette node (its file is `<id>.toml`).
        id: String,
        /// A link, hex codes, or a file path.
        #[arg(required = true)]
        source: Vec<String>,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    let registry = conduit_nodes::registry();
    match cli.command {
        Command::Run { dir } => exit_code(run::run(&dir, Arc::new(registry))),
        Command::Edit { dir } => exit_code(edit::edit(&dir, Arc::new(registry))),
        Command::Check { dir } => check(&dir, &registry),
        Command::Nodes => {
            print_nodes(&registry);
            ExitCode::SUCCESS
        }
        Command::Fonts => {
            use std::io::Write;
            let mut out = std::io::stdout().lock();
            // Stop quietly if the reader goes away (e.g. `| head`).
            let _ = conduit_nodes::text::font_families()
                .iter()
                .try_for_each(|family| writeln!(out, "{}", family.name))
                .and_then(|()| writeln!(out, "(also: serif, sans-serif, monospace)"));
            ExitCode::SUCCESS
        }
        Command::Palette(PaletteCommand::Import { dir, id, source }) => {
            exit_code(import_palette(&dir, &id, &source.join(" "), &registry))
        }
    }
}

/// `SUCCESS`, or `FAILURE` after printing the error (with its causes).
fn exit_code(result: Result<(), impl std::fmt::Display>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

fn import_palette(dir: &Path, id: &str, source: &str, registry: &Registry) -> Result<(), String> {
    let path = Path::new(source);
    let (text, note) = if path.is_file() {
        let text = std::fs::read_to_string(path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        (text, Some(source.to_string()))
    } else {
        let link = source.contains("://") || source.contains("coolors.co/");
        (source.to_string(), link.then(|| source.trim().to_string()))
    };
    let colors = conduit_core::parse_palette(&text)?;
    let spec = registry.spec("palette").expect("built-in");
    let entries: Vec<(String, toml::Value)> = colors
        .iter()
        .map(|(name, c)| (name.clone(), conduit_core::to_hex(*c).into()))
        .collect();
    let extra: Vec<(&str, toml::Value)> = note.map(|n| ("source", n.into())).into_iter().collect();
    conduit_core::edit::set_entries(dir, id, spec, "colors", &entries, &extra)
        .map_err(|e| e.to_string())?;
    for (name, hex) in &entries {
        println!("{name:<16} {}", hex.as_str().unwrap_or_default());
    }
    println!("wrote {}", dir.join(format!("{id}.toml")).display());
    Ok(())
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
            .spec(&node.kind)
            .expect("loaded with this registry");
        let outputs = spec.outputs_for(&node.params);
        if outputs.is_empty() {
            println!("{:<24} {}", node.id, node.kind);
        }
        for (port, _) in outputs {
            let key = conduit_core::Reference {
                node: node.id.clone(),
                port,
            };
            let summary = values.get(&key).map(describe).unwrap_or_default();
            println!("{:<24} {:<12} {summary}", key.to_string(), node.kind);
        }
    }
}

fn describe(value: &Value) -> String {
    match value {
        Value::Image(img) => format!("image {}x{}", img.width(), img.height()),
        Value::Layer(l) => format!("layer {l}"),
        Value::Number(n) => format!("number {n}"),
        Value::Color(c) => format!("color {}", conduit_core::to_hex(*c)),
        Value::Font(font) => match conduit_nodes::font::families(font) {
            Ok(families) => format!("font {} ({})", font.name, families.join(", ")),
            Err(_) => format!("font {}", font.name),
        },
    }
}

fn print_nodes(registry: &Registry) {
    for spec in registry.specs() {
        let gui = if spec.gui { "  (gui)" } else { "" };
        println!("{}{gui}", spec.kind);
        for p in spec.inputs.iter().filter(|p| !p.param) {
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
            if p.entries {
                println!("  out  <name>     {} (one per entry of `{}`)", p.ty, p.name);
                continue;
            }
            if !p.repeat.is_empty() {
                let name = format!("{0}1..{0}N", p.name);
                let per: Vec<String> = p
                    .repeat
                    .iter()
                    .map(|r| match spec.param(r).map(|p| &p.kind) {
                        Some(conduit_core::ParamKind::Text) => format!("characters in `{r}`"),
                        _ => format!("`{r}`"),
                    })
                    .collect();
                let grouped = if p.grouped { ", grouped" } else { "" };
                println!(
                    "  out  {name:<10} {} (N = {}{grouped})",
                    p.ty,
                    per.join(" × ")
                );
                continue;
            }
            println!("  out  {:<10} {}", p.name, p.ty);
        }
        for p in &spec.params {
            let linkable = spec.input(p.name).is_some_and(|i| i.param);
            let need = match p.default {
                _ if p.required && linkable => " (required, linkable)".to_string(),
                _ if p.required => " (required)".to_string(),
                Some(d) if linkable => format!(" (default {d}, linkable)"),
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
