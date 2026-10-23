#!/usr/bin/env python3
"""Every Rust code block on the docs site is copied from a compiled example.

cr-47: guide code must be compiled in CI from a real source file. The site does
not compile Markdown, so each ```rust block in website/docs must name its source
in the fence, e.g.

    ```rust title="examples/gpu_inference_pool.rs"

and its lines must appear, in order and contiguously, in that file (trailing
whitespace ignored). The examples themselves are built and, where they need no
server, run by the docs-examples workflow, so a block that passes this check is
code that compiles and runs.

Exit status 1 lists every block that has no title, names a missing file, or has
drifted from its source. Stdlib only; run from the repository root.
"""

from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "website" / "docs"
FENCE = re.compile(r"^(\s*)(`{3,})(\S*)(.*)$")
TITLE = re.compile(r'title="([^"]+)"')


def rust_blocks(path: Path):
    """Yield (line number, info string, body lines) for each ```rust block."""
    lines = path.read_text(encoding="utf-8").splitlines()
    i = 0
    while i < len(lines):
        match = FENCE.match(lines[i])
        if not match:
            i += 1
            continue
        indent, ticks, lang, info = match.groups()
        start = i
        body: list[str] = []
        i += 1
        while i < len(lines) and not lines[i].strip().startswith(ticks):
            line = lines[i]
            body.append(line[len(indent):] if line.startswith(indent) else line)
            i += 1
        i += 1
        if lang == "rust":
            yield start + 1, info, body


def contains(haystack: list[str], needle: list[str]) -> bool:
    hay = [line.rstrip() for line in haystack]
    pin = [line.rstrip() for line in needle]
    while pin and not pin[0]:
        pin.pop(0)
    while pin and not pin[-1]:
        pin.pop()
    if not pin:
        return False
    return any(hay[k : k + len(pin)] == pin for k in range(len(hay) - len(pin) + 1))


def main() -> int:
    failures: list[str] = []
    checked = 0
    for page in sorted(DOCS.rglob("*.md*")):
        rel = page.relative_to(ROOT)
        for line_no, info, body in rust_blocks(page):
            checked += 1
            title = TITLE.search(info)
            if not title:
                failures.append(f"{rel}:{line_no}: rust block has no title=\"examples/<file>.rs\"")
                continue
            source = ROOT / title.group(1)
            if not source.is_file():
                failures.append(f"{rel}:{line_no}: {title.group(1)} does not exist")
                continue
            if not contains(source.read_text(encoding="utf-8").splitlines(), body):
                failures.append(
                    f"{rel}:{line_no}: block is not a verbatim excerpt of {title.group(1)}"
                )
    for failure in failures:
        print(failure, file=sys.stderr)
    print(f"checked {checked} rust blocks in {DOCS.relative_to(ROOT)}: {len(failures)} failing")
    return 1 if failures else 0


if __name__ == "__main__":
    sys.exit(main())
