"""Screens for story 1: a blue rectangle in a preview window."""

from tui import (
    COLOUR_EDITOR, FLOW, GLOBAL_EMPTY, MISSING, NONE, OPTIONAL, PICKER, PROMPT,
    Node, Screen,
)

NODE_ACTIONS = "(e)dit (d)issolve"  # nothing feeds these nodes, so no (m)ove (§6.5)
INPUT_ACTIONS = "(c)onnect (i)nsert"
END_ACTIONS = "(c)onnect (i)nsert (d)isconnect"

# The nodes, as they look at each stage.
rect_unused = Node("rect", "blue_rect", outputs=[("out", NONE)])
rect = Node("rect", "blue_rect", outputs=[("out", FLOW)])
comp_open = Node("composite", "comp", [("base", MISSING), ("layers", OPTIONAL)], [("out", NONE)])
comp_fed = Node("composite", "comp", [("base", FLOW), ("layers", OPTIONAL)], [("out", NONE)])
comp = Node("composite", "comp", [("base", FLOW), ("layers", OPTIONAL)], [("out", FLOW)])
preview_open = Node("preview", "preview", [("image", MISSING)])
preview = Node("preview", "preview", [("image", FLOW)])

EMPTY_HINT = ("This project has no nodes yet.", "Press a to add one.")
RECT_DEFAULTS = ["width  = 100 (default)", "height = 100 (default)"]
COMP_NOT_CONNECTED = "✕ comp.base is not connected  ·  1 error"
PREVIEW_NOT_CONNECTED = "✕ preview.image is not connected  ·  1 error"
CONNECTED_BASE = "connected comp.base ← blue_rect"


def rect_edit(params, status, bar):
    return (Screen(1).edit_view("rect", "blue_rect", params, outputs=["out"])
            .status(status).bar(*bar))


SCREENS = {
    # 1. Add the rectangle
    "empty": Screen(0).hint(*EMPTY_HINT).bar("Project Actions", globals_=GLOBAL_EMPTY),
    "add_rect": Screen(0).hint(*EMPTY_HINT)
        .popup(4, "add node of type (1 of 14)", ["/ rect▏", "rect  () → (image)"])
        .bar(PICKER),
    "id_rect": Screen(0).hint(*EMPTY_HINT)
        .popup(5, "id for new rect", ["blue_rect▏"]).bar(PROMPT),
    "rect": Screen(1).flow(rect_unused)
        .status("added blue_rect (rect)").bar("Node Actions", NODE_ACTIONS),

    # 2. Make it large and blue
    "edit_name": rect_edit(["color  = ██ white (default)"] + RECT_DEFAULTS,
                           "blue_rect (rect)", ("Name Actions", "(e)dit")),
    "edit_color": rect_edit(["color  = ██ white (default)"] + RECT_DEFAULTS,
                            "blue_rect.color = white",
                            ("Param Actions", "(e)dit (C)picker (c)onnect (i)nsert")),
    "color_editor": Screen(1)
        .edit_view("rect", "blue_rect", ["color  = ██ white (default)"] + RECT_DEFAULTS,
                   outputs=["out"])
        .popup(4, "color  blue_rect.color  (HSL)", [
            "████████████████  #0000ff",
            "████████████████  was #ffffff ██",
            "# blue▏",
            "▸ H 240°  ██████████████████████████┃████████",
            "  S 100%  ██████████████████████████████████┃",
            "  L  50%  █████████████████┃█████████████████",
            "  A 100%  ██████████████████████████████████┃",
        ], width=56)
        .status("blue_rect.color = blue").bar(COLOUR_EDITOR),
    "width_prompt": Screen(1)
        .edit_view("rect", "blue_rect", ["color  = ██ blue"] + RECT_DEFAULTS, outputs=["out"])
        .popup(13, "blue_rect.width (empty = default)", ["800▏"])
        .status("blue_rect.width = 100").bar(PROMPT),
    "edit_done": rect_edit(["color  = ██ blue", "width  = 800", "height = 600"],
                           "set blue_rect.height = 600",
                           ("Param Actions", "(e)dit (c)onnect (i)nsert (d)efault")),
    "rect_done": Screen(1).flow(rect_unused)
        .status("blue_rect (rect)  ·  800×600").bar("Node Actions", NODE_ACTIONS),

    # 3. Add the composite
    "add_comp": Screen(1).flow(rect_unused)
        .popup(12, "add node of type (1 of 14)",
               ["/ comp▏", "composite  (image, layer) → (image)"])
        .status("blue_rect (rect)").bar(PICKER),
    "id_comp": Screen(1).flow(rect_unused)
        .popup(10, "id for new composite", ["comp▏"])
        .status("blue_rect (rect)").bar(PROMPT),
    "comp": Screen(2).flow(comp_open)
        .status("added comp (composite)  ·  1 error").bar("Node Actions", NODE_ACTIONS),

    # 4. Connect the rectangle to the composite's background
    "comp_base": Screen(2).flow(comp_open)
        .status(COMP_NOT_CONNECTED).bar("Input Actions", INPUT_ACTIONS),
    "comp_pick": Screen(2).flow(comp_open)
        .popup(12, "connect comp.base to (1 of 1)", ["/ ▏", "blue_rect  rect"])
        .status(COMP_NOT_CONNECTED).bar(PICKER),
    "connected": Screen(2).flow(rect, comp_fed)
        .status(CONNECTED_BASE).bar("Conduit End Actions", END_ACTIONS),

    # 5. Add the preview
    "add_prev": Screen(2).flow(rect, comp_fed)
        .popup(12, "add node of type (1 of 14)", ["/ prev▏", "preview  (image) → ()"])
        .status(CONNECTED_BASE).bar(PICKER),
    "id_prev": Screen(2).flow(rect, comp_fed)
        .popup(13, "id for new preview", ["preview▏"])
        .status(CONNECTED_BASE).bar(PROMPT),
    "prev": Screen(3).flow(preview_open)
        .status("added preview (preview)  ·  1 error").bar("Node Actions", NODE_ACTIONS),

    # 6. Connect the composite to the preview
    "prev_image": Screen(3).flow(preview_open)
        .status(PREVIEW_NOT_CONNECTED).bar("Input Actions", INPUT_ACTIONS),
    "prev_pick": Screen(3).flow(preview_open)
        .popup(12, "connect preview.image to (2 of 2)",
               ["/ ▏", "blue_rect  rect", "comp  composite"])
        .status(PREVIEW_NOT_CONNECTED).bar(PICKER),
    "final": Screen(3).flow(rect, comp, preview)
        .status("connected preview.image ← comp").bar("Conduit End Actions", END_ACTIONS),
}
