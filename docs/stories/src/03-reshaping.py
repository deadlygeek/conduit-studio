"""Screens for story 3: reshaping a chain — slide, move, dissolve, delete."""

from tui import CONFIRM, FLOW, OFF_FLOW, OPTIONAL, PICKER, Node, Screen

PROJECT = "chain"


def chain(nodes, *flow, scroll=0, centre=None):
    return Screen(nodes, PROJECT).flow(*flow, scroll=scroll, centre=centre)


def comp(*ports, flow_input):
    """`comp` with input rows `ports`, its flow input on `flow_input`."""
    return Node("composite", "comp",
                [(p, FLOW if p == flow_input else OFF_FLOW) for p in ports]
                + [("layers", OPTIONAL)],   # the list's empty slot (§3.1)
                [("out", FLOW)])


LAYERS_2 = ("base", "layers[0]", "layers[1]")

bg = Node("rect", "bg", outputs=[("out", FLOW)])
square = Node("rect", "square", outputs=[("out", FLOW)])
stripe = Node("rect", "stripe", outputs=[("out", FLOW)])
placed = Node("transform", "placed", [("layer", FLOW)], [("out", FLOW)])
toned = Node("tint", "toned", [("layer", FLOW)], [("out", FLOW)])
preview = Node("preview", "preview", [("image", FLOW)])

START = (bg, comp(*LAYERS_2, flow_input="base"), preview)
CHAIN = (square, placed, toned, comp(*LAYERS_2, flow_input="layers[0]"), preview)
SLID = (square, toned, placed, comp(*LAYERS_2, flow_input="layers[0]"), preview)
GAP = (square, toned, comp(*LAYERS_2, flow_input="layers[0]"), preview)
STRIPE = (stripe, placed, comp(*LAYERS_2, flow_input="layers[1]"), preview)
BRIDGED = (square, comp(*LAYERS_2, flow_input="layers[0]"), preview)
END = (bg, comp("base", "layers[0]", flow_input="base"), preview)

NODE = "(e)dit (m)ove (d)issolve"
MOVE = ("Move Actions", "(m)drop here (esc)cancel", "[/]find [?]help [q]uit")
MOVING = "⇄ moving placed (transform)"

SCREENS = {
    # 1. Find the chain
    "find": chain(7, *START).popup(12, "find node (1 of 7)", ["/ ton▏", "toned  tint"])
        .status("rendered 7 nodes in 19 ms").bar(PICKER),
    "chain": chain(7, *CHAIN, centre="toned").status("toned (tint)")
        .bar("Node Actions", "(e)dit (m)ove (H)slide (d)issolve"),

    # 2. Slide the tint
    "slid": chain(7, *SLID, scroll=4).status("slid toned before placed")
        .bar("Node Actions", "(e)dit (m)ove (L)slide (d)issolve"),
    "refused": chain(7, *CHAIN, scroll=4)
        .status("can't slide toned after comp: preview.image takes an image, not a layer")
        .bar("Node Actions", "(e)dit (m)ove (H)slide (d)issolve"),

    # 3. Pick up the transform
    "picked": chain(7, *GAP)
        .status(MOVING + "  ·  square.out → toned.layer").bar(*MOVE),
    "target": chain(7, *GAP)
        .status(MOVING + "  ·  comp.layers[1] ← stripe").bar(*MOVE),
    "dropped": chain(7, *STRIPE, centre="placed")
        .status("moved placed to stripe → comp.layers[1]").bar("Node Actions", NODE),

    # 4. Dissolve the tint
    "dissolve": chain(7, *GAP, centre="toned")
        .popup(12, "confirm", ["Dissolve `toned`? Connects square → comp.layers[0].", "",
                               "y to confirm, any other key to cancel"])
        .status("toned (tint)").bar(CONFIRM),
    "dissolved": chain(6, *BRIDGED).status("dissolved toned: square → comp.layers[0]")
        .bar("Conduit Actions", "(c)onnect (i)nsert (d)isconnect"),

    # 5. Delete the transform
    "delete": chain(6, *STRIPE)
        .popup(12, "confirm", ["Delete `placed`? This disconnects comp.layers[1].", "",
                               "y to confirm, any other key to cancel"])
        .status("placed (transform)").bar(CONFIRM),
    "deleted": chain(5, *END, centre="comp").status("deleted placed")
        .bar("Node Actions", NODE),
}
