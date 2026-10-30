# Conduit Studio

A text-first, node-based art generation tool written in Rust.

A project is a folder of small TOML files, one per node. Each node knows only its own
inputs, outputs and settings; nodes are wired together by naming the node whose output
they take. GUI nodes (currently just `preview`) run as separate, keyboard-driven windows.
Save a file and the image updates — or use `conduit edit`, a keyboard-driven terminal UI
for following flows through the graph and editing it.

```
white_bg ─────────────────────┐
                              ├─► comp ─► preview
black_sq ─► placed (300,300) ─┘
```

## Quick start

Development happens inside the `bevy-dev` distrobox (the host has no `mold` linker,
which `~/.cargo/config.toml` requires):

```sh
distrobox enter bevy-dev
cargo build --workspace
target/debug/conduit run examples/basic
```

A window opens showing an 800×600 white canvas with a 50×50 black square at (300, 300).
Edit `examples/basic/placed.toml`, save, and the square moves.

To explore and edit the project interactively instead (it renders alongside):

```sh
target/debug/conduit edit examples/basic
```

> Build the whole workspace, not just `conduit-cli`: `conduit run` launches the
> `conduit-preview` binary, which is looked for next to `conduit`, then on `$PATH`.
> Set `CONDUIT_PREVIEW_BIN` to use a different program.

## Commands

| Command | What it does |
|---|---|
| `conduit run <dir>` | Render the project, open a window per `preview` node, and re-render whenever a `.toml` file in `<dir>` changes. Ctrl-C to stop. |
| `conduit edit <dir>` | Everything `run` does, plus a terminal UI for exploring and editing the project (see [Editing projects](#editing-projects)). |
| `conduit check <dir>` | Load and render without opening windows. Prints each node's output, or every problem found, and exits non-zero on errors. |
| `conduit nodes` | List the available node types with their inputs, outputs and params. |

## Writing a project

Each `*.toml` file directly inside the project folder is one node. **The file name is
the node's id**: `white_bg.toml` defines the node `white_bg`. Ids may contain letters,
digits, `_` and `-`.

```toml
# placed.toml
type = "position"          # which kind of node this is (see `conduit nodes`)

[inputs]
image = "black_sq"         # take the `out` output of the node `black_sq`

[params]
x = 300
y = 300
```

- **`type`** is required.
- **`[inputs]`** maps each input of this node to a source:
  - `"node"` means the `out` output of `node`; `"node.port"` names an output explicitly.
  - Inputs that accept a list use an array; order matters (e.g. drawing order):
    `layers = ["background_shape", "placed"]`.
- **`[params]`** holds the node's settings. Unknown or mistyped settings are errors.

Colors are named (`white`, `black`, `red`, `green`, `lime`, `blue`, `yellow`, `cyan`,
`magenta`, `orange`, `purple`, `gray`/`grey`, `transparent`) or hex: `"#ff8800"`,
`"#ff880080"` (with alpha).

Everything is validated before rendering, and all problems are reported at once, each
with its file: syntax errors, unknown types/inputs/params, missing connections,
references to nodes or outputs that don't exist, type mismatches and cycles.

### Values

Connections carry typed values:

| Type | Meaning |
|---|---|
| `image` | RGBA pixels (straight alpha) |
| `layer` | an image placed at an x, y offset, ready to composite |
| `number`, `color` | reserved for future nodes |

An `image` can be connected where a `layer` is expected; it is placed at (0, 0).

### Built-in nodes

| Type | Inputs | Params | Output `out` |
|---|---|---|---|
| `rect` | — | `color`, `width`, `height` (1–16384) | image filled with `color` |
| `position` | `image`: image | `x`, `y` (default 0, may be negative) | layer |
| `composite` | `base`: image, `layers`: list of layers (optional) | — | `base` with each layer drawn over it in order (alpha blended, clipped to `base`) |
| `preview` | `image`: image | `title` (optional) | — (shows the image in a window) |

## Preview window

All controls are on the keyboard:

| Key | Action |
|---|---|
| `+` / `-` | zoom in / out |
| `0` | actual size (100%) |
| `f` | fit to window (the default) |
| arrow keys | pan when zoomed in |
| `s` | save the image as `<project>/out/<node>-<timestamp>.png` |
| `q` / `Esc` | close the window (it reopens on the next change) |

Transparent areas show a checkerboard. If the project has an error, a red banner shows
it above the last good image until the error is fixed.

## Editing projects

`conduit edit <dir>` opens a terminal UI centred on one node at a time. Large graphs stay
manageable because you never look at the whole graph — only the flows through the node in
focus: everything upstream of it on the left, everything that uses it on the right.

```
 conduit edit examples/basic  ·  5 nodes
┌ ← inputs (3) ───────────┐┏ ● comp  composite ━━━━━━━━━━━━┓┌ used by → (1) ────────┐
│   base ← white_bg  rect  │┃ in  base       ← white_bg     ┃│   preview.image       │
│ ▾ layers ← placed  posi… │┃ in  layers[0]  ← placed       ┃│                       │
│     image ← black_sq     │┃ out out        → preview.image┃│                       │
└──────────────────────────┘┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛└───────────────────────┘
 ● rendered 5 nodes in 18 ms                                      probe: comp.out
 h/l follow   j/k move   ⏎ edit   c/d connect   i insert   a add   …   ? help
```

The middle panel lists the node's inputs (one row per connection), params (defaults shown
dimmed), outputs (with who uses them) and any errors. In the trees, `↺` marks a node
already shown higher up, and broken or missing nodes are red. Below 100 columns the panes
stack vertically.

### Moving around

| Key | Action |
|---|---|
| `h` / `l` | follow the connection under the cursor upstream / downstream (repeat to walk a chain) |
| `j` / `k`, `g` / `G` | move the cursor; top / bottom |
| `H` / `L` | move into the inputs / used-by tree; there `⏎` focuses a node and `space` folds a branch |
| `tab`, `esc` | cycle panes; back to the node panel |
| `[` / `]` | back / forward through nodes you've focused (also `ctrl-o`, backspace) |
| `/` | fuzzy-find a node by name or type (`wbg` finds `white_bg`) |
| `p` | probe: show the output under the cursor in its own preview window |
| `P` | probe follows focus on / off |
| `?` / `q` | help / quit |

The probe window updates on every render, so you can watch any point in a flow while you
edit.

### Changing the project

| Key | Action |
|---|---|
| `⏎` on a param | edit it; the value is checked (type, range, the node's own validation) before anything is written. Empty resets to the default. |
| `⏎` or `c` on an input | pick a source: only type-compatible outputs that don't create a cycle are offered. List inputs append; single inputs are replaced. |
| `d` on an input | disconnect that connection |
| `i` on an input | insert a new node into that connection (`source → new → this`); only node types that fit are offered |
| `a` | add a node: pick a type, give it an id (suggested `<type>_N`) |
| `r` / `D` | rename the focused node (every reference is updated) / delete it (after confirmation; its connections are removed) |
| `E` | open the focused node's file in `$VISUAL` / `$EDITOR` (default `vi`) |
| `u` | undo the last edit (restores the files exactly; this session only) |

Edits are written straight to the node files, keeping your comments and formatting, and the
renderer picks them up like any other change. Changes made in another editor show up live
too. A project can be temporarily broken while you work (an unconnected input, say) — the
problem is shown in red until it's fixed.

## How it fits together

```
crates/
  conduit-core     values, the Node trait (ports + param specs), registry, graph loading,
                   validation and evaluation; plus, for editors, a lenient project view
                   (`inspect`), lineage/cycle/compatibility queries, and file edits with undo
  conduit-nodes    built-in nodes (one module each) and register_all()
  conduit-ipc      the runner → GUI process message format
  conduit-cli      the `conduit` binary: run, edit (the TUI, ratatui), check, nodes
  conduit-preview  the `conduit-preview` window (egui/eframe)
examples/basic     the example project
```

- Non-GUI nodes run inside the `conduit` process. Each render loads every file, checks
  the graph, then evaluates nodes in dependency order.
- The renderer (`conduit-cli/src/run.rs`) runs on a background thread: it watches the
  folder, re-renders only when node files actually change, and reports events. `conduit run`
  prints them; `conduit edit` shows them in its status line and sends it probe commands.
- A node marked `gui` sends its image to a `GuiHost`. The runner's host starts one
  `conduit-preview` process per GUI node (plus one for the probe) and streams frames to
  its stdin. Under `conduit check` the images are discarded.
- The editor never edits files any other way than through `conduit_core::edit`, which
  uses `toml_edit` (comments survive), applies each edit all-or-nothing, and returns the
  previous file contents for undo.
- **IPC format** (`conduit-ipc`): each message is a `u32` little-endian length followed by
  a one-byte tag: `1` frame (`u32` width, `u32` height, RGBA bytes), `2` error (UTF-8 text),
  `3` close. A preview window exits when its stdin closes, so stopping the runner closes
  its windows.

## Adding a node

1. Create a module in `crates/conduit-nodes/src/` that implements `conduit_core::Node`.
2. Add it to `register_all()` in `crates/conduit-nodes/src/lib.rs`.

For example, a node that scales an image's transparency:

```rust
// crates/conduit-nodes/src/opacity.rs
use std::sync::Arc;

use conduit_core::{
    EvalCtx, Inputs, Node, NodeResult, NodeSpec, Outputs, ParamSpec, PortSpec, Value,
    ValueType, parse_params,
};
use serde::Deserialize;

pub struct Opacity;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)] // reject typos in [params]
struct Params {
    amount: f32, // 0.0 = invisible, 1.0 = unchanged
}

impl Node for Opacity {
    fn spec(&self) -> NodeSpec {
        NodeSpec {
            kind: "opacity", // the `type = "..."` used in node files
            inputs: vec![PortSpec::new("image", ValueType::Image)],
            outputs: vec![PortSpec::new("out", ValueType::Image)],
            // Describes [params] for editors and new-node templates.
            params: vec![
                ParamSpec::float("amount", 0.0, 1.0, "1.0")
                    .doc("0 = invisible, 1 = unchanged"),
            ],
            gui: false,
        }
    }

    // Called at load time, so bad params are reported before anything renders.
    fn check_params(&self, params: &toml::Table) -> NodeResult<()> {
        parse_params::<Params>(params).map(|_| ())
    }

    fn eval(&self, inputs: &Inputs, params: &toml::Table, _: &mut EvalCtx) -> NodeResult<Outputs> {
        let p: Params = parse_params(params)?;
        let mut image = inputs.image("image")?.as_ref().clone();
        for pixel in image.pixels_mut() {
            pixel[3] = (f32::from(pixel[3]) * p.amount.clamp(0.0, 1.0)).round() as u8;
        }
        Ok(Outputs::out(Value::Image(Arc::new(image))))
    }
}
```

Things to know:

- Inputs arrive already converted to the declared port type. Use `inputs.image(..)`,
  `inputs.layers(..)` or `inputs.all(..)` to read them.
- `PortSpec::new(..).multi()` accepts a list of connections; `.optional()` allows leaving
  the input unconnected.
- List every param in `params` with `ParamSpec::int / float / color / text(name, …, example)`;
  `.default("…")` makes one optional. `conduit edit` uses these for its forms and for new
  nodes, and a test in `conduit-nodes` fails if they drift from what `check_params` accepts.
- Return every declared output (`Outputs::out(..)` for the usual single `out` port);
  evaluation fails otherwise.
- Images are `Arc<RgbaImage>`, so passing them along is cheap; clone the pixels only
  when you change them.
- Add unit tests next to the node; `crates/conduit-nodes/src/test_util.rs` has helpers
  for building images and running a node directly.

## Development

```sh
cargo test --workspace                       # unit + integration tests (no display needed)
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
```

To try the preview window without the runner, pipe the test sender into it:

```sh
cargo run -q -p conduit-ipc --example send_frames | cargo run -q -p conduit-preview
```

## Roadmap

- **Next:** skip re-rendering nodes whose inputs and params haven't changed; let params be
  driven by inputs (e.g. a `number` node feeding `position.x`); `rotate`, `opacity` and
  `text` nodes; a headless `save_png` node.
- **Editor:** a path filter (mark a node, then show only the flows between it and the
  focused node); reordering list inputs; image thumbnails in the terminal.
- **Later:** nodes as external programs written in any language; editor GUI nodes (color
  picker, slider) that write their values back to the node's TOML file.
