#!/usr/bin/env python3
"""Nonpublishing, scratch-only lockstep release preparation (Python >= 3.11).

See ADR 0005. `check` rejects inconsistent package/dependency/lockfile versions.
`prepare` clones an immutable candidate into OUTPUT/workspace, runs supplied
pinned tools, and writes a deterministic release-plan.json. It never installs,
publishes, signs, tags, commits or pushes. Run only at the final phase boundary.
"""

from __future__ import annotations

import argparse
import datetime
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import tomllib


PACKAGES = {
    "prometheus-parking-lot-core": "crates/parking-lot-core",
    "prometheus-parking-lot-runtime-tokio": "crates/parking-lot-runtime-tokio",
    "prometheus-parking-lot-pg": "crates/parking-lot-pg",
    "prometheus-parking-lot-embedded": "crates/parking-lot-embedded",
    "prometheus-parking-lot-iggy": "crates/parking-lot-iggy",
    "prometheus-parking-lot-wasm": "crates/parking-lot-wasm",
    "prometheus_parking_lot": ".",
}
FFI = "prometheus-parking-lot-ffi"
MEMBERS = {**PACKAGES, FFI: "crates/parking-lot-ffi"}
VERSION = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")
SHA = re.compile(r"[0-9a-f]{40}")


class PreparationError(Exception):
    """A release-policy violation or a failed collaborator command."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise PreparationError(message)


def read_toml(path: Path) -> dict:
    return tomllib.loads(path.read_text(encoding="utf-8"))


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def environment() -> dict[str, str]:
    # Named boundary: ambient Git/cliff overrides must not change the selected
    # source/range/template; preparation has no use for registry/forge tokens.
    env = {
        key: value for key, value in os.environ.items()
        if not key.startswith(("GIT_", "GIT_CLIFF_"))
        and key not in {"RUSTUP_TOOLCHAIN", "GITHUB_TOKEN", "GH_TOKEN", "GITLAB_TOKEN", "GITEA_TOKEN",
                        "CARGO_REGISTRY_TOKEN", "BITBUCKET_TOKEN", "AZURE_DEVOPS_TOKEN"}
        and not (key.startswith("CARGO_REGISTRIES_") and key.endswith("_TOKEN"))
    }
    env.update(CARGO_NET_OFFLINE="true", RUSTUP_AUTO_INSTALL="0",
               GIT_TERMINAL_PROMPT="0", NO_COLOR="1", TZ="UTC")
    return env


def run(argv: list[str], cwd: Path, records: list | None = None) -> str:
    result = subprocess.run(argv, cwd=cwd, env=environment(), capture_output=True,
                            text=True, check=False)
    if records is not None:
        records.append({"argv": argv, "cwd": str(cwd), "exit": result.returncode,
                        "stdout": result.stdout, "stderr": result.stderr})
    require(result.returncode == 0,
            f"command failed ({result.returncode}): {argv!r}\n{result.stderr}")
    return result.stdout


def git(root: Path, *args: str) -> str:
    return run(["git", "-c", "core.hooksPath=/dev/null", "-c", "core.fsmonitor=false",
                *args], root)


def dependency_tables(document: dict):
    for kind in ("dependencies", "dev-dependencies", "build-dependencies"):
        yield document.get(kind, {})
    for target in document.get("target", {}).values():
        yield from dependency_tables(target)


def check(workspace: Path, expected: str | None = None) -> str:
    """Check the real manifests without resolving dependencies or running Cargo."""
    workspace = workspace.resolve()
    root = read_toml(workspace / "Cargo.toml")
    shared = root["workspace"]
    version = shared["package"]["version"]
    require(isinstance(version, str) and VERSION.fullmatch(version) is not None,
            "workspace version must be a stable MAJOR.MINOR.PATCH")
    require(expected is None or expected == version,
            f"workspace version {version} differs from expected {expected}")
    require(set(shared["members"]) == {path for path in MEMBERS.values() if path != "."},
            "workspace membership differs from the seven-crate policy plus unpublished FFI")
    workspace_dependencies = shared["dependencies"]
    local_dependencies = {
        name: dep for name, dep in workspace_dependencies.items()
        if isinstance(dep, dict) and "path" in dep
    }
    require(set(local_dependencies) == set(PACKAGES) - {"prometheus_parking_lot"},
            "workspace internal dependency set differs from the release policy")
    for name, dep in local_dependencies.items():
        require(dep.get("version") == version, f"internal dependency {name} version mismatch")
        require(dep.get("path") == MEMBERS[name], f"internal dependency {name} path mismatch")

    for name, directory in MEMBERS.items():
        manifest = workspace / directory / "Cargo.toml"
        document = read_toml(manifest)
        package = document["package"]
        require(package["name"] == name, f"package name mismatch: {manifest}")
        require(package.get("version") == {"workspace": True},
                f"package {name} must inherit workspace version")
        require((package.get("publish") is False) == (name == FFI),
                f"publish eligibility mismatch: {name}")
        for table in dependency_tables(document):
            for alias, dep in table.items():
                package_name = dep.get("package", alias) if isinstance(dep, dict) else alias
                if package_name not in MEMBERS:
                    continue
                require(isinstance(dep, dict), f"internal dependency {alias} must be a table")
                if dep.get("workspace") is True:
                    require(alias in local_dependencies and "version" not in dep and "path" not in dep,
                            f"internal dependency {name}/{alias} overrides inheritance")
                else:
                    # Incumbent self-dev-dependency is path-only so Cargo strips
                    # its test-only feature edge when packaging.
                    self_dev = (table is document.get("dev-dependencies")
                                and package_name == name and dep.get("path") == "."
                                and "version" not in dep)
                    require(self_dev or (dep.get("version") == version and "path" in dep),
                            f"internal dependency {name}/{alias} version mismatch")
                if "path" in dep:
                    require((manifest.parent / dep["path"]).resolve()
                            == (workspace / MEMBERS[package_name]).resolve(),
                            f"internal dependency {name}/{alias} points outside its package")
                require(package_name != FFI or name == FFI,
                        f"publishable package {name} depends on unpublished FFI")

    config = read_toml(workspace / "release-plz.toml")
    defaults = config["workspace"]
    for flag in ("release", "release_always", "publish", "git_tag_enable", "git_release_enable",
                 "dependencies_update", "changelog_update"):
        require(defaults.get(flag) is False, f"preparation must keep workspace.{flag}=false")
    entries = config["package"]
    require(len(entries) == len(MEMBERS) and {p["name"] for p in entries} == set(MEMBERS),
            "release-plz package set differs from the release policy")
    for entry in entries:
        name = entry["name"]
        require(entry.get("release") is (name != FFI), f"release-plz release mismatch: {name}")
        require(entry.get("changelog_path") == "CHANGELOG.md", f"shared changelog mismatch: {name}")
        for flag in ("publish", "git_tag_enable", "git_release_enable"):
            require(entry.get(flag, False) is False, f"preparation enables {flag}: {name}")
        if name != FFI:
            require(entry.get("version_group") == "parking-lot", f"version_group mismatch: {name}")
        else:
            require("version_group" not in entry, "FFI must not join the release group")

    locked = read_toml(workspace / "Cargo.lock")["package"]
    for name in MEMBERS:
        local = [p for p in locked if p["name"] == name and "source" not in p]
        require(len(local) == 1 and local[0]["version"] == version,
                f"lockfile package {name} version mismatch")
    return version


def verify_tool(path: Path, pin: dict, cwd: Path) -> dict:
    require(path.is_file(), f"supply a local executable: {path}")
    observed = run([str(path), "--version"], cwd).strip()
    require(observed == pin["version_output"],
            f"tool version mismatch: expected {pin['version_output']!r}, observed {observed!r}")
    return {**pin, "path": str(path), "sha256": digest(path), "observed_version": observed}


def prepare(args: argparse.Namespace) -> None:
    source = args.source.resolve()
    output = args.output.resolve()
    require(not output.is_relative_to(source) and not source.is_relative_to(output),
            "output must be separate from the source checkout")
    require(SHA.fullmatch(args.candidate) is not None and SHA.fullmatch(args.base) is not None,
            "candidate and base must be full lowercase 40-character commit SHAs")
    require(VERSION.fullmatch(args.version) is not None, "version must be stable MAJOR.MINOR.PATCH")
    datetime.date.fromisoformat(args.date)
    require(git(source, "rev-parse", "--show-toplevel").strip() == str(source),
            "source must be the repository root")
    for revision in (args.candidate, args.base):
        require(git(source, "rev-parse", "--verify", f"{revision}^{{commit}}").strip() == revision,
                f"not an available commit: {revision}")
    git(source, "merge-base", "--is-ancestor", args.base, args.candidate)
    identity = {"candidate": args.candidate, "base": args.base, "version": args.version,
                "date": args.date, "source": str(source)}
    marker = output / "preparation-input.json"
    scratch = output / "workspace"
    if output.exists():
        require(marker.is_file() and json.loads(marker.read_text()) == identity,
                "output is not this exact preparation; choose a fresh output directory")
        require(scratch.is_dir(), "incomplete output; choose a fresh output directory")
    else:
        output.mkdir(parents=True)
        marker.write_text(json.dumps(identity, indent=2, sort_keys=True) + "\n", encoding="utf-8")
        git(source, "clone", "--no-hardlinks", "--no-checkout", "--", str(source), str(scratch))
        git(scratch, "checkout", "--detach", args.candidate)
        git(scratch, "remote", "remove", "origin")
    # A failed rerun must not leave an earlier success receipt usable.
    (output / "release-plan.json").unlink(missing_ok=True)
    require(git(scratch, "rev-parse", "HEAD").strip() == args.candidate,
            "scratch checkout no longer names the candidate")
    current = check(scratch)
    allowed_changes = {"Cargo.toml", "Cargo.lock", "CHANGELOG.md"}
    changed = set(git(scratch, "diff", "--name-only", args.candidate, "--").splitlines())
    require(changed <= allowed_changes, f"unexpected preparation inputs: {sorted(changed - allowed_changes)}")
    baseline_manifest = tomllib.loads(git(source, "show", f"{args.candidate}:Cargo.toml"))
    baseline_version = baseline_manifest["workspace"]["package"]["version"]
    require(current in {baseline_version, args.version}, "scratch version is not this preparation")
    require(tuple(map(int, args.version.split("."))) > tuple(map(int, baseline_version.split("."))),
            "release version must be greater than the candidate's current version")
    pins = read_toml(scratch / "release-tools.toml")["tools"]
    tools = {
        "release-plz": verify_tool(args.release_plz.resolve(), pins["release-plz"], scratch),
        "git-cliff": verify_tool(args.git_cliff.resolve(), pins["git-cliff"], scratch),
    }
    commands = []
    sections = []
    for name, directory in PACKAGES.items():
        paths = ["src/**", "Cargo.toml"] if directory == "." else [f"{directory}/**", "Cargo.toml"]
        argv = [tools["git-cliff"]["path"], "--config", str(scratch / "cliff.toml"),
                "--offline", "--no-exec", "--sort", "oldest"]
        for path in paths:
            argv.extend(["--include-path", path])
        argv.extend(["--", f"{args.base}..{args.candidate}"])
        notes = run(argv, scratch, commands).strip()
        if not notes:
            notes = "- No package-local commits; included in the lockstep release."
        sections.append(f"### {name}\n\n{notes}\n")
    historical = git(source, "show", f"{args.candidate}:CHANGELOG.md")
    # The pinned parser rejects duplicate versions across the entire changelog.
    # Give set-version only the generated current-version section to rename;
    # append the immutable historical text after its one-heading update.
    changelog = (f"# Changelog\n\n## [{current}] - {args.date}\n\n"
                 + "\n".join(sections))
    (scratch / "CHANGELOG.md").write_text(changelog, encoding="utf-8")
    run([tools["release-plz"]["path"], "set-version", args.version,
         "--manifest-path", str(scratch / "Cargo.toml"),
         "--config", str(scratch / "release-plz.toml")], scratch, commands)
    check(scratch, args.version)
    result_changelog = (scratch / "CHANGELOG.md").read_text(encoding="utf-8")
    require(result_changelog.startswith(f"# Changelog\n\n## [{args.version}] - {args.date}\n"),
            "release-plz did not prepare the requested changelog heading")
    (scratch / "CHANGELOG.md").write_text(
        result_changelog + "\n" + historical.removeprefix("# Changelog").lstrip(), encoding="utf-8")
    changed = set(git(scratch, "diff", "--name-only", args.candidate, "--").splitlines())
    require(changed <= allowed_changes, f"unexpected preparation writes: {sorted(changed - allowed_changes)}")
    files = sorted({"Cargo.toml", "Cargo.lock", "CHANGELOG.md", "release-plz.toml", "cliff.toml",
                    "release-tools.toml", *(f"{p}/Cargo.toml" for p in MEMBERS.values() if p != ".")})
    plan = {"schema_version": 1, "policy": "lockstep", **identity, "publish": False,
            "packages": [{"name": n, "version": args.version, "path": p} for n, p in PACKAGES.items()],
            "excluded": [FFI], "tools": tools,
            # Raw outputs can vary by a collaborator's elapsed time or first-run
            # update messages. Keep them in a separate invocation log.
            "commands": [{"argv": c["argv"], "cwd": c["cwd"]} for c in commands],
            "files_sha256": {p: digest(scratch / p) for p in files}}
    (output / "commands.json").write_text(json.dumps(commands, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (output / "release-plan.json").write_text(json.dumps(plan, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    print(f"PREPARED {args.version}: {len(PACKAGES)} publishable crates; FFI excluded; {output}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    checker = commands.add_parser("check", help="reject manifest/config/lockfile mismatches")
    checker.add_argument("--workspace", required=True, type=Path)
    checker.add_argument("--version")
    preparer = commands.add_parser("prepare", help="prepare only in a separate output clone")
    for name in ("source", "output", "release-plz", "git-cliff"):
        preparer.add_argument(f"--{name}", required=True, type=Path)
    for name in ("candidate", "base", "version", "date"):
        preparer.add_argument(f"--{name}", required=True)
    args = parser.parse_args()
    try:
        if args.command == "check":
            version = check(args.workspace, args.version)
            print(f"CONSISTENT {version}: seven publishable crates; FFI excluded")
        else:
            prepare(args)
    except (PreparationError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"release-preparation: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
