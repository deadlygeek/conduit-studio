//! File-level edits to a project, for editors. Every edit preserves comments and formatting
//! (via `toml_edit`), is all-or-nothing, and returns a [`Change`] that can undo it.

use std::io;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item, TableLike};

use crate::graph::{Reference, is_ident};
use crate::node::NodeSpec;
use crate::project::Project;

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
            .map_err(|source| EditError::Io {
                path: path.clone(),
                source,
            })?;
        }
        Ok(())
    }

    /// Files this edit touched.
    pub fn paths(&self) -> impl Iterator<Item = &Path> {
        self.before.iter().map(|(p, _)| p.as_path())
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
            Err(source) => {
                return Err(EditError::Io {
                    path: path.to_path_buf(),
                    source,
                });
            }
        };
        self.before.push((path.to_path_buf(), contents));
        Ok(())
    }

    fn read_doc(&self, path: &Path) -> EditResult<DocumentMut> {
        let text = std::fs::read_to_string(path).map_err(|source| EditError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        text.parse()
            .map_err(|e: toml_edit::TomlError| EditError::Parse {
                path: path.to_path_buf(),
                message: e.message().to_string(),
            })
    }

    fn write(&mut self, path: &Path, text: &str) -> EditResult<()> {
        self.record(path)?;
        std::fs::write(path, text).map_err(|source| EditError::Io {
            path: path.to_path_buf(),
            source,
        })
    }

    fn remove(&mut self, path: &Path) -> EditResult<()> {
        self.record(path)?;
        std::fs::remove_file(path).map_err(|source| EditError::Io {
            path: path.to_path_buf(),
            source,
        })
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

/// How a reference is written in a node file: `"node"` for the default `out` port.
fn reference_text(r: &Reference) -> String {
    if r.port == "out" {
        r.node.clone()
    } else {
        r.to_string()
    }
}

/// The `[name]` table of a document, created if absent.
fn table_mut<'a>(doc: &'a mut DocumentMut, name: &str) -> EditResult<&'a mut dyn TableLike> {
    doc.entry(name)
        .or_insert_with(toml_edit::table)
        .as_table_like_mut()
        .ok_or_else(|| EditError::Invalid(format!("`{name}` is not a table")))
}

fn to_edit_value(value: &toml::Value) -> EditResult<toml_edit::Value> {
    value
        .to_string()
        .parse()
        .map_err(|_| EditError::Invalid(format!("cannot write value {value}")))
}

/// Replaces an item's value, keeping its surrounding whitespace and trailing comment.
fn replace_value(item: &mut Item, mut new: toml_edit::Value) {
    if let Some(old) = item.as_value() {
        *new.decor_mut() = old.decor().clone();
    }
    *item = Item::Value(new);
}

/// Sets `[params] name = value` on a node.
pub fn set_param(dir: &Path, node: &str, name: &str, value: &toml::Value) -> EditResult<Change> {
    let value = to_edit_value(value)?;
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            let params = table_mut(doc, "params")?;
            match params.get_mut(name) {
                Some(item) => replace_value(item, value),
                None => {
                    params.insert(name, Item::Value(value));
                }
            }
            Ok(())
        })
    })
}

/// Removes `[params] name` from a node, so its default applies.
pub fn remove_param(dir: &Path, node: &str, name: &str) -> EditResult<Change> {
    transact(|tx| {
        tx.update(&node_path(dir, node), |doc| {
            table_mut(doc, "params")?.remove(name);
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
    let text = reference_text(source);
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

/// Creates a new node file from `spec`'s template.
pub fn add_node(dir: &Path, id: &str, spec: &NodeSpec) -> EditResult<Change> {
    transact(|tx| add_node_in(tx, dir, id, spec))
}

fn add_node_in(tx: &mut Tx, dir: &Path, id: &str, spec: &NodeSpec) -> EditResult<()> {
    if !is_ident(id) {
        return Err(EditError::Invalid(format!(
            "`{id}` is not a valid node id (letters, digits, `_`, `-`)"
        )));
    }
    let path = node_path(dir, id);
    if path.exists() {
        return Err(EditError::Invalid(format!("node `{id}` already exists")));
    }
    tx.write(&path, &spec.template())
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
    let source_ty = project
        .get(&source.node)
        .and_then(|n| n.spec.as_ref())
        .and_then(|s| s.output(&source.port))
        .map(|o| o.ty)
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
        add_node_in(tx, dir, new_id, spec)?;
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
    let text = reference_text(new);
    let item = table_mut(doc, "inputs")?
        .get_mut(port)
        .ok_or_else(|| invalid_input(port))?;
    match item.as_array_mut() {
        Some(items) => {
            let slot = items.get_mut(index).ok_or_else(|| invalid_input(port))?;
            let decor = slot.decor().clone();
            *slot = text.into();
            *slot.decor_mut() = decor;
        }
        None => replace_value(item, text.into()),
    }
    Ok(())
}

/// Renames a node: moves its file and rewrites every reference to it.
pub fn rename_node(dir: &Path, project: &Project, old: &str, new: &str) -> EditResult<Change> {
    let node = project.get(old).ok_or_else(|| missing(old))?;
    transact(|tx| {
        let text = std::fs::read_to_string(&node.path).map_err(|source| EditError::Io {
            path: node.path.clone(),
            source,
        })?;
        // Create the new file first so an invalid or taken name fails before anything moves.
        add_raw(tx, dir, new, &text)?;
        tx.remove(&node.path)?;

        let mut consumers: Vec<&str> = project
            .consumers(old)
            .iter()
            .map(|c| c.node.as_str())
            .collect();
        consumers.dedup();
        for consumer in consumers {
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
        let mut consumers: Vec<&str> = project
            .consumers(id)
            .iter()
            .map(|c| c.node.as_str())
            .collect();
        consumers.dedup();
        for consumer in consumers.into_iter().filter(|c| *c != id) {
            tx.update(&node_path(dir, consumer), |doc| {
                remove_references(doc, |r| r.node == id);
                Ok(())
            })?;
        }
        tx.remove(&node.path)
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
        let decor = v.decor().clone();
        *v = reference_text(&new).into();
        *v.decor_mut() = decor;
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
    fn add_node_from_template() {
        let f = fixture();
        let spec = registry().get("source").unwrap().spec();
        let after = f.apply_and_undo(|| add_node(f.path(), "src2", &spec));
        assert_eq!(after["src2.toml"], spec.template());

        assert!(add_node(f.path(), "src", &spec).is_err(), "exists");
        assert!(add_node(f.path(), "bad id", &spec).is_err(), "invalid id");
    }

    #[test]
    fn insert_between_rewires() {
        let f = fixture();
        let project = f.project();
        let pass = registry().get("pass").unwrap().spec();
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
        let source = registry().get("source").unwrap().spec();
        assert!(insert_between(f.path(), &project, "w", "image", 0, "x", &source).is_err());
        // `wrap` outputs a layer, but `w.image` needs an image.
        let wrap = registry().get("wrap").unwrap().spec();
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
}
