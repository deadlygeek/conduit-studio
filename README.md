# Conduit Studio

A text-first, node-based art generation tool written in Rust.

A project is a folder of small TOML files, one per node. Each node knows only its own
inputs, outputs and settings; nodes are wired together by naming the node whose output
they take. GUI tools (the `preview` window and the color picker) run as separate,
keyboard-driven windows.
Save a file and the image updates — or use `conduit edit`, a keyboard-driven terminal UI
for following flows through the graph and editing it.

```
white_bg ─────────────────────┐
                              ├─► comp ─► preview
black_sq ─► placed (300,300) ─┘
```

## Quick start

You need stable Rust 1.85 or newer (the workspace uses edition 2024). On Linux, the GUI
windows (built on eframe/egui) also need the Wayland/X11, xkbcommon and OpenGL
development libraries.

```sh
cargo build --workspace
target/debug/conduit run examples/basic
```

A window opens showing an 800×600 white canvas with a 50×50 black square at (300, 300).
Edit `examples/basic/placed.toml`, save, and the square moves.

`examples/linked` is the same idea with its numbers coming from `int` nodes: one `size`
node sets both the square's width and height, and one `offset` node its x and y.
`examples/palette` takes its colors from a palette imported from Coolors, with a `color`
node deriving a darker accent.

To explore and edit the project interactively instead (it renders alongside):

```sh
target/debug/conduit edit examples/basic
```

> Build the whole workspace, not just `conduit-cli`: `conduit run` launches the
> `conduit-preview` binary (and `conduit edit` the `conduit-picker` binary), looked for
> next to `conduit`, then on `$PATH`. Set `CONDUIT_PREVIEW_BIN` / `CONDUIT_PICKER_BIN` to
> use different programs.

## Commands

| Command | What it does |
|---|---|
| `conduit run <dir>` | Render the project, open a window per `preview` node, and re-render whenever a `.toml` file in `<dir>` changes. Ctrl-C to stop. |
| `conduit edit <dir>` | Everything `run` does, plus a terminal UI for exploring and editing the project (see [Editing projects](#editing-projects)). |
| `conduit check <dir>` | Load and render without opening windows. Prints each node's output, or every problem found, and exits non-zero on errors. |
| `conduit nodes` | List the available node types with their inputs, outputs and params. |
| `conduit palette import <dir> <id> <source>` | Create the palette node `<id>` (or replace its colors) from a Coolors link, hex codes, or a palette file — see [Palettes](#palettes). |

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

### Numbers: typed, default, or linked

Every number param (`width`, `x`, …) has a default, can be typed in `[params]`, or can be
**linked** to another node's number output by naming it in `[inputs]` — a link wins over a
typed value:

```toml
# square.toml
type = "rect"

[inputs]
width = "size"      # both come from the `int` node `size`
height = "size"

[params]
color = "#e63946"
```

```toml
# size.toml
type = "int"

[params]
value = 120
```

Change `size.toml` and everything linked to it follows. A linked value outside the param's
range (e.g. a width of 0) is reported as an error on the node that uses it.

### Colors: typed or linked

Colors are named (`white`, `black`, `red`, `green`, `lime`, `blue`, `yellow`, `cyan`,
`magenta`, `orange`, `purple`, `gray`/`grey`, `transparent`) or hex: `"#ff8800"`,
`"#ff880080"` (with alpha).

Like numbers, every color param can instead be **linked** to a color output — a `color`
node, a palette color, or anything else that outputs a color. A linked color param needs
no typed value, even when it's required:

```toml
# accent_dark.toml — a color node: a base color plus adjustments
type = "color"

[inputs]
color = "brand.accent"   # the base comes from the palette `brand` (or type one in [params])

[params]
lightness = -15          # -100..100; also hue (-180..180°), saturation (-100..100), alpha (0..100%)
```

```toml
# square.toml
type = "rect"

[inputs]
color = "accent_dark"
```

Color nodes chain: a palette color → a lighter version → a semi-transparent version of
that, and so on.

### Palettes

A palette is a named set of colors. **Each color is an output of its own**, referenced as
`palette.name`:

```toml
# brand.toml
type = "palette"

[params]
source = "https://coolors.co/264653-2a9d8f-e9c46a-f4a261-e76f51"   # optional note

[params.colors]
primary = "#264653"
teal = "#2a9d8f"
sand = "#e9c46a"
orange = "#f4a261"
accent = "#e76f51"
```

Colors keep the order they have in the file. Names follow the same rules as node ids.

To bring in a palette from elsewhere, paste it — `conduit palette import <dir> <id> …`, or
`I` in `conduit edit`. Accepted:

- a **Coolors link**: `https://coolors.co/264653-2a9d8f-e9c46a-f4a261-e76f51` (also the
  `/palette/…` form). Coolors has no public API, but its links carry the colors, so nothing
  is downloaded;
- **hex codes** in any list form: `#264653 #2a9d8f`, one per line (Lospec `.hex`),
  `["264653","2a9d8f"]`;
- Coolors' **CSS** (`--charcoal: #264653ff;`) and **JSON** (`{"Charcoal":"264653"}`)
  exports, which keep their names (`charcoal`, `persian_green`);
- **GIMP `.gpl`** palettes;
- a **file** containing any of the above (CLI: pass its path).

Colors without a name become `c1`, `c2`, …; rename them in the editor (`r`) and every
link follows.

Everything is validated before rendering, and all problems are reported at once, each
with its file: syntax errors, unknown types/inputs/params, missing connections,
references to nodes or outputs that don't exist, type mismatches and cycles.

### Values

Connections carry typed values:

| Type | Meaning |
|---|---|
| `image` | RGBA pixels (straight alpha) |
| `layer` | an image placed at an x, y offset, ready to composite |
| `number` | a number (from `int`); feeds number params |
| `color` | an RGBA color (from `color` or a palette); feeds color params |

An `image` can be connected where a `layer` is expected; it is placed at (0, 0).

### Built-in nodes

| Type | Inputs | Params | Output `out` |
|---|---|---|---|
| `rect` | — | `color`, `width`, `height` (1–16384, default 100) | image filled with `color` |
| `position` | `image`: image | `x`, `y` (default 0, may be negative) | layer |
| `composite` | `base`: image, `layers`: list of layers (optional) | — | `base` with each layer drawn over it in order (alpha blended, clipped to `base`) |
| `preview` | `image`: image | `title` (optional) | — (shows the image in a window) |
| `int` | — | `value` (default 0) | number |
| `color` | — | `color` (base), `hue`, `saturation`, `lightness`, `alpha` | the base color, adjusted |
| `palette` | — | `colors` (name = color), `source` (optional) | one color output per entry, named after it (no `out`) |

Number and color params (`width`, `x`, `color`, …) can also be linked from an output of the
same type instead of typed; see above.

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
dimmed), outputs (with who uses them) and any errors. Colors — typed, linked or in a
palette — get a true-color swatch (`██ #e76f51`); a palette shows one `col` row per color,
with who uses it. In the trees, `↺` marks a node
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
| `⏎` on a param | edit it; the value is checked (type, range, the node's own validation) before anything is written. Empty resets to the default. Either one replaces a link. |
| `c` / `d` on a number param | link it to a number output (e.g. an `int` node) / unlink it, going back to its typed or default value. A linked param shows its source and live value: `par width ← size  = 120`. |
| `+` `-` / `>` `<` / `}` `{` on a whole-number param | nudge it by ±1 / ±10 / ±50 (`=` and `_` work for ±1 too), clamped to its range and written straight to the file, so the preview follows. On a linked param this nudges the `int` feeding it. A run of nudges to one param undoes in a single `u`. |
| `i` on a number or color param | feed it from a new node: pick a type (`int`, `color`), accept the suggested id `<node>_<param>`, and the new node starts at the param's current value |
| `⏎` on a color | open the **color editor** (below) |
| `C` on a color | open the **color picker** window (below) |
| `c` / `d` on a color param | link it to a color output (a palette color or `color` node) / unlink it |
| `r` / `o` / `x` on a palette color | rename it (links follow) / add a color (a copy of this one) / remove it (asks first if it's used) |
| `I` on a palette | import colors: paste a Coolors link or hex codes; replaces the palette's colors. Adding a palette with `a` opens this straight away. |
| `⏎` or `c` on an input | pick a source: only type-compatible outputs that don't create a cycle are offered. List inputs append; single inputs are replaced. |
| `d` on an input | disconnect that connection |
| `i` on an input | insert a new node into that connection (`source → new → this`); only node types that fit are offered |
| `a` | add a node: pick a type, give it an id (suggested `<type>_N`) |
| `r` / `D` | rename the focused node (every reference is updated) / delete it (after confirmation; its connections are removed) |
| `E` | open the focused node's file in `$VISUAL` / `$EDITOR` (default `vi`) |
| `u` | undo the last edit (restores the files exactly; this session only) |

### Adjusting colors

**In the terminal**, `⏎` on a color opens the color editor: the new and old color, and one
row per channel with a gradient showing where it sits.

```
┏ color  brand.accent  (HSL) ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
┃ ████████████████  #e76f51                              ┃
┃ ████████████████  was #e76f51 ██                       ┃
┃ ▸ H  12°  █┃██████████████████████████████             ┃
┃   S  76%  ███████████████████████┃████████             ┃
┃   L  61%  ███████████████████┃████████████             ┃
┃   A 100%  ███████████████████████████████┃             ┃
┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
```

| Key | Action |
|---|---|
| `j` / `k` | choose a channel |
| `+` `-` / `>` `<` / `}` `{` | nudge it by 1 / 10 / 50 (degrees, percent, or 0–255) |
| `tab` | switch between H S L A and R G B A |
| `#` | type a hex code or color name |
| `⏎` / `esc` | keep / put everything back as it was (including a link) |

Every change is written as you go, so the preview follows; the whole session undoes in one
`u`. Hue survives going through greys, white and black.

**In a window**, `C` on a color opens `conduit-picker`: an HSV square with hue and alpha
bars, a hex field, H S L A and R G B sliders, and every palette color in the project as a
numbered swatch. It works with the mouse, and with the keyboard:

| Key | Action |
|---|---|
| `h` `s` `l` `a`, `r` `g` `b` | choose a channel |
| `←` / `→` (`shift`: ×10) | nudge it; the editor's `+ - > < } {` work too |
| `#` | type a hex code |
| `1`–`9` | take that palette color |
| `⏎` / `q` | done |
| `esc` | put the file back as it was, and close |

The picker writes to the node file as you go, like any other edit. The editor remembers the
file as it was when you pressed `C`, so `u` undoes the whole picker session.

### Notes

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
  conduit-cli      the `conduit` binary: run, edit (the TUI, ratatui), check, nodes, palette
  conduit-preview  the `conduit-preview` window (egui/eframe)
  conduit-picker   the `conduit-picker` color picker window (egui/eframe)
examples/basic     the example project
examples/linked    the same, with sizes and positions fed from `int` nodes
examples/palette   colors from an imported palette, adjusted with a `color` node
```

- Non-GUI nodes run inside the `conduit` process. Each render loads every file, checks
  the graph, then evaluates nodes in dependency order. Nodes whose type, params and
  upstream nodes are unchanged since the last render reuse their previous outputs (a
  fingerprint per node, chained through its sources); GUI nodes always run so their
  windows stay current. The status shows e.g. `rendered 7 nodes (5 unchanged) in 3 ms`.
- The renderer (`conduit-cli/src/run.rs`) runs on a background thread: it watches the
  folder, re-renders only when node files actually change, and reports events. `conduit run`
  prints them; `conduit edit` shows them in its status line and sends it probe commands.
- A node marked `gui` sends its image to a `GuiHost`. The runner's host starts one
  `conduit-preview` process per GUI node (plus one for the probe) and streams frames to
  its stdin. Under `conduit check` the images are discarded.
- The editor never edits files any other way than through `conduit_core::edit`, which
  uses `toml_edit` (comments survive), applies each edit all-or-nothing, and returns the
  previous file contents for undo. The color picker is a separate process that writes
  through the same functions; it never talks to the editor, which just sees the file change.
- Color maths (HSL conversion, adjustments, the channel model shared by the terminal editor
  and the picker) is in `conduit-core/src/color.rs`; palette text parsing (Coolors links,
  hex lists, CSS/JSON exports, GIMP palettes) in `conduit-core/src/palette.rs`.
- A palette's colors are outputs because its output port is marked `.entries()`: the
  node's actual outputs are the keys of the `[params.colors]` table
  (`NodeSpec::outputs_for(params)`).
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
    #[serde(default = "one")]
    amount: f32, // 0.0 = invisible, 1.0 = unchanged
}

fn one() -> f32 {
    1.0
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
                    .default("1.0") // number params need one (see below)
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
- **Int and float params must have a default** (registration panics otherwise), because
  each one automatically gets an optional `number` input of the same name: when it's
  connected, the incoming value (rounded for ints, range-checked) replaces the param before
  `eval` sees it. Color params get a `color` input the same way (written in as hex), and
  may stay required. Nothing else is needed for a node's numbers and colors to be linkable.
- For outputs that depend on the node's file, like a palette's colors, use
  `PortSpec::new("colors", ValueType::Color).entries()` with a `ParamSpec::colors("colors", …)`
  table param, and return one output per table key.
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

- **Next:** `opacity` and `text` nodes; a headless `save_png` node; nodes that
  compute numbers (add, multiply, …) so one `int` can drive related values; a `float` node.
- **Transforms:** a `layer` becomes an image plus a 2D affine matrix instead of an x, y
  offset. A single `transform` node (x, y, rotate, scale, skew) replaces `position`: it takes
  a layer (or an image, as an untransformed layer) and returns a layer, only updating the
  matrix, so chained transforms never resample. `composite` resamples each layer once using
  its final matrix. Pixel effects (blur, color, opacity) stay image → image nodes; to apply
  one after a transform, composite onto its own canvas first. Transforms stay separate nodes
  rather than settings on `composite`'s inputs, so they keep per-node params and linking.
- **Color:** a `mix` node (blend two colors) and gradients; palette generators (shades and
  tints of one color, complementary / analogous / triadic schemes); an eyedropper that picks
  a color from the preview window; optional online palette sources (e.g. Lospec's palette
  list, Colormind's generator) behind an explicit command.
- **Editor:** a path filter (mark a node, then show only the flows between it and the
  focused node); reordering list inputs; image thumbnails in the terminal; nudging float
  params (with steps that suit their range).
- **Later:** nodes as external programs written in any language; more GUI tools in the
  picker's style (e.g. a slider window for numbers) that write values back to the node's
  TOML file.
