# Story checklist

Stories still to write, in the order to tackle them. Each one walks part of the spec
([`../specs/node-editor-ui.md`](../specs/node-editor-ui.md)) screen by screen, to find
what the spec doesn't decide yet. Gaps a story finds go into the spec, and the story's
"Spec gaps" section points at where each was decided (see story 1).

When a story is done: tick it, link it, and add it to the table in [`README.md`](README.md).

## Checklist

- [x] **01 — A blue background in a preview window** → [01-first-image.md](01-first-image.md)
  - From an empty project: add a canvas, make it large and blue, add a composite and a
    preview, connect them.
  - Covered: empty project, adding nodes, the edit view, the colour editor, prompts and
    pickers, connecting first inputs.

- [x] **02 — Exploring an existing project: `▣`, `f` and history** → [02-exploring.md](02-exploring.md)
  - Start from `examples/effects` (and `examples/transform`). Walk the flow with `h` `l` `j` `k`, press `f` on a `▣` to
    switch to a branch, go back and forth between flows with `[` / `]`, jump with `/`.
  - Checks: the "first branch" rule (§4.2) on a real graph, redrawing only one side
    (§4.4), scrolling (§4.5), focus history.
  - Likely gaps: where focus lands after `[`; one node feeding two `layers` entries.

- [x] **03 — Reshaping a chain: slide, move, dissolve, delete** → [03-reshaping.md](03-reshaping.md)
  - Start from `rectangle → transform → tint → composite`. Slide `tint` with `H` / `L`, pick up
    `transform` with `m` and drop it in another flow, `d` (dissolve) one node, `D` (delete)
    another.
  - Checks: all of §6.5, including a slide refused because the types don't fit; magenta on
    the node being moved (§5.3).
  - Likely gaps: what the flow shows while a node is picked up; where the flow and focus
    go when a dissolve or delete removes the anchor.

- [x] **04 — Layers: insert, reorder, disconnect** → [04-layers.md](04-layers.md)
  - Give a composite a second and third layer, reorder them with `K` / `J`, `i` a
    `transform` into one, `d` another.
  - Checks: `c` / `i` redrawing the flow (§4.4), list numbering after a reorder, which
    layer is "first" for the flow.
  - Likely gaps: where focus goes after `d` removes the conduit you were on; how `K` / `J`
    move the flow when the in-flow entry moves.

- [ ] **05 — Linked params**
  - Start from `examples/linked`. Link `width` / `height` to an `int` with `c`, nudge it
    with `+` / `-`, feed a param from a new node with `i`, unlink with `d`.
  - Checks: params moving between the edit view's params section and its input rows
    (§7), the `x ← offset` labels (§8), flow-view rows for linked params (§3.1).
  - Likely gaps: whether nudges work on a linked row in the flow view or only in the edit
    view; where focus goes when a row moves between sections.

- [ ] **06 — Palettes**
  - Start from `examples/palette`. Import with `I`; rename, insert, reorder and remove
    colours; link a rectangle's fill colour to one; remove a colour that's in use.
  - Checks: §7.1, palette output rows with swatches.
  - Likely gaps: the "colour in use" confirmation; what happens to linked params when
    their colour is removed; long palettes in a 24-column box.

- [ ] **07 — Breaking and fixing things**
  - Set a value out of range, disconnect a required input, break a node outside the
    current flow; then find each error and fix it.
  - Checks: §5.4 — red borders, red `▣`, the error count, the edit view's error section,
    the status line.
  - Likely gaps: getting to an error in another flow (the postponed "jump to next error"
    key); errors in nodes that no flow reaches.

- [ ] **08 — Undo across flows**
  - Make edits in two different flows, then `u` repeatedly.
  - Likely gaps: does undo bring back the flow and focus where each edit happened; what
    happens if undoing removes the node in focus.

- [ ] **09 — Small and resized terminals**
  - Shrink below 80×24 mid-edit and with a popup open, then grow back.
  - Checks: §4.6's promise to restore the exact state.

## Ideas for later

Add stories here as they come up.
