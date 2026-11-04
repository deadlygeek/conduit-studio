//! Editing from the TUI: pickers, prompts and confirmations that change node files through
//! `conduit_core::edit`, each recorded for undo.

use std::path::Path;

use conduit_core::edit::{self as ops, Change, EditResult};
use conduit_core::{
    Consumer, DetachPlan, NodeSpec, ParamKind, ParamSpec, Project, ProjectNode, Reference,
    ValueType, float_value, inspect,
};

use super::{Action, App, Mode, Row};
use crate::edit::finder;

/// What a picker list is choosing.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    FindNode,
    /// A source for the focused node's input: list entry `index` if given, else the
    /// input (appended to a list).
    Connect {
        port: String,
        multi: bool,
        index: Option<usize>,
    },
    /// A new user for the focused node's `output` (flow view: `(c)onnect to`).
    ConnectTo {
        output: String,
    },
    /// The type of a new node to feed the focused node's input `port` (flow view: `i` on
    /// an unconnected input or a list's slot).
    FeedInput {
        port: String,
    },
    /// The type of a new node.
    AddType,
    /// The type of a node to insert before `node.port[index]` (the focused node's input,
    /// or a connection in a tree).
    InsertType {
        node: String,
        port: String,
        index: usize,
    },
    /// The type of a new node to feed the focused node's number param.
    FeedType {
        param: String,
    },
    /// A font for the focused node's font param, previewed live.
    Font {
        param: String,
    },
    /// A project file for the focused node's file param, previewed live.
    File {
        param: String,
    },
}

impl Pick {
    pub fn title(&self, focus: &str) -> String {
        match self {
            Pick::FindNode => "find node".into(),
            Pick::Connect { port, .. } => format!("connect {focus}.{port} to"),
            Pick::ConnectTo { output } => format!("connect {focus}.{output} to"),
            Pick::FeedInput { port } => format!("feed {focus}.{port} from a new"),
            Pick::AddType => "add node of type".into(),
            Pick::InsertType { node, port, .. } => format!("insert before {node}.{port}: type"),
            Pick::FeedType { param } => format!("feed {focus}.{param} from a new"),
            Pick::Font { param } => format!("font for {focus}.{param}"),
            Pick::File { param } => format!("file for {focus}.{param}"),
        }
    }
}

/// What a text prompt is asking for.
#[derive(Debug, Clone, PartialEq)]
pub enum Prompt {
    Param {
        name: String,
    },
    AddId {
        kind: String,
    },
    InsertId {
        kind: String,
        node: String,
        port: String,
        index: usize,
    },
    FeedId {
        kind: String,
        param: String,
    },
    /// Id for a new node feeding the focused node's input `port` (see [`Pick::FeedInput`]).
    FeedInputId {
        kind: String,
        port: String,
    },
    Rename,
    /// Name for a new palette color, a copy of `after` placed below it (else the last).
    AddEntry {
        param: String,
        after: Option<String>,
    },
    RenameEntry {
        param: String,
        key: String,
    },
    /// Pasted palette text (Coolors link, hex codes, …) replacing a palette's colors.
    Import {
        param: String,
    },
}

impl Prompt {
    pub fn label(&self, focus: &str) -> String {
        match self {
            Prompt::Param { name } => format!("{focus}.{name} (empty = default)"),
            Prompt::AddId { kind }
            | Prompt::InsertId { kind, .. }
            | Prompt::FeedId { kind, .. }
            | Prompt::FeedInputId { kind, .. } => format!("id for new {kind}"),
            Prompt::Rename => format!("rename {focus} to"),
            Prompt::AddEntry { .. } => format!("name for the new color in {focus}"),
            Prompt::RenameEntry { key, .. } => format!("rename {focus}.{key} to"),
            Prompt::Import { .. } => "paste a Coolors link or hex codes (empty: keep)".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Confirm {
    Delete(String),
    Dissolve(String),
    /// Replace the node with the plan's nodes (see `conduit_core::detach_plan`).
    Detach(String, DetachPlan),
    RemoveEntry {
        param: String,
        key: String,
    },
}

impl App {
    // --- pickers -----------------------------------------------------------------------

    pub(super) fn open_pick(&mut self, purpose: Pick) {
        if self.pick_items(&purpose).is_empty() && purpose != Pick::FindNode {
            self.message = Some(match purpose {
                Pick::Connect { .. } => "no compatible outputs to connect".into(),
                Pick::ConnectTo { .. } => "no input can take this output".into(),
                Pick::InsertType { .. } | Pick::FeedType { .. } | Pick::FeedInput { .. } => {
                    "no node type fits here".into()
                }
                _ => "nothing to choose from".into(),
            });
            return;
        }
        self.mode = Mode::Pick {
            purpose,
            query: String::new(),
            selected: 0,
        };
    }

    /// `(key, label)` for every choice, unfiltered.
    pub fn pick_items(&self, purpose: &Pick) -> Vec<(String, String)> {
        let kind = |id: &str| {
            self.project
                .get(id)
                .and_then(|n| n.kind.clone())
                .unwrap_or_default()
        };
        match purpose {
            Pick::FindNode => self
                .project
                .nodes
                .values()
                .map(|n| (n.id.clone(), format!("{}  {}", n.id, kind(&n.id))))
                .collect(),
            Pick::Connect { port, .. } => {
                let Some(focus) = &self.focus else {
                    return Vec::new();
                };
                self.project
                    .compatible_sources(focus, port)
                    .into_iter()
                    .map(|r| (r.to_string(), format!("{}  {}", r.short(), kind(&r.node))))
                    .collect()
            }
            Pick::ConnectTo { output } => {
                let Some(focus) = &self.focus else {
                    return Vec::new();
                };
                let source = Reference {
                    node: focus.clone(),
                    port: output.clone(),
                };
                self.project
                    .compatible_targets(&source)
                    .into_iter()
                    .map(|(node, port)| {
                        let n = self.project.get(&node);
                        let refs = n.and_then(|n| n.inputs.get(&port));
                        let multi = n
                            .and_then(|n| n.spec.as_ref()?.input(&port))
                            .is_some_and(|p| p.multi);
                        let now = match refs.map(Vec::as_slice) {
                            _ if multi => "  (append)".to_string(),
                            Some([r, ..]) => format!("  ← {}", r.short()),
                            _ => String::new(),
                        };
                        let label = format!("{node}.{port}{now}  {}", kind(&node));
                        (format!("{node}.{port}"), label)
                    })
                    .collect()
            }
            Pick::FeedInput { port } => {
                let Some(ty) = self.input_spec(port).map(|p| p.ty) else {
                    return Vec::new();
                };
                self.registry
                    .specs()
                    .filter(|s| output_of(s, ty).is_some())
                    .map(|s| (s.kind.to_string(), type_label(s)))
                    .collect()
            }
            Pick::AddType => self
                .registry
                .specs()
                .map(|s| (s.kind.to_string(), type_label(s)))
                .collect(),
            Pick::InsertType { node, port, index } => {
                let Some((source_ty, target_ty)) = self.insert_types(node, port, *index) else {
                    return Vec::new();
                };
                self.registry
                    .specs()
                    .filter(|s| insertable(s, source_ty, target_ty))
                    .map(|s| (s.kind.to_string(), type_label(s)))
                    .collect()
            }
            Pick::FeedType { param } => {
                let Some(ty) = self.param_input_type(param) else {
                    return Vec::new();
                };
                self.registry
                    .specs()
                    .filter(|s| output_of(s, ty).is_some())
                    .map(|s| (s.kind.to_string(), type_label(s)))
                    .collect()
            }
            Pick::Font { param } => self.font_items(param),
            Pick::File { .. } => self.files.clone(),
        }
    }

    /// The picker's choices matching its query, best first.
    pub fn pick_matches(&self) -> Vec<(String, String)> {
        match &self.mode {
            Mode::Pick { purpose, query, .. } => self.matches_for(purpose, query),
            _ => Vec::new(),
        }
    }

    /// `purpose`'s choices matching `query`, best first.
    pub(super) fn matches_for(&self, purpose: &Pick, query: &str) -> Vec<(String, String)> {
        let items = self.pick_items(purpose);
        // Fonts (recent, generic, A–Z) and files (by path) are listed in a deliberate order;
        // keep it unfiltered.
        if matches!(purpose, Pick::Font { .. } | Pick::File { .. }) && query.trim().is_empty() {
            return items;
        }
        finder::rank(
            query,
            items.iter().map(|item| (item.clone(), item.1.as_str())),
        )
    }

    pub(super) fn choose(&mut self, purpose: Pick, key: String) -> Option<Action> {
        match purpose {
            Pick::FindNode => return self.focus_on(&key),
            Pick::Connect { port, multi, index } => {
                let (Some(focus), Ok(source)) = (self.focus.clone(), Reference::parse(&key)) else {
                    return None;
                };
                let entry = index.map_or(port.clone(), |i| format!("{port}[{i}]"));
                let label = format!("connected {focus}.{entry} ← {}", source.short());
                self.apply_or_report(label, |dir, _| match index {
                    Some(i) => ops::reconnect(dir, &focus, &port, i, &source),
                    None => ops::connect(dir, &focus, &port, &source, multi),
                });
            }
            Pick::ConnectTo { output } => {
                let (Some(focus), Some((node, port))) = (self.focus.clone(), key.split_once('.'))
                else {
                    return None;
                };
                let source = Reference {
                    node: focus,
                    port: output,
                };
                let multi = self
                    .project
                    .get(node)
                    .and_then(|n| n.spec.as_ref()?.input(port))
                    .is_some_and(|p| p.multi);
                let label = format!("connected {node}.{port} ← {}", source.short());
                self.apply_or_report(label, |dir, _| {
                    ops::connect(dir, node, port, &source, multi)
                });
            }
            Pick::FeedInput { port } => {
                let id = self.free_id(&key);
                self.open_prompt(Prompt::FeedInputId { kind: key, port }, id);
            }
            Pick::AddType => {
                let id = self.free_id(&key);
                self.open_prompt(Prompt::AddId { kind: key }, id);
            }
            Pick::InsertType { node, port, index } => {
                let id = self.free_id(&key);
                self.open_prompt(
                    Prompt::InsertId {
                        kind: key,
                        node,
                        port,
                        index,
                    },
                    id,
                );
            }
            Pick::FeedType { param } => {
                let focus = self.focus.clone().unwrap_or_default();
                let id = self.free_id_like(&format!("{focus}_{param}"));
                self.open_prompt(Prompt::FeedId { kind: key, param }, id);
            }
            Pick::Font { param } => self.choose_font(&param, key),
            Pick::File { param } => self.keep_pick(&param, &key),
        }
        None
    }

    // --- prompts -----------------------------------------------------------------------

    pub(super) fn open_prompt(&mut self, purpose: Prompt, input: String) {
        self.mode = Mode::Prompt {
            purpose,
            selected: !input.is_empty(),
            input,
            error: None,
        };
    }

    /// Runs a submitted prompt. `Err` keeps the prompt open showing the message.
    pub(super) fn submit_prompt(
        &mut self,
        purpose: Prompt,
        input: &str,
    ) -> Result<Option<Action>, String> {
        let input = input.trim();
        let focus = self.focus.clone().unwrap_or_default();
        match purpose {
            Prompt::Param { name } => self.set_param(&name, input).map(|()| None),
            Prompt::AddId { kind } => {
                let spec = self.spec(&kind)?;
                let group = self.group_for(input, &spec);
                let label = format!("added {input} ({kind}){}", group_label(&group));
                let registry = self.registry.clone();
                self.apply(label, |dir, _| {
                    let change = ops::add_node(dir, input, &spec)?;
                    with_group(dir, &registry, input, group.as_deref(), change)
                })?;
                let action = self.focus_on(input);
                // A new palette: offer to fill it straight away.
                if let Some(param) = self.entries_param() {
                    self.open_prompt(Prompt::Import { param }, String::new());
                }
                Ok(action)
            }
            Prompt::InsertId {
                kind,
                node,
                port,
                index,
            } => {
                let spec = self.spec(&kind)?;
                let group = self.group_for(input, &spec);
                let label = format!(
                    "inserted {input} ({kind}) before {node}.{port}{}",
                    group_label(&group)
                );
                let registry = self.registry.clone();
                self.apply(label, |dir, project| {
                    let change =
                        ops::insert_between(dir, project, &node, &port, index, input, &spec)?;
                    with_group(dir, &registry, input, group.as_deref(), change)
                })?;
                Ok(self.focus_on(input))
            }
            Prompt::FeedId { kind, param } => {
                let spec = self.spec(&kind)?;
                let ty = self
                    .param_input_type(&param)
                    .ok_or("not a linkable param")?;
                let output = output_of(&spec, ty).ok_or("no matching output")?;
                // Start the new node at the param's current value.
                let params = match (self.current_value(&param), param_input_of(&spec, ty)) {
                    (Some(value), Some(name)) => {
                        toml::Table::from_iter([(name.to_string(), value)])
                    }
                    _ => toml::Table::new(),
                };
                let label = format!("added {input} ({kind}) feeding {focus}.{param}");
                self.apply(label, |dir, _| {
                    ops::add_feeding(dir, input, &spec, &params, &output, &focus, &param)
                })?;
                // A node that reads a file (e.g. `font` feeding `text.font`) needs one chosen
                // before it's any use: go to it and open the file picker.
                let file_param = spec
                    .params
                    .iter()
                    .find(|p| matches!(p.kind, ParamKind::File { .. }));
                let Some(file_param) = file_param else {
                    return Ok(None);
                };
                let action = self.focus_on(input);
                self.select_row(
                    |r| matches!(r, Row::Param { name, .. } if name == file_param.name),
                );
                self.open_file_picker();
                Ok(action)
            }
            Prompt::FeedInputId { kind, port } => {
                let spec = self.spec(&kind)?;
                let target = self.input_spec(&port).ok_or("no such input")?.clone();
                let output = output_of(&spec, target.ty).ok_or("no matching output")?;
                let group = self.group_for(input, &spec);
                let label = format!(
                    "added {input} ({kind}) feeding {focus}.{port}{}",
                    group_label(&group)
                );
                let registry = self.registry.clone();
                self.apply(label, |dir, _| {
                    let change = ops::add_feeding_to(
                        dir,
                        input,
                        &spec,
                        &toml::Table::new(),
                        &output,
                        &focus,
                        &port,
                        target.multi,
                    )?;
                    with_group(dir, &registry, input, group.as_deref(), change)
                })?;
                Ok(None)
            }
            Prompt::AddEntry { param, after } => {
                let color = self.entry_color().unwrap_or_else(|| "#808080".into());
                self.apply(format!("added {focus}.{input}"), |dir, _| {
                    ops::insert_entry(dir, &focus, &param, after.as_deref(), input, &color.into())
                })?;
                self.select_row(|r| matches!(r, Row::Entry { key, .. } if key == input));
                self.follow_colour(&param, input);
                Ok(None)
            }
            Prompt::RenameEntry { param, key } => {
                if input == key {
                    return Ok(None);
                }
                self.apply(
                    format!("renamed {focus}.{key} to {input}"),
                    |dir, project| ops::rename_entry(dir, project, &focus, &param, &key, input),
                )?;
                self.select_row(|r| matches!(r, Row::Entry { key, .. } if key == input));
                self.follow_colour(&param, input);
                Ok(None)
            }
            Prompt::Import { param } => {
                if input.is_empty() {
                    return Ok(None);
                }
                let colors = conduit_core::parse_palette(input)?;
                let kind = self
                    .focused()
                    .and_then(|n| n.kind.clone())
                    .unwrap_or_default();
                let spec = self.spec(&kind)?;
                let entries: Vec<(String, toml::Value)> = colors
                    .iter()
                    .map(|(name, c)| (name.clone(), conduit_core::to_hex(*c).into()))
                    .collect();
                let link = input.contains("://") || input.contains("coolors.co/");
                let extra: Vec<(&str, toml::Value)> =
                    link.then(|| ("source", input.into())).into_iter().collect();
                let label = format!("imported {} colors into {focus}", entries.len());
                self.apply(label, |dir, _| {
                    ops::set_entries(dir, &focus, &spec, &param, &entries, &extra)
                })?;
                Ok(None)
            }
            Prompt::Rename => {
                if input == focus {
                    return Ok(None);
                }
                self.apply(format!("renamed {focus} to {input}"), |dir, project| {
                    ops::rename_node(dir, project, &focus, input)
                })?;
                for id in self.back.iter_mut().chain(self.forward.iter_mut()) {
                    if *id == focus {
                        *id = input.to_string();
                    }
                }
                self.focus = Some(input.to_string());
                self.refresh();
                Ok(None)
            }
        }
    }

    /// The focused node's typed value for a param, else its default.
    pub(super) fn current_value(&self, name: &str) -> Option<toml::Value> {
        let node = self.focused()?;
        if let Some(value) = node.params.get(name) {
            return Some(value.clone());
        }
        node.spec.as_ref()?.param(name)?.default_value()
    }

    /// Initial text for editing a param: its current value, else its default.
    fn param_input(&self, name: &str) -> String {
        match self.current_value(name) {
            Some(toml::Value::String(s)) => s,
            Some(other) => other.to_string(),
            None => String::new(),
        }
    }

    /// Validates and writes a param. Empty input removes it (so its default applies).
    fn set_param(&mut self, name: &str, input: &str) -> Result<(), String> {
        let node = self.focused().cloned().ok_or("no node selected")?;
        let id = node.id.clone();
        let spec = node.spec.as_ref().and_then(|s| s.param(name)).cloned();
        if input.is_empty() {
            if spec.as_ref().is_some_and(|p| p.required) {
                return Err(format!("`{name}` is required"));
            }
            return self.apply(format!("reset {id}.{name}"), |dir, project| {
                ops::set_param_synced(dir, project, &id, name, None)
            });
        }
        let value = match &spec {
            Some(p) => p.parse_input(input)?,
            None => parse_literal(input),
        };
        self.check_param(&node, name, &value)?;
        self.apply(format!("set {id}.{name} = {value}"), |dir, project| {
            ops::set_param_synced(dir, project, &id, name, Some(&value))
        })
    }

    // --- direct edits ------------------------------------------------------------------

    /// `⏎` in the node panel: edit a param (typing a value replaces a link), or pick a
    /// source for an input.
    pub(super) fn edit_row(&mut self) {
        if self.open_color_editor() || self.open_font_picker() || self.open_file_picker() {
            return;
        }
        match self.current_row().cloned() {
            Some(Row::Param { name, .. }) => {
                let input = self.param_input(&name);
                self.open_prompt(Prompt::Param { name }, input);
            }
            Some(Row::Input { .. }) => self.connect_row(),
            _ => {}
        }
    }

    /// `c`: pick a source for the input (or number param) under the cursor.
    pub(super) fn connect_row(&mut self) {
        let port = match self.current_row().cloned() {
            Some(Row::Input { port, .. }) => port,
            Some(Row::Param { name, .. }) if self.linkable(&name) => name,
            Some(Row::Param { name, .. }) => {
                self.message = Some(format!(
                    "`{name}` can't be linked; only numbers and colors can"
                ));
                return;
            }
            _ => {
                self.message = Some("move the cursor to an input to connect it".into());
                return;
            }
        };
        let multi = self.input_spec(&port).is_some_and(|p| p.multi);
        self.open_pick(Pick::Connect {
            port,
            multi,
            index: None,
        });
    }

    /// `d`: disconnect the connection under the cursor; on a linked param, go back to
    /// its typed (or default) value.
    pub(super) fn disconnect_row(&mut self) {
        if let (
            Some(Row::Param {
                name,
                link: Some(link),
            }),
            Some(focus),
        ) = (self.current_row().cloned(), self.focus.clone())
        {
            let label = format!("unlinked {focus}.{name} from {}", link.short());
            self.apply_or_report(label, |dir, _| ops::disconnect(dir, &focus, &name, None));
            return;
        }
        let (
            Some(Row::Input {
                port,
                index,
                source: Some(source),
            }),
            Some(focus),
        ) = (self.current_row().cloned(), self.focus.clone())
        else {
            self.message = Some("move the cursor to a connected input to disconnect it".into());
            return;
        };
        let label = format!("disconnected {focus}.{port} ← {}", source.short());
        self.apply_or_report(label, |dir, _| ops::disconnect(dir, &focus, &port, index));
    }

    /// `J` / `K`: move the list connection under the cursor one place later (`step` 1) or
    /// earlier (-1). For `composite`'s layers, later draws on top.
    pub(super) fn move_row(&mut self, step: isize) {
        let (
            Some(Row::Input {
                port,
                index: Some(i),
                source: Some(source),
            }),
            Some(focus),
        ) = (self.current_row().cloned(), self.focus.clone())
        else {
            self.message = Some("move the cursor to a connection in a list to reorder it".into());
            return;
        };
        let len = self
            .rows
            .iter()
            .filter(|r| matches!(r, Row::Input { port: p, index: Some(_), .. } if *p == port))
            .count();
        let Some(j) = i.checked_add_signed(step).filter(|j| *j < len) else {
            let end = if step < 0 { "first" } else { "last" };
            self.message = Some(format!("{focus}.{port}[{i}] is already {end}"));
            return;
        };
        let label = format!("moved {focus}.{port} ← {} to [{j}]", source.short());
        if self.apply_or_report(label, |dir, _| {
            ops::swap_connections(dir, &focus, &port, i, j)
        }) {
            self.select_row(|r| {
                matches!(r, Row::Input { port: p, index: Some(k), .. } if *p == port && *k == j)
            });
        }
    }

    /// `i`: insert a new node into the connection under the cursor. On an unlinked number
    /// param, add a node (e.g. `int`) that feeds it, starting at the param's value.
    pub(super) fn insert_row(&mut self) {
        let node = self.focus.clone().unwrap_or_default();
        match self.current_row().cloned() {
            Some(Row::Input {
                port,
                index,
                source: Some(_),
            }) => self.open_pick(Pick::InsertType {
                node,
                port,
                index: index.unwrap_or(0),
            }),
            Some(Row::Param {
                name,
                link: Some(_),
            }) => self.open_pick(Pick::InsertType {
                node,
                port: name,
                index: 0,
            }),
            Some(Row::Param { name, link: None }) if self.linkable(&name) => {
                self.open_pick(Pick::FeedType { param: name })
            }
            _ => {
                self.message = Some(
                    "move the cursor to a connection or number param to insert a node there".into(),
                )
            }
        }
    }

    /// `i` in a tree: insert a new node into the connection that row stands for.
    pub(super) fn insert_tree_row(&mut self) {
        match self.tree_connection() {
            Some(c) => self.open_pick(Pick::InsertType {
                node: c.node,
                port: c.port,
                index: c.index,
            }),
            None => self.message = Some("move the cursor to a connection to insert a node".into()),
        }
    }

    /// Nudge keys: add `delta` to the number param under the cursor, clamped to its
    /// range. On a linked param, nudges the number of the node feeding it (e.g. its `int`).
    pub(super) fn nudge_row(&mut self, delta: i64) {
        if let Err(e) = self.try_nudge(delta) {
            self.message = Some(e);
        }
    }

    fn try_nudge(&mut self, delta: i64) -> Result<(), String> {
        let (node_id, name) = match self.current_row().cloned() {
            Some(Row::Param { name, link: None }) => (self.focus.clone().unwrap_or_default(), name),
            Some(Row::Param {
                name,
                link: Some(link),
            }) => {
                let source = self.project.get(&link.node).ok_or("the link is broken")?;
                let mut ints = source.spec.iter().flat_map(|s| &s.params).filter(|p| {
                    matches!(p.kind, ParamKind::Int { .. }) && !source.inputs.contains_key(p.name)
                });
                match (ints.next(), ints.next()) {
                    (Some(p), None) => (link.node.clone(), p.name.to_string()),
                    _ => {
                        return Err(format!(
                            "`{name}` is linked to {}; nudge it there",
                            link.short()
                        ));
                    }
                }
            }
            _ => return Err("move the cursor to a number param to nudge it".into()),
        };
        let node = self.project.get(&node_id).cloned().ok_or("no such node")?;
        let spec = node.spec.as_ref().and_then(|s| s.param(&name)).cloned();
        let kind = spec.as_ref().map(|p| p.kind.clone());
        let current = node
            .params
            .get(&name)
            .cloned()
            .or_else(|| spec.as_ref().and_then(ParamSpec::default_value));
        let no_value = || format!("`{name}` has no number to nudge");
        let (current, new): (toml::Value, toml::Value) = match kind {
            Some(ParamKind::Int { min, max }) => {
                let current = current.and_then(|v| v.as_integer()).ok_or_else(no_value)?;
                (
                    current.into(),
                    current.saturating_add(delta).clamp(min, max).into(),
                )
            }
            Some(ParamKind::Float { min, max }) => {
                let current = current
                    .and_then(|v| v.as_float().or_else(|| v.as_integer().map(|i| i as f64)))
                    .ok_or_else(no_value)?;
                let new = (current + delta as f64).clamp(min, max);
                (float_value(current), float_value(new))
            }
            Some(ParamKind::Color) => {
                return Err(format!("press ⏎ to adjust `{name}` in the color editor"));
            }
            _ => return Err(format!("only numbers can be nudged, not `{name}`")),
        };
        if new == current {
            let bound = if delta > 0 { "maximum" } else { "minimum" };
            return Err(format!(
                "{node_id}.{name} is at its {bound}, {}",
                plain(&current)
            ));
        }
        self.check_param(&node, &name, &new)?;

        let change = ops::set_param_synced(&self.dir, &self.project, &node_id, &name, Some(&new))
            .map_err(|e| e.to_string())?;
        // Repeated nudges of one param undo together.
        let key = format!("{node_id}.{name}");
        let label = format!("nudged {key} to {}", plain(&new));
        self.record_coalesced(key, label.clone(), change);
        self.message = Some(format!("{label} ({delta:+})"));
        Ok(())
    }

    /// Lets the node itself validate `name = value` (setting a param also unlinks it), but
    /// doesn't block fixing one param because another one is already invalid.
    fn check_param(
        &self,
        node: &ProjectNode,
        name: &str,
        value: &toml::Value,
    ) -> Result<(), String> {
        let (Some(node_impl), Some(spec)) = (
            node.kind.as_deref().and_then(|k| self.registry.get(k)),
            node.spec.as_ref(),
        ) else {
            return Ok(());
        };
        let links = node.inputs.keys().map(String::as_str);
        let before = spec.params_with_links(&node.params, links.clone());
        let mut after = spec.params_with_links(&node.params, links.filter(|l| *l != name));
        after.insert(name.to_string(), value.clone());
        match node_impl.check_params(&after) {
            Err(e) if node_impl.check_params(&before).is_ok() => Err(e.0),
            _ => Ok(()),
        }
    }

    /// The type of the input that feeds the focused node's param `name`, if linkable.
    fn param_input_type(&self, name: &str) -> Option<ValueType> {
        let input = self.input_spec(name)?;
        input.param.then_some(input.ty)
    }

    /// Whether the focused node's param `name` can be fed from another node.
    pub(super) fn linkable(&self, name: &str) -> bool {
        self.param_input_type(name).is_some()
    }

    pub(super) fn start_rename(&mut self) {
        if self.pane == super::Pane::Node
            && let Some(Row::Entry { param, key }) = self.current_row().cloned()
        {
            self.open_prompt(
                Prompt::RenameEntry {
                    param,
                    key: key.clone(),
                },
                key,
            );
            return;
        }
        if let Some(id) = self.focus.clone() {
            self.open_prompt(Prompt::Rename, id);
        }
    }

    pub(super) fn start_delete(&mut self) {
        let Some(id) = self.target_node() else { return };
        let users = self.project.consumers(&id);
        let effect = if users.is_empty() {
            String::new()
        } else {
            format!(" This disconnects {}.", uses(users))
        };
        self.mode = Mode::Confirm {
            question: format!("Delete `{id}`?{effect}"),
            purpose: Confirm::Delete(id),
        };
    }

    /// `X`: delete the node and connect what fed it to its users, after confirmation.
    pub(super) fn start_dissolve(&mut self) {
        let Some(id) = self.target_node() else { return };
        let plan = match ops::bypass_plan(&self.project, &id) {
            Ok(plan) => plan,
            Err(e) => {
                self.message = Some(format!("can't dissolve {id}: {e}"));
                return;
            }
        };
        let mut question = format!("Dissolve `{id}`?");
        if let (Some((_, through)), false) = (&plan.through, plan.reconnect.is_empty()) {
            question += &format!(" Connects {} → {}.", through.short(), uses(&plan.reconnect));
        }
        if !plan.disconnect.is_empty() {
            question += &format!(" Disconnects {}.", uses(&plan.disconnect));
        }
        self.mode = Mode::Confirm {
            question,
            purpose: Confirm::Dissolve(id),
        };
    }

    /// `X` in the flow view: replace an `array` or `split_text` with ordinary nodes, after
    /// confirmation. The plan is worked out now, from the project as it renders.
    pub(super) fn start_detach(&mut self, id: &str) -> Option<DetachPlan> {
        let plan = conduit_core::load(&self.dir, &self.registry)
            .map_err(|e| e.to_string())
            .and_then(|graph| {
                conduit_core::detach_plan(&graph, &self.registry, id).map_err(|e| e.to_string())
            });
        let plan = match plan {
            Ok(plan) => plan,
            Err(e) => {
                self.message = Some(format!("can't detach {id}: {e}"));
                return None;
            }
        };
        let users = self.project.consumers(id);
        let mut question = format!("Detach `{id}`? Makes {} nodes", plan.nodes.len());
        if let Some((out, name)) = users
            .first()
            .and_then(|c| plan.outputs.iter().find(|(out, _)| *out == c.output))
        {
            question += &format!("; {id}.{out} → {id}_{name}");
            if users.len() > 1 {
                question += " …";
            }
        }
        question.push('?');
        self.mode = Mode::Confirm {
            question,
            purpose: Confirm::Detach(id.to_string(), plan.clone()),
        };
        Some(plan)
    }

    pub(super) fn confirmed(&mut self, purpose: Confirm) {
        match purpose {
            Confirm::RemoveEntry { param, key } => self.remove_entry(&param, &key),
            Confirm::Delete(id) => {
                self.remove_node(format!("deleted {id}"), &id, ops::delete_node);
            }
            Confirm::Dissolve(id) => {
                let through = ops::bypass_plan(&self.project, &id)
                    .ok()
                    .and_then(|p| p.through.map(|(_, r)| format!(" ({} → …)", r.short())))
                    .unwrap_or_default();
                self.remove_node(format!("dissolved {id}{through}"), &id, ops::dissolve_node);
            }
            Confirm::Detach(id, plan) => {
                let label = format!("detached {id} into {} nodes", plan.nodes.len());
                self.apply_or_report(label, |dir, project| {
                    ops::detach_node(dir, project, &id, &plan)
                });
            }
        }
    }

    /// Runs an edit that removes `id`; if it was focused, goes back to an earlier node.
    fn remove_node(
        &mut self,
        label: String,
        id: &str,
        edit: fn(&Path, &Project, &str) -> EditResult<Change>,
    ) {
        let previous = self.back.iter().rev().find(|b| *b != id).cloned();
        let was_focused = self.focus.as_deref() == Some(id);
        if self.apply_or_report(label, |dir, project| edit(dir, project, id))
            && was_focused
            && let Some(previous) = previous.filter(|p| self.project.nodes.contains_key(p))
        {
            self.focus = Some(previous);
            self.refresh();
        }
    }

    /// `m`: pick up the node to move it; the next `m` or `⏎` on a connection drops it there.
    pub(super) fn start_move(&mut self) {
        if let Some(id) = self.carrying.take() {
            return self.drop_carried(id);
        }
        let Some(id) = self.target_node() else { return };
        if let Err(e) = ops::bypass_plan(&self.project, &id) {
            self.message = Some(format!("can't move {id}: {e}"));
            return;
        }
        self.message = Some(format!(
            "moving {id}: go to a connection, then m or ⏎ to put it there (esc cancels)"
        ));
        self.carrying = Some(id);
    }

    /// Puts the carried node `id` into the connection under the cursor. On failure it
    /// stays carried.
    pub(super) fn drop_carried(&mut self, id: String) {
        let Some(target) = self.cursor_connection() else {
            self.message = Some(format!(
                "moving {id}: that's not a connection; go to one, or esc to cancel"
            ));
            self.carrying = Some(id);
            return;
        };
        let label = format!(
            "moved {id} to {} → {}.{}",
            target.source.short(),
            target.node,
            target.port
        );
        if let Err(e) = self.apply(label, |dir, project| {
            ops::move_node(dir, project, &id, &target)
        }) {
            self.message = Some(format!("moving {id}: {e} (esc cancels)"));
            self.carrying = Some(id);
        }
    }

    /// The connection under the cursor: a tree row, or a connected input or linked param
    /// of the focused node.
    fn cursor_connection(&self) -> Option<ops::Connection> {
        if self.pane != super::Pane::Node {
            return self.tree_connection();
        }
        let node = self.focus.clone()?;
        let (port, index, source) = match self.current_row()? {
            Row::Input {
                port,
                index,
                source: Some(source),
            } => (port, index.unwrap_or(0), source),
            Row::Param {
                name,
                link: Some(link),
            } => (name, 0, link),
            _ => return None,
        };
        Some(ops::Connection {
            source: source.clone(),
            node,
            port: port.clone(),
            index,
        })
    }

    /// `u`: undo the most recent edit.
    pub(super) fn undo(&mut self) {
        self.nudging = None;
        let Some((label, change)) = self.undo.pop() else {
            self.message = Some("nothing to undo".into());
            return;
        };
        match change.undo() {
            Ok(()) => {
                self.reload_from_disk();
                self.message = Some(format!("undid: {label}"));
            }
            Err(e) => self.message = Some(format!("undo failed: {e}")),
        }
    }

    // --- helpers -----------------------------------------------------------------------

    /// Runs an edit; on success records it for undo, re-reads the project and reports it.
    pub(super) fn apply(
        &mut self,
        label: String,
        edit: impl FnOnce(&Path, &Project) -> EditResult<Change>,
    ) -> Result<(), String> {
        let change = edit(&self.dir, &self.project).map_err(|e| e.to_string())?;
        self.undo.push((label.clone(), change));
        self.nudging = None;
        self.reload_from_disk();
        self.message = Some(label);
        Ok(())
    }

    /// Like [`Self::apply`], reporting failure in the message line. Returns whether it worked.
    pub(super) fn apply_or_report(
        &mut self,
        label: String,
        edit: impl FnOnce(&Path, &Project) -> EditResult<Change>,
    ) -> bool {
        match self.apply(label, edit) {
            Ok(()) => true,
            Err(e) => {
                self.message = Some(format!("error: {e}"));
                false
            }
        }
    }

    pub(crate) fn reload_from_disk(&mut self) {
        match inspect(&self.dir, &self.registry) {
            Ok(project) => self.reload(project),
            Err(e) => self.message = Some(e.to_string()),
        }
    }

    fn spec(&self, kind: &str) -> Result<NodeSpec, String> {
        self.registry
            .spec(kind)
            .cloned()
            .ok_or_else(|| format!("unknown node type `{kind}`"))
    }

    /// `base` if it's free, else `base_<n>` with the smallest free `n`.
    fn free_id_like(&self, base: &str) -> String {
        if !self.project.nodes.contains_key(base) {
            return base.to_string();
        }
        self.free_id(base)
    }

    /// `<kind>_<n>` with the smallest `n` not already taken.
    fn free_id(&self, kind: &str) -> String {
        (1..)
            .map(|n| format!("{kind}_{n}"))
            .find(|id| !self.project.nodes.contains_key(id))
            .expect("unbounded")
    }

    /// Type of the source feeding `node.port[index]`, and the type that input expects.
    fn insert_types(&self, node: &str, port: &str, index: usize) -> Option<(ValueType, ValueType)> {
        let node = self.project.get(node)?;
        let target = node.spec.as_ref()?.input(port)?.ty;
        let source = node.inputs.get(port)?.get(index)?;
        let source_node = self.project.get(&source.node)?;
        let source_ty = source_node
            .spec
            .as_ref()?
            .output_type(&source_node.params, &source.port)?;
        Some((source_ty, target))
    }
}

/// `a.items, b.image` for a list of uses.
/// The node kind and list input that gather a node's grouped outputs (see
/// `PortSpec::grouped`).
const GROUP_KIND: &str = "group";
const GROUP_PORT: &str = "layers";

impl App {
    /// The id for the group a new node `id` of kind `spec` gets, if its outputs are grouped.
    fn group_for(&self, id: &str, spec: &NodeSpec) -> Option<String> {
        let grouped = spec.outputs.iter().any(|p| p.grouped);
        let registered = self.registry.spec(GROUP_KIND).is_some();
        (grouped && registered).then(|| self.free_id_like(&format!("{id}_group")))
    }
}

/// `change` (which added node `id`), followed by adding `group` gathering `id`'s outputs
/// when there is one, as one change.
fn with_group(
    dir: &Path,
    registry: &conduit_core::Registry,
    id: &str,
    group: Option<&str>,
    change: Change,
) -> EditResult<Change> {
    let Some(group) = group else {
        return Ok(change);
    };
    let grouped = inspect(dir, registry)
        .map_err(|e| ops::EditError::Invalid(e.to_string()))
        .and_then(|project| ops::group_repeated(dir, &project, id, group, GROUP_KIND, GROUP_PORT));
    match grouped {
        Ok(later) => Ok(change.then(later)),
        Err(e) => {
            let _ = change.undo();
            Err(e)
        }
    }
}

fn group_label(group: &Option<String>) -> String {
    group
        .as_ref()
        .map(|g| format!(", grouped by {g}"))
        .unwrap_or_default()
}

pub fn uses<'a>(consumers: impl IntoIterator<Item = &'a Consumer>) -> String {
    consumers
        .into_iter()
        .map(|c| format!("{}.{}", c.node, c.port))
        .collect::<Vec<_>>()
        .join(", ")
}

fn type_label(spec: &NodeSpec) -> String {
    let ports = |ports: &[conduit_core::PortSpec]| {
        ports
            .iter()
            .map(|p| p.ty.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    };
    // Inputs that only feed params are left out: every number param has one.
    let inputs: Vec<_> = spec.inputs.iter().filter(|p| !p.param).cloned().collect();
    format!(
        "{}  ({}) → ({})",
        spec.kind,
        ports(&inputs),
        ports(&spec.outputs)
    )
}

/// The first output of a new node of this kind (default params, so no palette entries)
/// that can feed a `ty` param.
fn output_of(spec: &NodeSpec, ty: ValueType) -> Option<String> {
    spec.outputs_for(&toml::Table::new())
        .into_iter()
        .find(|(_, t)| t.coerces_to(ty))
        .map(|(name, _)| name)
}

/// The param a new feeding node is seeded with (e.g. `int`'s `value`, `color`'s `color`).
fn param_input_of(spec: &NodeSpec, ty: ValueType) -> Option<&'static str> {
    spec.inputs
        .iter()
        .find(|p| p.param && p.ty == ty)
        .map(|p| p.name)
}

/// Whether a node of this kind can sit between a `source_ty` output and a `target_ty` input.
fn insertable(spec: &NodeSpec, source_ty: ValueType, target_ty: ValueType) -> bool {
    spec.inputs.iter().any(|p| source_ty.coerces_to(p.ty))
        && spec.outputs.iter().any(|o| o.ty.coerces_to(target_ty))
}

/// A TOML literal (`300`, `"text"`, `true`), or the text itself if it isn't one.
fn parse_literal(input: &str) -> toml::Value {
    conduit_core::parse_literal(input).unwrap_or_else(|| toml::Value::String(input.to_string()))
}

/// A number as shown in messages (`360`, `2.5`).
fn plain(v: &toml::Value) -> String {
    match v {
        toml::Value::Float(f) => f.to_string(),
        other => other.to_string(),
    }
}
