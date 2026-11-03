# Story 1 — a blue rectangle in a preview window

Spec: [`../specs/node-editor-ui.md`](../specs/node-editor-ui.md) (draft 6).

**As** someone starting a new project, **I want** to add a large blue rectangle, put it on
a composite's background and show the composite in a preview window, **so that** I have a
first image to build on, without leaving the editor.

Every screen below is 80×24, the minimum size (§4.6). Colours can't be shown in a code
block, so each screen is followed by what's **green** (the focus), **magenta** (related)
and **red** (errors). Everything else is white on black.

## Start

```sh
mkdir story
conduit edit story
```

```
 conduit edit story  ·  0 nodes






                        This project has no nodes yet.
                        Press a to add one.










  Project Actions ─────────────────────────────────────────────────────────────


  global  [a]dd [u]ndo [/]find [?]help [q]uit

```

The project has no nodes, so there is nothing to focus. The bar has no focus actions,
only the global keys that make sense without a node (`[a]dd`, `[u]ndo`, `[/]find`,
`[?]help`, `[q]uit`).

## 1. Add the rectangle

**`a`**, then type **`rect`**. The type picker narrows to one match.

```
 conduit edit story  ·  0 nodes



        ┏ add node of type (1 of 14) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / rect▏                                                      ┃
        ┃ rect  () → (image)                                           ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
                        Press a to add one.










  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

**`⏎`** picks `rect`. The id prompt opens on the suggested `rect_1`, highlighted, so typing
replaces it. Type **`blue_rect`**.

```
 conduit edit story  ·  0 nodes




        ┏ id for new rect ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ blue_rect▏                                                   ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
                        Press a to add one.










  Prompt Actions ──────────────────────────────────────────────────────────────

  type to replace  (→)keep and edit  (⏎)accept  (esc)cancel


```

**`⏎`**. `blue_rect.toml` is written and the new node becomes the anchor and focus of the
flow (§4.2). It's the only node in the flow, so its `out` row has no marker: nothing uses
it yet (§4.3).

```
 conduit edit story  ·  1 node


    ╭─ rect ────╮
    │ blue_rect │
    │───────────│
    │       out─┼─
    ╰───────────╯









 added blue_rect (rect)

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: the whole `blue_rect` box, including the `out─┼─` stub.
- The bar leaves out `(H)/(L)` slide: there's nothing to slide past.

## 2. Make it large and blue

**`e`** opens the edit view. Focus starts on the name row.

```
 conduit edit story  ·  1 node

              ╭─ rect ────────────────────────────────────────╮
              │                                               │
              │ name: blue_rect                               │
              │───────────────────────────────────────────────│
              │  color  = ██ white (default)                  │
              │  width  = 100 (default)                       │
              │  height = 100 (default)                       │
              │───────────────────────────────────────────────│
              │                                          out ─┼─
              ╰───────────────────────────────────────────────╯





 blue_rect (rect)

  Name Actions ────────────────────────────────────────────────────────────────

  (e)dit
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `blue_rect`. The rest of the row (`name:`) is magenta.

**`j`** moves to `color`.

```
 conduit edit story  ·  1 node

              ╭─ rect ────────────────────────────────────────╮
              │                                               │
              │ name: blue_rect                               │
              │───────────────────────────────────────────────│
              │  color  = ██ white (default)                  │
              │  width  = 100 (default)                       │
              │  height = 100 (default)                       │
              │───────────────────────────────────────────────│
              │                                          out ─┼─
              ╰───────────────────────────────────────────────╯





 blue_rect.color = white

  Param Actions ───────────────────────────────────────────────────────────────

  (e)dit (C)picker (c)onnect (i)nsert
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `██ white` (the swatch is drawn in the colour itself). Magenta: `color`.
  `(default)` stays dimmed.
- `(d)efault` isn't listed because the value already is the default.

**`e`** opens the colour editor. **`#`**, type **`blue`**, **`⏎`**.

```
 conduit edit story  ·  1 node

              ╭─ rect ────────────────────────────────────────╮
              │                                               │
            ┏ color  blue_rect.color  (HSL) ━━━━━━━━━━━━━━━━━━━━━━━┓
            ┃ ████████████████  #0000ff                            ┃
            ┃ ████████████████  was #ffffff ██                     ┃
            ┃ # blue▏                                              ┃
            ┃ ▸ H 240°  ██████████████████████████┃████████        ┃
            ┃   S 100%  ██████████████████████████████████┃        ┃
            ┃   L  50%  █████████████████┃█████████████████        ┃
            ┃   A 100%  ██████████████████████████████████┃        ┃
            ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛




 blue_rect.color = blue

  Colour Editor Actions ───────────────────────────────────────────────────────

  (j/k)channel (+ - > < } {)nudge (tab)RGB (#)hex (⏎)keep (esc)revert


```

- The new colour is written to `blue_rect.toml` straight away, so a preview would follow.
- **`⏎`** keeps it and closes the editor.

**`j`** to `width` (`100` turns green), **`e`**. The prompt opens on `100`, highlighted;
type **`800`**.

```
 conduit edit story  ·  1 node

              ╭─ rect ────────────────────────────────────────╮
              │                                               │
              │ name: blue_rect                               │
              │───────────────────────────────────────────────│
              │  color  = ██ blue                             │
              │  width  = 100 (default)                       │
              │  height = 100 (default)                       │
              │───────────────────────────────────────────────│
              │                                          out ─┼─
              ╰───────────────────────────────────────────────╯

        ┏ blue_rect.width (empty = default) ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ 800▏                                                         ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

 blue_rect.width = 100

  Prompt Actions ──────────────────────────────────────────────────────────────

  type to replace  (→)keep and edit  (⏎)accept  (esc)cancel


```

**`⏎`**, then **`j`** to `height`, **`e`**, **`600`**, **`⏎`**.

```
 conduit edit story  ·  1 node

              ╭─ rect ────────────────────────────────────────╮
              │                                               │
              │ name: blue_rect                               │
              │───────────────────────────────────────────────│
              │  color  = ██ blue                             │
              │  width  = 800                                 │
              │  height = 600                                 │
              │───────────────────────────────────────────────│
              │                                          out ─┼─
              ╰───────────────────────────────────────────────╯





 set blue_rect.height = 600

  Param Actions ───────────────────────────────────────────────────────────────

  (e)dit (c)onnect (i)nsert (d)efault
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `600`. Magenta: `height`.
- Now that `height` isn't the default, `(d)efault` appears in the bar.

**`esc`** closes the edit view.

```
 conduit edit story  ·  1 node


    ╭─ rect ────╮
    │ blue_rect │
    │───────────│
    │       out─┼─
    ╰───────────╯









 blue_rect (rect)  ·  800×600

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

## 3. Add the composite

**`a`**, type **`comp`**.

```
 conduit edit story  ·  1 node


    ╭─ rect ────╮
    │ blue_rect │
    │───────────│
    │       out─┼─
    ╰───────────╯




        ┏ add node of type (1 of 14) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / comp▏                                                      ┃
        ┃ composite  (image, layer) → (image)                          ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

 blue_rect (rect)

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

**`⏎`**, type **`comp`** over the suggested `composite_1`.

```
 conduit edit story  ·  1 node


    ╭─ rect ────╮
    │ blue_rect │
    │───────────│
    │       out─┼─
    ╰───────────╯


        ┏ id for new composite ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ comp▏                                                        ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛




 blue_rect (rect)

  Prompt Actions ──────────────────────────────────────────────────────────────

  type to replace  (→)keep and edit  (⏎)accept  (esc)cancel


```

**`⏎`**. `comp` becomes the anchor. It isn't connected to anything, so the flow is just
`comp`; `blue_rect` is still in the project but not in this flow.

```
 conduit edit story  ·  2 nodes


      ╭─ composite ─╮
      │ comp        │
      │─────────────│
    ✕─┼─base        │
    ·─┼─layers      │
      │─────────────│
      │         out─┼─
      ╰─────────────╯






 added comp (composite)  ·  1 error

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: the whole `comp` box. Its border would be red (it's broken: `base` isn't
  connected), but focus wins (§5.4).
- Red: `✕` on `base`, a required input with no connection; the status line counts the
  error.
- `·` on `layers` (dimmed): optional and empty, so not an error.

## 4. Connect the rectangle to the composite's background

**`j`** moves into the box, onto its first row, `base`.

```
 conduit edit story  ·  2 nodes


      ╭─ composite ─╮
      │ comp        │
      │─────────────│
    ✕─┼─base        │
    ·─┼─layers      │
      │─────────────│
      │         out─┼─
      ╰─────────────╯






 ✕ comp.base is not connected  ·  1 error

  Input Actions ───────────────────────────────────────────────────────────────

  (c)onnect (i)nsert
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `✕─┼─` on `base`. Magenta: the label `base`.
- The status line says why it's red.

**`c`** opens the source picker. It only offers type-compatible outputs that don't make a
cycle: here, just `blue_rect`.

```
 conduit edit story  ·  2 nodes


      ╭─ composite ─╮
      │ comp        │
      │─────────────│
    ✕─┼─base        │
    ·─┼─layers      │
      │─────────────│
      │         out─┼─
      ╰─────────────╯

        ┏ connect comp.base to (1 of 1) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / ▏                                                          ┃
        ┃ blue_rect  rect                                              ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

 ✕ comp.base is not connected  ·  1 error

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

**`⏎`**. `comp.toml` gets `base = "blue_rect"`. Focus stays on the same end. The flow is
rebuilt around the anchor `comp`, and its first connected input is now `base`, so
`blue_rect` is drawn to its left (§4.2).

```
 conduit edit story  ·  2 nodes


    ╭─ rect ────╮      ╭─ composite ─╮
    │ blue_rect │      │ comp        │
    │───────────│      │─────────────│
    │       out─┼──────┼─base        │
    ╰───────────╯    ·─┼─layers      │
                       │─────────────│
                       │         out─┼─
                       ╰─────────────╯






 connected comp.base ← blue_rect

  Conduit End Actions ─────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `─┼─` on `comp.base`. Magenta: `base`, and `out` on `blue_rect` (the other end,
  now in the flow).
- The error count is gone and `comp`'s border is no longer red.

## 5. Add the preview

**`a`**, type **`prev`**, **`⏎`**, then **`⏎`** to accept the suggestion `preview`.

```
 conduit edit story  ·  2 nodes


    ╭─ rect ────╮      ╭─ composite ─╮
    │ blue_rect │      │ comp        │
    │───────────│      │─────────────│
    │       out─┼──────┼─base        │
    ╰───────────╯    ·─┼─layers      │
                       │─────────────│
                       │         out─┼─
                       ╰─────────────╯

        ┏ add node of type (1 of 14) ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / prev▏                                                      ┃
        ┃ preview  (image) → ()                                        ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

 connected comp.base ← blue_rect

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

```
 conduit edit story  ·  2 nodes


    ╭─ rect ────╮      ╭─ composite ─╮
    │ blue_rect │      │ comp        │
    │───────────│      │─────────────│
    │       out─┼──────┼─base        │
    ╰───────────╯    ·─┼─layers      │
                       │─────────────│
                       │         out─┼─
                       ╰─────────────╯


        ┏ id for new preview ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ preview▏                                                     ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛

 connected comp.base ← blue_rect

  Prompt Actions ──────────────────────────────────────────────────────────────

  type to replace  (→)keep and edit  (⏎)accept  (esc)cancel


```

The flow is rebuilt around the new node, `preview`, which is unconnected, so it stands
alone.

```
 conduit edit story  ·  3 nodes


      ╭─ preview ─╮
      │ preview   │
      │───────────│
    ✕─┼─image     │
      ╰───────────╯









 added preview (preview)  ·  1 error

  Node Actions ────────────────────────────────────────────────────────────────

  (e)dit (m)ove (d)issolve
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

## 6. Connect the composite to the preview

**`j`** to `image`.

```
 conduit edit story  ·  3 nodes


      ╭─ preview ─╮
      │ preview   │
      │───────────│
    ✕─┼─image     │
      ╰───────────╯









 ✕ preview.image is not connected  ·  1 error

  Input Actions ───────────────────────────────────────────────────────────────

  (c)onnect (i)nsert
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

**`c`**. Both `blue_rect` and `comp` output images, so both are offered. **`↓`** to `comp`,
**`⏎`**.

```
 conduit edit story  ·  3 nodes


      ╭─ preview ─╮
      │ preview   │
      │───────────│
    ✕─┼─image     │
      ╰───────────╯




        ┏ connect preview.image to (2 of 2) ━━━━━━━━━━━━━━━━━━━━━━━━━━━┓
        ┃ / ▏                                                          ┃
        ┃ blue_rect  rect                                              ┃
        ┃ comp  composite                                              ┃
        ┗━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━┛
 ✕ preview.image is not connected  ·  1 error

  Picker Actions ──────────────────────────────────────────────────────────────

  type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel


```

`preview.toml` gets `image = "comp"`. The flow is rebuilt around `preview`: upstream
through `image` to `comp`, then through `comp`'s first connected input, `base`, to
`blue_rect`.

```
 conduit edit story  ·  3 nodes


    ╭─ rect ────╮      ╭─ composite ─╮      ╭─ preview ─╮
    │ blue_rect │      │ comp        │      │ preview   │
    │───────────│      │─────────────│      │───────────│
    │       out─┼──────┼─base        │  ╭───┼─image     │
    ╰───────────╯    ·─┼─layers      │  │   ╰───────────╯
                       │─────────────│  │
                       │         out─┼──╯
                       ╰─────────────╯






 connected preview.image ← comp

  Conduit End Actions ─────────────────────────────────────────────────────────

  (c)onnect (i)nsert (d)isconnect
  global  [a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit

```

- Green: `─┼─` on `preview.image`. Magenta: `image`, and `out` on `comp`.
- A preview window opens showing an 800×600 blue image, because `conduit edit` runs
  everything `conduit run` does.

## Result

Keys pressed, in order (text typed into pickers and prompts shown in quotes):

| Step | Keys |
|---|---|
| Add the rectangle | `a` "rect" `⏎` "blue_rect" `⏎` |
| Make it large and blue | `e` `j` `e` `#` "blue" `⏎` `⏎` `j` `e` "800" `⏎` `j` `e` "600" `⏎` `esc` |
| Add the composite | `a` "comp" `⏎` "comp" `⏎` |
| Connect it | `j` `c` `⏎` |
| Add the preview | `a` "prev" `⏎` `⏎` |
| Connect it | `j` `c` `↓` `⏎` |

The project folder now holds:

```toml
# blue_rect.toml
type = "rect"

[params]
color = "#0000ff"
width = 800
height = 600
```

```toml
# comp.toml
type = "composite"

[inputs]
base = "blue_rect"
```

```toml
# preview.toml
type = "preview"

[inputs]
image = "comp"
```

## Spec gaps this story turned up

Walking the story needed answers draft 5 of the spec didn't give. They are now in the
spec (draft 6):

| Gap | Where it's decided |
|---|---|
| Empty project screen and bar | §4.8 |
| A new node becomes the anchor and focus, dropping the old flow from view | §4.2 |
| Bar title on an unconnected input row (`Input Actions`) | §6.3 |
| Edit view opens on the name row, flow hidden | §7 |
| Pickers, prompts, colour editor, and the bar while they're open | §6.6 |
| Unavailable actions left out of the bar, globals included | §6.4 |
| Title line, and the screen's rows | §4.7 |
| `c` redraws the flow through the conduit it makes | §4.4 |

In this story each `c` connects a node's first input, so the flow would have been rebuilt
through it anyway.
