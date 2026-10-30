//! `conduit run`: evaluate a project, feed GUI windows, and re-run whenever a node file changes.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};

use conduit_core::{Registry, evaluate, load};
use notify_debouncer_mini::new_debouncer;
use notify_debouncer_mini::notify::RecursiveMode;

use crate::gui::{GuiProcesses, preview_program};

const DEBOUNCE: Duration = Duration::from_millis(150);

pub fn run(dir: &Path, registry: &Registry) -> anyhow::Result<()> {
    let mut gui = GuiProcesses::new(preview_program(), dir.join("out"));

    let (tx, rx) = mpsc::channel();
    let mut debouncer = new_debouncer(DEBOUNCE, tx)?;
    debouncer
        .watcher()
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|e| anyhow::anyhow!("cannot watch {}: {e}", dir.display()))?;
    println!(
        "watching {} — edit a .toml file to re-render, Ctrl-C to stop",
        dir.display()
    );

    let mut last = snapshot(dir);
    render(dir, registry, &mut gui);
    for result in rx {
        match result {
            Ok(events) if events.iter().any(|e| is_node_file(&e.path)) => {
                // The watcher also reports our own reads of the files, so only re-render
                // when something about them actually changed.
                let current = snapshot(dir);
                if current != last {
                    last = current;
                    render(dir, registry, &mut gui);
                }
            }
            Ok(_) => {}
            Err(e) => eprintln!("watch error: {e}"),
        }
    }
    Ok(())
}

/// Size and modification time of every node file. Uses metadata only: reading the
/// files would itself generate watch events.
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, (u64, Option<SystemTime>)> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return BTreeMap::new();
    };
    entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| is_node_file(p))
        .filter_map(|p| {
            let meta = std::fs::metadata(&p).ok()?;
            Some((p, (meta.len(), meta.modified().ok())))
        })
        .collect()
}

fn render(dir: &Path, registry: &Registry, gui: &mut GuiProcesses) {
    let start = Instant::now();
    let graph = match load(dir, registry) {
        Ok(graph) => graph,
        Err(errors) => return report_error(gui, &errors.to_string()),
    };
    gui.sync(&graph, registry);
    match evaluate(&graph, registry, gui) {
        Ok(_) => println!(
            "rendered {} node(s) in {} ms",
            graph.nodes.len(),
            start.elapsed().as_millis()
        ),
        Err(e) => report_error(gui, &e.to_string()),
    }
}

fn report_error(gui: &mut GuiProcesses, message: &str) {
    eprintln!("{message}");
    gui.show_error(message);
}

/// Only node files trigger a re-render, not editor swap files or saved PNGs.
fn is_node_file(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "toml")
}
