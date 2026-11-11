# Story 4 — layers: insert, reorder, disconnect

Spec: [`../specs/node-editor-ui.md`](../specs/node-editor-ui.md) (draft 9).

**As** someone building up a picture, **I want** to put several layers on a composite,
change which is drawn on top, and take layers off again, **so that** I can arrange the
picture without editing the composite's file by hand.

This carries on from story 1: the project `story` has `blue_bg → comp.base`,
`comp → preview`, and nothing in `comp.layers`. `composite` draws its layers in list
order, so later layers are drawn on top.

Screens are 80×24. Under each: what's **green** (the focus) and **magenta** (related).
Everything else is white on black. The project has no errors, so nothing is red.

## 1. The first layer

```sh
conduit edit story
```

The flow is `blue_bg → comp → preview`, focus on `preview`. **`h`** **`h`** to `comp`,
**`j`** **`j`** to `layers`.

```
 conduit edit story  ·  3 nodes


      ╭─ canvas ─╮      ╭─ composite ─╮      ╭─ preview ─╮
      │ blue_bg  │      │ comp        │      │ preview   │
      │──────────│      │─────────────│      │───────────│
    ·─┼─image    │  ╭───┼─base        │  ╭───┼─image     │
      │──────────│  │ ·─┼─layers      │  │   ╰───────────╯
      │      out─┼──╯   │─────────────│  │
      ╰──────────╯      │         out─┼──╯
                        ╰─────────────╯






 comp.layers: no layers (optional)

  Input Actions ───────────────────────────────────────────────────────────────

  (c)onnect (i)nsert
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `·─┼─` on `layers`, the dimmed marker for an unconnected optional input (§4.3).
  Magenta: `layers`.

**`i`** inserts a new node feeding this input. Type **`rect`**: the picker only offers
kinds whose output can be a layer, and `rect` matches `rectangle` and, less closely,
`intersect`. A rectangle is a vector shape, and a shape can be a layer.

```
 conduit edit story  ·  3 nodes


      ╭─ canvas ─╮      ╭─ composite ─╮      ╭─ preview ─╮
      │ blue_bg  │      │ comp        │      │ preview   │
      │──────────│      │─────────────│      │───────────│
    ·─┼─image    │  ╭───┼─base        │  ╭───┼─image     │
      │──────────│  │ ·─┼─layers      │  │   ╰───────────╯
      │      out─┼──╯   │─────────────│  │
      ╰──────────╯      │         out─┼──╯
                        ╰─────────────╯


        ┏ insert before comp.layers: type (2 of 19) ━━━━━━━━━━━━━━━━━━━┓
        ┃ / rect▏                                                      ┃
        ┃ rectangle  () → (shape)                                      ┃
        ┃ intersect  (shape) → (shape)                                 ┃
 comp.layers: no layers (optional)━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

**`⏎`**, type the id **`card`**, **`⏎`**.

```
 conduit edit story  ·  4 nodes


    ╭─ rectangle ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ card        │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │───────────│
    │         out─┼──╮ ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯  ╰───┼─layers[0]   │  │   ╰───────────╯
                       ·─┼─layers      │  │
                         │─────────────│  │
                         │         out─┼──╯
                         ╰─────────────╯





 inserted card (rectangle) before comp.layers[0]

  Conduit End Actions ─────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- `card` is `layers[0]`, and the flow is redrawn through it (§4.4): `card → comp →
  preview`. `base` is now `▣`.
- A list keeps an empty **slot** row after its entries, `·─┼─layers`; `c` or `i` on it
  appends a new entry (§3.1). The slot is now below `layers[0]`.
- Focus stays on the end, which is now the new entry, `layers[0]` (§4.4). Green: `─┼─`
  on `layers[0]`. Magenta: `layers[0]`, and `out` on `card`.
- No `(K)earlier` / `(J)later`: `layers[0]` is the only entry; the slot doesn't count.

## 2. Two more

**`j`** to the slot, **`i`**, `rect`, **`badge`**. Again for **`shadow`**: **`j`**
**`i`**, `rect`, **`shadow`**.

```
 conduit edit story  ·  6 nodes


    ╭─ rectangle ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │───────────│
    │         out─┼──╮ ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯  │ ▣─┼─layers[0]   │  │   ╰───────────╯
                     │ ▣─┼─layers[1]   │  │
                     ╰───┼─layers[2]   │  │
                       ·─┼─layers      │  │
                         │─────────────│  │
                         │         out─┼──╯
                         ╰─────────────╯



 inserted shadow (rectangle) before comp.layers[2]

  Conduit End Actions ─────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect (K)earlier
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- `comp.layers` is `["card", "badge", "shadow"]`. Each insert redrew the flow through the
  new layer, so the flow now runs through `layers[2]`.
- Because focus stays on the end, the slot is always one `j` away: adding several layers
  is `j` `i`, `j` `i`, ….
- `(K)earlier` but no `(J)later`: `layers[2]` is the last entry.

## 3. Put the shadow underneath

The shadow should be drawn first, under the others. **`K`** **`K`**.

```
 conduit edit story  ·  6 nodes


    ╭─ rectangle ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │───────────│
    │         out─┼──╮ ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯  ╰───┼─layers[0]   │  │   ╰───────────╯
                       ▣─┼─layers[1]   │  │
                       ▣─┼─layers[2]   │  │
                       ·─┼─layers      │  │
                         │─────────────│  │
                         │         out─┼──╯
                         ╰─────────────╯



 moved comp.layers ← shadow to [0]

  Conduit End Actions ─────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect (J)later
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- `comp.layers` is now `["shadow", "card", "badge"]`; the other entries are renumbered.
- Focus follows the entry, as today: it's on `layers[0]`, still `shadow` (§6.2).
- A reorder isn't a flow change. The flow still runs through `shadow`; its line just
  moves up with the row. Minimal scroll, no history step. Each `K` is one `u`.
- `(J)later` but no `(K)earlier`.

## 4. Offset the shadow

**`i`** on `layers[0]` inserts a node into the conduit `shadow → comp.layers[0]`. Type
**`trans`**:

```
 conduit edit story  ·  6 nodes


    ╭─ rectangle ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │───────────│
    │         out─┼──╮ ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯  ╰───┼─layers[0]   │  │   ╰───────────╯
                       ▣─┼─layers[1]   │  │
                       ▣─┼─layers[2]   │  │
                       ·─┼─layers      │  │
                         │─────────────│  │
                         │         out─┼──╯
        ┏ insert before comp.layers[0]: type (1 of 14) ━━━━━━━━━━━━━━━━┓
        ┃ / trans▏                                                     ┃
        ┃ transform  (layer) → (layer)                                 ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 comp.layers[0] ← shadow

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

**`⏎`**:

```
 conduit edit story  ·  6 nodes


    ╭─ rectangle ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │───────────│
    │         out─┼──╮ ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯  ╰───┼─layers[0]   │  │   ╰───────────╯
                       ▣─┼─layers[1]   │  │
                       ▣─┼─layers[2]   │  │
                       ·─┼─layers      │  │
                         │─────────────│  │
                         │         out─┼──╯
                         ╰─────────────╯
        ┏ id for new transform ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ shadow_at▏                                                   ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 comp.layers[0] ← shadow

  Prompt Actions ──────────────────────────────────────────────────────────────

  type to replace  (→)keep and edit  (⏎)accept  (esc)cancel


```

**`⏎`**.

```
 conduit edit story  ·  7 nodes


  ╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ composite ─╮      ╭─ preview ─╮
  │ shadow      │      │ shadow_at   │      │ comp        │      │ preview   │
  │─────────────│      │─────────────│      │─────────────│      │───────────│
  │         out─┼──────┼─layer       │    ▣─┼─base        │  ╭───┼─image     │
  ╰─────────────╯      │─────────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                       │         out─┼──╯ ▣─┼─layers[1]   │  │
                       ╰─────────────╯    ▣─┼─layers[2]   │  │
                                          ·─┼─layers      │  │
                                            │─────────────│  │
                                            │         out─┼──╯
                                            ╰─────────────╯



 inserted shadow_at (transform) before comp.layers[0]

  Conduit End Actions ─────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect (J)later
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- `shadow → shadow_at → comp.layers[0]`. Focus stays on `comp.layers[0]`, which is now
  fed by `shadow_at`; **`h`** **`h`** reaches `shadow_at` to `e`dit its `x` and `y`.

## 5. Take off a layer outside the flow

`card` isn't wanted. **`j`** to `layers[1]`.

```
 conduit edit story  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ shadow_at   │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer       │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │─────────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                         │         out─┼──╯ ▣─┼─layers[1]   │  │
                         ╰─────────────╯    ▣─┼─layers[2]   │  │
                                            ·─┼─layers      │  │
                                              │─────────────│  │
                                              │         out─┼──╯
                                              ╰─────────────╯



 comp.layers[1] ← card

  Conduit End Actions ─────────────────────────────────────────────────────────

  (f)ocus (c)onnect (i)nsert (d)isconnect (K)earlier (J)later
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

**`d`** disconnects it. Disconnecting doesn't delete a node, so there's no confirmation.

```
 conduit edit story  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ shadow_at   │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer       │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │─────────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                         │         out─┼──╯ ▣─┼─layers[1]   │  │
                         ╰─────────────╯    ·─┼─layers      │  │
                                              │─────────────│  │
                                              │         out─┼──╯
                                              ╰─────────────╯




 disconnected comp.layers[1] ← card

  Conduit End Actions ─────────────────────────────────────────────────────────

  (f)ocus (c)onnect (i)nsert (d)isconnect (K)earlier
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- `comp.layers` is `["shadow_at", "badge"]`; `badge` moved up to `layers[1]`.
- When `d` removes the row in focus, focus goes to the row that takes its place, here
  `badge`'s, now `layers[1]` (§6.7). If no row takes its place, it goes to the row above;
  if there is none, to the node.
- `card` is still in the project, unused; only `/` reaches it.

## 6. Take off the layer in the flow

**`k`** to `layers[0]`, **`h`** onto its conduit.

```
 conduit edit story  ·  7 nodes


    ╭─ rectangle ─╮      ╭─ transform ─╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ shadow      │      │ shadow_at   │      │ comp        │      │ preview   │
    │─────────────│      │─────────────│      │─────────────│      │───────────│
    │         out─┼──────┼─layer       │    ▣─┼─base        │  ╭───┼─image     │
    ╰─────────────╯      │─────────────│  ╭───┼─layers[0]   │  │   ╰───────────╯
                         │         out─┼──╯ ▣─┼─layers[1]   │  │
                         ╰─────────────╯    ·─┼─layers      │  │
                                              │─────────────│  │
                                              │         out─┼──╯
                                              ╰─────────────╯




 shadow_at.out → comp.layers[0]

  Conduit Actions ─────────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

**`d`**.

```
 conduit edit story  ·  7 nodes


      ╭─ canvas ─╮      ╭─ composite ─╮      ╭─ preview ─╮
      │ blue_bg  │      │ comp        │      │ preview   │
      │──────────│      │─────────────│      │───────────│
    ·─┼─image    │  ╭───┼─base        │  ╭───┼─image     │
      │──────────│  │ ▣─┼─layers[0]   │  │   ╰───────────╯
      │      out─┼──╯ ·─┼─layers      │  │
      ╰──────────╯      │─────────────│  │
                        │         out─┼──╯
                        ╰─────────────╯





 disconnected comp.layers[0] ← shadow_at

  Conduit End Actions ─────────────────────────────────────────────────────────

  (f)ocus (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- The flow's conduit into `comp` is gone, so the flow can't stay as it was.
- As for a delete (§6.7), the node downstream of the removed conduit, `comp`, keeps its
  place and the upstream side is rebuilt from its first connected input, which is `base`
  (§4.2). It's a flow change: re-centre and a history step.
- Focus goes to the row that took the removed one's place on `comp`: `layers[0]`, now
  `badge`. On a conduit, the removed row is its input end.
- Green: `▣─┼─` on `layers[0]`. Magenta: `layers[0]`.

## Result

| Step | Keys |
|---|---|
| To the `layers` slot, insert `card` | `h` `h` `j` `j` `i` "rect" `⏎` "card" `⏎` |
| Insert `badge` and `shadow` | `j` `i` … ; `j` `i` … |
| Shadow first | `K` `K` |
| Insert `shadow_at` | `i` "trans" `⏎` "shadow_at" `⏎` |
| Disconnect `card` | `j` `d` |
| Disconnect `shadow_at` | `k` `h` `d` |

Files at the end: `comp.toml` has `layers = ["badge"]`; `card`, `shadow` and
`shadow_at` are in the project but feed nothing that reaches `preview`.

## Spec gaps this story turned up

Decided in the spec (draft 9):

| Gap | Decision |
|---|---|
| No row to add a second list entry from | An empty slot row `·─┼─layers` after a list's entries; `c` / `i` on it append (§3.1, §6.3) |
| Focus after `i`: the end or the new node? | The end, as for `c`; inserting from the slot leaves it on the new entry (§4.4) |
| `(m)ove` on a node that can't be moved (nothing feeds it, or no output) | Not offered; `m` says why (§6.5) |
| Focus after `K` / `J` | Follows the entry (§6.2) |
| Is a reorder a flow change? | No: the line moves with its row; minimal scroll, no history step (§6.2) |
| Focus after `d` removes the row in focus | The row that takes its place; else the row above; else the node (§6.7) |
| `d` on the flow's conduit (or either end) | The downstream node stays; the upstream side is rebuilt from its first connected input; a flow change (§6.7) |
