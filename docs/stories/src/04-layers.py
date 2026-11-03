"""Screens for story 4: layers — insert, reorder, disconnect."""

from tui import FLOW, OFF_FLOW, OPTIONAL, PICKER, PROMPT, Node, Screen


def story(nodes, *flow, centre=None):
    return Screen(nodes, "story").flow(*flow, centre=centre)


def comp(entries, flow_input):
    """`comp` with `entries` layers, then the empty `layers` slot; the flow input is
    `flow_input` ("base" or an index)."""
    ports = ["base"] + [f"layers[{i}]" for i in range(entries)]
    flow = "base" if flow_input == "base" else f"layers[{flow_input}]"
    rows = [(p, FLOW if p == flow else OFF_FLOW) for p in ports]
    return Node("composite", "comp", rows + [("layers", OPTIONAL)], [("out", FLOW)])


def rect(id_):
    return Node("rect", id_, outputs=[("out", FLOW)])


preview = Node("preview", "preview", [("image", FLOW)])
shadow_at = Node("transform", "shadow_at", [("layer", FLOW)], [("out", FLOW)])

END = "(c)onnect (i)nsert (d)isconnect"
OFF_END = "(f)ocus " + END

SCREENS = {
    # 1. The empty slot
    "slot": story(3, rect("blue_rect"), comp(0, "base"), preview)
        .status("comp.layers: no layers (optional)").bar("Input Actions", "(c)onnect (i)nsert"),
    "pick_rect": story(3, rect("blue_rect"), comp(0, "base"), preview)
        .popup(13, "insert before comp.layers: type (1 of 9)", ["/ rect▏", "rect  () → (image)"])
        .status("comp.layers: no layers (optional)").bar(PICKER),
    "card": story(4, rect("card"), comp(1, 0), preview, centre=("comp", "in"))
        .status("inserted card (rect) before comp.layers[0]").bar("Conduit End Actions", END),

    # 2. Two more
    "shadow": story(6, rect("shadow"), comp(3, 2), preview, centre=("comp", "in"))
        .status("inserted shadow (rect) before comp.layers[2]")
        .bar("Conduit End Actions", END + " (K)earlier"),

    # 3. Reorder
    "reordered": story(6, rect("shadow"), comp(3, 0), preview)
        .status("moved comp.layers ← shadow to [0]")
        .bar("Conduit End Actions", END + " (J)later"),

    # 4. Insert a transform
    "pick_transform": story(6, rect("shadow"), comp(3, 0), preview)
        .popup(13, "insert before comp.layers[0]: type (1 of 9)",
               ["/ trans▏", "transform  (layer) → (layer)"])
        .status("comp.layers[0] ← shadow").bar(PICKER),
    "id_transform": story(6, rect("shadow"), comp(3, 0), preview)
        .popup(14, "id for new transform", ["shadow_at▏"])
        .status("comp.layers[0] ← shadow").bar(PROMPT),
    "inserted": story(7, rect("shadow"), shadow_at, comp(3, 0), preview, centre=("comp", "in"))
        .status("inserted shadow_at (transform) before comp.layers[0]")
        .bar("Conduit End Actions", END + " (J)later"),

    # 5. Disconnect a layer outside the flow
    "card_end": story(7, rect("shadow"), shadow_at, comp(3, 0), preview)
        .status("comp.layers[1] ← card")
        .bar("Conduit End Actions", OFF_END + " (K)earlier (J)later"),
    "card_gone": story(7, rect("shadow"), shadow_at, comp(2, 0), preview)
        .status("disconnected comp.layers[1] ← card")
        .bar("Conduit End Actions", OFF_END + " (K)earlier"),

    # 6. Disconnect the layer in the flow
    "flow_conduit": story(7, rect("shadow"), shadow_at, comp(2, 0), preview)
        .status("shadow_at.out → comp.layers[0]").bar("Conduit Actions", END),
    "shadow_gone": story(7, rect("blue_rect"), comp(1, "base"), preview, centre=("comp", "in"))
        .status("disconnected comp.layers[0] ← shadow_at").bar("Conduit End Actions", OFF_END),
}
