//! Editing from the TUI: pickers, prompts and confirmations that change node files through
//! `conduit_core::edit`, each recorded for undo.

use std::path::Path;

use conduit_core::edit::{self as ops, Change, EditResult};
use conduit_core::{NodeSpec, ParamKind, Project, Reference, ValueType, inspect};

use super::{Action, App, Mode, Row};
use crate::edit::finder;

/// What a picker list is choosing.
#[derive(Debug, Clone, PartialEq)]
pub enum Pick {
    FindNode,
    /// A source for the focused node's input.
    Connect {
        port: String,
        multi: bool,
    },
    /// The type of a new node.
    AddType,
    /// The type of a node to insert before the focused node's input.
    InsertType {
        port: String,
        index: usize,
    },
    /// The type of a new node to feed the focused node's number param.
    FeedType {
        param: String,
    },
}

impl Pick {
    pub fn title(&self, focus: &str) -> String {
        match self {
            Pick::FindNode => "find node".into(),
            Pick::Connect { port, .. } => format!("connect {focus}.{port} to"),
            Pick::AddType => "add node of type".into(),
            Pick::InsertType { port, .. } => format!("insert before {focus}.{port}: type"),
            Pick::FeedType { param } => format!("feed {focus}.{param} from a new"),
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
        port: String,
        index: usize,
    },
    FeedId {
        kind: String,
        param: String,
    },
    Rename,
}

impl Prompt {
    pub fn label(&self, focus: &str) -> String {
        match self {
            Prompt::Param { name } => format!("{focus}.{name} (empty = default)"),
            Prompt::AddId { kind } => format!("id for new {kind}"),
            Prompt::InsertId { kind, .. } | Prompt::FeedId { kind, .. } => {
                format!("id for new {kind}")
            }
            Prompt::Rename => format!("rename {focus} to"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Confirm {
    Delete(String),
}

impl App {
    // --- pickers -----------------------------------------------------------------------

    pub(super) fn open_pick(&mut self, purpose: Pick) {
        if self.pick_items(&purpose).is_empty() && purpose != Pick::FindNode {
            self.message = Some(match purpose {
                Pick::Connect { .. } => "no compatible outputs to connect".into(),
                Pick::InsertType { .. } | Pick::FeedType { .. } => "no node type fits here".into(),
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
                    .map(|r| (r.to_string(), format!("{}  {}", short(&r), kind(&r.node))))
                    .collect()
            }
            Pick::AddType => self
                .registry
                .specs()
                .map(|s| (s.kind.to_string(), type_label(s)))
                .collect(),
            Pick::InsertType { port, index } => {
                let Some((source_ty, target_ty)) = self.insert_types(port, *index) else {
                    return Vec::new();
                };
                self.registry
                    .specs()
                    .filter(|s| insertable(s, source_ty, target_ty))
                    .map(|s| (s.kind.to_string(), type_label(s)))
                    .collect()
            }
            Pick::FeedType { .. } => self
                .registry
                .specs()
                .filter(|s| number_output(s).is_some())
                .map(|s| (s.kind.to_string(), type_label(s)))
                .collect(),
        }
    }

    /// The picker's choices matching its query, best first.
    pub fn pick_matches(&self) -> Vec<(String, String)> {
        let Mode::Pick { purpose, query, .. } = &self.mode else {
            return Vec::new();
        };
        let items = self.pick_items(purpose);
        finder::rank(
            query,
            items.iter().map(|item| (item.clone(), item.1.as_str())),
        )
    }

    pub(super) fn choose(&mut self, purpose: Pick, key: String) -> Option<Action> {
        match purpose {
            Pick::FindNode => return self.focus_on(&key),
            Pick::Connect { port, multi } => {
                let (Some(focus), Ok(source)) = (self.focus.clone(), Reference::parse(&key)) else {
                    return None;
                };
                let label = format!("connected {focus}.{port} ← {}", short(&source));
                self.apply_or_report(label, |dir, _| {
                    ops::connect(dir, &focus, &port, &source, multi)
                });
            }
            Pick::AddType => {
                let id = self.free_id(&key);
                self.open_prompt(Prompt::AddId { kind: key }, id);
            }
            Pick::InsertType { port, index } => {
                let id = self.free_id(&key);
                self.open_prompt(
                    Prompt::InsertId {
                        kind: key,
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
        }
        None
    }

    // --- prompts -----------------------------------------------------------------------

    fn open_prompt(&mut self, purpose: Prompt, input: String) {
        self.mode = Mode::Prompt {
            purpose,
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
                self.apply(format!("added {input} ({kind})"), |dir, _| {
                    ops::add_node(dir, input, &spec)
                })?;
                Ok(self.focus_on(input))
            }
            Prompt::InsertId { kind, port, index } => {
                let spec = self.spec(&kind)?;
                let label = format!("inserted {input} ({kind}) before {focus}.{port}");
                self.apply(label, |dir, project| {
                    ops::insert_between(dir, project, &focus, &port, index, input, &spec)
                })?;
                Ok(self.focus_on(input))
            }
            Prompt::FeedId { kind, param } => {
                let spec = self.spec(&kind)?;
                let output = number_output(&spec).ok_or("no number output")?.to_string();
                // Start the new node at the param's current value.
                let params = match (self.current_value(&param), first_number_param(&spec)) {
                    (Some(value), Some(name)) => {
                        toml::Table::from_iter([(name.to_string(), value)])
                    }
                    _ => toml::Table::new(),
                };
                let label = format!("added {input} ({kind}) feeding {focus}.{param}");
                self.apply(label, |dir, _| {
                    ops::add_feeding(dir, input, &spec, &params, &output, &focus, &param)
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
    fn current_value(&self, name: &str) -> Option<toml::Value> {
        let node = self.focused()?;
        if let Some(value) = node.params.get(name) {
            return Some(value.clone());
        }
        let spec = node.spec.as_ref()?.param(name)?;
        spec.default.map(parse_literal)
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
            return self.apply(format!("reset {id}.{name}"), |dir, _| {
                ops::remove_param(dir, &id, name)
            });
        }
        let value = match &spec {
            Some(p) => p.parse_input(input)?,
            None => parse_literal(input),
        };
        // Let the node itself validate the result, but don't block fixing one param
        // because another one is already invalid.
        if let Some(node_impl) = node.kind.as_deref().and_then(|k| self.registry.get(k)) {
            let mut params = node.params.clone();
            params.insert(name.to_string(), value.clone());
            if let Err(e) = node_impl.check_params(&params)
                && node_impl.check_params(&node.params).is_ok()
            {
                return Err(e.0);
            }
        }
        self.apply(format!("set {id}.{name} = {value}"), |dir, _| {
            ops::set_param(dir, &id, name, &value)
        })
    }

    // --- direct edits ------------------------------------------------------------------

    /// `⏎` in the node panel: edit a param (typing a value replaces a link), or pick a
    /// source for an input.
    pub(super) fn edit_row(&mut self) {
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
                self.message = Some(format!("`{name}` can't be linked; only numbers can"));
                return;
            }
            _ => {
                self.message = Some("move the cursor to an input to connect it".into());
                return;
            }
        };
        let multi = self
            .focused()
            .and_then(|n| n.spec.as_ref())
            .and_then(|s| s.input(&port))
            .is_some_and(|p| p.multi);
        self.open_pick(Pick::Connect { port, multi });
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
            let label = format!("unlinked {focus}.{name} from {}", short(&link));
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
        let label = format!("disconnected {focus}.{port} ← {}", short(&source));
        self.apply_or_report(label, |dir, _| ops::disconnect(dir, &focus, &port, index));
    }

    /// `i`: insert a new node into the connection under the cursor. On an unlinked number
    /// param, add a node (e.g. `int`) that feeds it, starting at the param's value.
    pub(super) fn insert_row(&mut self) {
        match self.current_row().cloned() {
            Some(Row::Input {
                port,
                index,
                source: Some(_),
            }) => self.open_pick(Pick::InsertType {
                port,
                index: index.unwrap_or(0),
            }),
            Some(Row::Param {
                name,
                link: Some(_),
            }) => self.open_pick(Pick::InsertType {
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

    /// Nudge keys: add `delta` to the whole-number param under the cursor, clamped to its
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
                            short(&link)
                        ));
                    }
                }
            }
            _ => return Err("move the cursor to a whole-number param to nudge it".into()),
        };
        let node = self.project.get(&node_id).cloned().ok_or("no such node")?;
        let spec = node.spec.as_ref().and_then(|s| s.param(&name)).cloned();
        let Some(ParamKind::Int { min, max }) = spec.as_ref().map(|p| p.kind.clone()) else {
            return Err(format!("only whole numbers can be nudged, not `{name}`"));
        };
        let current = node
            .params
            .get(&name)
            .cloned()
            .or_else(|| spec.as_ref().and_then(|p| p.default).map(parse_literal))
            .and_then(|v| v.as_integer())
            .ok_or_else(|| format!("`{name}` has no whole-number value to nudge"))?;
        let new = current.saturating_add(delta).clamp(min, max);
        if new == current {
            let bound = if delta > 0 { "maximum" } else { "minimum" };
            return Err(format!("{node_id}.{name} is at its {bound}, {current}"));
        }
        let mut params = node.params.clone();
        params.insert(name.clone(), new.into());
        if let Some(node_impl) = node.kind.as_deref().and_then(|k| self.registry.get(k))
            && let Err(e) = node_impl.check_params(&params)
            && node_impl.check_params(&node.params).is_ok()
        {
            return Err(e.0);
        }

        let change =
            ops::set_param(&self.dir, &node_id, &name, &new.into()).map_err(|e| e.to_string())?;
        let key = format!("{node_id}.{name}");
        let label = format!("nudged {key} to {new}");
        // Repeated nudges of one param undo together: keep the first change, which holds
        // the contents from before the first nudge.
        match self.undo.last_mut() {
            Some(last) if self.nudging.as_ref() == Some(&key) => last.0 = label.clone(),
            _ => self.undo.push((label.clone(), change)),
        }
        self.nudging = Some(key);
        self.reload_from_disk();
        self.message = Some(format!("{label} ({delta:+})"));
        Ok(())
    }

    /// Whether the focused node's param `name` can be fed from another node.
    fn linkable(&self, name: &str) -> bool {
        self.focused()
            .and_then(|n| n.spec.as_ref())
            .and_then(|s| s.input(name))
            .is_some_and(|p| p.param)
    }

    pub(super) fn start_rename(&mut self) {
        if let Some(id) = self.focus.clone() {
            self.open_prompt(Prompt::Rename, id);
        }
    }

    pub(super) fn start_delete(&mut self) {
        let Some(id) = self.focus.clone() else { return };
        let users: Vec<String> = self
            .project
            .consumers(&id)
            .iter()
            .map(|c| format!("{}.{}", c.node, c.port))
            .collect();
        let effect = if users.is_empty() {
            String::new()
        } else {
            format!(" This disconnects {}.", users.join(", "))
        };
        self.mode = Mode::Confirm {
            question: format!("Delete `{id}`?{effect}"),
            purpose: Confirm::Delete(id),
        };
    }

    pub(super) fn confirmed(&mut self, purpose: Confirm) {
        match purpose {
            Confirm::Delete(id) => {
                let previous = self.back.iter().rev().find(|b| **b != id).cloned();
                if self.apply_or_report(format!("deleted {id}"), |dir, project| {
                    ops::delete_node(dir, project, &id)
                }) && let Some(previous) =
                    previous.filter(|p| self.project.nodes.contains_key(p))
                {
                    self.focus = Some(previous);
                    self.refresh();
                }
            }
        }
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
    fn apply(
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
    fn apply_or_report(
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

    /// Type of the source feeding `focus.port[index]`, and the type that input expects.
    fn insert_types(&self, port: &str, index: usize) -> Option<(ValueType, ValueType)> {
        let node = self.focused()?;
        let target = node.spec.as_ref()?.input(port)?.ty;
        let source = node.inputs.get(port)?.get(index)?;
        let source_ty = self
            .project
            .get(&source.node)?
            .spec
            .as_ref()?
            .output(&source.port)?
            .ty;
        Some((source_ty, target))
    }
}

/// `"node"` for the default output, `"node.port"` otherwise.
pub(super) fn short(r: &Reference) -> String {
    if r.port == "out" {
        r.node.clone()
    } else {
        r.to_string()
    }
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

/// The first output that can feed a number param.
fn number_output(spec: &NodeSpec) -> Option<&'static str> {
    spec.outputs
        .iter()
        .find(|o| o.ty.coerces_to(ValueType::Number))
        .map(|o| o.name)
}

/// The param a new feeding node is seeded with (e.g. `int`'s `value`).
fn first_number_param(spec: &NodeSpec) -> Option<&'static str> {
    spec.inputs.iter().find(|p| p.param).map(|p| p.name)
}

/// Whether a node of this kind can sit between a `source_ty` output and a `target_ty` input.
fn insertable(spec: &NodeSpec, source_ty: ValueType, target_ty: ValueType) -> bool {
    spec.inputs.iter().any(|p| source_ty.coerces_to(p.ty))
        && spec.outputs.iter().any(|o| o.ty.coerces_to(target_ty))
}

/// A TOML literal (`300`, `"text"`, `true`), or the text itself if it isn't one.
fn parse_literal(input: &str) -> toml::Value {
    toml::from_str::<toml::Table>(&format!("v = {input}"))
        .ok()
        .and_then(|mut t| t.remove("v"))
        .unwrap_or_else(|| toml::Value::String(input.to_string()))
}
