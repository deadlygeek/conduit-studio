//! A lenient view of a project folder for editors: every node file is included even when
//! the project doesn't load, with load errors attached to the nodes they concern.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::graph::{LoadError, NodeFile, Reference, Sources, from_sources, is_ident, read_sources};
use crate::node::NodeSpec;
use crate::registry::Registry;

#[derive(Debug, Clone)]
pub struct ProjectNode {
    pub id: String,
    pub path: PathBuf,
    /// `None` if the file couldn't be read or parsed.
    pub kind: Option<String>,
    /// Spec of `kind`, if it's a registered node type.
    pub spec: Option<NodeSpec>,
    /// Every well-formed reference in `[inputs]`, by port, in file order.
    pub inputs: BTreeMap<String, Vec<Reference>>,
    pub params: toml::Table,
}

impl ProjectNode {
    /// Input ports in display order: the spec's order, then any ports the spec doesn't know.
    /// Unconnected spec ports are included with no references.
    pub fn input_ports(&self) -> Vec<(String, Vec<Reference>)> {
        let mut ports: Vec<(String, Vec<Reference>)> = self
            .spec
            .iter()
            .flat_map(|s| &s.inputs)
            .map(|p| {
                (
                    p.name.to_string(),
                    self.inputs.get(p.name).cloned().unwrap_or_default(),
                )
            })
            .collect();
        for (port, refs) in &self.inputs {
            if !ports.iter().any(|(p, _)| p == port) {
                ports.push((port.clone(), refs.clone()));
            }
        }
        ports
    }
}

/// One use of a node's output: `node.port[index]` takes `<source>.<output>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumer {
    pub node: String,
    pub port: String,
    pub index: usize,
    pub output: String,
}

#[derive(Debug, Default)]
pub struct Project {
    pub nodes: BTreeMap<String, ProjectNode>,
    /// Everything that stops the project from loading (same errors as `graph::load`).
    pub errors: Vec<LoadError>,
    /// For each node id, the inputs that reference it.
    pub(crate) consumers: BTreeMap<String, Vec<Consumer>>,
}

impl Project {
    pub fn get(&self, id: &str) -> Option<&ProjectNode> {
        self.nodes.get(id)
    }

    /// Whether the project would load and render.
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }

    /// Errors about the given node's file, plus cycles it's part of.
    pub fn errors_for(&self, id: &str) -> Vec<&LoadError> {
        self.errors
            .iter()
            .filter(|e| match e {
                LoadError::Cycle { nodes } => nodes.iter().any(|n| n == id),
                e => e.path().and_then(node_id).is_some_and(|n| n == id),
            })
            .collect()
    }

    /// The inputs that reference `id`, ordered by consumer id, port and position.
    pub fn consumers(&self, id: &str) -> &[Consumer] {
        self.consumers.get(id).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn node_id(path: &Path) -> Option<&str> {
    path.file_stem().and_then(|s| s.to_str())
}

/// Inspects every `*.toml` file directly inside `dir`. Only failing to list `dir` is an error.
pub fn inspect(dir: &Path, registry: &Registry) -> Result<Project, LoadError> {
    let (sources, read_errors) = read_sources(dir)?;
    let mut project = inspect_sources(sources, registry);
    for e in read_errors {
        if let Some(id) = e.path().and_then(node_id).filter(|id| is_ident(id)) {
            project
                .nodes
                .insert(id.to_string(), broken_node(id, e.path().unwrap()));
        }
        project.errors.insert(0, e);
    }
    Ok(project)
}

/// Like [`inspect`], from `(path, contents)` pairs.
pub fn inspect_sources(sources: Sources, registry: &Registry) -> Project {
    let errors = from_sources(sources.clone(), registry)
        .err()
        .map(|e| e.0)
        .unwrap_or_default();

    let mut nodes = BTreeMap::new();
    for (path, text) in sources {
        let Some(id) = node_id(&path).filter(|id| is_ident(id)).map(str::to_string) else {
            continue; // reported as InvalidId
        };
        let node = match toml::from_str::<NodeFile>(&text) {
            Ok(file) => ProjectNode {
                id: id.clone(),
                spec: registry.get(&file.kind).map(|n| n.spec()),
                kind: Some(file.kind),
                inputs: lenient_inputs(&file.inputs),
                params: file.params,
                path,
            },
            Err(_) => broken_node(&id, &path),
        };
        nodes.insert(id, node);
    }

    let mut consumers: BTreeMap<String, Vec<Consumer>> = BTreeMap::new();
    for node in nodes.values() {
        for (port, refs) in &node.inputs {
            for (index, r) in refs.iter().enumerate() {
                consumers.entry(r.node.clone()).or_default().push(Consumer {
                    node: node.id.clone(),
                    port: port.clone(),
                    index,
                    output: r.port.clone(),
                });
            }
        }
    }
    Project {
        nodes,
        errors,
        consumers,
    }
}

fn broken_node(id: &str, path: &Path) -> ProjectNode {
    ProjectNode {
        id: id.to_string(),
        path: path.to_path_buf(),
        kind: None,
        spec: None,
        inputs: BTreeMap::new(),
        params: toml::Table::new(),
    }
}

/// Keeps every reference that parses, whether or not the port or shape is valid.
fn lenient_inputs(inputs: &toml::Table) -> BTreeMap<String, Vec<Reference>> {
    inputs
        .iter()
        .map(|(port, value)| {
            let items: Vec<&toml::Value> = match value {
                toml::Value::Array(items) => items.iter().collect(),
                single => vec![single],
            };
            let refs = items
                .into_iter()
                .filter_map(|v| v.as_str().and_then(|s| Reference::parse(s).ok()))
                .collect();
            (port.clone(), refs)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_nodes::registry;

    fn project(files: &[(&str, &str)]) -> Project {
        let sources = files
            .iter()
            .map(|(id, text)| (PathBuf::from(format!("{id}.toml")), text.to_string()))
            .collect();
        inspect_sources(sources, &registry())
    }

    #[test]
    fn valid_project_has_no_errors() {
        let p = project(&[
            ("src", r#"type = "source""#),
            ("w", "type = \"wrap\"\n[inputs]\nimage = \"src\""),
        ]);
        assert!(p.is_valid());
        assert_eq!(p.get("w").unwrap().spec.as_ref().unwrap().kind, "wrap");
        assert_eq!(
            p.consumers("src"),
            [Consumer {
                node: "w".into(),
                port: "image".into(),
                index: 0,
                output: "out".into()
            }]
        );
    }

    #[test]
    fn broken_project_keeps_every_node() {
        let p = project(&[
            ("bad_syntax", "type = "),
            ("unknown", r#"type = "nope""#),
            (
                "w",
                "type = \"wrap\"\n[inputs]\nimage = \"ghost\"\nextra = [\"bad_syntax\", \"not valid!\"]",
            ),
        ]);
        assert!(!p.is_valid());
        assert_eq!(p.nodes.len(), 3);
        assert!(p.get("bad_syntax").unwrap().kind.is_none());
        assert_eq!(p.get("unknown").unwrap().kind.as_deref(), Some("nope"));
        assert!(p.get("unknown").unwrap().spec.is_none());

        let w = p.get("w").unwrap();
        assert_eq!(w.inputs["image"][0].node, "ghost");
        assert_eq!(w.inputs["extra"].len(), 1, "unparseable reference dropped");
        assert_eq!(p.consumers("bad_syntax").len(), 1);

        assert_eq!(p.errors_for("bad_syntax").len(), 1);
        assert_eq!(p.errors_for("w").len(), 1, "{:?}", p.errors_for("w"));
        assert_eq!(p.errors_for("unknown").len(), 1);
    }

    #[test]
    fn input_ports_follow_spec_order() {
        let p = project(&[("s", "type = \"sink\"\n[inputs]\nzzz = \"a\"")]);
        let ports: Vec<String> = p
            .get("s")
            .unwrap()
            .input_ports()
            .into_iter()
            .map(|(p, _)| p)
            .collect();
        assert_eq!(ports, ["items", "zzz"]);
    }

    #[test]
    fn inspect_reads_folder() {
        let dir = std::env::temp_dir().join(format!("conduit-inspect-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("a.toml"), r#"type = "source""#).unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();
        let p = inspect(&dir, &registry()).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(p.nodes.keys().collect::<Vec<_>>(), ["a"]);
        assert!(inspect(Path::new("/nonexistent/conduit"), &registry()).is_err());
    }
}
