"""Screens for story 1: a blue background in a preview window."""

from tui import (
    COLOUR_EDITOR, FLOW, GLOBAL_EMPTY, MISSING, NONE, OPTIONAL, PICKER, PROMPT,
    Node, Screen,
)

NODE_ACTIONS = "(e)dit (d)issolve"  # nothing feeds these nodes, so no (m)ove (§6.5)
INPUT_ACTIONS = "(c)onnect (i)nsert"
END_ACTIONS = "(c)onnect (i)nsert (d)isconnect"

# The nodes, as they look at each stage.
bg_unused = Node("canvas", "blue_bg", [("image", OPTIONAL)], [("out", NONE)])
bg = Node("canvas", "blue_bg", [("image", OPTIONAL)], [("out", FLOW)])
comp_open = Node("composite", "comp", [("base", MISSING), ("layers", OPTIONAL)], [("out", NONE)])
comp_fed = Node("composite", "comp", [("base", FLOW), ("layers", OPTIONAL)], [("out", NONE)])
comp = Node("composite", "comp", [("base", FLOW), ("layers", OPTIONAL)], [("out", FLOW)])
preview_open = Node("preview", "preview", [("image", MISSING)])
preview = Node("preview", "preview", [("image", FLOW)])

EMPTY_HINT = ("This project has no nodes yet.", "Press a to add one.")
CANVAS_INPUTS = [(NONE, "image  (optional)")]
CANVAS_DEFAULTS = ["width   = 1920 (default)", "height  = 1080 (default)"]
OPACITY = ["opacity = 100 (default)"]
COMP_NOT_CONNECTED = "✕ comp.base is not connected  ·  1 error"
PREVIEW_NOT_CONNECTED = "✕ preview.image is not connected  ·  1 error"
CONNECTED_BASE = "connected comp.base ← blue_bg"


def bg_edit_view(screen, params):
    return screen.edit_view("canvas", "blue_bg", params + OPACITY, outputs=["out"],
                            inputs=CANVAS_INPUTS)


def bg_edit(params, status, bar):
    return bg_edit_view(Screen(1), params).status(status).bar(*bar)


SCREENS = {
    # 1. Add the canvas
    "empty": Screen(0).hint(*EMPTY_HINT).bar("Project Actions", globals_=GLOBAL_EMPTY),
    "add_canvas": Screen(0).hint(*EMPTY_HINT)
        .popup(4, "add node of type (1 of 25)", ["/ canv▏", "canvas  (image) → (image)"])
        .bar(PICKER),
    "id_canvas": Screen(0).hint(*EMPTY_HINT)
        .popup(5, "id for new canvas", ["blue_bg▏"]).bar(PROMPT),
    "canvas": Screen(1).flow(bg_unused)
        .status("added blue_bg (canvas)").bar("Node Actions", NODE_ACTIONS),

    # 2. Make it large and blue
    "edit_name": bg_edit(["color   = ██ white (default)"] + CANVAS_DEFAULTS,
                         "blue_bg (canvas)", ("Name Actions", "(e)dit")),
    "edit_color": bg_edit(["color   = ██ white (default)"] + CANVAS_DEFAULTS,
                          "blue_bg.color = white",
                          ("Param Actions", "(e)dit (C)picker (c)onnect (i)nsert")),
    "color_editor": bg_edit_view(Screen(1), ["color   = ██ white (default)"] + CANVAS_DEFAULTS)
        .popup(4, "color  blue_bg.color  (HSL)", [
            "████████████████  #0000ff",
            "████████████████  was #ffffff ██",
            "# blue▏",
            "▸ H 240°  ██████████████████████████┃████████",
            "  S 100%  ██████████████████████████████████┃",
            "  L  50%  █████████████████┃█████████████████",
            "  A 100%  ██████████████████████████████████┃",
        ], width=56)
        .status("blue_bg.color = blue").bar(COLOUR_EDITOR),
    "width_prompt": bg_edit_view(Screen(1), ["color   = ██ blue"] + CANVAS_DEFAULTS)
        .popup(11, "blue_bg.width (empty = default)", ["800▏"])
        .status("blue_bg.width = 1920").bar(PROMPT),
    "edit_done": bg_edit(["color   = ██ blue", "width   = 800", "height  = 600"],
                         "set blue_bg.height = 600",
                         ("Param Actions", "(e)dit (c)onnect (i)nsert (d)efault")),
    "canvas_done": Screen(1).flow(bg_unused)
        .status("blue_bg (canvas)  ·  800×600").bar("Node Actions", NODE_ACTIONS),

    # 3. Add the composite
    "add_comp": Screen(1).flow(bg_unused)
        .popup(12, "add node of type (1 of 25)",
               ["/ comp▏", "composite  (image, layer) → (image)"])
        .status("blue_bg (canvas)").bar(PICKER),
    "id_comp": Screen(1).flow(bg_unused)
        .popup(10, "id for new composite", ["comp▏"])
        .status("blue_bg (canvas)").bar(PROMPT),
    "comp": Screen(2).flow(comp_open)
        .status("added comp (composite)  ·  1 error").bar("Node Actions", NODE_ACTIONS),

    # 4. Connect the canvas to the composite's background
    "comp_base": Screen(2).flow(comp_open)
        .status(COMP_NOT_CONNECTED).bar("Input Actions", INPUT_ACTIONS),
    "comp_pick": Screen(2).flow(comp_open)
        .popup(12, "connect comp.base to (1 of 1)", ["/ ▏", "blue_bg  canvas"])
        .status(COMP_NOT_CONNECTED).bar(PICKER),
    "connected": Screen(2).flow(bg, comp_fed)
        .status(CONNECTED_BASE).bar("Conduit End Actions", END_ACTIONS),

    # 5. Add the preview
    "add_prev": Screen(2).flow(bg, comp_fed)
        .popup(12, "add node of type (1 of 25)", ["/ prev▏", "preview  (image) → ()"])
        .status(CONNECTED_BASE).bar(PICKER),
    "id_prev": Screen(2).flow(bg, comp_fed)
        .popup(13, "id for new preview", ["preview▏"])
        .status(CONNECTED_BASE).bar(PROMPT),
    "prev": Screen(3).flow(preview_open)
        .status("added preview (preview)  ·  1 error").bar("Node Actions", NODE_ACTIONS),

    # 6. Connect the composite to the preview
    "prev_image": Screen(3).flow(preview_open)
        .status(PREVIEW_NOT_CONNECTED).bar("Input Actions", INPUT_ACTIONS),
    "prev_pick": Screen(3).flow(preview_open)
        .popup(12, "connect preview.image to (2 of 2)",
               ["/ ▏", "blue_bg  canvas", "comp  composite"])
        .status(PREVIEW_NOT_CONNECTED).bar(PICKER),
    "final": Screen(3).flow(bg, comp, preview)
        .status("connected preview.image ← comp").bar("Conduit End Actions", END_ACTIONS),
}
