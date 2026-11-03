"""Screens for story 2: exploring an existing project — `▣`, `f` and history."""

from tui import FLOW, NONE, OFF_FLOW, OPTIONAL, PICKER, Node, Screen

FX = "examples/effects"
TR = "examples/transform"


def fx(*nodes, scroll=0, centre=None):
    return Screen(13, FX).flow(*nodes, scroll=scroll, centre=centre)


def tr(*nodes, scroll=0, centre=None):
    return Screen(8, TR).flow(*nodes, scroll=scroll, centre=centre)


# ── examples/effects ─────────────────────────────────────────────────────────

def comp_fx(flow_input):
    """`comp` with its flow input on `flow_input` ("base" or a "layers[n]")."""
    ports = ["base", "layers[0]", "layers[1]", "layers[2]"]
    return Node("composite", "comp",
                [(p, FLOW if p == flow_input else OFF_FLOW) for p in ports]
                + [("layers", OPTIONAL)],   # the list's empty slot (§3.1)
                [("out", FLOW)])


preview = Node("preview", "preview", [("image", FLOW)])
bg = Node("rect", "bg", outputs=[("out", FLOW)])
logo = Node("text", "logo", outputs=[("out", FLOW)])


def logo_tinted(flow_input):
    return Node("tint", "logo_tinted",
                [(p, FLOW if p == flow_input else OFF_FLOW) for p in ("layer", "black")],
                [("out", FLOW)])


logo_at = Node("transform", "logo_at", [("layer", FLOW)], [("out", FLOW)])


def brand(flow_output):
    """The palette, one row per colour per user (§3.1, §7.1)."""
    users = {"primary": OFF_FLOW, "teal": OFF_FLOW, "sand": NONE, "orange": NONE,
             "accent": OFF_FLOW}
    users[flow_output] = FLOW
    return Node("palette", "brand", outputs=[(f"██ {c}", m) for c, m in users.items()])


bar = Node("rect", "bar", [("color", FLOW)], [("out", FLOW)])
bar_turned = Node("transform", "bar_turned", [("layer", FLOW)], [("out", FLOW)])
bar_inverted = Node("invert", "bar_inverted", [("layer", FLOW)], [("out", FLOW)])
swatch = Node("rect", "swatch", [("color", FLOW)], [("out", FLOW)])
swatch_shifted = Node("hsl", "swatch_shifted", [("layer", FLOW)], [("out", FLOW)])
swatch_at = Node("transform", "swatch_at", [("layer", FLOW)], [("out", FLOW)])

FLOW_A = (bg, comp_fx("base"), preview)
FLOW_B = (logo, logo_tinted("layer"), logo_at, comp_fx("layers[0]"), preview)
FLOW_C = (brand("accent"), logo_tinted("black"), logo_at, comp_fx("layers[0]"), preview)
FLOW_D = (brand("primary"), bar, bar_turned, bar_inverted, comp_fx("layers[2]"), preview)
FLOW_E = (brand("teal"), swatch, swatch_shifted, swatch_at, comp_fx("layers[1]"), preview)

NODE = "(e)dit (m)ove (d)issolve"
SINK = "(e)dit (d)issolve"  # `preview` has no output, so no (m)ove (§6.5)
CONDUIT = "(c)onnect (i)nsert (d)isconnect"

# ── examples/transform ───────────────────────────────────────────────────────

def comp_tr(flow_input):
    ports = ["base", "layers[0]", "layers[1]", "layers[2]"]
    return Node("composite", "comp",
                [(p, FLOW if p == flow_input else OFF_FLOW) for p in ports]
                + [("layers", OPTIONAL)],   # the list's empty slot (§3.1)
                [("out", FLOW)])


bar_tr = Node("rect", "bar", outputs=[("out", OFF_FLOW), ("out", FLOW)])
turned_two = Node("transform", "turned", [("layer", FLOW)],
                  [("out", OFF_FLOW), ("out", FLOW)])   # users: comp, grown (id order)
turned_comp = Node("transform", "turned", [("layer", FLOW)],
                   [("out", FLOW), ("out", OFF_FLOW)])
grown = Node("transform", "grown", [("layer", FLOW)], [("out", FLOW)])
baked = Node("realize", "baked", [("layer", FLOW)], [("out", FLOW)])
preview_tr = Node("preview", "preview", [("image", FLOW)])

SCREENS = {
    # 1. Open the project
    "start": fx(*FLOW_A).status("rendered 13 nodes in 41 ms").bar("Node Actions", SINK),

    # 2. Walk the flow
    "conduit": fx(*FLOW_A).status("comp.out → preview.image").bar("Conduit Actions", CONDUIT),
    "comp": fx(*FLOW_A).status("comp (composite)").bar("Node Actions", NODE),
    "layer0": fx(*FLOW_A).status("comp.layers[0] ← logo_at")
        .bar("Conduit End Actions", "(f)ocus (c)onnect (i)nsert (d)isconnect (J)later"),

    # 3. Follow a layer
    "flow_b": fx(*FLOW_B, centre=("comp", "in")).status("comp.layers[0] ← logo_at")
        .bar("Conduit End Actions", "(c)onnect (i)nsert (d)isconnect (J)later"),
    "black": fx(*FLOW_B, scroll=4).status("logo_tinted.black ← brand.accent")
        .bar("Conduit End Actions", "(f)ocus (c)onnect (i)nsert (d)isconnect"),

    # 4. Follow the colour into the palette
    "flow_c": fx(*FLOW_C, centre=("logo_tinted", "in")).status("logo_tinted.black ← brand.accent")
        .bar("Conduit End Actions", "(c)onnect (i)nsert (d)isconnect"),
    "primary": fx(*FLOW_C).status("brand.primary → bar.color")
        .bar("Conduit End Actions", "(f)ocus (c)onnect to (i)nsert (d)isconnect"),

    # 5. Follow it downstream
    "flow_d": fx(*FLOW_D, centre=("brand", "out")).status("brand.primary → bar.color")
        .bar("Conduit End Actions", "(c)onnect to (i)nsert (d)isconnect"),

    # 6. Back and forward
    "back": fx(*FLOW_C, centre=("brand", "out")).status("back: brand.primary → bar.color")
        .bar("Conduit End Actions", "(f)ocus (c)onnect to (i)nsert (d)isconnect"),

    # 7. Jump with find
    "find": fx(*FLOW_C, centre=("brand", "out"))
        .popup(12, "find node (1 of 13)", ["/ swsh▏", "swatch_shifted  hsl"])
        .status("back: brand.primary → bar.color").bar(PICKER),
    "flow_e": fx(*FLOW_E, centre="swatch_shifted").status("swatch_shifted (hsl)")
        .bar("Node Actions", "(e)dit (m)ove (L)slide (d)issolve"),

    # 8. One node, two places (examples/transform)
    "tr_start": tr(Node("rect", "bg", outputs=[("out", FLOW)]), comp_tr("base"), preview_tr)
        .status("rendered 8 nodes in 23 ms").bar("Node Actions", SINK),
    "tr_flow": tr(bar_tr, turned_two, grown, baked, comp_tr("layers[2]"), preview_tr, centre=("comp", "in"))
        .status("comp.layers[2] ← baked")
        .bar("Conduit End Actions", "(c)onnect (i)nsert (d)isconnect (K)earlier"),
    "tr_turned": tr(bar_tr, turned_two, grown, baked, comp_tr("layers[2]"), preview_tr, scroll=4)
        .status("turned.out → comp.layers[0]")
        .bar("Conduit End Actions", "(f)ocus (c)onnect to (i)nsert (d)isconnect"),
    "tr_flow2": tr(bar_tr, turned_comp, comp_tr("layers[0]"), preview_tr, centre=("turned", "out"))
        .status("turned.out → comp.layers[0]")
        .bar("Conduit End Actions", "(c)onnect to (i)nsert (d)isconnect"),
}
