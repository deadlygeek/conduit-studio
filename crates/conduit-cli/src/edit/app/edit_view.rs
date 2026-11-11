//! The edit view (spec §7): `e` on a node shows it enlarged, with a row for its name,
//! each conduit end, each unlinked param, each palette colour and each output.
//!
//! The view's cursor is an [`EditRow`]. On a conduit end the flow focus follows the
//! cursor, so the flow verbs (`c i d K J`, `p`, `f`) work on it as they do in the flow
//! view. Params and colours reuse the node panel's row edits.

use conduit_core::ParamKind;
use conduit_core::edit as ops;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::walk::{BarItem, item, primary};
use super::{Action, App, Row, nudge_step};
use crate::edit::flow::{
    Focus, InRow, InState, OutRow, input_rows, is_flow_input, is_flow_output, output_rows,
};

/// One row of the edit view, in order: name, inputs, params, colours, outputs (§7).
#[derive(Debug, Clone, PartialEq)]
pub enum EditRow {
    Name,
    /// An input's conduit end; a linked param is one too.
    In(InRow),
    /// An unlinked param.
    Param(String),
    /// A palette colour (§7.1).
    Colour {
        param: String,
        key: String,
    },
    Out(OutRow),
}

/// Whether `a` and `b` are the same thing, though its state may have changed: a param
/// is the same row once linked (an input row) or unlinked again (§7).
fn same(a: &EditRow, b: &EditRow) -> bool {
    match (a, b) {
        (EditRow::In(x), EditRow::In(y)) => x.port == y.port && x.index == y.index,
        (EditRow::In(r), EditRow::Param(name)) | (EditRow::Param(name), EditRow::In(r)) => {
            r.port == *name && r.index.is_none()
        }
        (EditRow::Out(x), EditRow::Out(y)) => x == y,
        _ => a == b,
    }
}

/// A value as typed: strings without quotes.
pub fn shown(value: &toml::Value) -> String {
    match value {
        toml::Value::String(s) => s.clone(),
        toml::Value::Float(f) => f.to_string(),
        other => other.to_string(),
    }
}

impl App {
    /// The focused node's edit view rows (§7).
    pub fn edit_rows(&self) -> Vec<EditRow> {
        let Some(node) = self.focused() else {
            return Vec::new();
        };
        let mut rows = vec![EditRow::Name];
        rows.extend(input_rows(node).into_iter().map(EditRow::In));
        rows.extend(self.rows.iter().filter_map(|r| match r {
            Row::Param { name, link: None } => Some(EditRow::Param(name.clone())),
            _ => None,
        }));
        rows.extend(self.rows.iter().filter_map(|r| match r {
            Row::Entry { param, key } => Some(EditRow::Colour {
                param: param.clone(),
                key: key.clone(),
            }),
            _ => None,
        }));
        rows.extend(
            output_rows(&self.project, node)
                .into_iter()
                .map(EditRow::Out),
        );
        rows
    }

    /// The focused node's errors, without the file name (§5.4).
    pub fn edit_errors(&self) -> Vec<String> {
        self.rows
            .iter()
            .filter_map(|r| match r {
                Row::Error(e) => Some(e.clone()),
                _ => None,
            })
            .collect()
    }

    /// `e` on a node: open the edit view on its name row (§7).
    pub(super) fn open_edit_view(&mut self) {
        if let Some(Focus::Node(_)) = self.flow_focus {
            self.edit_view = Some(EditRow::Name);
        }
    }

    /// Back to the node in the flow.
    fn close_edit_view(&mut self) {
        self.edit_view = None;
        if let Some(id) = self.focus.clone() {
            self.flow_focus = Some(Focus::Node(id));
        }
    }

    /// The flow focus a row stands for: its conduit end, else the node.
    fn row_focus(id: &str, row: &EditRow) -> Focus {
        let node = id.to_string();
        match row {
            EditRow::In(row) => Focus::In {
                node,
                row: row.clone(),
            },
            EditRow::Out(row) => Focus::Out {
                node,
                row: row.clone(),
            },
            _ => Focus::Node(node),
        }
    }

    fn set_edit_row(&mut self, row: EditRow) {
        if let Some(id) = &self.focus {
            self.flow_focus = Some(Self::row_focus(id, &row));
        }
        self.edit_at = self.edit_rows().iter().position(|r| *r == row).unwrap_or(0);
        self.edit_view = Some(row);
    }

    /// After adding or renaming a palette colour: the cursor goes to it.
    pub(super) fn follow_colour(&mut self, param: &str, key: &str) {
        if self.edit_view.is_some() {
            self.edit_view = Some(EditRow::Colour {
                param: param.to_string(),
                key: key.to_string(),
            });
        }
    }

    /// After a key: keep the cursor on its row, which an edit may have moved (a linked
    /// param becomes an input row, a reordered entry moves). A verb that moved the flow
    /// focus to another of the node's ends takes the cursor with it.
    pub(super) fn sync_edit_view(&mut self) {
        let Some(row) = self.edit_view.clone() else {
            return;
        };
        let Some(id) = self.focus.clone().filter(|_| self.flow_view) else {
            self.edit_view = None;
            return;
        };
        let wanted = match &self.flow_focus {
            Some(Focus::In { node, row }) if *node == id => EditRow::In(row.clone()),
            Some(Focus::Out { node, row }) if *node == id => EditRow::Out(row.clone()),
            _ => row,
        };
        let rows = self.edit_rows();
        let found = (rows.iter().find(|r| same(r, &wanted)))
            .or_else(|| match &wanted {
                // An output row whose user has gone: the output's first row.
                EditRow::Out(o) => rows
                    .iter()
                    .find(|r| matches!(r, EditRow::Out(p) if p.port == o.port)),
                _ => None,
            })
            // A row that's gone (a removed colour): the row now in its place (§6.7).
            .or_else(|| rows.get(self.edit_at.min(rows.len().saturating_sub(1))))
            .cloned()
            .unwrap_or(EditRow::Name);
        self.set_edit_row(found);
    }

    /// Keys while the edit view is open. `None` lets a global key through.
    pub(super) fn edit_view_key(&mut self, key: KeyEvent) -> Option<Option<Action>> {
        let row = self.edit_view.clone()?;
        let Some(id) = self.focus.clone() else {
            self.edit_view = None;
            return None;
        };
        let rows = self.edit_rows();
        let at = rows.iter().position(|r| *r == row).unwrap_or(0);
        let goto = |i: usize| rows.get(i).cloned();
        let target = match key.code {
            KeyCode::Char('j') | KeyCode::Down => goto(at + 1),
            KeyCode::Char('k') | KeyCode::Up => at.checked_sub(1).and_then(goto),
            KeyCode::Char('g') | KeyCode::Home => goto(0),
            KeyCode::Char('G') | KeyCode::End => rows.last().cloned(),
            KeyCode::Esc => {
                self.close_edit_view();
                return Some(None);
            }
            // Follow a conduit outward to the node at its other end (§7.2).
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some((target, there)) = self.across(&id, &row, true) {
                    self.follow_conduit(&id, &row, target, there);
                }
                return Some(None);
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if let Some((target, there)) = self.across(&id, &row, false) {
                    self.follow_conduit(&id, &row, target, there);
                }
                return Some(None);
            }
            // Flow history: back to the node (and row) a jump came from (§7.2).
            KeyCode::Char('[' | ']') | KeyCode::Backspace => return None,
            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => return None,
            // A palette's import, from any row (§7.1).
            KeyCode::Char('I') if self.entries_param().is_some() => {
                let param = self.entries_param().expect("checked");
                self.open_prompt(super::Prompt::Import { param }, String::new());
                return Some(None);
            }
            // On a colour row, `r` renames the colour (§7.1).
            KeyCode::Char('r') if matches!(row, EditRow::Colour { .. }) => {
                return Some(self.edit_row_key(key, &id, row));
            }
            // Globals; `p` probes the focus, `r` renames the node.
            KeyCode::Char('q' | '?' | 'u' | 'P' | 'E' | 'p' | 'r') => return None,
            // Globals that go elsewhere close the edit view first.
            KeyCode::Char('a' | '/' | 'D') => {
                self.close_edit_view();
                return None;
            }
            _ => return Some(self.edit_row_key(key, &id, row)),
        };
        if let Some(target) = target {
            self.set_edit_row(target);
        }
        Some(None)
    }

    /// The row's own keys (§7's table).
    fn edit_row_key(&mut self, key: KeyEvent, id: &str, row: EditRow) -> Option<Action> {
        let c = match key.code {
            KeyCode::Char(c) => c,
            KeyCode::Enter => '\n',
            _ => return None,
        };
        let enter = c == '\n';
        match row {
            EditRow::Name => {
                if enter || c == 'e' {
                    let r = KeyEvent::from(KeyCode::Char('r'));
                    return self.flow_verb(r, Focus::Node(id.to_string())).flatten();
                }
            }
            EditRow::In(_) | EditRow::Out(_) => {
                let focus = Self::row_focus(id, &row);
                let off_flow = self.off_flow(&focus);
                if c == 'f' || enter && off_flow {
                    // Close and redraw the flow through this conduit (§7).
                    self.edit_view = None;
                    self.flow_focus = Some(focus.clone());
                    return self.bring_in(&focus);
                }
                if let EditRow::In(r) = &row
                    && c == 'd'
                    && self.is_param_port(&r.port)
                {
                    // Unlinking a param moves it back to the params (§7).
                    let name = r.port.clone();
                    self.select_row(|x| matches!(x, Row::Param { name: n, .. } if *n == name));
                    self.disconnect_row();
                    return None;
                }
                if enter || matches!(c, 'c' | 'i' | 'd' | 'K' | 'J') {
                    return self.flow_verb(key, focus).flatten();
                }
            }
            EditRow::Param(name) => {
                self.select_row(|x| matches!(x, Row::Param { name: n, .. } if *n == name));
                let kind = self.param_spec(&name).map(|p| p.kind.clone());
                match c {
                    _ if enter || c == 'e' => self.edit_row(),
                    'c' if self.linkable(&name) => self.connect_row(),
                    'i' if self.linkable(&name) => self.insert_row(),
                    'd' if self.resettable(&name) => {
                        let id = id.to_string();
                        self.apply_or_report(format!("reset {id}.{name}"), |dir, _| {
                            ops::remove_param(dir, &id, &name)
                        });
                    }
                    'C' if kind == Some(ParamKind::Color) => return self.open_picker(),
                    _ if matches!(kind, Some(ParamKind::Int { .. } | ParamKind::Float { .. })) => {
                        if let Some(step) = nudge_step(c) {
                            self.nudge_row(step);
                        }
                    }
                    _ => {}
                }
            }
            EditRow::Colour { param, key } => {
                self.select_row(
                    |x| matches!(x, Row::Entry { param: p, key: k } if *p == param && *k == key),
                );
                self.pane = super::Pane::Node;
                match c {
                    _ if enter || c == 'e' => self.edit_row(),
                    'C' => return self.open_picker(),
                    'r' => self.start_rename(),
                    'i' => self.start_add_entry(),
                    'd' => self.start_remove_entry(),
                    'K' | 'J' => {
                        let step = if c == 'K' { -1 } else { 1 };
                        if self.can_move_colour(&key, step) {
                            let id = id.to_string();
                            let label = format!("moved {id}.{key}");
                            self.apply_or_report(label, |dir, _| {
                                ops::move_entry(dir, &id, &param, &key, step)
                            });
                        }
                    }
                    _ => {}
                }
            }
        }
        None
    }

    /// The node across the row's conduit, going upstream (`h`, from an input row) or
    /// downstream (`l`, from an output row), and its row for the same conduit.
    pub(super) fn across(
        &self,
        id: &str,
        row: &EditRow,
        upstream: bool,
    ) -> Option<(String, EditRow)> {
        type Matches = Box<dyn Fn(&EditRow) -> bool>;
        let (target, matches): (String, Matches) = match (row, upstream) {
            (EditRow::In(r), true) => {
                let InState::Connected(source) = &r.state else {
                    return None;
                };
                let (port, index) = (r.port.clone(), r.index.unwrap_or(0));
                let output = source.port.clone();
                let id = id.to_string();
                (
                    source.node.clone(),
                    Box::new(move |x| {
                        matches!(x, EditRow::Out(o) if o.port == output
                        && o.user.as_ref().is_some_and(|u| {
                            u.node == id && u.port == port && u.index == index
                        }))
                    }),
                )
            }
            (EditRow::Out(o), false) => {
                let user = o.user.clone()?;
                let id = id.to_string();
                let output = o.port.clone();
                (
                    user.node.clone(),
                    Box::new(move |x| {
                        matches!(x, EditRow::In(r) if r.port == user.port
                            && r.index.unwrap_or(0) == user.index
                            && matches!(&r.state, InState::Connected(s)
                                if s.node == id && s.port == output))
                    }),
                )
            }
            _ => return None,
        };
        let node = self.project.get(&target)?;
        let there = if upstream {
            (output_rows(&self.project, node).into_iter())
                .map(EditRow::Out)
                .find(|x| matches(x))
        } else {
            input_rows(node)
                .into_iter()
                .map(EditRow::In)
                .find(|x| matches(x))
        };
        Some((target, there.unwrap_or(EditRow::Name)))
    }

    /// `h` / `l` (§7.2): redraw the flow through the row's conduit, as `f` does, then
    /// edit the node at its other end with the cursor on the same conduit's row.
    fn follow_conduit(&mut self, id: &str, row: &EditRow, target: String, there: EditRow) {
        let end = Self::row_focus(id, row);
        let history = self.flow_back.len();
        self.flow_focus = Some(end.clone());
        self.bring_in(&end);
        if self.flow_back.len() == history {
            // Already in the flow: still a step, so `[` comes back here.
            self.remember_flow(self.flow.clone(), end);
        }
        self.focus = Some(target);
        self.refresh();
        self.set_edit_row(there);
    }

    /// Whether palette colour `key` can move `step` places (`K` / `J`).
    fn can_move_colour(&self, key: &str, step: isize) -> bool {
        let rows = self.edit_rows();
        let colours: Vec<&str> = (rows.iter())
            .filter_map(|r| match r {
                EditRow::Colour { key, .. } => Some(key.as_str()),
                _ => None,
            })
            .collect();
        (colours.iter().position(|k| *k == key))
            .and_then(|i| i.checked_add_signed(step))
            .is_some_and(|j| j < colours.len())
    }

    /// A conduit end whose conduit isn't drawn in the flow (a `▣`).
    fn off_flow(&self, focus: &Focus) -> bool {
        let i = self.flow.position(focus.node()).unwrap_or(0);
        match focus {
            Focus::In { row, .. } => {
                matches!(row.state, InState::Connected(_)) && !is_flow_input(&self.flow, i, row)
            }
            Focus::Out { row, .. } => row.user.is_some() && !is_flow_output(&self.flow, i, row),
            _ => false,
        }
    }

    /// Whether the focused node's input `port` is a linked param.
    fn is_param_port(&self, port: &str) -> bool {
        self.input_spec(port).is_some_and(|p| p.param)
    }

    /// Whether `d` can put the param back to its default: it has a value and isn't required.
    fn resettable(&self, name: &str) -> bool {
        let set = self.focused().is_some_and(|n| n.params.contains_key(name));
        set && !self.param_spec(name).is_some_and(|p| p.required)
    }

    /// The status line's text for a param or colour row, e.g. `blue_rect.width = 100`.
    pub(super) fn edit_focus_text(&self) -> Option<String> {
        let id = self.focus.as_deref()?;
        match self.edit_view.as_ref()? {
            EditRow::Param(name) => {
                let value = self
                    .current_value(name)
                    .map_or_else(|| "unset".to_string(), |v| shown(&v));
                Some(format!("{id}.{name} = {value}"))
            }
            EditRow::Colour { param, key } => {
                let value = self
                    .focused()?
                    .params
                    .get(param)
                    .and_then(|t| t.get(key))
                    .map(shown)
                    .unwrap_or_default();
                Some(format!("{id}.{key} = {value}"))
            }
            _ => None,
        }
    }

    /// The bar for the edit view's name, param and colour rows; conduit ends use the
    /// flow view's (§7).
    pub(super) fn edit_actions(&self) -> Option<(&'static str, Vec<BarItem>)> {
        match self.edit_view.as_ref()? {
            EditRow::Name => Some(("Name Actions", vec![primary("(e)dit")])),
            EditRow::Param(name) => {
                let mut actions = vec![primary("(e)dit")];
                if self
                    .param_spec(name)
                    .is_some_and(|p| p.kind == ParamKind::Color)
                {
                    actions.push(item("(C)picker"));
                }
                if self.linkable(name) {
                    actions.push(item("(c)onnect"));
                    actions.push(item("(i)nsert"));
                }
                if self.resettable(name) {
                    actions.push(item("(d)efault"));
                }
                Some(("Param Actions", actions))
            }
            EditRow::Colour { key, .. } => {
                let mut actions = vec![
                    primary("(e)dit"),
                    item("(C)picker"),
                    item("(r)ename"),
                    item("(i)nsert colour"),
                    item("(d)elete"),
                ];
                if self.can_move_colour(key, -1) {
                    actions.push(item("(K)earlier"));
                }
                if self.can_move_colour(key, 1) {
                    actions.push(item("(J)later"));
                }
                Some(("Colour Actions", actions))
            }
            EditRow::In(_) | EditRow::Out(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::temp_project;
    use super::*;

    /// A temp copy of `examples/linked` in the flow view, editing `id`.
    fn editing(id: &str) -> (tempfile::TempDir, App) {
        editing_in("linked", id)
    }

    /// A temp copy of `examples/<project>` in the flow view, editing `id`.
    fn editing_in(project: &str, id: &str) -> (tempfile::TempDir, App) {
        let (tmp, mut app) = temp_project(project);
        app.flow_view = true;
        app.focus = Some(id.into());
        app.refresh();
        app.sync_flow();
        keys(&mut app, "e");
        assert_eq!(app.edit_view, Some(EditRow::Name));
        (tmp, app)
    }

    fn keys(app: &mut App, keys: &str) {
        for c in keys.chars() {
            let code = match c {
                '⏎' => KeyCode::Enter,
                '⎋' => KeyCode::Esc,
                c => KeyCode::Char(c),
            };
            app.handle_key(KeyEvent::from(code));
        }
    }

    fn read(app: &App, id: &str) -> String {
        std::fs::read_to_string(app.dir.join(format!("{id}.toml"))).unwrap()
    }

    /// Moves the cursor down to the first row `pred` matches.
    fn down_to(app: &mut App, pred: impl Fn(&EditRow) -> bool) {
        for _ in 0..30 {
            if app.edit_view.as_ref().is_some_and(&pred) {
                return;
            }
            keys(app, "j");
        }
        panic!("no such row: {:?}", app.edit_rows());
    }

    fn input(port: &str) -> impl Fn(&EditRow) -> bool + '_ {
        move |r| matches!(r, EditRow::In(r) if r.port == port)
    }

    #[test]
    fn rows_are_in_sections() {
        let (_tmp, app) = editing("placed");
        let rows = app.edit_rows();
        let ports: Vec<String> = rows
            .iter()
            .map(|r| match r {
                EditRow::Name => "name".into(),
                EditRow::In(r) => format!("in {}", r.label()),
                EditRow::Param(p) => format!("param {p}"),
                EditRow::Colour { key, .. } => format!("colour {key}"),
                EditRow::Out(o) => format!("out {}", o.port),
            })
            .collect();
        assert_eq!(&ports[..4], ["name", "in layer", "in x", "in y"]);
        assert!(
            ports[4..ports.len() - 1]
                .iter()
                .all(|p| p.starts_with("param "))
        );
        assert_eq!(ports.last().unwrap(), "out out");
    }

    fn params(app: &App) -> Vec<String> {
        app.edit_rows()
            .into_iter()
            .filter_map(|r| match r {
                EditRow::Param(p) => Some(p),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_connected_image_hides_the_canvas_fill_params() {
        let (_tmp, app) = editing_in("canvas", "bg");
        assert_eq!(params(&app), ["color", "width", "height", "opacity"]);

        let (_tmp, mut app) = editing_in("canvas", "faded");
        assert_eq!(params(&app), ["opacity"]);
        // Unlinked from its image, the canvas draws a fill again.
        down_to(&mut app, input("image"));
        keys(&mut app, "d");
        assert!(!read(&app, "faded").contains("image ="));
        assert_eq!(params(&app), ["color", "width", "height", "opacity"]);
        // Focus stays on the (now empty) image input row.
        assert!(input("image")(app.edit_view.as_ref().unwrap()));
    }

    #[test]
    fn a_linked_param_stays_visible_while_it_does_not_apply() {
        let (tmp, mut app) = editing_in("canvas", "faded");
        let file = tmp.path().join("faded.toml");
        let text = std::fs::read_to_string(&file).unwrap();
        let linked = text.replace("image = \"comp\"", "image = \"comp\"\nwidth = \"n\"");
        std::fs::write(&file, linked).unwrap();
        let int = "type = \"int\"\n[params]\nvalue = 3\n";
        std::fs::write(tmp.path().join("n.toml"), int).unwrap();
        app.reload_from_disk();
        assert!(app.edit_rows().iter().any(input("width")));
        assert_eq!(params(&app), ["opacity"]);
    }

    #[test]
    fn unlinking_and_linking_a_param_moves_its_row() {
        let (_tmp, mut app) = editing("placed");
        down_to(&mut app, input("x"));
        assert_eq!(app.flow_status(), "placed.x ← offset");
        assert_eq!(app.bar().title, "Conduit End Actions");

        keys(&mut app, "d");
        assert!(!read(&app, "placed").contains("x = \"offset\""));
        assert_eq!(app.edit_view, Some(EditRow::Param("x".into())));
        assert_eq!(app.bar().title, "Param Actions");

        keys(&mut app, "coffset⏎");
        assert!(read(&app, "placed").contains("x = \"offset\""));
        assert!(
            input("x")(app.edit_view.as_ref().unwrap()),
            "{:?}",
            app.edit_view
        );

        keys(&mut app, "u");
        assert_eq!(app.edit_view, Some(EditRow::Param("x".into())));
    }

    #[test]
    fn d_puts_a_param_back_to_its_default() {
        let (_tmp, mut app) = editing("bg");
        down_to(&mut app, |r| *r == EditRow::Param("width".into()));
        assert_eq!(app.flow_status(), "bg.width = 800");
        let texts: Vec<String> = app.bar().actions.iter().map(|a| a.text.clone()).collect();
        assert!(texts.contains(&"(d)efault".to_string()), "{texts:?}");

        keys(&mut app, "d");
        assert!(!read(&app, "bg").contains("width"));
        assert_eq!(app.edit_view, Some(EditRow::Param("width".into())));
        assert_eq!(app.flow_status(), "reset bg.width");
        keys(&mut app, "j");
        keys(&mut app, "k");
        assert_eq!(app.flow_status(), "bg.width = 1920");
        let texts: Vec<String> = app.bar().actions.iter().map(|a| a.text.clone()).collect();
        assert!(!texts.contains(&"(d)efault".to_string()), "{texts:?}");
    }

    #[test]
    fn nudges_change_numbers() {
        let (_tmp, mut app) = editing("bg");
        down_to(&mut app, |r| *r == EditRow::Param("width".into()));
        keys(&mut app, ">+");
        assert!(read(&app, "bg").contains("width = 811"));
    }

    #[test]
    fn renaming_keeps_the_edit_view_open() {
        let (_tmp, mut app) = editing("bg");
        keys(&mut app, "eback⏎");
        assert_eq!(app.focus.as_deref(), Some("back"));
        assert_eq!(app.edit_view, Some(EditRow::Name));
        assert!(read(&app, "comp").contains("base = \"back\""));
    }

    #[test]
    fn esc_and_f_go_back_to_the_flow() {
        let (_tmp, mut app) = editing("placed");
        keys(&mut app, "j⎋");
        assert_eq!(app.edit_view, None);
        assert_eq!(app.flow_focus, Some(Focus::Node("placed".into())));

        // `f` on a `▣` end: the flow is redrawn through it.
        keys(&mut app, "e");
        down_to(&mut app, input("x"));
        assert!(app.flow.position("offset").is_none());
        keys(&mut app, "f");
        assert_eq!(app.edit_view, None);
        assert!(app.flow.position("offset").is_some());
        assert!(matches!(&app.flow_focus, Some(Focus::In { row, .. }) if row.port == "x"));
    }

    #[test]
    fn globals_that_go_elsewhere_close_it() {
        let (_tmp, mut app) = editing("placed");
        keys(&mut app, "/");
        assert_eq!(app.edit_view, None);
        assert!(matches!(app.mode, super::super::Mode::Pick { .. }));
    }

    #[test]
    fn errors_are_listed() {
        let (_tmp, mut app) = editing("bg");
        assert!(app.edit_errors().is_empty());
        down_to(&mut app, |r| *r == EditRow::Param("width".into()));
        std::fs::write(
            app.dir.join("bg.toml"),
            "type = \"canvas\"\n[params]\ncolor = \"white\"\nwidth = 0\n",
        )
        .unwrap();
        app.reload_from_disk();
        app.sync_edit_view();
        assert_eq!(app.edit_view, Some(EditRow::Param("width".into())));
        let errors = app.edit_errors();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("width"), "{errors:?}");
    }

    fn colour(key: &str) -> EditRow {
        EditRow::Colour {
            param: "colors".into(),
            key: key.into(),
        }
    }

    fn bar_texts(app: &App) -> Vec<String> {
        app.bar().actions.iter().map(|a| a.text.clone()).collect()
    }

    /// The palette's colour names, in file order.
    fn colours(app: &App) -> Vec<String> {
        (app.edit_rows().into_iter())
            .filter_map(|r| match r {
                EditRow::Colour { key, .. } => Some(key),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn palette_colour_rows() {
        let (_tmp, mut app) = editing_in("palette", "brand");
        assert_eq!(
            colours(&app),
            ["primary", "teal", "sand", "orange", "accent"]
        );
        assert!(bar_texts(&app).contains(&"(I)mport".to_string()));
        down_to(&mut app, |r| *r == colour("primary"));
        assert_eq!(app.flow_status(), "brand.primary = #264653");
        assert_eq!(app.bar().title, "Colour Actions");
        assert_eq!(
            bar_texts(&app),
            [
                "(e)dit",
                "(C)picker",
                "(r)ename",
                "(i)nsert colour",
                "(d)elete",
                "(J)later",
            ]
        );
        keys(&mut app, "j");
        assert!(bar_texts(&app).contains(&"(K)earlier".to_string()));
    }

    #[test]
    fn insert_rename_move_and_delete_colours() {
        let (_tmp, mut app) = editing_in("palette", "brand");
        down_to(&mut app, |r| *r == colour("teal"));

        // `i` adds a copy below, and the cursor goes to it.
        keys(&mut app, "isea⏎");
        assert_eq!(
            colours(&app),
            ["primary", "teal", "sea", "sand", "orange", "accent"]
        );
        assert!(read(&app, "brand").contains("sea = \"#2a9d8f\""));
        assert_eq!(app.edit_view, Some(colour("sea")));

        // `J` / `K` move it; the cursor follows.
        keys(&mut app, "J");
        assert_eq!(
            colours(&app),
            ["primary", "teal", "sand", "sea", "orange", "accent"]
        );
        assert_eq!(app.edit_view, Some(colour("sea")));
        keys(&mut app, "K");
        assert_eq!(colours(&app)[2], "sea");

        // `r` renames the colour, not the node.
        keys(&mut app, "rocean⏎");
        assert_eq!(app.focus.as_deref(), Some("brand"));
        assert_eq!(app.edit_view, Some(colour("ocean")));

        // `d` removes an unused colour at once; the cursor takes the next row.
        keys(&mut app, "d");
        assert_eq!(
            colours(&app),
            ["primary", "teal", "sand", "orange", "accent"]
        );
        assert_eq!(app.edit_view, Some(colour("sand")));

        // A used one asks first, then disconnects its users.
        down_to(&mut app, |r| *r == colour("accent"));
        keys(&mut app, "d");
        assert!(matches!(app.mode, super::super::Mode::Confirm { .. }));
        keys(&mut app, "y");
        assert!(!colours(&app).contains(&"accent".to_string()));
        assert!(!read(&app, "accent_dark").contains("brand.accent"));
    }

    #[test]
    fn import_from_any_row() {
        let (_tmp, mut app) = editing_in("palette", "brand");
        keys(&mut app, "I");
        assert!(matches!(
            &app.mode,
            super::super::Mode::Prompt {
                purpose: super::super::Prompt::Import { .. },
                ..
            }
        ));
    }

    fn output_to(user: &str) -> impl Fn(&EditRow) -> bool + '_ {
        move |r| matches!(r, EditRow::Out(o) if o.user.as_ref().is_some_and(|u| u.node == user))
    }

    #[test]
    fn h_and_l_follow_a_conduit_to_the_next_node() {
        let (_tmp, mut app) = editing("placed");
        down_to(&mut app, input("x"));
        assert!(bar_texts(&app).contains(&"(h)edit offset".to_string()));
        assert!(!bar_texts(&app).iter().any(|t| t.starts_with("(l)")));

        // `l` on an input row goes nowhere.
        keys(&mut app, "l");
        assert_eq!(app.focus.as_deref(), Some("placed"));

        // `h` lands on offset's row for the same conduit, with it drawn in the flow.
        keys(&mut app, "h");
        assert_eq!(app.focus.as_deref(), Some("offset"));
        let row = app.edit_view.clone().unwrap();
        assert!(
            matches!(&row, EditRow::Out(o) if o.user.as_ref().is_some_and(|u| u.node == "placed" && u.port == "x")),
            "{row:?}"
        );
        assert!(app.flow.position("offset").is_some());
        assert!(bar_texts(&app).contains(&"(l)edit placed".to_string()));

        // `l` comes straight back, on the same row.
        keys(&mut app, "l");
        assert_eq!(app.focus.as_deref(), Some("placed"));
        assert!(input("x")(app.edit_view.as_ref().unwrap()));

        // Each jump is a flow change: `[` retraces them, staying in the edit view.
        keys(&mut app, "[");
        assert_eq!(app.focus.as_deref(), Some("offset"));
        assert!(output_to("placed")(app.edit_view.as_ref().unwrap()));
        keys(&mut app, "[");
        assert_eq!(app.focus.as_deref(), Some("placed"));
        assert!(input("x")(app.edit_view.as_ref().unwrap()));

        // Downstream from an output row; `esc` leaves you on that node in the flow.
        down_to(&mut app, output_to("comp"));
        keys(&mut app, "h");
        assert_eq!(
            app.focus.as_deref(),
            Some("placed"),
            "h on an output goes nowhere"
        );
        keys(&mut app, "l");
        assert_eq!(app.focus.as_deref(), Some("comp"));
        assert!(
            matches!(app.edit_view.as_ref().unwrap(), EditRow::In(r) if r.port == "layers"),
            "{:?}",
            app.edit_view
        );
        keys(&mut app, "⎋");
        assert_eq!(app.flow_focus, Some(Focus::Node("comp".into())));
    }

    #[test]
    fn h_does_nothing_on_rows_without_a_conduit() {
        let (_tmp, mut app) = editing("placed");
        keys(&mut app, "h");
        assert_eq!(app.focus.as_deref(), Some("placed"));
        assert_eq!(app.edit_view, Some(EditRow::Name));
        down_to(&mut app, |r| matches!(r, EditRow::Param(_)));
        keys(&mut app, "hl");
        assert_eq!(app.focus.as_deref(), Some("placed"));
    }
}
