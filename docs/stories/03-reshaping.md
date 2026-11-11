# Story 3 — reshaping a chain: slide, move, dissolve, delete

Spec: [`../specs/node-editor-ui.md`](../specs/node-editor-ui.md) (draft 8).

**As** someone tidying a project, **I want** to reorder the steps in a chain, move a
step to another chain, and take steps out, **so that** I can change how the image is
made without rewiring every connection by hand.

The project `chain` has 7 nodes: a background and two layers on a composite, shown in a
preview. The first layer is a chain of three steps.

| File | Type | Feeds |
|---|---|---|
| `bg` | canvas, 800×600 | `comp.base` |
| `square` | rectangle | `placed.layer` |
| `placed` | transform | `toned.layer` |
| `toned` | tint | `comp.layers[0]` |
| `stripe` | rectangle | `comp.layers[1]` |
| `comp` | composite | `preview.image` |
| `preview` | preview | — |

Port types matter here (from `crates/conduit-nodes`): `canvas` outputs an **image**;
`rectangle` outputs a **shape**; `transform` and `tint` take and output a **layer**;
`composite` takes an image as `base` and layers as `layers`, and outputs an image;
`preview` takes an image. An image or a shape can feed a layer input, but a layer can't
feed an image input.

Screens are 80×24. Under each: what's **green** (the focus) and **magenta** (related).
Everything else is white on black. The project has no errors, so nothing is red.

## 1. Find the chain

```sh
conduit edit chain
```

The flow starts as `bg → comp → preview` (the first sink, `comp`'s first input). **`/`**,
type **`ton`**:

```
 conduit edit chain  ·  7 nodes


      ╭─ canvas ─╮      ╭─ composite ─╮      ╭─ preview ─╮
      │ bg       │      │ comp        │      │ preview   │
      │──────────│      │─────────────│      │───────────│
    ·─┼─image    │  ╭───┼─base        │  ╭───┼─image     │
      │──────────│  │ ▣─┼─layers[0]   │  │   ╰───────────╯
      │      out─┼──╯ ▣─┼─layers[1]   │  │
      ╰──────────╯    ·─┼─layers      │  │
                        │─────────────│  │
                        │         out─┼──╯
        ┏ find node (1 of 7) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / ton▏                                                       ┃
        ┃ toned  tint                                                  ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

 rendered 7 nodes in 19 ms

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

**`⏎`**. `toned` is the anchor: upstream through `placed` to `square`, downstream through
`comp.layers[0]` to `preview`.

```
 conduit edit chain  ·  7 nodes


ngle ─╮      ╭─ transform ─╮      ╭─ tint ───╮      ╭─ composite ─╮      ╭─ prev
      │      │ placed      │      │ toned    │      │ comp        │      │ previ
──────│      │─────────────│      │──────────│      │─────────────│      │──────
  out─┼──────┼─layer       │  ╭───┼─layer    │    ▣─┼─base        │  ╭───┼─image
──────╯      │─────────────│  │   │──────────│  ╭───┼─layers[0]   │  │   ╰──────
             │         out─┼──╯   │      out─┼──╯ ▣─┼─layers[1]   │  │
             ╰─────────────╯      ╰──────────╯    ·─┼─layers      │  │
                                                    │─────────────│  │
                                                    │         out─┼──╯
                                                    ╰─────────────╯




 toned (tint)

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (H)slide (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: the `toned` box. Magenta: the conduits in and out of it.
- Re-centred on `toned`, so `square` is cut by the left edge and `preview` by the right.
- Only `(H)slide`. Sliding `toned` downstream past `comp` would leave `toned` feeding
  `preview`, and `preview` needs an image.

## 2. Slide the tint

**`H`**. `toned` swaps places with `placed` (§6.5): `square` now feeds `toned`, `toned`
feeds `placed`, and `placed` feeds `comp.layers[0]`. A shape into a layer input is
fine, so every type fits.

```
 conduit edit chain  ·  7 nodes


╭─ rectangle ─╮      ╭─ tint ───╮      ╭─ transform ─╮      ╭─ composite ─╮
│ square      │      │ toned    │      │ placed      │      │ comp        │
│─────────────│      │──────────│      │─────────────│      │─────────────│
│         out─┼──────┼─layer    │  ╭───┼─layer       │    ▣─┼─base        │  ╭──
╰─────────────╯      │──────────│  │   │─────────────│  ╭───┼─layers[0]   │  │
                     │      out─┼──╯   │         out─┼──╯ ▣─┼─layers[1]   │  │
                     ╰──────────╯      ╰─────────────╯    ·─┼─layers      │  │
                                                            │─────────────│  │
                                                            │         out─┼──╯
                                                            ╰─────────────╯




 slid toned before placed

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (L)slide (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Focus stays on `toned`, now second. Green: its box. Magenta: its conduits.
- A slide isn't a flow change (§6.5): the view scrolls as little as possible to keep a box
  to spare on the left, so `square` comes into view, and `[` history doesn't change.
- Now only `(L)slide`: `square` is a rectangle, with no input for `toned` to feed.
- The tint now applies before the transform. For a tint the image looks the same either
  way; for a `realize` or a filter, the order would matter.

**`L`** puts it back: `square → placed → toned → comp`, as on the previous screen.
**`L`** again, though the bar doesn't offer it:

```
 conduit edit chain  ·  7 nodes


╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ tint ───╮      ╭─ composite ─╮
│ square      │      │ placed      │      │ toned    │      │ comp        │
│─────────────│      │─────────────│      │──────────│      │─────────────│
│         out─┼──────┼─layer       │  ╭───┼─layer    │    ▣─┼─base        │  ╭──
╰─────────────╯      │─────────────│  │   │──────────│  ╭───┼─layers[0]   │  │
                     │         out─┼──╯   │      out─┼──╯ ▣─┼─layers[1]   │  │
                     ╰─────────────╯      ╰──────────╯    ·─┼─layers      │  │
                                                            │─────────────│  │
                                                            │         out─┼──╯
                                                            ╰─────────────╯




 can't slide toned after comp: preview.image takes an image, not a layer

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (H)slide (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Nothing changes. `L` is a real action refused here, so the status line says why, even
  though the bar leaves it out (§6). A key that means nothing for this focus would do
  nothing.

## 3. Move the transform to another layer

`placed` should move `stripe` instead of `square`. That's not a slide: `stripe` is in
another flow. **`h`** **`h`** to `placed`, then **`m`** picks it up.

```
 conduit edit chain  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ tint ───╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ square      │      │ toned    │      │ comp        │      │ preview   │
    │─────────────│      │──────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer    │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │──────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                         │      out─┼──╯ ▣─┼─layers[1]   │  │
                         ╰──────────╯    ·─┼─layers      │  │
                                           │─────────────│  │
                                           │         out─┼──╯
                                           ╰─────────────╯




 ⇄ moving placed (transform)  ·  square.out → toned.layer

  Move Actions ────────────────────────────────────────────────────────────────

  (m)drop here (esc)cancel
  global  [/]find [?]help [q]uit

```

- `placed`'s old place is closed up as `d` would (§6.5): `square` feeds `toned`
  directly, and the flow is drawn without `placed`.
- Focus moves to the conduit that closed the gap, `square → toned` (§6.5). Dropping
  `placed` straight back there puts it where it was.
- `placed` isn't drawn anywhere while it's carried: the status line names it,
  `⇄ moving placed (transform)`, followed by the focus as usual.
- Green: the conduit `square → toned`. Magenta: the port labels `out` and `layer` at its
  ends.
- The bar is `Move Actions`: only the move's keys and the globals that still work. `c`,
  `i`, `d`, `e` and the editing globals do nothing until the node is dropped or `esc`
  cancels.
- Nothing has been written yet: the files change when it's dropped.

**`l`** to `toned`, **`l`** onto its conduit, **`l`** to `comp`, then **`j`** **`j`**
**`j`** to `layers[1]`, the `▣` end fed by `stripe`.

```
 conduit edit chain  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ tint ───╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ square      │      │ toned    │      │ comp        │      │ preview   │
    │─────────────│      │──────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer    │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │──────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                         │      out─┼──╯ ▣─┼─layers[1]   │  │
                         ╰──────────╯    ·─┼─layers      │  │
                                           │─────────────│  │
                                           │         out─┼──╯
                                           ╰─────────────╯




 ⇄ moving placed (transform)  ·  comp.layers[1] ← stripe

  Move Actions ────────────────────────────────────────────────────────────────

  (m)drop here (esc)cancel
  global  [/]find [?]help [q]uit

```

- An end stands for its conduit (§6.3), so this `▣` is a place to drop, even though the
  conduit `stripe → comp.layers[1]` isn't drawn.
- Green: `▣─┼─` on `layers[1]`. Magenta: `layers[1]`.

**`m`** drops it: `stripe → placed → comp.layers[1]`.

```
 conduit edit chain  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ stripe      │      │ placed      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer       │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │─────────────│    ▣─┼─layers[0]   │  │   ╰───────────╯
                         │         out─┼──────┼─layers[1]   │  │
                         ╰─────────────╯    ·─┼─layers      │  │
                                              │─────────────│  │
                                              │         out─┼──╯
                                              ╰─────────────╯




 moved placed to stripe → comp.layers[1]

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Dropping redraws the flow through where the node landed, as `c` and `i` do (§4.4). It's
  a flow change: the view re-centres and it's a step in `[` history.
- Focus goes to the dropped node, so you can check it or go on to edit it (§6.5).
- `placed` is now in `comp.layers[1]`'s chain; `layers[0]` (`toned`) is `▣`.
- No slide: `stripe` is a rectangle with no input, and past `comp`, `placed` would feed
  `preview` a layer.
- One `u` would undo the whole move, pick-up and drop together.

## 4. Dissolve the tint

The tint isn't wanted after all. **`l`** **`l`** to `comp`, **`j`** **`j`** to
`layers[0]`, **`f`** to bring `toned`'s chain back, then **`h`** **`h`** to `toned`.
**`d`**:

```
 conduit edit chain  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ tint ───╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ square      │      │ toned    │      │ comp        │      │ preview   │
    │─────────────│      │──────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer    │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │──────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                         │      out─┼──╯ ▣─┼─layers[1]   │  │
                         ╰──────────╯    ·─┼─layers      │  │
                                           │─────────────│  │
                                           │         out─┼──╯
        ┏ confirm ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ Dissolve `toned`? Connects square → comp.layers[0].          ┃
        ┃                                                              ┃
        ┃ y to confirm, any other key to cancel                        ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 toned (tint)

  Confirm Actions ─────────────────────────────────────────────────────────────

  (y)es  any other key cancels


```

- `d` on a node asks first, as today, since it deletes a file; the question says what will
  be reconnected (§6.7). The confirmation is a popup like the others, with a
  `Confirm Actions` bar and no global line (§6.6).

**`y`**. `toned.toml` is removed and `square` now feeds `comp.layers[0]`.

```
 conduit edit chain  ·  6 nodes


    ╭─ rectangle ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ square      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │───────────│
    │         out─┼──╮ ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯  ╰───┼─layers[0]   │  │   ╰───────────╯
                       ▣─┼─layers[1]   │  │
                       ·─┼─layers      │  │
                         │─────────────│  │
                         │         out─┼──╯
                         ╰─────────────╯




 dissolved toned: square → comp.layers[0]

  Conduit Actions ─────────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- As with the pick-up, focus moves to the conduit that closed the gap,
  `square → comp.layers[0]`, and the flow keeps its path without the node (§6.7). Not a
  flow change: no re-centring, no history step.
- Green: the line from `square` to `comp`. Magenta: `out` and `layers[0]`.

## 5. Delete the transform

`stripe` shouldn't be moved either. **`l`** to `comp`, **`j`** **`j`** **`j`** to
`layers[1]`, **`f`**, then **`h`** **`h`** to `placed`. **`D`** deletes it outright: its
connections are removed, not bridged.

```
 conduit edit chain  ·  6 nodes


    ╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ stripe      │      │ placed      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer       │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │─────────────│    ▣─┼─layers[0]   │  │   ╰───────────╯
                         │         out─┼──────┼─layers[1]   │  │
                         ╰─────────────╯    ·─┼─layers      │  │
                                              │─────────────│  │
                                              │         out─┼──╯
        ┏ confirm ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ Delete `placed`? This disconnects comp.layers[1].            ┃
        ┃                                                              ┃
        ┃ y to confirm, any other key to cancel                        ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 placed (transform)

  Confirm Actions ─────────────────────────────────────────────────────────────

  (y)es  any other key cancels


```

**`y`**.

```
 conduit edit chain  ·  5 nodes


      ╭─ canvas ─╮      ╭─ composite ─╮      ╭─ preview ─╮
      │ bg       │      │ comp        │      │ preview   │
      │──────────│      │─────────────│      │───────────│
    ·─┼─image    │  ╭───┼─base        │  ╭───┼─image     │
      │──────────│  │ ▣─┼─layers[0]   │  │   ╰───────────╯
      │      out─┼──╯ ·─┼─layers      │  │
      ╰──────────╯      │─────────────│  │
                        │         out─┼──╯
                        ╰─────────────╯





 deleted placed

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- With no conduit to close the gap, the node downstream of the deleted one (`comp`)
  becomes the anchor and the focus, and the flow is rebuilt around it (§6.7, §4.2).
  `comp`'s first connected input is now `base`, so the flow is `bg → comp → preview`.
  Deleting a sink would hand this to the node upstream instead. This is a flow change:
  re-centre and a history step.
- `layers[1]` is gone, so `comp` has one layer left, `layers[0]` from `square`.
- `stripe` now feeds nothing, so no flow reaches it: only `/` finds it. It isn't an error,
  so the status line doesn't mention it.

## Result

| Step | Keys |
|---|---|
| Find `toned` | `/` "ton" `⏎` |
| Slide it upstream, back, and try past `comp` | `H` `L` `L` |
| Pick up `placed` | `h` `h` `m` |
| Drop it on `comp.layers[1]` | `l` `l` `l` `j` `j` `j` `m` |
| Dissolve `toned` | `l` `l` `j` `j` `f` `h` `h` `d` `y` |
| Delete `placed` | `l` `j` `j` `j` `f` `h` `h` `D` `y` |

Files at the end: `toned.toml` and `placed.toml` are gone; `comp.toml` has
`layers = ["square"]`; `stripe.toml` is unchanged and unused.

## Spec gaps this story turned up

Decided in the spec (draft 8):

| Gap | Decision |
|---|---|
| A key the bar leaves out because it's refused here (`L` past `comp`): silent (§6) or a message (§6.5)? | A message saying why; keys that mean nothing for the focus stay silent (§6) |
| Is a slide a flow change? | No: minimal scroll, no history step (§6.5) |
| Where focus goes on a pick-up (`m`) | The conduit that closed the gap (§6.5) |
| How the picked-up node is shown, now that it's out of the flow | Not drawn; the status line names it (§5.3, §6.5) |
| The bar while moving | `Move Actions`: `(m)drop here (esc)cancel`, plus `[/]find [?]help [q]uit` (§6.5) |
| The flow and focus after a drop | Redrawn through the drop; focus on the node; a flow change (§6.5) |
| Do `d` and `D` on a node ask first? | Yes, as today, in a confirmation popup (§6.6, §6.7) |
| Flow and focus after a dissolve | Same path without the node; focus on the conduit that closed the gap; not a flow change (§6.7) |
| Flow and focus after a delete | Downstream neighbour (upstream for a sink) becomes anchor and focus; flow rebuilt; a flow change (§6.7) |
| `[` back to a flow that an edit has since changed | Redraw the conduits that still exist, rebuild the rest as §4.2 does; focus on the anchor if the old focus is gone (§6.1) |
