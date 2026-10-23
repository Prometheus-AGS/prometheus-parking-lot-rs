#!/usr/bin/env python3
"""Fail when a .rs file under a `tests/` directory is not compiled by any target.

An orphan is a test file that Cargo never builds: a sub-directory without a
`main.rs`, a file in a package with `autotests = false` and no `[[test]]` entry,
or a helper nobody declares with `mod`. Orphans pass CI silently while the
coverage baseline counts tests that never run (cr-35).

How it decides, with no build needed:

1. `cargo metadata --no-deps` lists every target root of every workspace
   package (auto-discovered and explicit `[[test]]`/`[[bench]]`/`[[example]]`
   alike, honouring `autotests = false`). Standalone workspaces that are not
   members of the root one (bindings/tauri-desktop, bindings/uniffi-bindgen:
   a tracked `Cargo.toml` with its own `[workspace]` table) are listed the same
   way, so their tests are checked too.
2. From each root the script follows out-of-line `mod name;` declarations and
   `#[path = "..."]` attributes using rustc's file-resolution rules. A module
   behind a `cfg` is still followed (the loom suites declare `mod loom_support;`
   behind `#![cfg(parking_lot_loom)]`).
3. Every tracked or untracked-but-not-ignored `.rs` file with a `tests`
   directory in its path must be in the followed set.

Usage: check-orphan-tests.py            (run from anywhere inside the repository)
Exit status: 0 no orphans, 1 orphans found, 2 tooling error.
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from pathlib import Path

MOD_DECL = re.compile(
    r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:unsafe\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;",
)
PATH_ATTR = re.compile(r'^\s*#\[\s*path\s*=\s*"([^"]+)"\s*\]')


def run(*cmd: str, cwd: Path | None = None) -> str:
    try:
        return subprocess.run(
            cmd, check=True, capture_output=True, text=True, cwd=cwd
        ).stdout
    except (OSError, subprocess.CalledProcessError) as err:
        detail = getattr(err, "stderr", "") or str(err)
        print(f"check-orphan-tests: `{' '.join(cmd)}` failed: {detail}", file=sys.stderr)
        sys.exit(2)


def child_modules(file: Path, owns_directory: bool) -> list[Path]:
    """Files declared by `mod name;` in `file`, resolved like rustc does.

    `owns_directory` is true for crate roots and `mod.rs` files, whose child
    modules sit next to them; any other file `foo.rs` keeps its children in `foo/`.
    """
    base = file.parent if owns_directory else file.parent / file.stem
    found: list[Path] = []
    pending_path: str | None = None
    for line in file.read_text(encoding="utf-8", errors="replace").splitlines():
        if line.lstrip().startswith("//"):
            continue
        attr = PATH_ATTR.match(line)
        if attr:
            pending_path = attr.group(1)
            continue
        decl = MOD_DECL.match(line)
        if not decl:
            if line.strip() and not line.lstrip().startswith("#["):
                pending_path = None
            continue
        if pending_path is not None:
            found.append((file.parent / pending_path).resolve())
        else:
            name = decl.group(1)
            for candidate in (base / f"{name}.rs", base / name / "mod.rs"):
                if candidate.is_file():
                    found.append(candidate.resolve())
                    break
        pending_path = None
    return found


def compiled_files(target_roots: list[Path]) -> set[Path]:
    seen: set[Path] = set()
    stack = [(root.resolve(), True) for root in target_roots]
    while stack:
        file, owns_directory = stack.pop()
        if file in seen or not file.is_file():
            continue
        seen.add(file)
        for child in child_modules(file, owns_directory):
            stack.append((child, child.name == "mod.rs"))
    return seen


def target_roots(top: Path, manifest: Path | None = None) -> tuple[list[Path], set[Path]]:
    """Target roots and package manifest paths of one workspace."""
    cmd = ["cargo", "metadata", "--no-deps", "--format-version", "1"]
    if manifest is not None:
        cmd += ["--manifest-path", str(manifest)]
    metadata = json.loads(run(*cmd, cwd=top))
    roots = [
        Path(target["src_path"])
        for package in metadata["packages"]
        for target in package["targets"]
    ]
    manifests = {Path(package["manifest_path"]).resolve() for package in metadata["packages"]}
    return roots, manifests


def standalone_workspaces(top: Path, known: set[Path]) -> list[Path]:
    """Tracked manifests that open their own workspace and are not in the root one."""
    listed = run("git", "ls-files", "--cached", "--others", "--exclude-standard", "-z",
                 "--", "Cargo.toml", "**/Cargo.toml", cwd=top)
    found = []
    for name in sorted(set(filter(None, listed.split("\0")))):
        path = (top / name).resolve()
        if path in known:
            continue
        if re.search(r"^\[workspace\]", path.read_text(encoding="utf-8"), re.MULTILINE):
            found.append(path)
    return found


def main() -> int:
    top = Path(run("git", "rev-parse", "--show-toplevel").strip())
    roots, manifests = target_roots(top)
    for manifest in standalone_workspaces(top, manifests):
        extra_roots, extra_manifests = target_roots(top, manifest)
        roots += extra_roots
        manifests |= extra_manifests
    compiled = compiled_files(roots)

    listed = run(
        "git", "ls-files", "--cached", "--others", "--exclude-standard", "-z", "--", "*.rs", cwd=top
    )
    candidates = sorted(
        (top / name).resolve()
        for name in listed.split("\0")
        if name and "tests" in Path(name).parts[:-1]
    )

    orphans = [path for path in candidates if path not in compiled]
    print(f"check-orphan-tests: {len(candidates)} test files, {len(compiled)} compiled files reached")
    if not orphans:
        print("check-orphan-tests: ok (every .rs file under tests/ is compiled by a target)")
        return 0
    for path in orphans:
        rel = path.relative_to(top.resolve())
        print(f"::error file={rel}::orphan test file: no Cargo target compiles it", file=sys.stderr)
        print(f"orphan: {rel}", file=sys.stderr)
    print(
        "check-orphan-tests: register each orphan (tests/<name>.rs, tests/<name>/main.rs, "
        "a [[test]] entry, or a `mod` declaration in a compiled file) or delete it.",
        file=sys.stderr,
    )
    return 1


if __name__ == "__main__":
    sys.exit(main())
