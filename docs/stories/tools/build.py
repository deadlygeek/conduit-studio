"""Builds docs/stories/NN-name.md from src/NN-name.md.in and src/NN-name.py.

    python3 docs/stories/tools/build.py           # write every story
    python3 docs/stories/tools/build.py --check   # fail if any story is out of date

Each `{{name}}` in a template is replaced by the screen `SCREENS[name]` from the
story's .py file, in a code block. Standard library only.
"""

import re
import runpy
import sys
from pathlib import Path

sys.dont_write_bytecode = True  # keep __pycache__ out of docs/

TOOLS = Path(__file__).resolve().parent
STORIES = TOOLS.parent
SRC = STORIES / "src"


def build(template: Path) -> str:
    screens = runpy.run_path(str(template.with_suffix("").with_suffix(".py")))["SCREENS"]
    text = template.read_text()
    used = set(re.findall(r"\{\{(\w+)\}\}", text))
    if missing := used - screens.keys():
        sys.exit(f"{template.name}: no screen for {sorted(missing)}")
    if unused := screens.keys() - used:
        sys.exit(f"{template.name}: screens not used: {sorted(unused)}")
    return re.sub(
        r"\{\{(\w+)\}\}", lambda m: "```\n" + screens[m.group(1)].text() + "\n```", text
    )


def main() -> None:
    check = "--check" in sys.argv[1:]
    sys.path.insert(0, str(TOOLS))
    stale = []
    for template in sorted(SRC.glob("*.md.in")):
        out = STORIES / template.name.removesuffix(".in")
        text = build(template)
        if check:
            if not out.exists() or out.read_text() != text:
                stale.append(out.name)
        else:
            out.write_text(text)
            print(f"wrote {out.relative_to(STORIES.parent.parent)}")
    if stale:
        sys.exit(f"out of date (run build.py): {', '.join(stale)}")


if __name__ == "__main__":
    main()
