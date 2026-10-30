//! Launching and feeding GUI node processes (`conduit-preview`).

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;

use conduit_core::{Graph, GuiHost, Registry, RgbaImage};

/// Overrides the GUI program; used by tests and handy for trying alternative previewers.
pub const PROGRAM_ENV: &str = "CONDUIT_PREVIEW_BIN";

/// Finds `conduit-preview`: `$CONDUIT_PREVIEW_BIN`, else next to this executable, else `$PATH`.
pub fn preview_program() -> PathBuf {
    if let Some(p) = std::env::var_os(PROGRAM_ENV) {
        return p.into();
    }
    let sibling = std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join("conduit-preview")))
        .filter(|p| p.is_file());
    sibling.unwrap_or_else(|| PathBuf::from("conduit-preview"))
}

struct GuiChild {
    child: Child,
    stdin: ChildStdin,
}

/// One child process per GUI node, spawned on first use and respawned after the user
/// closes its window. Implements [`GuiHost`] so GUI nodes can send it frames.
pub struct GuiProcesses {
    program: PathBuf,
    save_dir: PathBuf,
    /// Window title for each GUI node in the current graph.
    titles: HashMap<String, String>,
    children: HashMap<String, GuiChild>,
}

impl GuiProcesses {
    pub fn new(program: PathBuf, save_dir: PathBuf) -> Self {
        Self {
            program,
            save_dir,
            titles: HashMap::new(),
            children: HashMap::new(),
        }
    }

    /// Records the GUI nodes in a freshly loaded graph and closes windows whose node is gone.
    pub fn sync(&mut self, graph: &Graph, registry: &Registry) {
        self.titles = graph
            .nodes
            .iter()
            .filter(|n| registry.get(&n.kind).is_some_and(|k| k.spec().gui))
            .map(|n| {
                let title = n
                    .params
                    .get("title")
                    .and_then(|t| t.as_str())
                    .unwrap_or(&n.id);
                (n.id.clone(), title.to_string())
            })
            .collect();
        self.children.retain(|id, _| self.titles.contains_key(id));
    }

    /// Shows an error in every open window (they keep their last good image).
    pub fn show_error(&mut self, message: &str) {
        let mut failed = Vec::new();
        for (id, gui) in &mut self.children {
            if conduit_ipc::write_error(&mut gui.stdin, message).is_err() {
                failed.push(id.clone());
            }
        }
        for id in failed {
            self.children.remove(&id);
        }
    }

    /// Returns a live process for `node_id`, spawning one if needed.
    fn ensure(&mut self, node_id: &str) -> Option<&mut GuiChild> {
        let alive = self
            .children
            .get_mut(node_id)
            .is_some_and(|g| matches!(g.child.try_wait(), Ok(None)));
        if !alive {
            self.children.remove(node_id);
            let gui = self.spawn(node_id)?;
            self.children.insert(node_id.to_string(), gui);
        }
        self.children.get_mut(node_id)
    }

    fn spawn(&self, node_id: &str) -> Option<GuiChild> {
        let title = self.titles.get(node_id).map_or(node_id, String::as_str);
        let args: [OsString; 6] = [
            "--node-id".into(),
            node_id.into(),
            "--title".into(),
            title.into(),
            "--save-dir".into(),
            self.save_dir.clone().into(),
        ];
        let mut child = match Command::new(&self.program)
            .args(&args)
            .stdin(Stdio::piped())
            .spawn()
        {
            Ok(child) => child,
            Err(e) => {
                eprintln!(
                    "cannot start {}: {e}\n  (build it with `cargo build --workspace`, or set {PROGRAM_ENV})",
                    self.program.display()
                );
                return None;
            }
        };
        let stdin = child.stdin.take().expect("stdin was piped");
        Some(GuiChild { child, stdin })
    }
}

impl GuiHost for GuiProcesses {
    fn show(&mut self, node_id: &str, image: Arc<RgbaImage>) {
        let Some(gui) = self.ensure(node_id) else {
            return;
        };
        let sent = conduit_ipc::write_frame(
            &mut gui.stdin,
            image.width(),
            image.height(),
            image.as_raw(),
        );
        if sent.is_err() {
            // Window was closed while we were writing; it will be reopened on the next change.
            self.children.remove(node_id);
        }
    }
}

impl Drop for GuiProcesses {
    fn drop(&mut self) {
        // Closing stdin makes each window exit.
        for (_, gui) in self.children.drain() {
            let GuiChild { mut child, stdin } = gui;
            drop(stdin);
            let _ = child.wait();
        }
    }
}
