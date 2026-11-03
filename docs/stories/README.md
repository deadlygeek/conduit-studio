# User stories

Walk-throughs of the node editor spec ([`../specs/node-editor-ui.md`](../specs/node-editor-ui.md)),
screen by screen: the keys pressed, then the screen that results.

| Story | |
|---|---|
| [01 — a blue rectangle in a preview window](01-first-image.md) | empty project → rect → composite → preview |
| [02 — exploring an existing project](02-exploring.md) | walking flows, `f` on `▣` up- and downstream, `[`, `/` |
| [03 — reshaping a chain](03-reshaping.md) | `H` / `L` slide, `m` move, `d` dissolve, `D` delete |

Stories still to write, and their order, are tracked in [`checklist.md`](checklist.md).

## Editing a story

The `NN-name.md` files are generated; don't edit them by hand. Each story has two sources
in `src/`:

- `NN-name.md.in` — the text. `{{name}}` marks where a screen goes.
- `NN-name.py` — the screens, as a `SCREENS` dict of name → `Screen`, drawn with
  `tools/tui.py` (node boxes, flows and conduits, the edit view, popups, the status line
  and action bar).

Rebuild after changing either, or the spec's look:

```sh
python3 docs/stories/tools/build.py           # write every story
python3 docs/stories/tools/build.py --check   # fail if a story is out of date
```

The build stops if a template names a screen that doesn't exist, or a screen goes unused.
Python 3.9+, standard library only.

A new story: copy `src/01-first-image.*` to `src/NN-name.*`, edit, build, add it to the
table above, and tick it in [`checklist.md`](checklist.md).
