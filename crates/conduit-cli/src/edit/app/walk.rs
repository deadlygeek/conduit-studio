//! The flow view's focus: walking it, `f`, and what the status line and action bar show
//! for it (spec §4.4, §6.1, §6.3, §6.4, §6.6).

use conduit_core::{Project, Reference};
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::{Action, App, Mode};
use crate::edit::flow::{
    Flow, Focus, InState, Link, Move, input_rows, is_flow_input, is_flow_output,
};
use crate::run::{self, Event};

/// One entry of the action bar, e.g. `(e)dit`: keys in round or square brackets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BarItem {
    pub text: String,
    /// The action `⏎` does, shown bold (§6.3).
    pub primary: bool,
}

/// The action bar's contents (§6.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub title: &'static str,
    pub actions: Vec<BarItem>,
    /// Between actions: one space for focus actions, two in a popup's bar.
    pub separator: &'static str,
    /// The global keys' line; `None` while a popup is open.
    pub globals: Option<Vec<&'static str>>,
}

const GLOBALS: &[&str] = &[
    "[a]dd", "[r]ename", "[D]elete", "[u]ndo", "[p]robe", "[/]find", "[?]help", "[q]uit",
];
const GLOBALS_EMPTY: &[&str] = &["[a]dd", "[u]ndo", "[/]find", "[?]help", "[q]uit"];
const GLOBALS_MOVING: &[&str] = &["[/]find", "[?]help", "[q]uit"];

pub(super) fn item(text: &str) -> BarItem {
    BarItem {
        text: text.to_string(),
        primary: false,
    }
}

pub(super) fn primary(text: &str) -> BarItem {
    BarItem {
        text: text.to_string(),
        primary: true,
    }
}

fn popup(title: &'static str, items: &[&str]) -> Bar {
    Bar {
        title,
        actions: items.iter().map(|t| item(t)).collect(),
        separator: "  ",
        globals: None,
    }
}

/// Whether `link` is still a connection in `project`.
fn link_exists(project: &Project, link: &Link) -> bool {
    project
        .get(&link.target)
        .and_then(|n| n.inputs.get(&link.port))
        .and_then(|refs| refs.get(link.index))
        .is_some_and(|r| r.node == link.source && r.port == link.output)
}

/// `layers[1]` if `node.port` is a list (or has several connections), else `port`.
fn input_label(project: &Project, node: &str, port: &str, index: usize) -> String {
    let multi = project.get(node).is_some_and(|n| {
        n.spec
            .as_ref()
            .and_then(|s| s.input(port))
            .is_some_and(|p| p.multi)
            || n.inputs.get(port).is_some_and(|r| r.len() > 1)
    });
    if multi {
        format!("{port}[{index}]")
    } else {
        port.to_string()
    }
}

impl App {
    /// Keeps the flow and its focus in step with `self.focus` and the project, after a
    /// key or a reload. A focus on another node (from `/`, `a`, `[`, …) rebuilds the flow
    /// around it (§4.2); an edit keeps the flow's path if its conduits all still exist,
    /// else rebuilds it (§6.8).
    pub(in crate::edit) fn sync_flow(&mut self) {
        let Some(id) = self.focus.clone() else {
            self.flow = Flow::default();
            self.flow_focus = None;
            return;
        };
        let on_flow = self.flow_focus.as_ref().is_some_and(|f| f.node() == id)
            && self.flow.position(&id).is_some();
        if !on_flow {
            // A jump (`/`, `a`, …) is a flow change; an edit records its own (`pending`).
            if self.pending.is_none()
                && let Some(old) = self.flow_focus.clone()
            {
                let flow = std::mem::take(&mut self.flow);
                self.remember_flow(flow, old);
            }
            self.flow = Flow::build(&self.project, &id);
            self.flow_focus = Some(Focus::Node(id));
            self.recentre = true;
            return;
        }
        let kept = self
            .flow
            .nodes
            .iter()
            .all(|n| self.project.get(n).is_some())
            && self
                .flow
                .links
                .iter()
                .all(|l| link_exists(&self.project, l));
        if kept {
            self.flow.extend(&self.project);
        } else {
            self.flow = Flow::build(&self.project, &id);
            self.recentre = true;
        }
        let focus = self.flow_focus.take().expect("checked above");
        self.flow_focus = Some(
            self.flow
                .locate(&self.project, &focus)
                .unwrap_or(Focus::Node(id)),
        );
    }

    /// Moves the flow focus, keeping `self.focus` on its node (§6.2).
    fn set_flow_focus(&mut self, focus: Focus) -> Option<Action> {
        let moved = self.focus.as_deref() != Some(focus.node());
        self.focus = Some(focus.node().to_string());
        self.flow_focus = Some(focus);
        if moved {
            self.refresh();
            return self.follow_probe();
        }
        None
    }

    /// Flow-view keys for the focus. `None` if the key isn't one of them.
    pub(super) fn flow_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        let focus = self.flow_focus.clone()?;
        let mv = match key.code {
            KeyCode::Char('h') | KeyCode::Left => Move::Upstream,
            KeyCode::Char('l') | KeyCode::Right => Move::Downstream,
            KeyCode::Char('j') | KeyCode::Down => Move::Next,
            KeyCode::Char('k') | KeyCode::Up => Move::Previous,
            KeyCode::Esc => Move::ToNode,
            KeyCode::Char('g') | KeyCode::Home => Move::First,
            KeyCode::Char('G') | KeyCode::End => Move::Last,
            KeyCode::Char('f') => return Some(self.bring_in(&focus)),
            KeyCode::Char('[') | KeyCode::Backspace if self.carrying.is_none() => {
                self.flow_history(true);
                return Some(None);
            }
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.flow_history(true);
                return Some(None);
            }
            KeyCode::Char(']') if self.carrying.is_none() => {
                self.flow_history(false);
                return Some(None);
            }
            KeyCode::Char('p') if self.carrying.is_none() => {
                return Some(self.probe(self.flow_probe_target()));
            }
            _ => return self.flow_verb(key, focus),
        };
        Some(
            self.flow
                .step(&self.project, &focus, mv)
                .and_then(|f| self.set_flow_focus(f)),
        )
    }

    /// Global keys the flow view leaves to the editor's own handlers (§6.2); `r`, `D`,
    /// `p` and history are handled with the focus.
    pub(super) fn is_flow_global(key: &KeyEvent) -> bool {
        matches!(
            key.code,
            KeyCode::Char('q' | 'v' | '?' | '/' | 'a' | 'u' | 'P' | 'E')
        )
    }

    /// The global keys that still work while a node is picked up (§6.5).
    pub(super) fn is_moving_global(key: &KeyEvent) -> bool {
        matches!(key.code, KeyCode::Char('q' | '?' | '/'))
    }

    /// What `p` probes (§6.2): a node's first output, the source feeding a conduit or
    /// input end, or an output end's output.
    fn flow_probe_target(&self) -> Option<Reference> {
        match self.flow_focus.as_ref()? {
            Focus::Node(id) => self.default_probe(id),
            Focus::Conduit(link) => Some(Reference {
                node: link.source.clone(),
                port: link.output.clone(),
            }),
            Focus::In { row, node } => match &row.state {
                InState::Connected(source) => Some(source.clone()),
                _ => self.default_probe(node),
            },
            Focus::Out { node, row } => Some(Reference {
                node: node.clone(),
                port: row.port.clone(),
            }),
        }
    }

    /// The flow view's status line (§4.7): the last message or render result, else the
    /// focus's full names; `⇄ moving …` first and the error count last.
    pub fn flow_status(&self) -> String {
        let mut parts = Vec::new();
        if let Some(id) = &self.carrying {
            parts.push(format!("⇄ moving {id} ({})", self.kind(id)));
        }
        let main = if let Some(message) = &self.message {
            message.clone()
        } else if self.show_render
            && let Some(status) = &self.status
        {
            match status {
                Event::Rendered { stats, elapsed, .. } => run::describe_render(stats, *elapsed),
                Event::Failed(m) => format!("✗ {}", m.lines().next().unwrap_or_default()),
                Event::Warning(m) => format!("! {}", m.lines().next().unwrap_or_default()),
                Event::FilesChanged => String::new(),
            }
        } else {
            self.focus_text()
        };
        if !main.is_empty() {
            parts.push(main);
        }
        let errors = self.project.errors.len();
        if errors > 0 {
            let s = if errors == 1 { "" } else { "s" };
            parts.push(format!("{errors} error{s}"));
        }
        parts.join("  ·  ")
    }

    fn kind(&self, id: &str) -> String {
        self.project
            .get(id)
            .and_then(|n| n.kind.clone())
            .unwrap_or_else(|| "?".into())
    }

    /// The focus's full names, e.g. `comp.layers[0] ← logo_at` (§5.3).
    fn focus_text(&self) -> String {
        let Some(focus) = &self.flow_focus else {
            return String::new();
        };
        if let Some(text) = self.edit_focus_text() {
            return text;
        }
        let p = &self.project;
        match focus {
            Focus::Node(id) => match p.errors_for(id).first() {
                Some(e) => {
                    let text = e.to_string();
                    let prefix = e.path().map(|path| format!("{}: ", path.display()));
                    let text = prefix
                        .and_then(|pre| text.strip_prefix(&pre).map(str::to_string))
                        .unwrap_or(text);
                    format!("✕ {text}")
                }
                None => format!("{id} ({})", self.kind(id)),
            },
            Focus::Conduit(l) => format!(
                "{}.{} → {}.{}",
                l.source,
                l.output,
                l.target,
                input_label(p, &l.target, &l.port, l.index)
            ),
            Focus::In { node, row } => match &row.state {
                InState::Connected(source) => {
                    format!("{node}.{} ← {}", row.label(), source.short())
                }
                InState::Slot => {
                    let entries = p
                        .get(node)
                        .and_then(|n| n.inputs.get(&row.port))
                        .map_or(0, Vec::len);
                    if entries == 0 {
                        format!("{node}.{}: no {} (optional)", row.port, row.port)
                    } else {
                        format!("{node}.{}: append another", row.port)
                    }
                }
                InState::Optional => format!("{node}.{}: not connected (optional)", row.port),
                InState::Missing => format!("✕ {node}.{} is not connected", row.port),
            },
            Focus::Out { node, row } => match &row.user {
                Some(u) => format!(
                    "{node}.{} → {}.{}",
                    row.port,
                    u.node,
                    input_label(p, &u.node, &u.port, u.index)
                ),
                None => format!("{node}.{}: no users", row.port),
            },
        }
    }

    /// The action bar for the current mode and focus (§6.3, §6.4, §6.5, §6.6).
    pub fn bar(&self) -> Bar {
        match &self.mode {
            Mode::Pick { .. } => {
                return popup(
                    "Picker Actions",
                    &["type to filter", "(↑/↓)choose", "(⏎)pick", "(esc)cancel"],
                );
            }
            Mode::Prompt { .. } => {
                return popup(
                    "Prompt Actions",
                    &[
                        "type to replace",
                        "(→)keep and edit",
                        "(⏎)accept",
                        "(esc)cancel",
                    ],
                );
            }
            Mode::Confirm { .. } => {
                return popup("Confirm Actions", &["(y)es", "any other key cancels"]);
            }
            Mode::Color(_) => {
                return Bar {
                    separator: " ",
                    ..popup(
                        "Colour Editor Actions",
                        &[
                            "(j/k)channel",
                            "(+ - > < } {)nudge",
                            "(tab)RGB",
                            "(#)hex",
                            "(⏎)keep",
                            "(esc)revert",
                        ],
                    )
                };
            }
            Mode::Help => return popup("Help Actions", &["any key closes"]),
            Mode::Normal => {}
        }
        let globals = |keys: &[&'static str]| Some(keys.to_vec());
        let Some(focus) = &self.flow_focus else {
            return Bar {
                title: "Project Actions",
                actions: Vec::new(),
                separator: " ",
                globals: globals(GLOBALS_EMPTY),
            };
        };
        if self.carrying.is_some() {
            let mut actions = Vec::new();
            if !matches!(focus, Focus::Node(_)) {
                actions.push(primary("(m)drop here"));
            }
            actions.push(item("(esc)cancel"));
            return Bar {
                title: "Move Actions",
                actions,
                separator: " ",
                globals: globals(GLOBALS_MOVING),
            };
        }
        let (title, mut actions) = self
            .edit_actions()
            .unwrap_or_else(|| self.focus_actions(focus));
        // Following the conduit to the node at its other end (§7.2).
        if let (Some(row), Some(id)) = (&self.edit_view, &self.focus) {
            if let Some((target, _)) = self.across(id, row, true) {
                actions.push(item(&format!("(h)edit {target}")));
            }
            if let Some((target, _)) = self.across(id, row, false) {
                actions.push(item(&format!("(l)edit {target}")));
            }
        }
        // A palette's import works from any row of its edit view (§7.1). A colour row's
        // bar is full without it, so it's listed on the others.
        if self.edit_view.is_some() && self.entries_param().is_some() && title != "Colour Actions" {
            actions.push(item("(I)mport"));
        }
        Bar {
            title,
            actions,
            separator: " ",
            globals: globals(GLOBALS),
        }
    }

    /// The bar's title and focus actions (§6.3); actions that can't apply are left out.
    fn focus_actions(&self, focus: &Focus) -> (&'static str, Vec<BarItem>) {
        let p = &self.project;
        let i = self.flow.position(focus.node()).unwrap_or(0);
        match focus {
            Focus::Node(id) => {
                let mut actions = vec![primary("(e)dit")];
                let node = p.get(id);
                let fed = node.is_some_and(|n| {
                    input_rows(n)
                        .iter()
                        .any(|r| matches!(r.state, InState::Connected(_)))
                });
                let output = node.is_some_and(|n| !n.outputs().is_empty());
                if fed && output {
                    actions.push(item("(m)ove"));
                }
                // Only slides that would work (§6.5); a refused one says why when pressed.
                let up = self.slide_plan(id, true).is_ok();
                let down = self.slide_plan(id, false).is_ok();
                match (up, down) {
                    (true, true) => actions.push(item("(H)/(L)slide")),
                    (true, false) => actions.push(item("(H)slide")),
                    (false, true) => actions.push(item("(L)slide")),
                    (false, false) => {}
                }
                actions.push(item("(d)issolve"));
                ("Node Actions", actions)
            }
            Focus::Conduit(_) => (
                "Conduit Actions",
                vec![primary("(c)onnect"), item("(i)nsert"), item("(d)isconnect")],
            ),
            Focus::In { node, row } => {
                if !matches!(row.state, InState::Connected(_)) {
                    return (
                        "Input Actions",
                        vec![primary("(c)onnect"), item("(i)nsert")],
                    );
                }
                let off = !is_flow_input(&self.flow, i, row);
                let mut actions = Vec::new();
                if off {
                    actions.push(primary("(f)ocus"));
                }
                actions.push(BarItem {
                    text: "(c)onnect".into(),
                    primary: !off,
                });
                actions.push(item("(i)nsert"));
                actions.push(item("(d)isconnect"));
                if let Some(k) = row.index {
                    let entries = p
                        .get(node)
                        .and_then(|n| n.inputs.get(&row.port))
                        .map_or(0, Vec::len);
                    if k > 0 {
                        actions.push(item("(K)earlier"));
                    }
                    if k + 1 < entries {
                        actions.push(item("(J)later"));
                    }
                }
                ("Conduit End Actions", actions)
            }
            Focus::Out { row, .. } => {
                if row.user.is_none() {
                    return ("Output Actions", vec![primary("(c)onnect to")]);
                }
                let off = !is_flow_output(&self.flow, i, row);
                let mut actions = Vec::new();
                if off {
                    actions.push(primary("(f)ocus"));
                }
                actions.push(BarItem {
                    text: "(c)onnect to".into(),
                    primary: !off,
                });
                actions.push(item("(i)nsert"));
                actions.push(item("(d)isconnect"));
                ("Conduit End Actions", actions)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;

    use conduit_core::inspect;
    use ratatui::crossterm::event::KeyEvent;

    use super::*;

    fn app(name: &str) -> App {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../examples")
            .join(name);
        let registry = Arc::new(conduit_nodes::registry());
        let project = inspect(&dir, &registry).unwrap();
        App::new(dir, project, registry)
    }

    fn press(app: &mut App, keys: &str) {
        for c in keys.chars() {
            app.handle_key(KeyEvent::from(KeyCode::Char(c)));
        }
    }

    fn bar_text(app: &App) -> (String, String) {
        let bar = app.bar();
        let actions: Vec<String> = bar.actions.iter().map(|a| a.text.clone()).collect();
        (bar.title.to_string(), actions.join(bar.separator))
    }

    #[test]
    fn starts_on_the_sink_and_walks() {
        // Story 2: the flow is bg → comp → preview, focus on preview.
        let mut app = app("effects");
        assert_eq!(app.flow.nodes, ["bg", "comp", "preview"]);
        assert_eq!(app.flow_focus, Some(Focus::Node("preview".into())));
        assert_eq!(
            bar_text(&app),
            ("Node Actions".into(), "(e)dit (d)issolve".into())
        );

        press(&mut app, "h");
        assert!(matches!(app.flow_focus, Some(Focus::Conduit(_))));
        assert_eq!(app.flow_status(), "comp.out → preview.image");
        assert_eq!(
            bar_text(&app),
            (
                "Conduit Actions".into(),
                "(c)onnect (i)nsert (d)isconnect".into()
            )
        );
        // Global keys act on a conduit's downstream node (§6.2).
        assert_eq!(app.focus.as_deref(), Some("preview"));

        press(&mut app, "h");
        assert_eq!(app.flow_status(), "comp (composite)");
        assert_eq!(app.focus.as_deref(), Some("comp"));
        assert_eq!(bar_text(&app).1, "(e)dit (m)ove (d)issolve");

        press(&mut app, "jj");
        assert_eq!(app.flow_status(), "comp.layers[0] ← logo_at");
        assert_eq!(
            bar_text(&app),
            (
                "Conduit End Actions".into(),
                "(f)ocus (c)onnect (i)nsert (d)isconnect (J)later".into()
            )
        );
        // Walking never changes the flow.
        assert_eq!(app.flow.nodes, ["bg", "comp", "preview"]);
    }

    #[test]
    fn f_brings_a_branch_into_the_flow() {
        let mut app = app("effects");
        press(&mut app, "hhjj"); // comp.layers[0]
        app.recentre = false;
        press(&mut app, "f");
        assert_eq!(
            app.flow.nodes,
            ["logo", "logo_tinted", "logo_at", "comp", "preview"]
        );
        assert!(app.recentre);
        // Focus stays on the end, now in the flow: `(c)onnect` is the primary action.
        assert_eq!(app.flow_status(), "comp.layers[0] ← logo_at");
        let bar = app.bar();
        assert!(bar.actions[0].primary && bar.actions[0].text == "(c)onnect");
        // h now goes onto its conduit.
        press(&mut app, "h");
        assert_eq!(app.flow_status(), "logo_at.out → comp.layers[0]");
    }

    #[test]
    fn slot_and_unused_output_bars() {
        let mut app = app("effects");
        press(&mut app, "hhk"); // comp.out
        assert_eq!(app.flow_status(), "comp.out → preview.image");
        press(&mut app, "k"); // the layers slot
        assert_eq!(app.flow_status(), "comp.layers: append another");
        assert_eq!(
            bar_text(&app),
            ("Input Actions".into(), "(c)onnect (i)nsert".into())
        );

        let mut app = app_on("effects", "brand");
        press(&mut app, "jjj"); // primary, teal, then sand: unused
        assert_eq!(app.flow_status(), "brand.sand: no users");
        assert_eq!(
            bar_text(&app),
            ("Output Actions".into(), "(c)onnect to".into())
        );
    }

    fn app_on(name: &str, id: &str) -> App {
        let mut app = app(name);
        app.focus = Some(id.into());
        app.sync_flow();
        app
    }

    #[test]
    fn probe_follows_the_focus() {
        let mut app = app("effects");
        press(&mut app, "h");
        let action = app.handle_key(KeyEvent::from(KeyCode::Char('p')));
        assert_eq!(
            action,
            Some(Action::Probe(Reference::parse("comp").unwrap()))
        );
    }

    #[test]
    fn popups_have_their_own_bar() {
        let mut app = app("effects");
        press(&mut app, "/");
        let bar = app.bar();
        assert_eq!(bar.title, "Picker Actions");
        assert!(bar.globals.is_none());
    }
}
