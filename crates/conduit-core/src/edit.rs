//! File-level edits to a project, for editors. Every edit preserves comments and formatting
//! (via `toml_edit`), is all-or-nothing, and returns a [`Change`] that can undo it.

use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

use crate::graph::{Reference, is_ident};
use crate::node::NodeSpec;
use crate::project::{Consumer, Project};
use crate::value::ValueType;

#[derive(Debug, thiserror::Error)]
pub enum EditError {
    #[error("{}: {source}", path.display())]
    Io { path: PathBuf, source: io::Error },
    #[error("{}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("{0}")]
    Invalid(String),
}

pub type EditResult<T> = Result<T, EditError>;

/// The previous contents of every file an edit touched (`None` = file didn't exist).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Change {
    before: Vec<(PathBuf, Option<String>)>,
}

impl Change {
    /// Restores every touched file to its state before the edit.
    pub fn undo(&self) -> EditResult<()> {
        for (path, contents) in self.before.iter().rev() {
            match contents {
                Some(text) => std::fs::write(path, text),
                None => std::fs::remove_file(path).or_else(ignore_not_found),
            }
            .map_err(io_err(path))?;
        }
        Ok(())
    }
}

/// A [`Change`] that puts `paths` back to their current contents: for edits made by another
/// program (e.g. the GUI color picker) that should still be undoable.
pub fn snapshot(paths: &[PathBuf]) -> EditResult<Change> {
    let mut tx = Tx::default();
    for path in paths {
        tx.record(path)?;
    }
    Ok(Change { before: tx.before })
}

fn io_err(path: &Path) -> impl FnOnce(io::Error) -> EditError + '_ {
    move |source| EditError::Io {
        path: path.to_path_buf(),
        source,
    }
}

fn ignore_not_found(e: io::Error) -> io::Result<()> {
    if e.kind() == io::ErrorKind::NotFound {
        Ok(())
    } else {
        Err(e)
    }
}

/// Records each file's contents before its first modification, so the whole edit can be
/// rolled back on failure or undone later.
#[derive(Default)]
struct Tx {
    before: Vec<(PathBuf, Option<String>)>,
}

impl Tx {
    fn record(&mut self, path: &Path) -> EditResult<()> {
        if self.before.iter().any(|(p, _)| p == path) {
            return Ok(());
        }
        let contents = match std::fs::read_to_string(path) {
            Ok(text) => Some(text),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(io_err(path)(e)),
        };
        self.before.push((path.to_path_buf(), contents));
        Ok(())
    }

    fn read(&self, path: &Path) -> EditResult<String> {
        std::fs::read_to_string(path).map_err(io_err(path))
    }

    fn read_doc(&self, path: &Path) -> EditResult<DocumentMut> {
        self.read(path)?
            .parse()
            .map_err(|e: toml_edit::TomlError| EditError::Parse {
                path: path.to_path_buf(),
                message: e.message().to_string(),
            })
    }

    fn write(&mut self, path: &Path, text: &str) -> EditResult<()> {
        self.record(path)?;
        std::fs::write(path, text).map_err(io_err(path))
    }

    fn remove(&mut self, path: &Path) -> EditResult<()> {
        self.record(path)?;
        std::fs::remove_file(path).map_err(io_err(path))
    }

    /// Reads a node file, applies `f` to it, and writes it back.
    fn update(
        &mut self,
        path: &Path,
        f: impl FnOnce(&mut DocumentMut) -> EditResult<()>,
    ) -> EditResult<()> {
        let mut doc = self.read_doc(path)?;
        f(&mut doc)?;
        self.write(path, &doc.to_string())
    }
}

/// Runs an edit; on failure, restores everything it had already changed.
fn transact(f: impl FnOnce(&mut Tx) -> EditResult<()>) -> EditResult<Change> {
    let mut tx = Tx::default();
    let result = f(&mut tx);
    let change = Change { before: tx.before };
    match result {
        Ok(()) => Ok(change),
        Err(e) => {
            let _ = change.undo();
            Err(e)
        }
    }
}

fn node_path(dir: &Path, id: &str) -> PathBuf {
    dir.join(format!("{id}.toml"))
}

/// The `[name]` sub-table of `table`, created if absent.
fn sub_table<'a>(table: &'a mut dyn TableLike, name: &str) -> EditResult<&'a mut dyn TableLike> {
    table
        .entry(name)
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .ok_or_else(|| EditError::Invalid(format!("`{name}` is not a table")))
}

/// The `[name]` table of a document, created if absent.
fn table_mut<'a>(doc: &'a mut DocumentMut, name: &str) -> EditResult<&'a mut dyn TableLike> {
    sub_table(doc.as_table_mut(), name)
}

/// Sets `key = value` in `table`, keeping the old value's formatting if there was one.
fn upsert(table: &mut dyn TableLike, key: &str, value: toml_edit::Value) {
    match table.get_mut(key) {
        Some(item) => replace_value(item, value),
        None => {
            table.insert(key, Item::Value(value));
        }
    }
}

fn to_edit_value(value: &toml::Value) -> EditResult<toml_edit::Value> {
    value
        .to_string()
        .parse()
        .map_err(|_| EditError::Invalid(format!("cannot write value {value}")))
}

/// Replaces an item's value, keeping its surrounding whitespace and trailing comment.
fn replace_value(item: &mut Item, new: toml_edit::Value) {
    let new = match item.as_value() {
        Some(old) => with_decor(new, old),
        None => new,
    };
    *item = Item::Value(new);
}

/// `new` with `old`'s surrounding whitespace and comments.
fn with_decor(mut new: toml_edit::Value, old: &toml_edit::Value) -> toml_edit::Value {
    *new.decor_mut() = old.decor().clone();
    new
}

/// Sets `[params] name = value` on a node. Also removes any input linked to that param,
/// since a linked value would override the one being set.
pub fn set_param(dir: &Path, node: &str, name: &str, value: &toml::Value) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            set_param_in(doc, name, value)?;
            unlink_param_in(doc, name);
            Ok(())
        })
    })
}

fn set_param_in(doc: &mut DocumentMut, name: &str, value: &toml::Value) -> EditResult<()> {
    upsert(table_mut(doc, "params")?, name, to_edit_value(value)?);
    Ok(())
}

fn unlink_param_in(doc: &mut DocumentMut, name: &str) {
    if let Some(inputs) = doc.get_mut("inputs").and_then(Item::as_table_like_mut) {
        inputs.remove(name);
    }
}

/// Removes `[params] name` from a node, and any input linked to it, so its default applies.
pub fn remove_param(dir: &Path, node: &str, name: &str) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            table_mut(doc, "params")?.remove(name);
            unlink_param_in(doc, name);
            Ok(())
        })
    })
}

/// Connects `source` to `node`'s input `port`. For list inputs (`multi`) the source is
/// appended; otherwise it replaces any existing connection.
pub fn connect(
    dir: &Path,
    node: &str,
    port: &str,
    source: &Reference,
    multi: bool,
) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            connect_in(doc, port, source, multi)
        })
    })
}

fn connect_in(
    doc: &mut DocumentMut,
    port: &str,
    source: &Reference,
    multi: bool,
) -> EditResult<()> {
    let text = source.short();
    let inputs = table_mut(doc, "inputs")?;
    match inputs.get_mut(port) {
        Some(item) if multi => {
            let value = item.as_value_mut().ok_or_else(|| invalid_input(port))?;
            match value {
                toml_edit::Value::Array(items) => items.push(text),
                single => {
                    // A single reference written for a list input: make it a list.
                    let mut items = toml_edit::Array::new();
                    items.push(single.clone().decorated("", ""));
                    items.push(text);
                    replace_value(item, toml_edit::Value::Array(items));
                }
            }
        }
        Some(item) => replace_value(item, text.into()),
        None => {
            let value: toml_edit::Value = if multi {
                toml_edit::Value::Array(toml_edit::Array::from_iter([text]))
            } else {
                text.into()
            };
            inputs.insert(port, Item::Value(value));
        }
    }
    Ok(())
}

fn invalid_input(port: &str) -> EditError {
    EditError::Invalid(format!("input `{port}` is not a reference or list"))
}

/// Makes `node`'s input `port` (list element `index`, or the single value) take `source`
/// instead of what it takes now.
pub fn reconnect(
    dir: &Path,
    node: &str,
    port: &str,
    index: usize,
    source: &Reference,
) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            replace_reference(doc, port, index, source)
        })
    })
}

/// Disconnects `node`'s input `port`: the list element at `index`, or the whole input.
/// Removing a list's last element removes the input.
pub fn disconnect(dir: &Path, node: &str, port: &str, index: Option<usize>) -> EditResult<Change> {
    transact(|tx| tx.update(&node_path(dir, node), |doc| disconnect_in(doc, port, index)))
}

fn disconnect_in(doc: &mut DocumentMut, port: &str, index: Option<usize>) -> EditResult<()> {
    let inputs = table_mut(doc, "inputs")?;
    let Some(item) = inputs.get_mut(port) else {
        return Ok(());
    };
    if let (Some(i), Some(items)) = (index, item.as_array_mut()) {
        if i < items.len() {
            items.remove(i);
        }
        if !items.is_empty() {
            return Ok(());
        }
    }
    inputs.remove(port);
    Ok(())
}

/// Swaps elements `a` and `b` of `node`'s list input `port` (e.g. to change which of
/// `composite`'s layers is drawn on top). Each position keeps its own formatting.
pub fn swap_connections(
    dir: &Path,
    node: &str,
    port: &str,
    a: usize,
    b: usize,
) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            let items = table_mut(doc, "inputs")?
                .get_mut(port)
                .and_then(Item::as_array_mut)
                .ok_or_else(|| EditError::Invalid(format!("input `{port}` is not a list")))?;
            let len = items.len();
            let (Some(va), Some(vb)) = (items.get(a).cloned(), items.get(b).cloned()) else {
                return Err(EditError::Invalid(format!(
                    "input `{port}` has {len} connection(s); can't swap {a} and {b}"
                )));
            };
            items.replace_formatted(a, with_decor(vb.clone(), &va));
            items.replace_formatted(b, with_decor(va, &vb));
            Ok(())
        })
    })
}

/// Creates a new node file from `spec`'s template.
pub fn add_node(dir: &Path, id: &str, spec: &NodeSpec) -> EditResult<Change> {
    transact(|tx| add_raw(tx, dir, id, &spec.template()))
}

/// Creates a node from `spec`'s template with `params` set, and connects its output
/// `output` to `node`'s input `port` (replacing any connection there). E.g. a new `int`
/// node feeding a number param.
pub fn add_feeding(
    dir: &Path,
    new_id: &str,
    spec: &NodeSpec,
    params: &toml::Table,
    output: &str,
    node: &str,
    port: &str,
) -> EditResult<Change> {
    add_feeding_to(dir, new_id, spec, params, output, node, port, false)
}

/// As [`add_feeding`]; with `multi`, the new node is appended to the list input `port`
/// instead of replacing what's there.
#[allow(clippy::too_many_arguments)]
pub fn add_feeding_to(
    dir: &Path,
    new_id: &str,
    spec: &NodeSpec,
    params: &toml::Table,
    output: &str,
    node: &str,
    port: &str,
    multi: bool,
) -> EditResult<Change> {
    if spec.output(output).is_none() {
        return Err(EditError::Invalid(format!(
            "`{}` has no output `{output}`",
            spec.kind
        )));
    }
    let source = Reference {
        node: new_id.to_string(),
        port: output.to_string(),
    };
    transact(|tx| {
        add_raw(tx, dir, new_id, &spec.template())?;
        tx.update(&node_path(dir, new_id), |doc| {
            params
                .iter()
                .try_for_each(|(name, value)| set_param_in(doc, name, value))
        })?;
        tx.update(&node_path(dir, node), |doc| {
            connect_in(doc, port, &source, multi)
        })
    })
}

/// Adds a node of kind `spec` between `node`'s input `port` (list element `index`) and
/// whatever currently feeds it: `source → new → node`.
pub fn insert_between(
    dir: &Path,
    project: &Project,
    node: &str,
    port: &str,
    index: usize,
    new_id: &str,
    spec: &NodeSpec,
) -> EditResult<Change> {
    let target = project.get(node).ok_or_else(|| missing(node))?;
    let target_port = target
        .spec
        .as_ref()
        .and_then(|s| s.input(port))
        .ok_or_else(|| EditError::Invalid(format!("`{node}` has no input `{port}`")))?;
    let source = target
        .inputs
        .get(port)
        .and_then(|refs| refs.get(index))
        .ok_or_else(|| EditError::Invalid(format!("`{node}.{port}` is not connected")))?;
    let source_ty = output_type(project, source)
        .ok_or_else(|| EditError::Invalid(format!("cannot tell the type of `{source}`")))?;

    let new_input = spec
        .inputs
        .iter()
        .find(|p| source_ty.coerces_to(p.ty))
        .ok_or_else(|| {
            EditError::Invalid(format!(
                "`{}` has no input that accepts {source_ty}",
                spec.kind
            ))
        })?;
    let new_output = spec
        .outputs
        .iter()
        .find(|o| o.ty.coerces_to(target_port.ty))
        .ok_or_else(|| {
            EditError::Invalid(format!(
                "`{}` has no output usable as {}",
                spec.kind, target_port.ty
            ))
        })?;
    let new_ref = Reference {
        node: new_id.to_string(),
        port: new_output.name.to_string(),
    };

    transact(|tx| {
        add_raw(tx, dir, new_id, &spec.template())?;
        tx.update(&node_path(dir, new_id), |doc| {
            connect_in(doc, new_input.name, source, new_input.multi)
        })?;
        tx.update(&node_path(dir, node), |doc| {
            replace_reference(doc, port, index, &new_ref)
        })
    })
}

/// Replaces the reference at `port` (list element `index`, or the single value).
fn replace_reference(
    doc: &mut DocumentMut,
    port: &str,
    index: usize,
    new: &Reference,
) -> EditResult<()> {
    let text = new.short();
    let item = table_mut(doc, "inputs")?
        .get_mut(port)
        .ok_or_else(|| invalid_input(port))?;
    match item.as_array_mut() {
        Some(items) => {
            let slot = items.get_mut(index).ok_or_else(|| invalid_input(port))?;
            *slot = with_decor(text.into(), slot);
        }
        None => replace_value(item, text.into()),
    }
    Ok(())
}

/// Renames a node: moves its file and rewrites every reference to it.
pub fn rename_node(dir: &Path, project: &Project, old: &str, new: &str) -> EditResult<Change> {
    let node = project.get(old).ok_or_else(|| missing(old))?;
    transact(|tx| {
        let text = tx.read(&node.path)?;
        // Create the new file first so an invalid or taken name fails before anything moves.
        add_raw(tx, dir, new, &text)?;
        tx.remove(&node.path)?;

        for consumer in consumer_ids(project, old, |_| true) {
            // A node that references itself now lives in the new file.
            let id = if consumer == old { new } else { consumer };
            tx.update(&node_path(dir, id), |doc| {
                rewrite_references(doc, |r| {
                    (r.node == old).then(|| Reference {
                        node: new.into(),
                        port: r.port.clone(),
                    })
                });
                Ok(())
            })?;
        }
        Ok(())
    })
}

fn add_raw(tx: &mut Tx, dir: &Path, id: &str, text: &str) -> EditResult<()> {
    if !is_ident(id) {
        return Err(EditError::Invalid(format!(
            "`{id}` is not a valid node id (letters, digits, `_`, `-`)"
        )));
    }
    let path = node_path(dir, id);
    if path.exists() {
        return Err(EditError::Invalid(format!("node `{id}` already exists")));
    }
    tx.write(&path, text)
}

/// Deletes a node's file and disconnects every input that referenced it.
pub fn delete_node(dir: &Path, project: &Project, id: &str) -> EditResult<Change> {
    let node = project.get(id).ok_or_else(|| missing(id))?;
    transact(|tx| {
        for consumer in consumer_ids(project, id, |c| c.node != id) {
            tx.update(&node_path(dir, consumer), |doc| {
                remove_references(doc, |r| r.node == id);
                Ok(())
            })?;
        }
        tx.remove(&node.path)
    })
}

/// One connection: `node.port[index]` takes `source` (`index` is 0 for single inputs).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub source: Reference,
    pub node: String,
    pub port: String,
    pub index: usize,
}

/// How taking a node out of the graph would rewire it (see [`bypass_plan`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bypass {
    /// The node's input that is passed through, and what feeds it. `None` if nothing
    /// feeds the node, so its users can only be disconnected.
    pub through: Option<(String, Reference)>,
    /// Uses of the node that will take `through` instead.
    pub reconnect: Vec<Consumer>,
    /// Uses that can't take `through` (wrong type, or nothing to pass) and will be
    /// disconnected.
    pub disconnect: Vec<Consumer>,
}

/// Plans taking `id` out from between its sources and its users: its first connected
/// input (in spec order, plain inputs before linked params) is passed through to every
/// user that accepts its type. So `a → b → c` becomes `a → c`, and taking out a `color`
/// node links its users to the palette color feeding it.
pub fn bypass_plan(project: &Project, id: &str) -> EditResult<Bypass> {
    let node = project.get(id).ok_or_else(|| missing(id))?;
    let mut through = None;
    if let Some(spec) = &node.spec {
        let ports = spec.inputs.iter().filter(|p| !p.param);
        for port in ports.chain(spec.inputs.iter().filter(|p| p.param)) {
            let refs: Vec<&Reference> = node
                .inputs
                .get(port.name)
                .into_iter()
                .flatten()
                .filter(|r| r.node != id)
                .collect();
            match refs.as_slice() {
                [] => continue,
                [r] => {
                    through = Some((port.name.to_string(), (*r).clone()));
                    break;
                }
                many => {
                    return Err(EditError::Invalid(format!(
                        "`{id}.{}` has {} connections; can't tell which one to reconnect",
                        port.name,
                        many.len()
                    )));
                }
            }
        }
    }
    let through_ty = through.as_ref().and_then(|(_, r)| output_type(project, r));
    let (mut reconnect, mut disconnect) = (Vec::new(), Vec::new());
    for c in project.consumers(id).iter().filter(|c| c.node != id) {
        let fits = through_ty
            .zip(input_type(project, &c.node, &c.port))
            .is_some_and(|(from, to)| from.coerces_to(to));
        if fits {
            reconnect.push(c.clone());
        } else {
            disconnect.push(c.clone());
        }
    }
    Ok(Bypass {
        through,
        reconnect,
        disconnect,
    })
}

fn output_type(project: &Project, r: &Reference) -> Option<ValueType> {
    let n = project.get(&r.node)?;
    n.spec.as_ref()?.output_type(&n.params, &r.port)
}

fn input_type(project: &Project, node: &str, port: &str) -> Option<ValueType> {
    Some(project.get(node)?.spec.as_ref()?.input(port)?.ty)
}

/// The reference at `port` (list element `index`, or the single value).
fn reference_at(doc: &DocumentMut, port: &str, index: usize) -> Option<Reference> {
    let value = doc.get("inputs")?.as_table_like()?.get(port)?.as_value()?;
    let value = match value.as_array() {
        Some(items) => items.get(index)?,
        None if index == 0 => value,
        None => return None,
    };
    Reference::parse(value.as_str()?).ok()
}

/// Checks `port[index]` still references `expected` (the file may have changed since the
/// project was read).
fn expect_reference(
    doc: &DocumentMut,
    node: &str,
    port: &str,
    index: usize,
    expected: &Reference,
) -> EditResult<()> {
    if reference_at(doc, port, index).as_ref() == Some(expected) {
        Ok(())
    } else {
        Err(EditError::Invalid(format!(
            "`{node}.{port}` no longer takes `{expected}`; the file changed, try again"
        )))
    }
}

/// Uses of a node in one file, each with its new source (`None`: disconnect).
type Uses<'a> = Vec<(&'a Consumer, Option<&'a Reference>)>;

/// Rewires `id`'s users as `plan` says.
fn bypass_in(tx: &mut Tx, dir: &Path, id: &str, plan: &Bypass) -> EditResult<()> {
    let mut by_node: Vec<(&str, Uses)> = Vec::new();
    let through = plan.through.as_ref().map(|(_, r)| r);
    let uses = (plan.reconnect.iter().map(|c| (c, through)))
        .chain(plan.disconnect.iter().map(|c| (c, None)));
    for (c, new) in uses {
        match by_node.iter_mut().find(|(n, _)| *n == c.node) {
            Some((_, list)) => list.push((c, new)),
            None => by_node.push((&c.node, vec![(c, new)])),
        }
    }
    for (node, mut uses) in by_node {
        // Highest index first, so removing a list element doesn't shift the others.
        uses.sort_by_key(|(c, _)| std::cmp::Reverse(c.index));
        tx.update(&node_path(dir, node), |doc| {
            for (c, new) in &uses {
                let old = Reference {
                    node: id.to_string(),
                    port: c.output.clone(),
                };
                expect_reference(doc, node, &c.port, c.index, &old)?;
                match new {
                    Some(new) => replace_reference(doc, &c.port, c.index, new)?,
                    None => disconnect_in(doc, &c.port, Some(c.index))?,
                }
            }
            Ok(())
        })?;
    }
    Ok(())
}

/// Deletes `id` and connects what fed it to its users instead (see [`bypass_plan`]).
/// Users that can't take it are disconnected, as with [`delete_node`].
pub fn dissolve_node(dir: &Path, project: &Project, id: &str) -> EditResult<Change> {
    let node = project.get(id).ok_or_else(|| missing(id))?;
    let plan = bypass_plan(project, id)?;
    transact(|tx| {
        bypass_in(tx, dir, id, &plan)?;
        tx.remove(&node.path)
    })
}

/// Takes `id` out from where it is (as [`dissolve_node`] does, but keeping it) and puts
/// it into `target`: `target.source → id → target.node`.
pub fn move_node(
    dir: &Path,
    project: &Project,
    id: &str,
    target: &Connection,
) -> EditResult<Change> {
    let node = project.get(id).ok_or_else(|| missing(id))?;
    let spec = node
        .spec
        .as_ref()
        .ok_or_else(|| EditError::Invalid(format!("`{id}` has an unknown type")))?;
    if target.source.node == id || target.node == id {
        return Err(EditError::Invalid(format!("`{id}` is already there")));
    }
    let plan = bypass_plan(project, id)?;
    let Some((through_port, _)) = &plan.through else {
        return Err(EditError::Invalid(format!(
            "nothing feeds `{id}`, so it can't pass a connection through; use `i` to insert a new node"
        )));
    };
    let through_ty = spec.input(through_port).map(|p| p.ty);
    let source_ty = output_type(project, &target.source).ok_or_else(|| {
        EditError::Invalid(format!("cannot tell the type of `{}`", target.source))
    })?;
    if !through_ty.is_some_and(|t| source_ty.coerces_to(t)) {
        return Err(EditError::Invalid(format!(
            "`{id}.{through_port}` can't take {source_ty} from `{}`",
            target.source
        )));
    }
    let target_ty = input_type(project, &target.node, &target.port).ok_or_else(|| {
        EditError::Invalid(format!("`{}` has no input `{}`", target.node, target.port))
    })?;
    let output = spec
        .outputs_for(&node.params)
        .into_iter()
        .find(|(_, ty)| ty.coerces_to(target_ty))
        .map(|(name, _)| name)
        .ok_or_else(|| {
            EditError::Invalid(format!(
                "`{id}` has no output usable as {target_ty} for `{}.{}`",
                target.node, target.port
            ))
        })?;
    // `id`'s other inputs stay, so they must not depend on where it's going.
    let downstream = project.descendants(&target.node);
    let other_sources = node
        .inputs
        .iter()
        .filter(|(port, _)| *port != through_port)
        .flat_map(|(_, refs)| refs);
    if let Some(r) = other_sources
        .into_iter()
        .find(|r| r.node == target.node || downstream.contains(&r.node))
    {
        return Err(EditError::Invalid(format!(
            "`{id}` takes `{r}`, which depends on `{}`: that would make a cycle",
            target.node
        )));
    }
    let new_ref = Reference {
        node: id.to_string(),
        port: output,
    };
    transact(|tx| {
        // The target first: its index is as read, before any bypass removal in that file.
        tx.update(&node_path(dir, &target.node), |doc| {
            expect_reference(
                doc,
                &target.node,
                &target.port,
                target.index,
                &target.source,
            )?;
            replace_reference(doc, &target.port, target.index, &new_ref)
        })?;
        bypass_in(tx, dir, id, &plan)?;
        tx.update(&node_path(dir, id), |doc| {
            replace_reference(doc, through_port, 0, &target.source)
        })
    })
}

/// Applies `f` to every reference in `[inputs]`, replacing it when `f` returns `Some`.
fn rewrite_references(doc: &mut DocumentMut, f: impl Fn(&Reference) -> Option<Reference>) {
    let Some(inputs) = doc.get_mut("inputs").and_then(Item::as_table_like_mut) else {
        return;
    };
    let rewrite = |v: &mut toml_edit::Value| {
        let Some(new) = v
            .as_str()
            .and_then(|s| Reference::parse(s).ok())
            .and_then(|r| f(&r))
        else {
            return;
        };
        *v = with_decor(new.short().into(), v);
    };
    for (_, item) in inputs.iter_mut() {
        match item.as_value_mut() {
            Some(toml_edit::Value::Array(items)) => items.iter_mut().for_each(rewrite),
            Some(v) => rewrite(v),
            None => {}
        }
    }
}

/// Removes every reference in `[inputs]` matching `pred`; inputs left empty are removed.
fn remove_references(doc: &mut DocumentMut, pred: impl Fn(&Reference) -> bool) {
    let Some(inputs) = doc.get_mut("inputs").and_then(Item::as_table_like_mut) else {
        return;
    };
    let matches = |v: &toml_edit::Value| {
        v.as_str()
            .and_then(|s| Reference::parse(s).ok())
            .is_some_and(|r| pred(&r))
    };
    let mut emptied = Vec::new();
    for (port, item) in inputs.iter_mut() {
        match item.as_value_mut() {
            Some(toml_edit::Value::Array(items)) => {
                items.retain(|v| !matches(v));
                if items.is_empty() {
                    emptied.push(port.get().to_string());
                }
            }
            Some(v) if matches(v) => emptied.push(port.get().to_string()),
            _ => {}
        }
    }
    for port in emptied {
        inputs.remove(&port);
    }
}

// --- table params whose entries are outputs (palette colors) -----------------------------

/// The `[params.<param>]` table of a node file, created if absent.
fn entries_mut<'a>(doc: &'a mut DocumentMut, param: &str) -> EditResult<&'a mut dyn TableLike> {
    sub_table(table_mut(doc, "params")?, param)
}

fn check_entry_name(key: &str) -> EditResult<()> {
    if is_ident(key) {
        Ok(())
    } else {
        Err(EditError::Invalid(format!(
            "`{key}` is not a valid name (letters, digits, `_`, `-`)"
        )))
    }
}

/// Replaces every entry of `node`'s table param `param` (e.g. a palette's colors), creating
/// the node from `spec`'s template first if it doesn't exist. Other params are kept.
/// `extra` params (e.g. the palette's `source`) are set too.
pub fn set_entries(
    dir: &Path,
    node: &str,
    spec: &NodeSpec,
    param: &str,
    entries: &[(String, toml::Value)],
    extra: &[(&str, toml::Value)],
) -> EditResult<Change> {
    for (key, _) in entries {
        check_entry_name(key)?;
    }
    transact(|tx| {
        let path = node_path(dir, node);
        if !path.exists() {
            add_raw(tx, dir, node, &spec.template())?;
        }
        tx.update(&path, |doc| {
            let mut table = toml_edit::Table::new();
            for (key, value) in entries {
                table.insert(key, Item::Value(to_edit_value(value)?));
            }
            table_mut(doc, "params")?.insert(param, Item::Table(table));
            extra
                .iter()
                .try_for_each(|(name, value)| set_param_in(doc, name, value))
        })
    })
}

/// Sets one entry of a table param, appending it if new.
pub fn set_entry(
    dir: &Path,
    node: &str,
    param: &str,
    key: &str,
    value: &toml::Value,
) -> EditResult<Change> {
    check_entry_name(key)?;
    let value = to_edit_value(value)?;
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            upsert(entries_mut(doc, param)?, key, value);
            Ok(())
        })
    })
}

/// Renames an entry (keeping its position and comments) and rewrites every reference to
/// the output it provides (`node.old` → `node.new`).
pub fn rename_entry(
    dir: &Path,
    project: &Project,
    node: &str,
    param: &str,
    old: &str,
    new: &str,
) -> EditResult<Change> {
    check_entry_name(new)?;
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            let table = entries_mut(doc, param)?;
            if table.contains_key(new) {
                return Err(EditError::Invalid(format!("`{new}` already exists")));
            }
            let entries: Vec<(String, Item)> = table
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect();
            if !entries.iter().any(|(k, _)| k == old) {
                return Err(EditError::Invalid(format!("no entry `{old}`")));
            }
            table.clear();
            for (key, item) in entries {
                table.insert(if key == old { new } else { &key }, item);
            }
            Ok(())
        })?;
        for consumer in consumer_ids(project, node, |c| c.output == old && c.node != node) {
            tx.update(&node_path(dir, consumer), |doc| {
                rewrite_references(doc, |r| {
                    (r.node == node && r.port == old).then(|| Reference {
                        node: node.into(),
                        port: new.into(),
                    })
                });
                Ok(())
            })?;
        }
        Ok(())
    })
}

/// Removes an entry and disconnects every input that used its output.
pub fn remove_entry(
    dir: &Path,
    project: &Project,
    node: &str,
    param: &str,
    key: &str,
) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            entries_mut(doc, param)?
                .remove(key)
                .map(|_| ())
                .ok_or_else(|| EditError::Invalid(format!("no entry `{key}`")))
        })?;
        for consumer in consumer_ids(project, node, |c| c.output == key && c.node != node) {
            tx.update(&node_path(dir, consumer), |doc| {
                remove_references(doc, |r| r.node == node && r.port == key);
                Ok(())
            })?;
        }
        Ok(())
    })
}

/// The distinct nodes among `node`'s consumers that `keep` accepts.
fn consumer_ids<'a>(
    project: &'a Project,
    node: &str,
    keep: impl Fn(&Consumer) -> bool,
) -> Vec<&'a str> {
    let mut ids: Vec<&str> = project
        .consumers(node)
        .iter()
        .filter(|c| keep(c))
        .map(|c| c.node.as_str())
        .collect();
    ids.dedup();
    ids
}

fn missing(id: &str) -> EditError {
    EditError::Invalid(format!("no node `{id}`"))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::project::inspect;
    use crate::test_nodes::registry;

    struct Fixture {
        dir: tempfile::TempDir,
    }

    impl Fixture {
        fn new(files: &[(&str, &str)]) -> Self {
            let dir = tempfile::tempdir().unwrap();
            for (id, text) in files {
                std::fs::write(dir.path().join(format!("{id}.toml")), text).unwrap();
            }
            Self { dir }
        }

        fn path(&self) -> &Path {
            self.dir.path()
        }

        fn read(&self, id: &str) -> String {
            std::fs::read_to_string(self.path().join(format!("{id}.toml"))).unwrap()
        }

        fn project(&self) -> Project {
            inspect(self.path(), &registry()).unwrap()
        }

        fn snapshot(&self) -> BTreeMap<String, String> {
            std::fs::read_dir(self.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .map(|p| {
                    (
                        p.file_name().unwrap().to_string_lossy().into_owned(),
                        std::fs::read_to_string(&p).unwrap(),
                    )
                })
                .collect()
        }

        /// Applies an edit, checks it leaves a valid project, undoes it, and checks the
        /// folder is byte-for-byte what it was.
        fn apply_and_undo(
            &self,
            edit: impl FnOnce() -> EditResult<Change>,
        ) -> BTreeMap<String, String> {
            let before = self.snapshot();
            let change = edit().unwrap();
            let after = self.snapshot();
            assert!(self.project().is_valid(), "{:?}", self.project().errors);
            change.undo().unwrap();
            assert_eq!(self.snapshot(), before, "undo restores the folder");
            after
        }
    }

    const SRC: &str = "# the source\ntype = \"source\"\n";
    const WRAP: &str = "type = \"wrap\"\n\n[inputs]\nimage = \"src\" # from src\n";
    const SINK: &str =
        "type = \"sink\"\n[inputs]\nitems = [\n  \"w\",  # first\n  \"src.out\",\n]\n";

    fn fixture() -> Fixture {
        Fixture::new(&[("src", SRC), ("w", WRAP), ("s", SINK)])
    }

    #[test]
    fn set_param_keeps_formatting() {
        let f = Fixture::new(&[(
            "src",
            "# top\ntype = \"source\"\n\n[params]\nsize = 3 # small\n",
        )]);
        let after =
            f.apply_and_undo(|| set_param(f.path(), "src", "size", &toml::Value::Integer(9)));
        assert_eq!(
            after["src.toml"],
            "# top\ntype = \"source\"\n\n[params]\nsize = 9 # small\n"
        );
    }

    #[test]
    fn set_param_creates_table_and_remove_param() {
        let f = fixture();
        let after =
            f.apply_and_undo(|| set_param(f.path(), "src", "size", &toml::Value::Integer(4)));
        assert_eq!(after["src.toml"], format!("{SRC}\n[params]\nsize = 4\n"));

        set_param(f.path(), "src", "size", &toml::Value::Integer(4)).unwrap();
        remove_param(f.path(), "src", "size").unwrap();
        assert!(!f.read("src").contains("size"));
    }

    #[test]
    fn connect_single_replaces() {
        let f = Fixture::new(&[("src", SRC), ("src2", SRC), ("w", WRAP)]);
        let src2 = Reference::parse("src2").unwrap();
        let after = f.apply_and_undo(|| connect(f.path(), "w", "image", &src2, false));
        assert_eq!(
            after["w.toml"],
            "type = \"wrap\"\n\n[inputs]\nimage = \"src2\" # from src\n"
        );
    }

    #[test]
    fn connect_list_appends() {
        let f = fixture();
        let after = f.apply_and_undo(|| {
            connect(
                f.path(),
                "s",
                "items",
                &Reference::parse("w").unwrap(),
                true,
            )
        });
        assert!(
            after["s.toml"].contains("\"src.out\", \"w\""),
            "{}",
            after["s.toml"]
        );

        // A single reference on a list input becomes a list.
        let f = Fixture::new(&[
            ("src", SRC),
            ("w", WRAP),
            ("s", "type = \"sink\"\n[inputs]\nitems = \"w\"\n"),
        ]);
        let after = f.apply_and_undo(|| {
            connect(
                f.path(),
                "s",
                "items",
                &Reference::parse("src").unwrap(),
                true,
            )
        });
        assert_eq!(
            after["s.toml"],
            "type = \"sink\"\n[inputs]\nitems = [\"w\", \"src\"]\n"
        );
    }

    #[test]
    fn connect_creates_inputs_table() {
        let f = Fixture::new(&[("src", SRC), ("w", "type = \"wrap\"\n")]);
        let after = f.apply_and_undo(|| {
            connect(
                f.path(),
                "w",
                "image",
                &Reference::parse("src").unwrap(),
                false,
            )
        });
        assert_eq!(
            after["w.toml"],
            "type = \"wrap\"\n\n[inputs]\nimage = \"src\"\n"
        );
    }

    #[test]
    fn reconnect_replaces_one_entry() {
        let f = fixture();
        let before = f.project().get("s").unwrap().inputs["items"].clone();
        let other = Reference::parse("src").unwrap();
        f.apply_and_undo(|| reconnect(f.path(), "s", "items", 0, &other));
        reconnect(f.path(), "s", "items", 0, &other).unwrap();
        let after = &f.project().get("s").unwrap().inputs["items"].clone();
        assert_eq!(after[0], other);
        assert_eq!(after[1..], before[1..]);
    }

    #[test]
    fn add_feeding_to_a_list_appends() {
        let f = fixture();
        let spec = registry().spec("wrap").unwrap().clone();
        let before = f.project().get("s").unwrap().inputs["items"].len();
        add_feeding_to(
            f.path(),
            "w2",
            &spec,
            &toml::Table::new(),
            "out",
            "s",
            "items",
            true,
        )
        .unwrap();
        let items = f.project().get("s").unwrap().inputs["items"].clone();
        assert_eq!(items.len(), before + 1);
        assert_eq!(items.last().unwrap().node, "w2");
    }

    #[test]
    fn disconnect_list_element_and_single() {
        let f = fixture();
        disconnect(f.path(), "s", "items", Some(0)).unwrap();
        let p = f.project();
        assert_eq!(
            p.get("s").unwrap().inputs["items"],
            [Reference::parse("src").unwrap()]
        );
        disconnect(f.path(), "s", "items", Some(0)).unwrap();
        assert!(!f.read("s").contains("items"), "empty list removed");

        let f = fixture();
        disconnect(f.path(), "w", "image", None).unwrap();
        assert!(f.project().get("w").unwrap().inputs.is_empty());
    }

    #[test]
    fn swap_list_connections() {
        let f = fixture();
        let after = f.apply_and_undo(|| swap_connections(f.path(), "s", "items", 0, 1));
        // The values trade places; the layout and comments stay where they were.
        assert_eq!(
            after["s.toml"],
            "type = \"sink\"\n[inputs]\nitems = [\n  \"src.out\",  # first\n  \"w\",\n]\n"
        );
        assert!(swap_connections(f.path(), "s", "items", 1, 2).is_err());
        assert!(swap_connections(f.path(), "w", "image", 0, 1).is_err());
    }

    #[test]
    fn add_node_from_template() {
        let f = fixture();
        let spec = registry().spec("source").unwrap().clone();
        let after = f.apply_and_undo(|| add_node(f.path(), "src2", &spec));
        assert_eq!(after["src2.toml"], spec.template());

        assert!(add_node(f.path(), "src", &spec).is_err(), "exists");
        assert!(add_node(f.path(), "bad id", &spec).is_err(), "invalid id");
    }

    #[test]
    fn insert_between_rewires() {
        let f = fixture();
        let project = f.project();
        let pass = registry().spec("pass").unwrap().clone();
        // Insert into the list input `s.items[1]` (currently `src.out`).
        f.apply_and_undo(|| insert_between(f.path(), &project, "s", "items", 1, "p", &pass));

        insert_between(f.path(), &project, "s", "items", 1, "p", &pass).unwrap();
        let p = f.project();
        assert!(p.is_valid());
        assert_eq!(
            p.get("p").unwrap().inputs["image"],
            [Reference::parse("src").unwrap()]
        );
        let items: Vec<String> = p.get("s").unwrap().inputs["items"]
            .iter()
            .map(|r| r.to_string())
            .collect();
        assert_eq!(items, ["w.out", "p.out"]);
        assert!(
            f.read("s").contains("\"w\",  # first"),
            "comment kept: {}",
            f.read("s")
        );
    }

    #[test]
    fn insert_between_checks_types() {
        let f = fixture();
        let project = f.project();
        // `source` has no inputs.
        let source = registry().spec("source").unwrap().clone();
        assert!(insert_between(f.path(), &project, "w", "image", 0, "x", &source).is_err());
        // `wrap` outputs a layer, but `w.image` needs an image.
        let wrap = registry().spec("wrap").unwrap().clone();
        assert!(insert_between(f.path(), &project, "w", "image", 0, "x", &wrap).is_err());
        assert!(!f.path().join("x.toml").exists());
    }

    #[test]
    fn rename_rewrites_references() {
        let f = fixture();
        let project = f.project();
        let after = f.apply_and_undo(|| rename_node(f.path(), &project, "src", "origin"));
        assert!(!after.contains_key("src.toml"));
        assert_eq!(after["origin.toml"], SRC);
        assert_eq!(
            after["w.toml"],
            "type = \"wrap\"\n\n[inputs]\nimage = \"origin\" # from src\n"
        );
        assert!(
            after["s.toml"].contains("\"w\",  # first\n  \"origin\","),
            "{}",
            after["s.toml"]
        );
    }

    #[test]
    fn rename_to_existing_changes_nothing() {
        let f = fixture();
        let before = f.snapshot();
        assert!(rename_node(f.path(), &f.project(), "src", "w").is_err());
        assert!(rename_node(f.path(), &f.project(), "src", "bad name").is_err());
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn delete_disconnects_consumers() {
        let f = fixture();
        let project = f.project();
        let before = f.snapshot();
        let change = delete_node(f.path(), &project, "w").unwrap();
        let after = f.snapshot();
        assert!(!after.contains_key("w.toml"));
        let p = f.project();
        assert_eq!(
            p.get("s").unwrap().inputs["items"],
            [Reference::parse("src").unwrap()]
        );
        assert!(p.is_valid(), "{:?}", p.errors);
        change.undo().unwrap();
        assert_eq!(f.snapshot(), before);
    }

    const PASS_SRC: &str = "type = \"pass\"\n[inputs]\nimage = \"src\"\n";

    fn inputs(f: &Fixture, id: &str, port: &str) -> Vec<String> {
        f.project().get(id).unwrap().inputs[port]
            .iter()
            .map(|r| r.to_string())
            .collect()
    }

    #[test]
    fn dissolve_reconnects_a_chain() {
        let wrap_p = "type = \"wrap\"\n[inputs]\nimage = \"p\" # from p\n";
        let f = Fixture::new(&[("src", SRC), ("p", PASS_SRC), ("w", wrap_p)]);
        let project = f.project();
        let after = f.apply_and_undo(|| dissolve_node(f.path(), &project, "p"));
        assert!(!after.contains_key("p.toml"));
        assert_eq!(
            after["w.toml"],
            "type = \"wrap\"\n[inputs]\nimage = \"src\" # from p\n"
        );
    }

    #[test]
    fn dissolve_keeps_list_position() {
        let f = fixture();
        let project = f.project();
        let plan = bypass_plan(&project, "w").unwrap();
        assert_eq!(
            plan.through,
            Some(("image".into(), Reference::parse("src").unwrap()))
        );
        assert_eq!(plan.reconnect.len(), 1);
        let after = f.apply_and_undo(|| dissolve_node(f.path(), &project, "w"));
        assert!(
            after["s.toml"].contains("\"src\",  # first\n  \"src.out\","),
            "{}",
            after["s.toml"]
        );
    }

    #[test]
    fn dissolve_without_inputs_disconnects() {
        let f = fixture();
        let project = f.project();
        let plan = bypass_plan(&project, "src").unwrap();
        assert_eq!(plan.through, None);
        assert_eq!(plan.disconnect.len(), 2);
        dissolve_node(f.path(), &project, "src").unwrap();
        assert!(!f.path().join("src.toml").exists());
        assert!(!f.project().get("w").unwrap().inputs.contains_key("image"));
    }

    #[test]
    fn dissolve_refuses_several_sources() {
        let f = fixture();
        let err = dissolve_node(f.path(), &f.project(), "s").unwrap_err();
        assert!(err.to_string().contains("can't tell which"), "{err}");
        assert!(f.path().join("s.toml").exists());
    }

    #[test]
    fn dissolve_links_a_param_to_what_fed_it() {
        let f = Fixture::new(&[
            ("pal", "type = \"pal\"\n[params.colors]\na = \"red\"\n"),
            ("t1", "type = \"tint\"\n[inputs]\ncolor = \"pal.a\"\n"),
            ("t2", "type = \"tint\"\n[inputs]\ncolor = \"t1\"\n"),
        ]);
        let project = f.project();
        f.apply_and_undo(|| dissolve_node(f.path(), &project, "t1"));
        dissolve_node(f.path(), &project, "t1").unwrap();
        assert_eq!(inputs(&f, "t2", "color"), ["pal.a"]);
    }

    fn chain() -> Fixture {
        // src → p1 → p2 → w
        Fixture::new(&[
            ("src", SRC),
            ("p1", PASS_SRC),
            ("p2", "type = \"pass\"\n[inputs]\nimage = \"p1\"\n"),
            ("w", "type = \"wrap\"\n[inputs]\nimage = \"p2\"\n"),
        ])
    }

    fn connection(source: &str, node: &str, port: &str, index: usize) -> Connection {
        Connection {
            source: Reference::parse(source).unwrap(),
            node: node.into(),
            port: port.into(),
            index,
        }
    }

    #[test]
    fn move_node_to_another_connection() {
        let f = chain();
        let project = f.project();
        let target = connection("p2", "w", "image", 0);
        f.apply_and_undo(|| move_node(f.path(), &project, "p1", &target));
        move_node(f.path(), &project, "p1", &target).unwrap();
        // src → p2 → p1 → w
        assert_eq!(inputs(&f, "p2", "image"), ["src.out"]);
        assert_eq!(inputs(&f, "p1", "image"), ["p2.out"]);
        assert_eq!(inputs(&f, "w", "image"), ["p1.out"]);
    }

    #[test]
    fn move_node_into_a_list() {
        // Move the unused pass `p` into s.items[1], which is `src.out`.
        let f = Fixture::new(&[("src", SRC), ("p", PASS_SRC), ("w", WRAP), ("s", SINK)]);
        let project = f.project();
        move_node(f.path(), &project, "p", &connection("src", "s", "items", 1)).unwrap();
        assert_eq!(inputs(&f, "s", "items"), ["w.out", "p.out"]);
        assert_eq!(inputs(&f, "p", "image"), ["src.out"]);
        assert!(f.read("s").contains("\"w\",  # first"), "{}", f.read("s"));
    }

    #[test]
    fn move_node_rejects_bad_targets() {
        let f = chain();
        let project = f.project();
        let before = f.snapshot();
        let fail = |id: &str, target: Connection| {
            let err = move_node(f.path(), &project, id, &target).unwrap_err();
            assert_eq!(
                f.snapshot(),
                before,
                "nothing written for {id} → {target:?}"
            );
            err.to_string()
        };
        assert!(fail("p1", connection("p1", "p2", "image", 0)).contains("already there"));
        assert!(fail("p1", connection("src", "p1", "image", 0)).contains("already there"));
        // `w` outputs a layer; `p1.image` needs an image.
        assert!(fail("w", connection("src", "p1", "image", 0)).contains("no output usable"));
        // Nothing feeds `src`.
        assert!(fail("src", connection("p2", "w", "image", 0)).contains("nothing feeds"));
    }

    #[test]
    fn failed_edit_rolls_back() {
        // `s` and `z` both use `w`. Deleting `w` rewrites `s` first, then fails on `z`
        // (corrupted after inspection): `s` must be restored and `w` kept.
        let user = "type = \"sink\"\n[inputs]\nitems = [\"w\"]\n";
        let f = Fixture::new(&[("src", SRC), ("w", WRAP), ("s", user), ("z", user)]);
        let project = f.project();
        std::fs::write(f.path().join("z.toml"), "type = [").unwrap();
        let before = f.snapshot();
        assert!(matches!(
            delete_node(f.path(), &project, "w"),
            Err(EditError::Parse { .. })
        ));
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn setting_a_param_unlinks_it() {
        let linked = "type = \"source\"\n[inputs]\nsize = \"n\"\n[params]\nsize = 3\n";
        let f = Fixture::new(&[("n", "type = \"num\""), ("src", linked)]);
        let after = f.apply_and_undo(|| set_param(f.path(), "src", "size", &4.into()));
        assert_eq!(
            after["src.toml"],
            "type = \"source\"\n[inputs]\n[params]\nsize = 4\n"
        );
        let after = f.apply_and_undo(|| remove_param(f.path(), "src", "size"));
        assert_eq!(after["src.toml"], "type = \"source\"\n[inputs]\n[params]\n");
    }

    #[test]
    fn add_feeding_creates_and_links() {
        let f = Fixture::new(&[("src", SRC)]);
        let spec = registry().spec("num").unwrap().clone();
        let params = toml::Table::from_iter([("value".to_string(), toml::Value::Float(5.0))]);
        let after = f.apply_and_undo(|| {
            add_feeding(f.path(), "src_size", &spec, &params, "out", "src", "size")
        });
        assert!(after["src_size.toml"].contains("value = 5.0"), "{after:?}");
        assert!(
            after["src.toml"].contains("size = \"src_size\""),
            "{after:?}"
        );

        let err = add_feeding(f.path(), "x", &spec, &params, "nope", "src", "size");
        assert!(err.is_err());
        assert_eq!(f.snapshot().len(), 1, "nothing created on failure");
    }

    const PAL: &str =
        "type = \"pal\"\n\n[params.colors]\nsky = \"#87ceeb\" # blue\nsun = \"yellow\"\n";
    const TINT: &str = "type = \"tint\"\n[inputs]\ncolor = \"p.sun\"\n";

    #[test]
    fn set_entries_creates_or_replaces() {
        let f = Fixture::new(&[]);
        let spec = registry().spec("pal").unwrap().clone();
        let entries = [("c1".to_string(), "#264653".into())];
        let extra = [("source", "https://coolors.co/264653".into())];
        let after =
            f.apply_and_undo(|| set_entries(f.path(), "p", &spec, "colors", &entries, &extra));
        assert!(
            after["p.toml"].contains("[params.colors]\nc1 = \"#264653\""),
            "{after:?}"
        );
        assert!(after["p.toml"].contains("source = \"https://coolors.co/264653\""));

        let f = Fixture::new(&[("p", PAL)]);
        let after = f.apply_and_undo(|| set_entries(f.path(), "p", &spec, "colors", &entries, &[]));
        assert!(!after["p.toml"].contains("sky"), "{after:?}");
        let bad = [("not ok".to_string(), "#264653".into())];
        assert!(set_entries(f.path(), "p", &spec, "colors", &bad, &[]).is_err());
    }

    #[test]
    fn edit_single_entries() {
        let f = Fixture::new(&[("p", PAL), ("t", TINT)]);
        let after =
            f.apply_and_undo(|| set_entry(f.path(), "p", "colors", "sky", &"#000080".into()));
        assert!(
            after["p.toml"].contains("sky = \"#000080\" # blue"),
            "{after:?}"
        );
        let after =
            f.apply_and_undo(|| set_entry(f.path(), "p", "colors", "moon", &"white".into()));
        assert!(
            after["p.toml"].ends_with("sun = \"yellow\"\nmoon = \"white\"\n"),
            "{after:?}"
        );

        // Renaming keeps the position and rewrites references.
        let after =
            f.apply_and_undo(|| rename_entry(f.path(), &f.project(), "p", "colors", "sun", "star"));
        assert!(
            after["p.toml"].ends_with("sky = \"#87ceeb\" # blue\nstar = \"yellow\"\n"),
            "{after:?}"
        );
        assert!(after["t.toml"].contains("color = \"p.star\""), "{after:?}");
        assert!(
            rename_entry(f.path(), &f.project(), "p", "colors", "sun", "sky").is_err(),
            "taken"
        );

        // Removing disconnects users (tint's color is then unset, which is still valid
        // for the test node).
        let before = f.snapshot();
        let change = remove_entry(f.path(), &f.project(), "p", "colors", "sun").unwrap();
        assert!(!f.read("p").contains("sun"));
        assert!(!f.read("t").contains("p.sun"));
        change.undo().unwrap();
        assert_eq!(f.snapshot(), before);
    }

    #[test]
    fn snapshot_restores_outside_edits() {
        let f = Fixture::new(&[("src", SRC)]);
        let change = snapshot(&[f.path().join("src.toml")]).unwrap();
        std::fs::write(f.path().join("src.toml"), "changed elsewhere").unwrap();
        change.undo().unwrap();
        assert_eq!(f.read("src"), SRC);
    }
}
