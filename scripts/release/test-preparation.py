#!/usr/bin/env python3
"""OA-02 final-boundary fixture: real tools and the real seven-crate workspace.

This intentionally runs release-plz (including its serialized Cargo metadata /
lockfile collaborator) and git-cliff. It is not a per-edit unit suite. Supply the
same frozen inputs as prepare.py; no mocks or registry publication are used.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import tomllib


sys.dont_write_bytecode = True
sys.path.insert(0, str(Path(__file__).resolve().parents[2] / ".github/scripts"))
from release_evidence import ReleaseError, git, validate_preparation  # noqa: E402

SCRIPT = Path(__file__).with_name("prepare.py")
EXPECTED = [
    "prometheus-parking-lot-core",
    "prometheus-parking-lot-runtime-tokio",
    "prometheus-parking-lot-pg",
    "prometheus-parking-lot-embedded",
    "prometheus-parking-lot-iggy",
    "prometheus-parking-lot-wasm",
    "prometheus_parking_lot",
]


def invoke(argv: list[str], output: Path, name: str, failure: str | None = None) -> None:
    result = subprocess.run([sys.executable, str(SCRIPT), *argv], capture_output=True,
                            text=True, check=False)
    log = {"argv": [sys.executable, str(SCRIPT), *argv], "exit": result.returncode,
           "stdout": result.stdout, "stderr": result.stderr}
    (output / f"{name}.json").write_text(json.dumps(log, indent=2) + "\n", encoding="utf-8")
    if failure is None:
        if result.returncode != 0:
            raise AssertionError(f"{name}: {result.stderr}")
    elif result.returncode == 0 or failure not in result.stderr:
        raise AssertionError(f"{name}: expected rejection {failure!r}, got {log!r}")


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def replace_once(path: Path, old: str, new: str) -> None:
    content = path.read_text(encoding="utf-8")
    if old not in content:
        raise AssertionError(f"fixture anchor missing in {path}: {old!r}")
    path.write_text(content.replace(old, new, 1), encoding="utf-8")


def compare_final_preparation(final: Path, original: dict, reproduced: dict, workspace: Path) -> None:
    # Absolute source/tool/scratch argv paths are invocation provenance, not
    # portable outputs. Never rewrite the original plan to hide their difference.
    fields = ("schema_version", "policy", "candidate", "base", "version", "date", "publish",
              "packages", "excluded", "files_sha256")
    if any(original.get(key) != reproduced.get(key) for key in fields):
        raise AssertionError("OA-02:final-preparation-mismatch: reproduced plan differs")
    original_tools = {name: {k: v for k, v in tool.items() if k != "path"}
                      for name, tool in original["tools"].items()}
    reproduced_tools = {name: {k: v for k, v in tool.items() if k != "path"}
                        for name, tool in reproduced["tools"].items()}
    if original_tools != reproduced_tools:
        raise AssertionError("OA-02:final-preparation-mismatch: tool identities differ")
    for name, expected in original["files_sha256"].items():
        if sha(final / name) != expected or sha(workspace / name) != expected:
            raise AssertionError("OA-02:final-preparation-mismatch: prepared/final bytes differ: " + name)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ("source", "output", "release-plz", "git-cliff"):
        parser.add_argument(f"--{name}", type=Path, required=True)
    for name in ("candidate", "base", "version", "date"):
        parser.add_argument(f"--{name}", required=True)
    parser.add_argument("--final-candidate", help="final prepared commit; --candidate remains the pre-bump source")
    parser.add_argument("--preparation-plan", type=Path, help="exact original plan, never reconstructed")
    args = parser.parse_args()
    if bool(args.final_candidate) != bool(args.preparation_plan):
        parser.error("--final-candidate and --preparation-plan must be supplied together")
    evidence = args.output.resolve()
    evidence.mkdir(parents=True, exist_ok=False)
    source = args.source.resolve()
    source_files = [source / "Cargo.toml", source / "Cargo.lock", source / "CHANGELOG.md"]
    original = {str(p): sha(p) for p in source_files}
    bound_bytes = args.preparation_plan.read_bytes() if args.preparation_plan else None
    bound_plan = json.loads(bound_bytes) if bound_bytes is not None else None
    if bound_plan is not None:
        if (bound_plan.get("candidate"), bound_plan.get("base"), bound_plan.get("date"), bound_plan.get("version")) != (
                args.candidate, args.base, args.date, args.version):
            raise AssertionError("OA-02:final-preparation-mismatch: original plan/input binding differs")
        if args.candidate == args.final_candidate or git(source, "rev-parse", "HEAD") != args.final_candidate:
            raise AssertionError("OA-02:final-preparation-mismatch: source ancestor/final identity differs")
        # Incumbent validator proves source ancestry and exact final metadata.
        validate_preparation(source, bound_plan, args.final_candidate, args.version)
        (evidence / "original-release-plan.json").write_bytes(bound_bytes)
    parameters = ["--source", str(source), "--candidate", args.candidate,
                  "--base", args.base, "--version", args.version, "--date", args.date,
                  "--release-plz", str(args.release_plz.resolve()),
                  "--git-cliff", str(args.git_cliff.resolve())]
    # Keep the actual successful prepared candidate and receipts for the boundary
    # runner. Only the disposable negative-control copies use TemporaryDirectory.
    prepared = evidence / "prepared"
    argv = ["prepare", *parameters, "--output", str(prepared)]
    invoke(argv, evidence, "first-preparation")
    shutil.copyfile(prepared / "commands.json", evidence / "first-tool-commands.json")
    receipt_path = prepared / "release-plan.json"
    receipt_bytes = receipt_path.read_bytes()
    receipt = json.loads(receipt_bytes)
    if [p["name"] for p in receipt["packages"]] != EXPECTED:
        raise AssertionError("preparation did not select exactly the seven publishable crates")
    if receipt["excluded"] != ["prometheus-parking-lot-ffi"] or receipt["publish"] is not False:
        raise AssertionError("FFI exclusion or nonpublishing policy changed")
    if any(p["version"] != args.version for p in receipt["packages"]):
        raise AssertionError("prepared package versions differ")
    workspace = prepared / "workspace"
    first_files = {p: sha(workspace / p) for p in receipt["files_sha256"]}
    changelog = (workspace / "CHANGELOG.md").read_text(encoding="utf-8")
    section = changelog.split("\n## [", 2)[1]
    headings = [line.removeprefix("### ") for line in section.splitlines() if line.startswith("### ")]
    if headings != EXPECTED or "### prometheus-parking-lot-ffi" in section:
        raise AssertionError("release changelog sections do not match the seven-crate policy")
    invoke(argv, evidence, "second-preparation")
    shutil.copyfile(prepared / "commands.json", evidence / "second-tool-commands.json")
    if receipt_path.read_bytes() != receipt_bytes:
        raise AssertionError("second preparation changed the deterministic release plan")
    if {p: sha(workspace / p) for p in first_files} != first_files:
        raise AssertionError("second preparation changed a manifest, lockfile, config or changelog")

    if bound_plan is not None:
        compare_final_preparation(source, bound_plan, receipt, workspace)
        mismatched = copy.deepcopy(receipt)
        mismatched["files_sha256"]["Cargo.toml"] = "0" * 64
        try:
            compare_final_preparation(source, bound_plan, mismatched, workspace)
        except AssertionError as error:
            if "OA-02:final-preparation-mismatch" not in str(error):
                raise
            (evidence / "final-plan-mismatch-control.json").write_text(json.dumps({
                "expected_rejection": str(error), "original_plan_sha256": sha(args.preparation_plan),
                "candidate": args.final_candidate, "preparation_source": args.candidate}, indent=2) + "\n")
        else:
            raise AssertionError("OA-02:final-preparation-mismatch control was accepted")
        # The shared production validator must also reject an altered plan even
        # when its package list still looks valid; never substitute a rebuilt plan.
        altered = copy.deepcopy(bound_plan)
        altered["files_sha256"]["Cargo.toml"] = "0" * 64
        try:
            validate_preparation(source, altered, args.final_candidate, args.version)
        except ReleaseError as error:
            if "content differs from candidate" not in str(error):
                raise
            (evidence / "original-plan-mismatch-control.json").write_text(json.dumps({
                "expected_rejection": str(error), "candidate": args.final_candidate}, indent=2) + "\n")
        else:
            raise AssertionError("OA-02:original-plan-mismatch control was accepted")

    with tempfile.TemporaryDirectory(prefix="parking-lot-release-controls-") as temporary:
        controls = Path(temporary)
        # Real prepared clones, with the actual production guard. A compiler,
        # tool-availability or syntax failure cannot satisfy these diagnostics.
        cases = [
            ("dependency-version", "Cargo.toml",
             f'path = "crates/parking-lot-core", version = "{args.version}"',
             'path = "crates/parking-lot-core", version = "99.0.0"',
             "internal dependency prometheus-parking-lot-core version mismatch"),
            ("package-version", "crates/parking-lot-pg/Cargo.toml",
             "version.workspace = true", 'version = "99.0.0"',
             "package prometheus-parking-lot-pg must inherit workspace version"),
            ("ffi-publish", "crates/parking-lot-ffi/Cargo.toml",
             "publish = false", "publish = true",
             "publish eligibility mismatch: prometheus-parking-lot-ffi"),
            ("lockfile-version", "Cargo.lock",
             f'name = "prometheus-parking-lot-pg"\nversion = "{args.version}"',
             'name = "prometheus-parking-lot-pg"\nversion = "99.0.0"',
             "lockfile package prometheus-parking-lot-pg version mismatch"),
            ("version-group", "release-plz.toml",
             'version_group = "parking-lot"', 'version_group = "independent"',
             "version_group mismatch: prometheus-parking-lot-core"),
        ]
        for name, file, old, new, diagnostic in cases:
            control = controls / name
            shutil.copytree(prepared, control, ignore=shutil.ignore_patterns("target"))
            replace_once(control / "workspace" / file, old, new)
            modified = sha(control / "workspace" / file)
            invoke(["prepare", *parameters, "--output", str(control)], evidence, name, diagnostic)
            if sha(control / "workspace" / file) != modified:
                raise AssertionError(f"{name}: preparation silently repaired its negative control")
            if (control / "release-plan.json").exists():
                raise AssertionError(f"{name}: failed preparation retained a success receipt")

    if {str(p): sha(p) for p in source_files} != original:
        raise AssertionError("preparation modified the source checkout")
    ffi = tomllib.loads((workspace / "crates/parking-lot-ffi/Cargo.toml").read_text())
    if ffi["package"]["publish"] is not False:
        raise AssertionError("FFI became publishable")
    if bound_plan is not None:
        if args.preparation_plan.read_bytes() != bound_bytes:
            raise AssertionError("OA-02:original-plan-mutated")
        compare_final_preparation(source, bound_plan, json.loads(receipt_path.read_bytes()), workspace)
        (evidence / "final-candidate-binding.json").write_text(json.dumps({
            "candidate": args.final_candidate, "preparation_source": args.candidate,
            "original_plan_sha256": sha(args.preparation_plan),
            "reproduced_plan_sha256": sha(receipt_path), "files_sha256": first_files}, indent=2) + "\n")
        print("PASS OA-02: original plan and reproduced ancestor outputs match final candidate; mismatch controls rejected")
    print("PASS OA-02: seven crate versions/dependencies/changelog; FFI excluded; second run unchanged")
    print("PASS OA-02 controls: dependency, package, lockfile, FFI and version-group mismatches rejected")
    print(f"Evidence: {evidence}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
