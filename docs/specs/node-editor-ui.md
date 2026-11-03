# Node editor UI — flow view (spec, draft 8)

Status: **draft 8**, 2026-11-03 — no open questions. Derived from the durdraw
sketches in `scratch-pad/`
(`node-graph-overview`, `node-graph`, `node-graph-node-focus`, `node-graph-conduit-focus`,
`node-graph-conduit-end-focus`, `node-edit`). Where the sketches disagree with each other or
with the code, this document decides; decisions are marked **[decided]**. The questions raised along the way and their answers are summarised in
[§10](#10-decision-log). User stories that walk through the spec screen by screen are in
[`../stories/`](../stories/).

Changes from draft 7, from walking story 3 (`../stories/03-reshaping.md`): a refused
key explains itself in the status line (§6); a slide isn't a flow change, and a pick-up
moves focus to the conduit that closed the gap; the carried node isn't drawn, the status
line names it; `Move Actions` bar; a drop redraws the flow and focuses the node (§6.5);
`d` / `D` on a node confirm first (§6.6); flow and focus after a dissolve or delete
(§6.7); `[` back to a flow an edit has changed (§6.1).

## 1. Goal

Replace the three-pane layout of `conduit edit` (inputs tree · focused node · used-by tree)
with a **flow view**: the nodes of one flow drawn as boxes, left to right, joined by drawn
**conduits** (connections). Connections not drawn in the flow are shown as `▣` markers and
can be brought into it. The user moves a **focus** between nodes, conduits and conduit
ends, and a contextual **action bar** lists what can be done to the thing in focus. A node
can be opened in a larger **edit view** to change its name and params.

Everything the current editor can do must stay reachable (§9 maps old keys to new).

## 2. Vocabulary

| Term | Meaning |
|---|---|
| **flow** | One path through the graph, from a source to a sink, drawn as a single row of node boxes (§4). |
| **node box** | A node drawn on the canvas (§3). |
| **port row** | One row inside a box: an input or output port with a stub through the box's side. Each port row is exactly one conduit end (§3.1). |
| **conduit** | One connection: an output of one node feeding an input of another. One entry of a list input is one conduit; an output used by three nodes has three conduits. |
| **conduit end** | Where a conduit meets a port row. If the conduit is drawn in the flow, a line runs to its other end; if not, the end shows `▣` — even when the node at the other end is in the flow. |
| **focus** | The single thing the keyboard acts on: a node, a conduit, or a conduit end. |
| **action bar** | The titled strip at the bottom of the screen listing the actions for the focus. |

## 3. Node box

From `node-graph`, corrected per §3.3:

```
      ╭─ transform ─╮
      │ blue-move   │
      │─────────────│
 ─────┼─layer       │
    ▣─┼─x           │
    ▣─┼─y           │
      │─────────────│
      │         out─┼─────
      ╰─────────────╯
```

- Rounded corners `╭ ╮ ╰ ╯`; the node **type** sits in the top border: `╭─ <type> ─╮`.
- Row 2 is the node **id** (its file name).
- **Inputs** on the left: the stub `─┼─` crosses the left border, the port name follows.
- **Outputs** on the right, right-aligned: name, then `─┼─` crossing the right border.
- Sections are separated by a divider; an **empty section and its divider are omitted**
  (a source has no input section, a sink no output section).

### 3.1 One row per conduit end

**[decided]** Every port row is one conduit end, so "the row under focus" and "the conduit
end under focus" are the same thing.

| Port | Rows |
|---|---|
| Single input, connected | one row |
| List input | one row per connection, `layers[0]`, `layers[1]`, … in list order |
| Linked param (a number or color param fed by a node) | one row, labelled with the param name — it's a conduit like any input |
| Unlinked param | no row in the flow view; it lives in the edit view (§7) |
| Unconnected input | one row, with the unconnected marker (§4.3) |
| Output with *n* users | *n* rows, each `out─┼─`, users in id order; the one in the flow draws its line, the rest show `▣`. **[decided]** The rows stay plain `out`: the status line names the user when a row is focused, the far end turns magenta when it's on screen (§5.3), and `f` follows it. |
| Output with no users | one row, no marker |

### 3.2 Width

**[decided]** Full type names everywhere (`transform`, `composite`, never `trans`/`compo`).
Each box grows to fit its widest text — type label, id, or port label — plus one column of
padding each side.

**[decided]** The content width is clamped to **8–24 columns**. Text longer than the cap
is cut and ends in `…` (`helloWorldTransfo…`). The full text is always shown in the
status line when that node or row is focused, and in the edit view, which has no cap. The
cap is one constant, so it's easy to tune.

### 3.3 Decisions on the sketches' inconsistencies

| Sketch shows | Decision |
|---|---|
| `node-graph` puts outputs **above** inputs; `node-graph-overview` and `node-edit` put inputs first. | **[decided]** Inputs above outputs. |
| Empty sections sometimes drawn as a bare divider. | **[decided]** Omit them (§3). |
| Type abbreviated (`trans`, `compo`). | **[decided]** Full names, width capped (§3.2). |
| Port names `image`, `layer` (as an output), `background` — none exist. `rect` and `transform` output `out`; `composite` takes `base` and `layers`. | **[decided]** Real port names from the node spec. |
| List entries numbered `layer[1]`, `layer[3]`, `layer[2]`. | **[decided]** Real name, 0-based, in list order: `layers[0]`, `layers[1]`, `layers[2]`. |
| Conduit bar `(d)issolve (i)insert`, titled `Conduit Focus Actions`. | **[decided]** See §6.3. |
| `node-edit` wraps the focused value in `► ◄`. | **[decided]** No markers: the focus colour (green, bold) is enough. |
| `node-edit` labels rows with the connected node's name for some ports and the port name for others. | **[decided]** Port name in the flow view, port and connected node in the edit view (§8). |

## 4. Flows

**[decided]** The view shows **one flow at a time**, so large graphs stay readable in a
terminal. Everything outside the flow is reached through `▣` ends.

### 4.1 What a flow is

A flow is a single path: a source node, then the node each one feeds, up to a sink. Every
node in it has at most one **flow input** (the row whose conduit is drawn to the node on
its left) and at most one **flow output** (the row drawn to the node on its right). Every
other connected row ends in `▣`.

In `node-graph` the flow is `rect → transform → composite → preview`:

```
╭─ rect ───────╮   ╭─ transform ─╮      ╭─ composite ─╮    ╭─ preview ─────╮
│ blue-square  │   │ blue-move   │      │ main-comp   │    │ main-preview  │
│──────────────│   │─────────────│      │─────────────│    │───────────────│
│          out─┼───┼─layer       │    ▣─┼─base        │ ╭──┼─image         │
╰──────────────╯ ▣─┼─x           │ ╭────┼─layers[0]   │ │  ╰───────────────╯
                 ▣─┼─y           │ │  ▣─┼─layers[1]   │ │
                   │─────────────│ │  ▣─┼─layers[2]   │ │
                   │         out─┼─╯    │─────────────│ │
                   ╰─────────────╯      │         out─┼─╯
                                        ╰─────────────╯
```

Because a flow is one row of boxes, layout is simple: boxes left to right, a fixed gap
between them, and each conduit runs from its output row to its input row with rounded
corners (`╮ ╰ ╭ ╯`). There is only ever one drawn conduit per gap.

### 4.2 Building a flow

A flow is built around one node, the **anchor**:

- **Upstream**: from the anchor, take its first connected input, then that source's first
  connected input, and so on to a source. Port order, with plain inputs before linked
  params — the same rule `X` (dissolve) uses today.
- **Downstream**: from the anchor, take the first user of its first output, and so on to a
  sink. Users in id order.

On start-up the anchor is the first sink (e.g. `preview`), or the first node if there is
none, and **[decided]** the focus starts on the anchor node. `/` (find) makes the found
node the anchor and the focus, and rebuilds the flow around it.

**[decided]** Adding a node with `a` makes the new node the anchor and the focus, and the
flow is rebuilt around it. A new node has no connections, so the flow is just that node,
and the previous flow leaves the screen until it is connected to (it stays in focus
history, so `[` returns to it).

**[decided]** No branch memory: a rebuilt flow always takes "first" as above. The way back
to an earlier flow is focus history (`[`).

### 4.3 Ends and markers

| Marker | Meaning |
|---|---|
| line to the next box | the conduit is in the flow |
| `▣─┼─` | connected, but the conduit isn't drawn in the flow — usually because the other node isn't in it, but also when both nodes are and the conduit isn't the single path between them (e.g. a node that reaches a composite both directly and through other nodes) |
| `·─┼─` dimmed **[decided]** | unconnected optional input |
| `✕─┼─` red **[decided]** | unconnected required input (an error today) |
| `─┼─` and nothing | output with no users |

### 4.4 Redrawing the flow through a `▣`

**[decided]** Focusing a `▣` end and pressing `f` (or `⏎`) brings that conduit into the
flow and redraws **only the side it points to**. Apart from `c` and `i` (below), dropping
a moved node (§6.5) and deleting one (§6.7), only `/` and focus history redraw the flow —
in particular not `h`/`l`.

- **`▣` on an input row** of node *N*: *N* and everything downstream of it stay; the
  upstream part is replaced by the path through this input's source (built as in §4.2).
- **`▣` on an output row** of *N*: *N* and everything upstream stay; the downstream part
  is replaced by the path through this user.

Focus stays on the same end, which now draws its line. Each redraw is a step in focus
history, so `[` returns to the previous flow.

**[decided]** Connecting also redraws the flow. When `c` (or `i`) from a conduit end
makes a conduit — e.g. a second `layers` entry, which isn't the node's first connected
input — the flow is redrawn through it exactly as `f` would, and focus stays on that end.
You see what you just connected without pressing `f`. (When the new conduit is the
node's first connected input, as in story 1, the result is the same either way.)

### 4.5 Scrolling

A flow can be wider than the screen. The view scrolls horizontally to keep the focus
visible with a margin of one box where possible. Boxes cut by the screen edge are drawn
clipped. A node with more rows than fit on screen scrolls vertically.

**[decided]** There is no extra indicator for boxes off screen: the conduit running off
the edge shows the flow carries on.

**[decided]** How far it scrolls depends on what moved the focus:

- **After a flow change** — `f`, `/`, `[` / `]`, `a`, a `c` / `i` that redraws, a drop
  (§6.5) or a `D` (§6.7) — the view re-centres on the focus, so you get your bearings after a jump.
- **While walking** — `h` `l` `j` `k`, `g` `G`, and after an edit that keeps the flow's
  path: a slide, a pick-up, a dissolve — it scrolls as little as possible to keep
  the focus visible with a box to spare on each side, so the view doesn't slide on every
  keypress.
- Either way it stops at the flow's ends: never scrolled past the first box or the last,
  and a flow that fits on screen never scrolls.

### 4.6 Too small a screen

**[decided]** Below a minimum size the editor doesn't try to squeeze the flow in. The
screen is replaced by a message:

```
  The terminal is too small for the editor: 62×18, needs at least 80×24.

  Make the window bigger, or edit the node files by hand:
    examples/basic/*.toml

  q quit
```

- **[decided]** Minimum **80×24**: room for three 24-column boxes with their gaps, the
  status line and the action bar.
- Rendering, file watching and preview windows carry on as normal; only `q` works.
- Growing the window back restores the flow and focus exactly as they were.
- The edit view has its own minimum (its widest row plus borders); if it doesn't fit, the
  same message shows until the window grows or `esc` closes the edit view.

### 4.7 Screen layout **[decided]**

Top to bottom, at the 80×24 minimum:

| Rows | What |
|---|---|
| 1 | Title line: ` conduit edit <dir>  ·  <n> nodes`, as today. |
| 2 – 17 | The flow canvas (or the edit view over it). |
| 18 | Status line: the last edit or render result, the focus's full names, `⇄ moving …`, the error count. |
| 19 | blank |
| 20 – 23 | Action bar (§6.4): title rule, blank row, focus actions, global keys. |
| 24 | blank |

A taller or wider terminal gives the extra space to the canvas.

### 4.8 Empty project **[decided]**

With no nodes there is no flow and no focus. The canvas shows a centred hint and the bar
has no focus actions, only the global keys that work without a node:

```
 conduit edit story  ·  0 nodes




                        This project has no nodes yet.
                        Press a to add one.


  Project Actions ─────────────────────────────────────────────────────────────

  global  [a]dd [u]ndo [/]find [?]help [q]uit
```

`a` adds the first node, which becomes the anchor (§4.2).

## 5. Colours and focus

### 5.1 Palette

**[decided]** The editor paints its own background **black** on every cell, whatever the
terminal's theme, and the standard colour for borders, text and conduits is **white**.
Two highlight colours mark what's in focus and what it's related to; nothing else uses
them.

| Role | Colour | Used for |
|---|---|---|
| background | black | every cell |
| standard | white | borders, labels, ids, conduits, the action bar's words |
| **focus** (primary highlight) | green, bold | the one thing in focus (§5.2) |
| **related** (secondary highlight) | magenta | what the focus points at or belongs to (§5.3) |
| dim | grey | `(default)`, `·` unconnected optional inputs, the action bar's `global` label |
| error | red | `✕` unconnected required inputs, broken nodes (§5.4) |
| swatch | the colour itself (true colour) | colour values and palette rows |

Bold on the focus means it still stands out with `NO_COLOR`. Magenta is suggested for
the secondary colour because it is easy to tell from green, including for the common
red–green colour-vision deficiencies.

### 5.2 Focus (green)

Taken from the sketches, cell by cell:

| Focus | Green | Not green | Sketch |
|---|---|---|---|
| **Node** | The whole box: all four borders and corners, the type, the id, dividers, every port label, and every stub `─┼─` with its marker (`▣`, `·`, `✕`). | The conduit lines between boxes. | `node-graph-node-focus` |
| **Conduit** | The line itself, from just outside the output stub to just outside the input stub, including its corners (`─╮ │ ╰────`). | Both nodes, including the `┼` where the line crosses each border. | `node-graph-conduit-focus` |
| **Conduit end** | That row's marker and stub, through the border: `▣─┼─`, `·─┼─`, `─┼─▣`. | The port label, the rest of the box, and the line. | `node-graph-conduit-end-focus` |
| **Edit view row** | The value; on the name row the id. | The row's label (`name:`, `scale`). | `node-edit` |
| **Action bar** | The bracketed key of each focus action, `(e)`. | The rest of the word. | all |

### 5.3 Related (magenta) **[decided]**

The secondary colour answers "what does this focus mean?" without having to read the
status line:

| Focus | Magenta |
|---|---|
| Node | its flow input and flow output conduits (the lines in and out of it) |
| Conduit | the port labels at both ends, e.g. `out` on `blue-move` and `layers[0]` on `main-comp` |
| Conduit end | its own port label, and the port label at the other end when that is in the flow |
| Edit view row | the row's label |

The status line still names the focus in full, e.g. `main-comp.layers[2] ← blue-move-2.out`
for an end.

**[decided]** A node being moved (§6.5) is out of the flow and isn't drawn, so it has no
colour; the status line names it.

### 5.4 Errors **[decided: C]**

Today the middle panel lists a node's errors in red, and broken or missing nodes are red in
the trees. In the flow view:

**A. Border and status line.** A broken node's box border is red (green wins while it's
focused). Focusing it puts the first error in the status line, `✕ width must be at most
4096`. Small, but you must focus the node to read why.

**B. A as well as an error row in the box.** The box gets a last row, `✕ width must be…`,
cut to the width cap. You can see what's wrong without focusing, but boxes get taller and
the message is usually truncated.

**C. A as well as an errors section in the edit view.** The edit view lists every error in
full, under the outputs, with the row it concerns (a param, an input) also in red.

**[decided]** Two additions on top of C:

- A `▣` whose off-flow node is broken is drawn red, so problems outside the flow are
  still visible.
- The status line shows a count, `2 errors`, when any node in the project is broken, so
  an error never goes unnoticed because it's in another flow. (Possible later: a key to
  jump to the next broken node.)

**[decided]** Option **C**: a broken node's border is red, focusing it shows the first
error in the status line, and the edit view lists every error in full with the row it
concerns in red. Options A and B are kept above for reference.

## 6. Keys

**[decided]** The aim is one small grammar: the same key means the same idea whatever is
focused, and keys that don't apply right now do nothing and aren't listed in the bar
(§6.4).

**[decided]** One exception: a key for a real action that is refused here — `L` on a
node that can't slide downstream, `m` on a node nothing feeds — isn't listed either, but
pressing it says why in the status line (`can't slide toned after comp: preview.image
takes an image, not a layer`). Keys that mean nothing for this focus stay silent.

### 6.1 Moving (same everywhere)

| Key | Does |
|---|---|
| `h` / `l` | Move upstream / downstream along the flow. Stops, left to right: node, the conduit leaving it, next node, …. On a row, `h` on an in-flow input row and `l` on an in-flow output row move onto that row's conduit; the other direction goes back to the node. On a `▣` row the outward direction does nothing — `f` brings it in. |
| `j` / `k` | On a node: into its first / last row. On a row: next / previous row; past the first or last row, back to the node. |
| `esc` | Back to the node (from a row or a conduit); in the edit view, close it. |
| `g` / `G` | First / last node of the flow. |
| `[` / `]` | Back / forward through flow history (also `ctrl-o`, backspace). **[decided]** A step is a flow change — `f`, `/`, `a`, a `c` / `i` that redraws, a drop, a `D` — not a focus move or an edit that keeps the flow's path; `[` restores the flow and the focus where you left it. To retrace steps within a flow, walk it with `h` / `l`. **[decided]** A step remembers the flow's conduits: if an edit has since removed some, `[` redraws those that still exist and rebuilds the rest as §4.2 does, and focus goes to the anchor if the old focus is gone. |
| `/` | Find a node; it becomes the anchor of a new flow (§4.2). |
| `⏎` | The focus's primary action (marked in §6.3). |

### 6.2 Verbs (same meaning everywhere)

| Key | Meaning |
|---|---|
| `e` | **edit** the thing itself: a node opens the edit view; a param or the name opens its prompt or picker |
| `c` | **connect**: on an input end, choose what feeds it (pick a source; link a param); on an output end, connect this output to another input as well (pick a new user) |
| `d` | **remove**, the gentle way: a node is dissolved (its neighbours reconnected), a conduit disconnected, a param reset to its default (unlinked if it was linked) |
| `i` | **insert** a new node: into a conduit (`source → new → target`), or feeding a param |
| `m` | **move** a node into another conduit; while moving, `m` or `⏎` on a conduit or end drops it there (§6.5) |
| `H` / `L` | **slide** a node one place upstream / downstream along the flow (§6.5) |
| `f` | **focus**: bring a `▣` end's conduit into the flow (§4.4) |
| `K` / `J` | reorder a list entry earlier / later; shown as `(K)earlier` / `(J)later`, each only when it can apply |
| `+ - > < } {` | nudge a number by ±1 / ±10 / ±50 |

Global, whatever the focus: `a` add a node · `r` rename the node · `D` delete the node
(hard: connections removed, not reconnected) · `u` undo · `p` / `P` probe · `E` open in
`$EDITOR` · `/` find · `?` help · `q` quit.

`d` vs `D`: lowercase removes the focused thing gently; uppercase deletes the node outright.
Shifted movement keys move the focused thing instead of the focus: `J`/`K` a list entry,
`H`/`L` a node.

### 6.3 What applies to each focus

`⏎` does the action in **bold**.

| Focus | Action bar |
|---|---|
| Node | **`(e)dit`** `(m)ove` `(H)/(L)` slide `(d)issolve` |
| Conduit | **`(c)onnect`** `(i)nsert` `(d)isconnect` — `c` re-picks the source feeding the downstream end |
| Input end, `▣` | **`(f)ocus`** `(c)onnect` `(i)nsert` `(d)isconnect`, plus `(K)earlier` / `(J)later` on a list entry |
| Input end, in flow | **`(c)onnect`** `(i)nsert` `(d)isconnect`, plus `(K)earlier` / `(J)later` on a list entry |
| Output end, `▣` | **`(f)ocus`** `(c)onnect to` `(i)nsert` `(d)isconnect` |
| Output end, in flow | **`(c)onnect to`** `(i)nsert` `(d)isconnect` |
| Unconnected input (bar title `Input Actions`) | **`(c)onnect`** `(i)nsert` |
| Output row with no users (bar title `Output Actions`) | **`(c)onnect to`** |

A conduit and either of its ends offer the same `i`/`d`: an end stands for its conduit.
`c` depends on the side: on a conduit or an input end it re-picks the source; on an
output end it adds a user.

**[decided]** `(c)onnect to` on an output end opens a picker of every input that can take
this output's type without making a cycle: unconnected inputs, list inputs (the new
connection is appended), and connected single inputs, shown with their current source
(connecting replaces it, as `c` on that input would). The new conduit is drawn in the
flow: the downstream side is redrawn through it (§4.4), and focus moves to the new
output row. While a node is being moved, the bar is `Move Actions` (§6.5).

### 6.4 Action bar

A title rule, a blank row, the focus actions, then the global keys:

```
  Conduit End Actions ────────────────────────────────────────────────────────

  (f)ocus (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit
```

**[decided]** Global keys are always shown, and marked so they read differently from the
focus actions **[decided]**:

- their own line, after the focus actions, starting with a dimmed `global` label;
- **square** brackets, `[a]`, where focus actions use round ones, `(e)`;
- keys in the standard white, not green — green always means "this is about the focus".

If the line doesn't fit, it keeps the keys in the order above and ends with `[?]more`;
`?` lists everything.

**[decided]** Actions that can't apply right now are left out of the bar, focus actions
and global keys alike. For example: `(H)/(L)` on a node with nothing to slide past,
`(d)efault` on a value that already is the default, and `[r]ename`, `[D]elete`, `[p]robe`
in an empty project.

Titles: `Node Actions`, `Conduit Actions`, `Conduit End Actions`, `Input Actions`
(unconnected input), `Output Actions` (output with no users), `Project Actions` (empty
project), `Move Actions` (while moving, §6.5), and in the edit view
`Name Actions`, `Param Actions`, `Colour Actions`; popups have their own (§6.6). The status line (render result, probe,
`⇄ moving …`, the focus's full names, the error count) sits above the bar.

### 6.5 Moving nodes **[decided]**

The tree editors moved a node by picking it up and dropping it into another connection.
In the flow view that splits into two moves:

**Slide along the flow — `H` / `L` on a node.** The node swaps places with its flow
neighbour, upstream or downstream. This is the common case — reordering steps in a chain —
and needs no picking up:

```
blue-square → blue-move → tint → main-comp        L on blue-move
blue-square → tint → blue-move → main-comp
```

- Only the flow conduits are rewired: the source that fed `blue-move` now feeds `tint`,
  `tint` feeds `blue-move`, and `blue-move` feeds what `tint` fed.
- Everything else stays attached to the node it's on: linked params, `▣` inputs and other
  users of its output travel with it.
- Not allowed (with a message in the status line) at either end of the flow, or when a
  type doesn't fit — e.g. a `transform` can't slide past `main-comp` to sit just before
  `preview`, because `preview` needs an image and `transform` outputs a layer.
- Focus stays on the moved node; each slide is one `u`.
- **[decided]** A slide isn't a flow change: the view scrolls as little as possible
  (§4.5) and it isn't a step in `[` history.

**Pick up and drop — `m` on a node.** For moving a node somewhere else entirely: `m`
picks it up, its old place is reconnected as `d` (dissolve) would, and the flow is drawn
without it. Use any navigation — `h`/`l`, `f` through a `▣`, `/` — to reach a conduit or
conduit end, then `m` or `⏎` drops it there (`source → node → user`). `esc` cancels. The
whole move undoes in one `u`, and no file changes until the drop.

**[decided]** While it's picked up:

- Focus moves to the conduit that closed the gap, so dropping straight back puts the node
  where it was. If nothing closed it (the node had no users), focus goes to the upstream
  neighbour.
- The node isn't drawn. The status line names it before the usual focus text:
  `⇄ moving placed (transform)  ·  square.out → toned.layer`.
- The bar is `Move Actions`: `(m)drop here` on a conduit or end, `(esc)cancel`, and a
  global line of only the keys that still work, `[/]find [?]help [q]uit`. Other verbs do
  nothing until the drop or `esc`.
- Not a flow change: the closed-up flow keeps its path and scrolls minimally.

**[decided]** A drop redraws the flow through where the node landed, as `c` and `i` do
(§4.4), and the dropped node takes the focus. It's a flow change: the view re-centres and
it's a step in `[` history.

A slide is allowed even when the node's output has other users (`▣` rows). They stay
connected to the node, so after the slide they receive its output with the neighbour's
change included; one `u` puts it back.

### 6.6 Popups **[decided]**

Pickers (add type, connect, insert, find, font, file), prompts (ids, values, renames) and
the colour editor keep today's behaviour and look: a centred overlay with a thick border
(`┏━┓`), its title in the top border.

```
        ┏ connect comp.base to (1 of 1) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / ▏                                                          ┃
        ┃ blue_rect  rect                                              ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
```

- They are placed clear of the focused node where the screen allows, so you can still see
  what you're acting on.
- While one is open the action bar shows **its** keys and drops the global line, since
  global keys don't work in a popup:

| Popup | Bar title | Keys |
|---|---|---|
| Picker | `Picker Actions` | type to filter · `(↑/↓)` choose · `(⏎)` pick · `(esc)` cancel |
| Prompt | `Prompt Actions` | type to replace · `(→)` keep and edit · `(⏎)` accept · `(esc)` cancel |
| Colour editor | `Colour Editor Actions` | `(j/k)` channel · `(+ - > < } {)` nudge · `(tab)` RGB · `(#)` hex · `(⏎)` keep · `(esc)` revert |
| Confirmation **[decided]** | `Confirm Actions` | `(y)es` · any other key cancels |

**[decided]** Confirmations keep today's popup, titled `confirm`: the question, a blank
row, then a dimmed `y to confirm, any other key to cancel`. They're used for removing a
node (§6.7) and a palette colour that's in use (§7.1).

```
        ┏ confirm ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ Dissolve `toned`? Connects square → comp.layers[0].          ┃
        ┃                                                              ┃
        ┃ y to confirm, any other key to cancel                        ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
```

### 6.7 Removing nodes **[decided]**

`d` (dissolve) and `D` (delete) on a node ask first, as today, since each removes a file.
The question says what changes: `Dissolve `toned`? Connects square → comp.layers[0].`,
`Delete `placed`? This disconnects comp.layers[1].` Each is one `u`.

- **Dissolve** bridges the gap, so the flow keeps its path without the node. Focus moves
  to the conduit that closed the gap. Not a flow change: no re-centring, no history step.
  If nothing bridges it (a source, or users that can't take the type), it's handled as a
  delete.
- **Delete** leaves a gap, so the flow is rebuilt. The node downstream of the deleted one
  becomes the anchor and the focus, and the flow is rebuilt around it (§4.2); deleting a
  sink hands this to the node upstream. Deleting the last node leaves the empty project
  (§4.8). This is a flow change: re-centre and a history step.
- Deleting a node outside the flow (from the edit view, or found with `/` …) leaves the
  flow as it is, apart from the `▣` rows that went with it.

A node left with no users after a delete stays in the project; only `/` reaches it if no
flow does.

## 7. Edit view

`e` on a node opens it enlarged (`node-edit`), at full width with no cap on names.
**[decided]** The flow is hidden while it is open, and focus starts on the name row:

```
            ╭─ transform ───────────────────────────────────────╮
            │                                                   │
            │ name: helloWorldTransform                         │
            │───────────────────────────────────────────────────│
         ▣──┼─ layer ← helloWorldLayer                          │
         ▣──┼─ x     ← offset                                   │
         ▣──┼─ y     ← offset                                   │
            │───────────────────────────────────────────────────│
            │  rotate   = 0 (default)                           │
            │  scale    = 2.5                                   │
            │  scale_x  = 1 (default)                           │
            │  scale_y  = 1 (default)                           │
            │  skew_x   = 0 (default)                           │
            │  skew_y   = 0 (default)                           │
            │  anchor_x = 0.5 (default)                         │
            │  anchor_y = 0.5 (default)                         │
            │  filter   = unset (default)                       │
            │───────────────────────────────────────────────────│
            │                             out → helloWorldMove ─┼──▣
            ╰───────────────────────────────────────────────────╯
```

Port rows are labelled `port ← source` / `port → user` (§8); `x` and `y` are linked to an
`int` node `offset`.

- **Rows**: name, then input rows (as in §3.1), then every unlinked param, then output
  rows. Empty sections are omitted.
- **Name row**: `name: <id>`. The focused value is shown in the focus colour; there are no
  other markers (§5.2).
- **Params**: `name = value`, names padded so `=` lines up. A value at its default is
  followed by `(default)`, dimmed; an optional param with no value shows `unset (default)`.
  Colors keep their true-colour swatch.
- `j` / `k` move between rows; `esc` closes, back to the node in the flow.

Same verbs as §6.2:

| Row | Action bar (`⏎` = **bold**) |
|---|---|
| Name | **`(e)dit`** — rename; every reference is updated |
| Input / output row | as the matching conduit end in §6.3; `f` closes the edit view and redraws the flow through that conduit |
| Number param | **`(e)dit`** `(c)onnect` `(i)nsert` `(d)efault`, nudges `+ - > < } {` |
| Color param | **`(e)dit`** (colour editor) `(C)` picker window, `(c)onnect` `(i)nsert` `(d)efault` |
| Font / file param | **`(e)dit`** (picker) `(d)efault` |
| Other param | **`(e)dit`** `(d)efault` |

Linking a param with `c` or `i` moves it from the params section to the input rows;
`d` on that input row unlinks it and moves it back.

### 7.1 Palettes **[decided]**

A palette is the awkward case: its colours are both its settings (names and values you
edit) and its outputs (one output per colour, each with its own users). Today it has its
own keys — `r` / `o` / `x` on a colour (rename / add a copy / remove) and `I` (import).

**Flow view.** The box has one output row per colour per user, as for any node (§3.1),
with the colour's swatch before the name so a long palette stays scannable:

```
╭─ palette ──────╮
│ brand          │
│────────────────│
│   ██ accent────┼───        ← in the flow
│   ██ accent────┼─▣         ← a second user of `accent`
│   ██ dark──────┼─▣
│   ██ light─────┼─          ← unused
╰────────────────╯
```

**Edit view.** Add a **Colours** section between params and outputs: one row per colour
(`██ #e76f51  accent`), whether used or not. The palette keys move there and fold into the
shared verbs where they fit:

| Today | New | Why |
|---|---|---|
| `⏎` edit colour | **`e`** colour editor, `C` picker window | same as a color param |
| `r` rename colour | **`r`** | `r` already means rename; on a colour row it renames the colour (links follow) |
| `x` remove colour | **`d`** | "remove the focused thing"; asks first if used |
| `o` add a copy | **`i`** insert a copy below | "insert a new thing here" |
| — | **`K` / `J`** reorder | new; same as list entries |
| `I` import | **`I`** on any row of the palette's edit view; also opens straight after `a` adds a palette, as today | rare, whole-palette action, worth its own key |

One wrinkle: on a colour row `i` means "insert a colour", not "insert a node". That's still
"insert a new thing at the focus"; the bar makes it explicit (`(i)nsert colour`).

(Considered and set aside: making each palette colour its own node, which needs no special
keys but changes the project format.)

## 8. Port labels

**[decided]** Option **D**: port name only in the flow view, `port ← source` /
`port → user` in the edit view. The options considered are kept below for reference.

The sketch labels the `layer` input `helloWorldLayer` (its source node) and the output
`helloWorldMove` (its user), but labels `x` and `y` by port name. Four options, shown in
the edit view for `transform` with `x`/`y` linked to an `int` node `offset`. The flow view
uses the same choice, cut to the width cap.

**A. Port name only.** The other end appears in the status line when the row is focused.

```
         ▣──┼─ layer                                            │
         ▣──┼─ x                                                │
         ▣──┼─ y                                                │
            │                                              out ─┼──▣
```

Compact and stable — labels don't change when connections do. You can't see where a `▣`
goes without focusing it.

**B. Connected node only** (as in the sketch, applied to every row).

```
         ▣──┼─ helloWorldLayer                                  │
         ▣──┼─ offset                                           │
         ▣──┼─ offset                                           │
            │                                   helloWorldMove ─┼──▣
```

Shows where each conduit goes, but loses which port is which (two `offset` rows here);
unconnected rows have nothing to show; list entries need the index anyway.

**C. Both, port first.**

```
         ▣──┼─ layer ← helloWorldLayer                          │
         ▣──┼─ x     ← offset                                   │
         ▣──┼─ y     ← offset                                   │
            │                             out → helloWorldMove ─┼──▣
```

Complete, at the cost of width; in the flow view it hits the 24-column cap often.
A non-default source port reads `x ← offset.out`.

**D. Both in the edit view (C), port name only in the flow view (A).** The flow view stays
compact and its drawn lines already show where in-flow conduits go; the edit view, which
has room, shows everything.

## 9. Keys: current → new

| Today | In the flow view |
|---|---|
| `h` / `l` follow a connection | `h` / `l` along the flow; `f` on a `▣` end to follow a connection outside it |
| `H` / `L` into the trees, `space` folds | gone: the flow and `▣` ends replace the trees |
| `tab` cycles panes | gone |
| `⏎` edit a param | `e` on the node, then `⏎`/`e` on the param |
| `⏎` / `c` on an input (pick a source) | `c` on a conduit or input end |
| `c` / `d` link / unlink a param | `c` / `d` on the param (edit view) or its input row |
| `d` disconnect | `d` on a conduit or end |
| `i` insert into a connection or feed a param | `i` on a conduit, end, or param |
| `X` dissolve | `d` on a node |
| `m` move, drop with `m`/`⏎` | unchanged; plus `H` / `L` to slide a node along the flow |
| `K` / `J` reorder | unchanged, on a list-entry end |
| `r` / `o` / `x` on a palette color · `I` import | `r` / `i` / `d` on a colour row, `I` (edit view, §7.1) |
| panes stack below 100 columns | below 80×24, a "too small" message (§4.6) |
| `a` `r` `D` `E` `u` `p` `P` `/` `[` `]` `?` `q`, nudges, `C` | unchanged |

## 10. Decision log

No open questions remain. Answers given while drafting:

| Question | Answer |
|---|---|
| Whole graph or one flow? | One flow at a time (§4). |
| What `▣` means | Connected to a node outside the flow; `f` brings it in, redrawing only that side (§4.4). |
| Type names | Full names; boxes grow to fit, capped at 24 columns (§3.2). |
| Port labels | Port name in the flow view; `port ← source` / `port → user` in the edit view (§8). |
| Unconnected inputs | Dimmed `·` if optional, red `✕` if required (§4.3). |
| Branch memory | None; a rebuilt flow takes the first branch (§4.2). |
| Does `h`/`l` redraw the flow? | No, only `f` (§4.4). |
| Keys | One grammar across every focus (§6). |
| Palettes | Colour rows in the edit view, keys folded into `e` `r` `d` `i` `K` `J`, `I` kept (§7.1). |
| Global keys | Always in the action bar: own line, `global` label, square brackets, white keys (§6.4). |
| Colours | Black background, white standard, green focus, magenta related (§5). |
| Moving nodes | `H`/`L` slide along the flow, `m` pick up and drop; slides allowed when the output has other users (§6.5). |
| Small terminals | Below 80×24, a message to enlarge the window or edit the files by hand (§4.6). |
| `► ◄` around the focused value in the edit view | Dropped; the focus colour is enough (§5.2). |
| Errors | Option C, plus red `▣` for broken off-flow nodes and a project-wide error count (§5.4). |
| Screen layout | Title line, canvas, status line, action bar (§4.7). |
| Empty project | A centred hint and a `Project Actions` bar with only the usable globals (§4.8). |
| Adding a node | The new node becomes the anchor and focus; the previous flow is a `[` away (§4.2). |
| Bar on an unconnected input | `Input Actions` (§6.3). |
| Unavailable actions | Left out of the bar, globals included (§6.4). |
| Popups | Today's pickers, prompts and colour editor; the bar shows the popup's keys, no globals (§6.6). |
| Edit view | Opens on the name row; the flow is hidden behind it (§7). |
| Does `c` redraw the flow? | Yes, through the new conduit, as `f` would; so does `i` (§4.4). |
| What `▣` means | The conduit isn't drawn in the flow, even if both its nodes are (§4.3). |
| `c` on an output end | Connect this output to another input too: pick a new user (§6.2, §6.3). |
| Focus at start-up | On the anchor node (§4.2). |
| Off-screen boxes | No indicator; the conduit running off the edge is enough (§4.5). |
| `K` / `J` labels | `(K)earlier` / `(J)later`, each only when it can apply (§6.3). |
| History steps (`[` / `]`) | Flow changes only, restoring the focus where you left it (§6.1). |
| Scroll rule | Re-centre after a flow change; scroll minimally while walking; stop at the flow's ends (§4.5). |
| Several users of one output | Plain `out` rows; status line, magenta and `f` tell them apart (§3.1). |
| A key the bar leaves out because it's refused here | Says why in the status line; meaningless keys stay silent (§6). |
| Is a slide a flow change? | No: minimal scroll, no history step (§6.5). |
| Focus after a pick-up | The conduit that closed the gap (§6.5). |
| Showing the picked-up node | Not drawn; the status line names it (§5.3, §6.5). |
| Bar while moving | `Move Actions`: `(m)drop here (esc)cancel`, `[/]find [?]help [q]uit` (§6.5). |
| After a drop | Flow redrawn through the drop, focus on the node, a flow change (§6.5). |
| Confirm before `d` / `D` on a node | Yes, today's popup with a `Confirm Actions` bar (§6.6, §6.7). |
| After a dissolve | Same path, focus on the bridging conduit, not a flow change (§6.7). |
| After a delete | Downstream neighbour (upstream for a sink) becomes anchor and focus, flow rebuilt (§6.7). |
| `[` to a flow an edit has changed | Redraw the conduits that remain, rebuild the rest, focus the anchor if the focus is gone (§6.1). |
