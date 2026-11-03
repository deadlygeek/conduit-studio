"""Draws mock `conduit edit` screens for the user stories, as plain text.

Screens follow docs/specs/node-editor-ui.md: 80×24, title line on row 1, the flow
canvas, the status line on row 18 and the action bar on rows 20–23. Colour can't be
shown in a code block, so the stories describe it under each screen.

A story's screen file (docs/stories/src/NN-name.py) builds a `SCREENS` dict of
name → `Screen`; build.py puts each one into the story's template where `{{name}}`
appears.
"""

W, H = 80, 24

STATUS_ROW = 17
BAR_ROW = 19

# Global keys for the action bar (§6.4): with nodes, and in an empty project (§4.8).
GLOBAL = "[a]dd [r]ename [D]elete [u]ndo [p]robe [/]find [?]help [q]uit"
GLOBAL_EMPTY = "[a]dd [u]ndo [/]find [?]help [q]uit"

# Bars shown while a popup is open (§6.6): no global line.
PICKER = ("Picker Actions", "type to filter  (↑/↓)choose  (⏎)pick  (esc)cancel")
PROMPT = ("Prompt Actions", "type to replace  (→)keep and edit  (⏎)accept  (esc)cancel")
COLOUR_EDITOR = (
    "Colour Editor Actions",
    "(j/k)channel (+ - > < } {)nudge (tab)RGB (#)hex (⏎)keep (esc)revert",
)

# Port row markers (§4.3). FLOW means the row's conduit is drawn to the next box.
FLOW, OFF_FLOW, OPTIONAL, MISSING, NONE = "flow", "▣", "·", "✕", ""

FLOW_TOP = 3     # canvas row of the boxes' top border
FLOW_LEFT = 4    # first box's column (plus 2 if it has input markers)
FLOW_GAP = 6     # columns between boxes


class Node:
    """A node box: `inputs` and `outputs` are lists of (label, marker)."""

    def __init__(self, kind, id, inputs=(), outputs=()):
        self.kind, self.id = kind, id
        self.inputs, self.outputs = list(inputs), list(outputs)

    def lines(self):
        """The box's lines, its width, and the line index of each input and output row."""
        labels = [len(label) + 2 for label, _ in self.inputs + self.outputs]
        inner = max([10, len(self.kind) + 4, len(self.id) + 2] + labels)
        lines = [
            "╭─ " + self.kind + " " + "─" * (inner - len(self.kind) - 3) + "╮",
            "│ " + self.id.ljust(inner - 1) + "│",
        ]
        in_rows, out_rows = [], []
        if self.inputs:
            lines.append("│" + "─" * inner + "│")
            for label, _ in self.inputs:
                in_rows.append(len(lines))
                lines.append("┼─" + label.ljust(inner - 1) + "│")
        if self.outputs:
            lines.append("│" + "─" * inner + "│")
            for label, _ in self.outputs:
                out_rows.append(len(lines))
                lines.append("│" + (label + "─").rjust(inner) + "┼")
        lines.append("╰" + "─" * inner + "╯")
        return lines, inner + 2, in_rows, out_rows


class Screen:
    def __init__(self, nodes, project="story"):
        """`nodes` is the project's node count, for the title line."""
        self.grid = [[" "] * W for _ in range(H)]
        self.scroll = 0  # columns the canvas is scrolled right by (§4.5)
        self.layout = {}  # node id → (first column, width) on the unscrolled canvas
        plural = "" if nodes == 1 else "s"
        self.put(0, 1, f"conduit edit {project}  ·  {nodes} node{plural}")

    def put(self, row, col, text):
        for i, ch in enumerate(text):
            if 0 <= col + i < W:
                self.grid[row][col + i] = ch
        return self

    def cput(self, row, col, text):
        """`put` on the canvas, which scrolls; text off either edge is clipped."""
        for i, ch in enumerate(text):
            c = col + i - self.scroll
            if 0 <= c < W:
                self.grid[row][c] = ch

    def text(self):
        return "\n".join("".join(row).rstrip() for row in self.grid)

    # Canvas

    def node(self, row, col, node):
        """Draws one box; returns (input rows, output rows, width) in screen rows."""
        lines, width, in_rows, out_rows = node.lines()
        self.layout[node.id] = (col, width)
        for i, line in enumerate(lines):
            self.cput(row + i, col, line)
        for (_, marker), r in zip(node.inputs, in_rows):
            if marker not in (FLOW, NONE):
                self.cput(row + r, col - 2, marker + "─")
        for (_, marker), r in zip(node.outputs, out_rows):
            self.cput(row + r, col + width, "─" + ("" if marker in (FLOW, NONE) else marker))
        return [row + r for r in in_rows], [row + r for r in out_rows], width

    def flow(self, *nodes, scroll=0, centre=None):
        """Draws a flow left to right, joining each node's FLOW output to the next
        node's FLOW input.

        For flows wider than the screen: `scroll` moves the view right by that many
        columns, as when walking (§4.5: as little as possible). `centre` re-centres on
        the focus instead, as after a flow change: a node id, or (id, "in") / (id, "out")
        for an end on that node's left / right border. Either way the view stops at the
        flow's ends."""
        start = FLOW_LEFT + (2 if nodes[0].inputs else 0)
        cols, col = {}, start
        for node in nodes:
            width = node.lines()[1]
            cols[node.id] = (col, width)
            col += width + FLOW_GAP
        right = col - FLOW_GAP + 2  # last box plus its output stub
        if centre is not None:
            id_, side = centre if isinstance(centre, tuple) else (centre, "node")
            c, w = cols[id_]
            focus = {"node": c + w // 2, "in": c, "out": c + w}[side]
            scroll = focus - W // 2
        self.scroll = max(0, min(scroll, right - W))
        col = start
        prev = None  # (screen row, column) of the previous node's flow output stub
        for node in nodes:
            ins, outs, width = self.node(FLOW_TOP, col, node)
            if prev:
                flow_in = [r for (_, m), r in zip(node.inputs, ins) if m == FLOW][0]
                self.conduit(prev[0], prev[1], flow_in, col)
            flows = [r for (_, m), r in zip(node.outputs, outs) if m == FLOW]
            prev = (flows[0], col + width) if flows else None
            col += width + FLOW_GAP
        return self

    def conduit(self, row_from, col_from, row_to, col_to):
        """A conduit from the cell after an output's `┼` to an input's `┼`."""
        if row_from == row_to:
            self.cput(row_from, col_from, "─" * (col_to - col_from))
            return
        # Bend two columns out, clear of other rows' `▣` markers (§4.3).
        up = row_to < row_from
        turn = col_from + 2
        self.cput(row_from, col_from, "──" + ("╯" if up else "╮"))
        for r in range(min(row_from, row_to) + 1, max(row_from, row_to)):
            self.cput(r, turn, "│")
        self.cput(row_to, turn, ("╭" if up else "╰") + "─" * (col_to - turn - 1))

    def hint(self, *lines, row=7, col=24):
        for i, line in enumerate(lines):
            self.put(row + i, col, line)
        return self

    def edit_view(self, kind, name, params, outputs=(), inputs=()):
        """The edit view (§7): `params` are "name = value" strings, `inputs` and
        `outputs` port-row labels such as "layer ← helloWorldLayer"."""
        inner, col, row = 47, 14, 2
        self.put(row, col, "╭─ " + kind + " " + "─" * (inner - len(kind) - 3) + "╮")
        rows = [("text", ""), ("text", " name: " + name), ("rule", None)]
        rows += [("in", label) for label in inputs]
        if inputs:
            rows.append(("rule", None))
        rows += [("text", "  " + p) for p in params]
        if outputs:
            rows.append(("rule", None))
            rows += [("out", label) for label in outputs]
        for i, (what, text) in enumerate(rows, start=row + 1):
            if what == "rule":
                self.put(i, col, "│" + "─" * inner + "│")
            elif what == "in":
                self.put(i, col - 3, "▣──┼─ " + text.ljust(inner - 2) + "│")
            elif what == "out":
                self.put(i, col, "│" + (text + " ─").rjust(inner) + "┼─")
            else:
                self.put(i, col, "│" + text.ljust(inner) + "│")
        self.put(row + 1 + len(rows), col, "╰" + "─" * inner + "╯")
        return self

    def popup(self, row, title, body, width=64):
        col = (W - width) // 2
        self.put(row, col, "┏ " + title + " " + "━" * (width - len(title) - 4) + "┓")
        for i, line in enumerate(body):
            self.put(row + 1 + i, col, "┃ " + line.ljust(width - 4) + " ┃")
        self.put(row + 1 + len(body), col, "┗" + "━" * (width - 2) + "┛")
        return self

    # Status line and action bar

    def status(self, text):
        return self.put(STATUS_ROW, 1, text)

    def bar(self, title, actions="", globals_=GLOBAL):
        """The action bar. Pass a popup's (title, keys) pair, e.g. `PICKER`, as `title`
        alone for a popup bar, which has no global line."""
        if isinstance(title, tuple):
            title, actions, globals_ = title[0], title[1], None
        self.put(BAR_ROW, 2, title + " " + "─" * (W - 4 - len(title)))
        if actions:
            self.put(BAR_ROW + 2, 2, actions)
        if globals_:
            self.put(BAR_ROW + 3, 2, "global  " + globals_)
        return self
