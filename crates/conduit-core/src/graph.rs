//! Loading a project folder (one `<node_id>.toml` per node) into a validated graph.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::node::NodeSpec;
use crate::registry::Registry;
use crate::value::ValueType;

/// A connection source: `"node"` (port `out`) or `"node.port"`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Reference {
    pub node: String,
    pub port: String,
}

impl Reference {
    pub fn parse(s: &str) -> Result<Self, String> {
        let (node, port) = s.split_once('.').unwrap_or((s, "out"));
        if !is_ident(node) {
            return Err(format!("`{node}` is not a valid node id"));
        }
        if !is_ident(port) {
            return Err(format!("`{port}` is not a valid port name"));
        }
        Ok(Self {
            node: node.to_string(),
            port: port.to_string(),
        })
    }
}

impl fmt::Display for Reference {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}", self.node, self.port)
    }
}

pub(crate) fn is_ident(s: &str) -> bool {
    !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
}

/// On-disk shape of a node file.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NodeFile {
    #[serde(rename = "type")]
    pub(crate) kind: String,
    #[serde(default)]
    pub(crate) inputs: toml::Table,
    #[serde(default)]
    pub(crate) params: toml::Table,
}

#[derive(Debug, Clone)]
pub struct GraphNode {
    pub id: String,
    pub kind: String,
    pub path: PathBuf,
    /// Connected input ports, each with its source references in order.
    pub inputs: BTreeMap<String, Vec<Reference>>,
    pub params: toml::Table,
}

/// A validated graph, with nodes in evaluation (topological) order.
#[derive(Debug, Clone, Default)]
pub struct Graph {
    pub nodes: Vec<GraphNode>,
    /// The project folder. Files that nodes read (e.g. `image`'s `path`) are relative to
    /// it. Empty (the current directory) for graphs not loaded from a folder.
    pub root: PathBuf,
}

impl Graph {
    pub fn get(&self, id: &str) -> Option<&GraphNode> {
        self.nodes.iter().find(|n| n.id == id)
    }

    /// Every file the nodes read besides their own node files (see [`Node::files`]),
    /// joined to [`Graph::root`]. The graph must have been loaded with this registry.
    ///
    /// [`Node::files`]: crate::Node::files
    pub fn files(&self, registry: &Registry) -> Vec<PathBuf> {
        let mut files: Vec<PathBuf> = self
            .nodes
            .iter()
            .filter_map(|n| registry.get(&n.kind).map(|k| k.files(&n.params)))
            .flatten()
            .map(|f| self.root.join(f))
            .collect();
        files.sort();
        files.dedup();
        files
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LoadError {
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{}: {message}", path.display())]
    Parse { path: PathBuf, message: String },
    #[error("{}: file name must be a valid node id (letters, digits, `_`, `-`)", path.display())]
    InvalidId { path: PathBuf },
    #[error("{}: unknown node type `{kind}`", path.display())]
    UnknownType { path: PathBuf, kind: String },
    #[error("{}: `{kind}` has no input `{port}`", path.display())]
    UnknownInput {
        path: PathBuf,
        kind: String,
        port: String,
    },
    #[error("{}: required input `{port}` is not connected", path.display())]
    MissingInput { path: PathBuf, port: String },
    #[error("{}: input `{port}` takes a single connection, not a list", path.display())]
    NotMulti { path: PathBuf, port: String },
    #[error("{}: input `{port}`: {reason}", path.display())]
    BadReference {
        path: PathBuf,
        port: String,
        reason: String,
    },
    #[error("{}: input `{port}` refers to missing node `{node}`", path.display())]
    MissingNode {
        path: PathBuf,
        port: String,
        node: String,
    },
    #[error("{}: input `{port}`: node `{node}` (`{kind}`) has no output `{output}`", path.display())]
    UnknownOutput {
        path: PathBuf,
        port: String,
        node: String,
        kind: String,
        output: String,
    },
    #[error("{}: input `{port}` expects {expected}, but `{source_ref}` is {found}", path.display())]
    TypeMismatch {
        path: PathBuf,
        port: String,
        source_ref: Reference,
        expected: ValueType,
        found: ValueType,
    },
    #[error("{}: params: {message}", path.display())]
    Params { path: PathBuf, message: String },
    #[error("cycle between nodes: {}", nodes.join(", "))]
    Cycle { nodes: Vec<String> },
}

impl LoadError {
    /// The node file this error is about, if it is about a single file.
    pub fn path(&self) -> Option<&Path> {
        match self {
            LoadError::Io { path, .. }
            | LoadError::Parse { path, .. }
            | LoadError::InvalidId { path }
            | LoadError::UnknownType { path, .. }
            | LoadError::UnknownInput { path, .. }
            | LoadError::MissingInput { path, .. }
            | LoadError::NotMulti { path, .. }
            | LoadError::BadReference { path, .. }
            | LoadError::MissingNode { path, .. }
            | LoadError::UnknownOutput { path, .. }
            | LoadError::TypeMismatch { path, .. }
            | LoadError::Params { path, .. } => Some(path),
            LoadError::Cycle { .. } => None,
        }
    }
}

/// All problems found while loading, so users can fix several at once.
#[derive(Debug, thiserror::Error)]
pub struct LoadErrors(pub Vec<LoadError>);

impl fmt::Display for LoadErrors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, e) in self.0.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            write!(f, "{e}")?;
        }
        Ok(())
    }
}

/// Loads every `*.toml` file directly inside `dir` as a node.
pub fn load(dir: &Path, registry: &Registry) -> Result<Graph, LoadErrors> {
    let (sources, mut errors) = read_sources(dir).map_err(|e| LoadErrors(vec![e]))?;
    match from_sources(sources, registry) {
        Ok(graph) if errors.is_empty() => Ok(Graph {
            root: dir.to_path_buf(),
            ..graph
        }),
        Ok(_) => Err(LoadErrors(errors)),
        Err(LoadErrors(more)) => {
            errors.extend(more);
            Err(LoadErrors(errors))
        }
    }
}

/// `(path, file contents)` for each node file.
pub type Sources = Vec<(PathBuf, String)>;

/// Reads every `*.toml` file directly inside `dir`, sorted by path. Files that can't be
/// read are returned as errors; failing to list `dir` at all is the outer error.
pub(crate) fn read_sources(dir: &Path) -> Result<(Sources, Vec<LoadError>), LoadError> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|source| LoadError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.is_file() && p.extension().is_some_and(|ext| ext == "toml"))
        .collect();
    paths.sort();

    let mut errors = Vec::new();
    let mut sources = Vec::new();
    for path in paths {
        match std::fs::read_to_string(&path) {
            Ok(text) => sources.push((path, text)),
            Err(source) => errors.push(LoadError::Io { path, source }),
        }
    }
    Ok((sources, errors))
}

/// Builds a graph from `(path, file contents)` pairs; the node id is the file stem.
pub fn from_sources(
    sources: impl IntoIterator<Item = (PathBuf, String)>,
    registry: &Registry,
) -> Result<Graph, LoadErrors> {
    let mut errors = Vec::new();
    let mut nodes = BTreeMap::new();
    // Nodes whose file exists but has errors; references to them aren't "missing".
    let mut broken = BTreeSet::new();

    for (path, text) in sources {
        match parse_node(&path, &text, registry) {
            Ok(node) => {
                nodes.insert(node.id.clone(), node);
            }
            Err(e) => {
                if let Some(id) = path.file_stem().and_then(|s| s.to_str()) {
                    broken.insert(id.to_string());
                }
                errors.extend(e);
            }
        }
    }

    for node in nodes.values() {
        check_connections(node, &nodes, &broken, registry, &mut errors);
    }
    if !errors.is_empty() {
        return Err(LoadErrors(errors));
    }

    topo_sort(nodes).map_err(|e| LoadErrors(vec![e]))
}

/// Parses one file and checks everything that doesn't depend on other nodes.
fn parse_node(path: &Path, text: &str, registry: &Registry) -> Result<GraphNode, Vec<LoadError>> {
    let id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or_default();
    if !is_ident(id) {
        return Err(vec![LoadError::InvalidId {
            path: path.to_path_buf(),
        }]);
    }
    let file: NodeFile = toml::from_str(text).map_err(|e| {
        vec![LoadError::Parse {
            path: path.to_path_buf(),
            message: e.message().to_string(),
        }]
    })?;
    let Some(node_impl) = registry.get(&file.kind) else {
        return Err(vec![LoadError::UnknownType {
            path: path.to_path_buf(),
            kind: file.kind,
        }]);
    };
    let spec = registry.spec(&file.kind).expect("registered");

    let mut errors = Vec::new();
    let mut inputs = BTreeMap::new();
    for (port, value) in &file.inputs {
        let Some(port_spec) = spec.input(port) else {
            errors.push(LoadError::UnknownInput {
                path: path.to_path_buf(),
                kind: file.kind.clone(),
                port: port.clone(),
            });
            continue;
        };
        let bad_ref = |reason: String| LoadError::BadReference {
            path: path.to_path_buf(),
            port: port.clone(),
            reason,
        };
        let raw: Vec<&toml::Value> = match value {
            toml::Value::Array(items) if port_spec.multi => items.iter().collect(),
            toml::Value::Array(_) => {
                errors.push(LoadError::NotMulti {
                    path: path.to_path_buf(),
                    port: port.clone(),
                });
                continue;
            }
            single => vec![single],
        };
        let mut refs = Vec::new();
        for item in raw {
            match item.as_str().map(Reference::parse) {
                Some(Ok(r)) => refs.push(r),
                Some(Err(reason)) => errors.push(bad_ref(reason)),
                None => errors.push(bad_ref(format!(
                    "expected a reference like \"node\" or \"node.port\", got {item}"
                ))),
            }
        }
        inputs.insert(port.clone(), refs);
    }

    for port in spec.inputs.iter().filter(|p| !p.optional) {
        if !file.inputs.contains_key(port.name) {
            errors.push(LoadError::MissingInput {
                path: path.to_path_buf(),
                port: port.name.to_string(),
            });
        }
    }

    let params = spec.params_with_links(&file.params, inputs.keys().map(String::as_str));
    if let Err(e) = node_impl.check_params(&params) {
        errors.push(LoadError::Params {
            path: path.to_path_buf(),
            message: e.0,
        });
    }
    for message in check_entry_names(spec, &file.params) {
        errors.push(LoadError::Params {
            path: path.to_path_buf(),
            message,
        });
    }

    if !errors.is_empty() {
        return Err(errors);
    }
    Ok(GraphNode {
        id: id.to_string(),
        kind: file.kind,
        path: path.to_path_buf(),
        inputs,
        params: file.params,
    })
}

/// Entries that become outputs (e.g. palette colors) need names usable in references,
/// distinct from the node's other outputs.
fn check_entry_names(spec: &NodeSpec, params: &toml::Table) -> Vec<String> {
    let mut problems = Vec::new();
    for port in spec.outputs.iter().filter(|p| p.entries) {
        let keys = params.get(port.name).and_then(|v| v.as_table());
        for key in keys.into_iter().flat_map(|t| t.keys()) {
            if !is_ident(key) {
                problems.push(format!(
                    "{}: `{key}` must be a valid name (letters, digits, `_`, `-`)",
                    port.name
                ));
            } else if spec.outputs.iter().any(|o| !o.entries && o.name == key) {
                problems.push(format!(
                    "{}: `{key}` clashes with the output of the same name",
                    port.name
                ));
            }
        }
    }
    problems
}

/// Checks that every reference points at an existing node output of a compatible type.
fn check_connections(
    node: &GraphNode,
    nodes: &BTreeMap<String, GraphNode>,
    broken: &BTreeSet<String>,
    registry: &Registry,
    errors: &mut Vec<LoadError>,
) {
    let spec = registry
        .spec(&node.kind)
        .expect("kind validated in parse_node");
    for (port, refs) in &node.inputs {
        let expected = spec.input(port).expect("port validated in parse_node").ty;
        for r in refs {
            let Some(source) = nodes.get(&r.node) else {
                if broken.contains(&r.node) {
                    continue; // its own errors are already reported
                }
                errors.push(LoadError::MissingNode {
                    path: node.path.clone(),
                    port: port.clone(),
                    node: r.node.clone(),
                });
                continue;
            };
            let source_spec = registry.spec(&source.kind).expect("kind validated");
            let Some(found) = source_spec.output_type(&source.params, &r.port) else {
                errors.push(LoadError::UnknownOutput {
                    path: node.path.clone(),
                    port: port.clone(),
                    node: r.node.clone(),
                    kind: source.kind.clone(),
                    output: r.port.clone(),
                });
                continue;
            };
            if !found.coerces_to(expected) {
                errors.push(LoadError::TypeMismatch {
                    path: node.path.clone(),
                    port: port.clone(),
                    source_ref: r.clone(),
                    expected,
                    found,
                });
            }
        }
    }
}

/// Kahn's algorithm; ties are broken by node id so evaluation order is deterministic.
fn topo_sort(mut nodes: BTreeMap<String, GraphNode>) -> Result<Graph, LoadError> {
    let mut indegree: HashMap<&str, usize> = nodes.keys().map(|id| (id.as_str(), 0)).collect();
    let mut dependents: HashMap<&str, Vec<&str>> = HashMap::new();
    for node in nodes.values() {
        let deps: BTreeSet<&str> = node
            .inputs
            .values()
            .flatten()
            .map(|r| r.node.as_str())
            .collect();
        for dep in deps {
            *indegree.get_mut(node.id.as_str()).expect("node present") += 1;
            dependents.entry(dep).or_default().push(&node.id);
        }
    }

    let mut ready: BTreeSet<&str> = indegree
        .iter()
        .filter(|(_, d)| **d == 0)
        .map(|(id, _)| *id)
        .collect();
    let mut order = Vec::with_capacity(nodes.len());
    while let Some(id) = ready.pop_first() {
        order.push(id.to_string());
        for dependent in dependents.get(id).into_iter().flatten() {
            let d = indegree.get_mut(dependent).expect("node present");
            *d -= 1;
            if *d == 0 {
                ready.insert(dependent);
            }
        }
    }

    if order.len() < nodes.len() {
        let mut stuck: Vec<String> = indegree
            .into_iter()
            .filter(|(_, d)| *d > 0)
            .map(|(id, _)| id.to_string())
            .collect();
        stuck.sort();
        return Err(LoadError::Cycle { nodes: stuck });
    }
    Ok(Graph {
        nodes: order
            .iter()
            .map(|id| nodes.remove(id).expect("node present"))
            .collect(),
        root: PathBuf::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_nodes::{registry, sources};

    fn load_str(files: &[(&str, &str)]) -> Result<Graph, LoadErrors> {
        from_sources(sources(files), &registry())
    }

    fn errors(files: &[(&str, &str)]) -> Vec<LoadError> {
        load_str(files).expect_err("expected load errors").0
    }

    #[test]
    fn reference_parsing() {
        assert_eq!(
            Reference::parse("a").unwrap(),
            Reference {
                node: "a".into(),
                port: "out".into()
            }
        );
        assert_eq!(
            Reference::parse("a.img").unwrap(),
            Reference {
                node: "a".into(),
                port: "img".into()
            }
        );
        assert!(Reference::parse("").is_err());
        assert!(Reference::parse("a.").is_err());
        assert!(Reference::parse("a.b.c").is_err());
        assert!(Reference::parse("has space").is_err());
    }

    #[test]
    fn loads_in_dependency_order() {
        let graph = load_str(&[
            ("z_src", r#"type = "source""#),
            (
                "a_sink",
                "type = \"sink\"\n[inputs]\nitems = [\"m_wrap\", \"z_src\"]",
            ),
            ("m_wrap", "type = \"wrap\"\n[inputs]\nimage = \"z_src.out\""),
        ])
        .unwrap();
        let ids: Vec<_> = graph.nodes.iter().map(|n| n.id.as_str()).collect();
        assert_eq!(ids, ["z_src", "m_wrap", "a_sink"]);
        assert_eq!(graph.get("a_sink").unwrap().inputs["items"].len(), 2);
    }

    #[test]
    fn unknown_type() {
        assert!(matches!(
            errors(&[("a", r#"type = "nope""#)])[..],
            [LoadError::UnknownType { .. }]
        ));
    }

    #[test]
    fn syntax_error_and_unknown_field() {
        assert!(matches!(
            errors(&[("a", "type = ")])[..],
            [LoadError::Parse { .. }]
        ));
        assert!(matches!(
            errors(&[("a", "type = \"source\"\nextra = 1")])[..],
            [LoadError::Parse { .. }]
        ));
    }

    #[test]
    fn missing_node_and_unknown_output() {
        let errs = errors(&[
            ("src", r#"type = "source""#),
            ("w1", "type = \"wrap\"\n[inputs]\nimage = \"ghost\""),
            ("w2", "type = \"wrap\"\n[inputs]\nimage = \"src.nope\""),
        ]);
        assert!(
            errs.iter()
                .any(|e| matches!(e, LoadError::MissingNode { node, .. } if node == "ghost"))
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, LoadError::UnknownOutput { output, .. } if output == "nope"))
        );
    }

    #[test]
    fn unknown_and_missing_inputs() {
        let errs = errors(&[("w", "type = \"wrap\"\n[inputs]\nimg = \"x\"")]);
        assert!(
            errs.iter()
                .any(|e| matches!(e, LoadError::UnknownInput { port, .. } if port == "img"))
        );
        assert!(
            errs.iter()
                .any(|e| matches!(e, LoadError::MissingInput { port, .. } if port == "image"))
        );
    }

    #[test]
    fn list_into_single_input() {
        let errs = errors(&[
            ("src", r#"type = "source""#),
            ("w", "type = \"wrap\"\n[inputs]\nimage = [\"src\"]"),
        ]);
        assert!(matches!(errs[..], [LoadError::NotMulti { .. }]));
    }

    #[test]
    fn type_mismatch() {
        // wrap outputs a layer; wrap.image expects an image (layer → image doesn't coerce).
        let errs = errors(&[
            ("src", r#"type = "source""#),
            ("w1", "type = \"wrap\"\n[inputs]\nimage = \"src\""),
            ("w2", "type = \"wrap\"\n[inputs]\nimage = \"w1\""),
        ]);
        assert!(matches!(
            errs[..],
            [LoadError::TypeMismatch {
                expected: ValueType::Image,
                found: ValueType::Layer,
                ..
            }]
        ));
    }

    #[test]
    fn bad_params() {
        let errs = errors(&[("src", "type = \"source\"\n[params]\nsize = \"big\"")]);
        assert!(matches!(errs[..], [LoadError::Params { .. }]));
        let errs = errors(&[("src", "type = \"source\"\n[params]\nunknown = 1")]);
        assert!(matches!(errs[..], [LoadError::Params { .. }]));
    }

    #[test]
    fn broken_node_is_not_reported_as_missing() {
        let errs = errors(&[
            ("src", "type = \"source\"\n[params]\nsize = \"big\""),
            ("w", "type = \"wrap\"\n[inputs]\nimage = \"src\""),
        ]);
        assert!(matches!(errs[..], [LoadError::Params { .. }]), "{errs:?}");
    }

    #[test]
    fn cycle() {
        // Layers coerce from images but not back, so build the cycle out of sinks-of-images.
        let errs = errors(&[
            ("a", "type = \"pass\"\n[inputs]\nimage = \"b\""),
            ("b", "type = \"pass\"\n[inputs]\nimage = \"a\""),
            ("c", r#"type = "source""#),
        ]);
        assert!(matches!(&errs[..], [LoadError::Cycle { nodes }] if nodes == &["a", "b"]));
    }

    #[test]
    fn invalid_file_name() {
        let sources = [(
            PathBuf::from("bad name.toml"),
            r#"type = "source""#.to_string(),
        )];
        let errs = from_sources(sources, &registry()).unwrap_err().0;
        assert!(matches!(errs[..], [LoadError::InvalidId { .. }]));
    }

    #[test]
    fn entry_outputs() {
        let pal = "type = \"pal\"\n[params.colors]\nsky = \"#87ceeb\"\nsun = \"yellow\"";
        let graph = load_str(&[
            ("p", pal),
            ("t", "type = \"tint\"\n[inputs]\ncolor = \"p.sun\""),
        ])
        .unwrap();
        assert_eq!(graph.nodes[0].id, "p");

        let errs = errors(&[
            ("p", pal),
            ("t", "type = \"tint\"\n[inputs]\ncolor = \"p.moon\""),
        ]);
        assert!(
            matches!(&errs[..], [LoadError::UnknownOutput { output, .. }] if output == "moon"),
            "{errs:?}"
        );
        let errs = errors(&[(
            "p",
            "type = \"pal\"\n[params.colors]\n\"no space\" = \"red\"",
        )]);
        assert!(
            matches!(&errs[..], [LoadError::Params { message, .. }] if message.contains("`no space`"))
        );
    }
}
