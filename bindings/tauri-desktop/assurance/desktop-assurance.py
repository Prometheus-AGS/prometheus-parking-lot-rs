#!/usr/bin/env python3
"""OA-15 native shell builds and isolated, manually operated macOS GUI fixtures.

No command fabricates GUI approval. Trace classification is supporting evidence;
actual captures and attributable operator observations require independent review.
Nothing runs until these explicit commands are invoked at the final boundary.
"""

from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import subprocess
import sys
import tempfile
import tomllib
import urllib.parse
import uuid


WORKSPACE = Path(__file__).resolve().parents[3]
DESKTOP = WORKSPACE / "bindings/tauri-desktop"
PROTOCOL_PATH = DESKTOP / "assurance/protocol.json"
PROTOCOL = json.loads(PROTOCOL_PATH.read_bytes())
ENVIRONMENT = PROTOCOL["root_environment"]


class AssuranceError(Exception):
    pass


class Blocked(AssuranceError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssuranceError(message)


def now() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def sha(path: Path) -> str:
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def write_json(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def sources() -> dict[str, str]:
    paths = [WORKSPACE / name for name in (
        "Cargo.toml", "Cargo.lock", "rust-toolchain.toml", ".cargo/config.toml",
        ".github/workflows/platform.yml",
        ".kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json")]
    paths.extend(path for path in DESKTOP.rglob("*") if path.is_file()
                 and not {"target", "gen", "__pycache__"}.intersection(path.relative_to(DESKTOP).parts))
    for crate in ("parking-lot-core", "parking-lot-embedded", "parking-lot-ffi"):
        directory = WORKSPACE / "crates" / crate
        paths.append(directory / "Cargo.toml")
        paths.extend((directory / "src").rglob("*.rs"))
    return {str(path.relative_to(WORKSPACE)): sha(path) for path in sorted(set(paths))}


def git(*args: str) -> str:
    result = subprocess.run(["git", *args], cwd=WORKSPACE, text=True, capture_output=True, check=False)
    require(result.returncode == 0, "candidate Git inspection failed")
    return result.stdout.strip()


def candidate() -> str:
    identity = git("rev-parse", "HEAD")
    require(re.fullmatch(r"[0-9a-f]{40}", identity) is not None, "full frozen candidate required")
    require(git("status", "--porcelain") == "", "clean frozen candidate checkout required")
    return identity


def external_output(path: Path) -> Path:
    result = path.resolve()
    require(not result.exists() and not result.is_relative_to(WORKSPACE), "fresh external output required")
    result.mkdir(parents=True)
    return result


def build(args: argparse.Namespace) -> int:
    output = external_output(args.output)
    identity = candidate()
    system = platform.system()
    if system not in PROTOCOL["build_cases"]:
        raise Blocked("native shell builds require macOS, Linux or Windows")
    locked = tomllib.loads((DESKTOP / "Cargo.lock").read_text())
    for name, version in PROTOCOL["locked_versions"].items():
        require([item["version"] for item in locked["package"] if item["name"] == name] == [version],
                f"{name}: adopted desktop lockfile pin mismatch")
    environment = {key: value for key, value in os.environ.items()
                   if key not in {ENVIRONMENT, "CARGO_BUILD_TARGET", "CARGO_ENCODED_RUSTFLAGS", "RUSTFLAGS"}}
    environment.update(RUSTUP_AUTO_INSTALL="0", RUSTUP_TOOLCHAIN=PROTOCOL["rust_toolchain"],
                       CARGO_TERM_COLOR="never", RUSTFLAGS="")
    commands = []

    def command(label: str, argv: list[str], timeout: int = 1800) -> tuple[int, str]:
        print(f"argv: {shlex.join(argv)}", flush=True)
        log = output / f"{len(commands) + 1:02}-{label}.txt"
        with log.open("w") as handle:
            try:
                result = subprocess.run(argv, cwd=WORKSPACE, env=environment, stdout=handle,
                                        stderr=subprocess.STDOUT, text=True, timeout=timeout, check=False)
            except subprocess.TimeoutExpired:
                commands.append({"argv": argv, "log": log.name, "status": "TIMEOUT"})
                write_json(output / "commands.json", commands)
                raise AssuranceError(f"{label}: timeout is not a passing build or GUI result") from None
        text = log.read_text()
        print(text, end="" if text.endswith("\n") else "\n", flush=True)
        print(f"exit: {result.returncode}", flush=True)
        commands.append({"argv": argv, "exit": result.returncode, "log": log.name})
        write_json(output / "commands.json", commands)
        return result.returncode, text

    if shutil.which("cargo") is None:
        raise Blocked("preinstalled Rust toolchain is unavailable")
    prerequisites = {}
    if system == "Linux":
        for tool in ("pkg-config", "cc"):
            if shutil.which(tool) is None:
                raise Blocked(f"preinstalled Linux build prerequisite missing: {tool}")
        for library in PROTOCOL["linux_pkg_config"]:
            rc, text = command("prerequisite-" + library, ["pkg-config", "--modversion", library], timeout=30)
            if rc != 0:
                raise Blocked(f"preinstalled Linux development library unavailable: {library}")
            prerequisites[library] = text.strip()
    if system == "Darwin":
        for label, argv in (("xcode", ["xcode-select", "-p"]), ("sdk", ["xcrun", "--show-sdk-version"])):
            rc, text = command(label, argv, timeout=30)
            if rc != 0:
                raise Blocked("preinstalled Apple SDK/command-line tools unavailable")
            prerequisites[label] = text.strip()
    versions = {}
    for tool in ("cargo", "rustc"):
        rc, text = command(tool + "-identity", [tool, "+" + PROTOCOL["rust_toolchain"], "--version", "--verbose"], timeout=30)
        if rc != 0:
            raise Blocked("selected preinstalled Rust toolchain is unavailable")
        versions[tool] = text.strip()
    before = sources()
    argv = ["cargo", "+" + PROTOCOL["rust_toolchain"], "build", "--manifest-path",
            "bindings/tauri-desktop/Cargo.toml", "--workspace", "--bins", "--locked",
            "--message-format=json-render-diagnostics"]
    rc, text = command("actual-tauri-shell-build", argv)
    require(rc == 0, "real native Tauri shell build failed")
    artifacts = []
    for line in text.splitlines():
        try:
            item = json.loads(line)
        except ValueError:
            continue
        if (isinstance(item, dict) and item.get("reason") == "compiler-artifact"
                and item.get("target", {}).get("name") == "parking-lot-tauri-example"
                and "bin" in item.get("target", {}).get("kind", []) and item.get("executable")):
            artifacts.append(Path(item["executable"]).resolve())
    require(len(set(artifacts)) == 1 and artifacts[0].is_file(), "actual Tauri binary artifact was not produced")
    require(candidate() == identity and sources() == before, "candidate/source changed during shell build")
    executable = artifacts[0]
    contract = json.loads((WORKSPACE / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json").read_bytes())
    receipt = {"schema_version": 1, "case_id": PROTOCOL["build_cases"][system], "status": "PASS",
               "runtime_status": "NOT_EXERCISED", "candidate": identity,
               "contract_digest": contract["identity"]["contractDigest"], "os": system,
               "platform": platform.platform(), "machine": platform.machine(), "at": now(),
               "python": sys.version,
               "argv": argv, "exit": rc, "versions": versions, "prerequisites": prerequisites,
               "locked_versions": PROTOCOL["locked_versions"], "sources_sha256": before,
               "binary": {"path": str(executable), "sha256": sha(executable)},
               "limit": "Real native shell build only; not GUI launch/restart evidence"}
    write_json(output / "build-receipt.json", receipt)
    print(f"BUILD_PASS: {receipt['case_id']}; GUI runtime NOT_EXERCISED")
    return 0


def macos_gui() -> None:
    if platform.system() != PROTOCOL["required_gui_os"]:
        raise Blocked("manual GUI fixture is scoped to an approved logged-in macOS display")


def prepare(args: argparse.Namespace) -> int:
    macos_gui()
    receipt = json.loads(args.build_receipt.read_bytes())
    require(receipt.get("status") == "PASS" and receipt.get("case_id") == "OA15-BUILD-MACOS"
            and receipt.get("os") == "Darwin" and receipt.get("candidate") == candidate(),
            "actual matching native macOS shell build receipt required")
    require(receipt.get("sources_sha256") == sources(), "build receipt no longer matches candidate sources")
    binary = Path(receipt["binary"]["path"])
    require(binary.is_absolute() and binary.is_file() and sha(binary) == receipt["binary"]["sha256"],
            "built shell executable is missing or changed")
    parent = args.parent.resolve()
    require(parent.is_dir() and not parent.is_relative_to(WORKSPACE), "existing external fixture parent required")
    root = Path(tempfile.mkdtemp(prefix=PROTOCOL["root_prefix"], dir=parent)).resolve()
    (root / "store").mkdir(mode=0o700)
    nonce = uuid.uuid4().hex
    (root / "owner.txt").write_text(f"{PROTOCOL['owner_marker']}\n{nonce}\n{receipt['candidate']}\n")
    fixture = {"schema_version": 1, "case_id": PROTOCOL["gui_cases"][args.case], "case": args.case,
               "candidate": receipt["candidate"], "contract_digest": receipt["contract_digest"],
               "nonce": nonce, "root": str(root), "input": "oa15-" + nonce,
               "binary": receipt["binary"], "sources_sha256": receipt["sources_sha256"],
               "build_receipt_sha256": sha(args.build_receipt), "prepared_at": now()}
    write_json(root / "fixture.json", fixture)
    write_json(root / "launches.json", [])
    (root / "manual-observation.md").write_text(
        "# OA-15 manual GUI observation — NOT_EXERCISED\n\n"
        f"Case: {fixture['case_id']}\nCandidate: {fixture['candidate']}\nRoot: {root}\n"
        f"Exact non-secret textarea input: {fixture['input']}\n\n"
        "Observer identity, approved role and actual observation timestamps: NOT_EXERCISED\n"
        "macOS version/build, machine, display/session and actual app/binary identity: NOT_EXERCISED\n"
        "First visible main-window Add to queue / Process next, no collection, full quit: NOT_EXERCISED\n"
        "Second actual process launch / Collect results / visible expected or missing result: NOT_EXERCISED\n"
        "Capture paths/digests and independent reviewer reference: NOT_EXERCISED\n\n"
        "Replace pending entries only with actual attributable observations. Trace classification alone "
        "cannot establish GUI authority. No caller-approved JSON is accepted as GUI certification.\n")
    print(json.dumps({"root": str(root), "case_id": fixture["case_id"], "input": fixture["input"],
                      "status": "PREPARED_ONLY", "gui_status": "NOT_EXERCISED"}, sort_keys=True))
    return 0


def load_fixture(value: Path) -> tuple[Path, dict]:
    root = value
    require(root.is_absolute() and root.resolve() == root and root.name.startswith(PROTOCOL["root_prefix"]),
            "canonical fixture-owned root required")
    require(not root.is_relative_to(WORKSPACE) and not root.is_symlink(), "fixture must be external and not a symlink")
    for name in ("owner.txt", "fixture.json", "launches.json"):
        require((root / name).is_file() and not (root / name).is_symlink(), "regular owned fixture metadata required")
    fixture = json.loads((root / "fixture.json").read_bytes())
    expected = f"{PROTOCOL['owner_marker']}\n{fixture['nonce']}\n{fixture['candidate']}\n"
    require((root / "owner.txt").read_text() == expected and fixture["root"] == str(root), "fixture owner marker mismatch")
    require(fixture["case"] in PROTOCOL["gui_cases"]
            and fixture["case_id"] == PROTOCOL["gui_cases"][fixture["case"]], "fixture case identity mismatch")
    require(fixture["candidate"] == candidate() and fixture["sources_sha256"] == sources(), "frozen fixture source changed")
    require((root / "store").is_dir() and (root / "store").resolve() == root / "store", "store directory redirected")
    return root, fixture


def store_file(root: Path, *, required: bool) -> Path:
    path = root / PROTOCOL["store_relative_path"]
    require(not path.is_symlink(), "owned store must not be a symlink")
    if required:
        require(path.is_file() and path.stat().st_size > 0, "existing nonempty owned store required")
    return path


def launch(args: argparse.Namespace) -> int:
    macos_gui()
    root, fixture = load_fixture(args.root)
    require(not (root / "active-launch.json").exists(), "previous owned application process is still active or unresolved")
    launches = json.loads((root / "launches.json").read_bytes())
    require(len(launches) < 2 and all(item["exit"] == 0 for item in launches), "exactly two successful closed GUI sessions required")
    binary = Path(fixture["binary"]["path"])
    require(binary.is_file() and sha(binary) == fixture["binary"]["sha256"], "built binary changed")
    index = len(launches) + 1
    store = store_file(root, required=False)
    record = {"launch": index, "argv": [str(binary)], "started_at": now(),
              "store_before_sha256": sha(store) if store.is_file() else None,
              "binary_sha256": fixture["binary"]["sha256"], "os": platform.platform()}
    environment = dict(os.environ)
    environment[ENVIRONMENT] = str(root)
    lock = root / "active-launch.json"
    with lock.open("x") as handle:
        json.dump({"wrapper_pid": os.getpid(), "launch": index}, handle)
    print(f"Launch {index}: operate only this visible test-app window; quit fully when instructed.\n"
          f"Exact non-secret input: {fixture['input']}\nargv: {shlex.join(record['argv'])}", flush=True)
    try:
        with (root / f"launch-{index}.txt").open("w") as log:
            with subprocess.Popen(record["argv"], cwd=binary.parent, env=environment,
                                  stdout=log, stderr=subprocess.STDOUT, text=True) as process:
                record["pid"] = process.pid
                write_json(lock, {"wrapper_pid": os.getpid(), "app_pid": process.pid, "launch": index})
                try:
                    record["exit"] = process.wait()
                except KeyboardInterrupt:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()
                    raise AssuranceError("interrupted GUI launch is not a completed manual observation") from None
        record["ended_at"] = now()
        record["store_after_sha256"] = sha(store) if store.is_file() else None
        launches.append(record)
        write_json(root / "launches.json", launches)
    finally:
        # Reached only after this Popen context waited for the owned app.
        lock.unlink(missing_ok=True)
    require(record["exit"] == 0, "actual GUI application process failed; inspect its raw launch log")
    print(f"APP_EXIT: launch={index} pid={record['pid']} exit={record['exit']}")
    return 0


def trace(root: Path) -> list[dict]:
    path = root / "app-trace.tsv"
    require(path.is_file() and not path.is_symlink(), "real Tauri command/page-load trace missing")
    records = []
    for line in path.read_text().splitlines():
        fields = line.split("\t")
        require(len(fields) == 7, "malformed Tauri trace record")
        timestamp, session, pid, kind, window, task, data = fields
        require(timestamp.isdecimal() and pid.isdecimal() and window == "main", "wrong actual main-webview trace identity")
        records.append({"timestamp_ms": int(timestamp), "session": session, "pid": int(pid), "kind": kind,
                        "task": None if task == "-" else int(task), "data": bytes.fromhex(data).decode()})
    return records


def first_session(root: Path, fixture: dict, launches: list, records: list) -> tuple[int, list, list]:
    require(1 <= len(launches) <= 2 and all(item["exit"] == 0 for item in launches), "actual closed application launch receipts required")
    starts = [item for item in records if item["kind"] == "launch"]
    require(len(starts) == len(launches), "actual process/trace launch count mismatch")
    for start, launch_record in zip(starts, launches, strict=True):
        require(start["pid"] == launch_record["pid"] and start["data"] == str(store_file(root, required=False)),
                "Tauri used a different process/store than the owned fixture")
        pages = [item for item in records if item["session"] == start["session"] and item["kind"] == "page-loaded"]
        require(bool(pages) and all(urllib.parse.urlsplit(item["data"]).scheme in {"tauri", "http", "https"}
                                   and urllib.parse.urlsplit(item["data"]).hostname in {"localhost", "tauri.localhost"}
                                   for item in pages), "real incumbent local main-page load missing")
    first = [item for item in records if item["session"] == starts[0]["session"]]
    submitted = [item for item in first if item["kind"] == "submit"]
    processed = [item for item in first if item["kind"] == "process"]
    require(len(submitted) == len(processed) == 1 and submitted[0]["data"] == fixture["input"]
            and submitted[0]["task"] is not None and submitted[0]["task"] == processed[0]["task"]
            and first.index(submitted[0]) < first.index(processed[0]), "actual GUI submit/process input and ID must agree")
    require(not any(item["kind"] in {"collect", "result"} for item in first), "result was collected before app restart")
    require(launches[0]["store_before_sha256"] is None and bool(launches[0]["store_after_sha256"]), "first GUI session did not create its own durable store")
    return submitted[0]["task"], starts, first


def delete_store(args: argparse.Namespace) -> int:
    root, fixture = load_fixture(args.root)
    require(fixture["case"] == "delete-store-control", "deletion is allowed only in the explicitly selected control fixture")
    require(not (root / "active-launch.json").exists(), "refuse deletion while an owned application process is active/unresolved")
    require(not (root / "store-deletion.json").exists(), "owned-store deletion was already attempted")
    launches = json.loads((root / "launches.json").read_bytes())
    require(len(launches) == 1, "delete only between the first and second actual GUI processes")
    first_session(root, fixture, launches, trace(root))
    store = store_file(root, required=True)
    before = sha(store)
    require(before == launches[0]["store_after_sha256"], "owned store changed after app exit")
    deletion = {"case_id": fixture["case_id"], "candidate": fixture["candidate"], "root": str(root),
                "path": str(store), "before_sha256": before, "at": now(), "status": "INTENT_RECORDED"}
    write_json(root / "store-deletion.json", deletion)
    # Exactly one owned regular file. No recursive deletion, glob, user app-data
    # resolution, profile deletion, external path or queue-service substitute.
    store.unlink()
    deletion["status"] = "OWNED_STORE_REMOVED"
    write_json(root / "store-deletion.json", deletion)
    print(f"CONTROL_APPLIED: deleted only {store}; restart and collect in the actual GUI")
    return 0


def capture(args: argparse.Namespace) -> int:
    root, _ = load_fixture(args.root)
    source = args.image.resolve()
    require(source.is_file() and source.stat().st_size <= 20 * 1024 * 1024, "actual app-only PNG/JPEG capture required")
    header = source.read_bytes()[:8]
    require(header.startswith(b"\x89PNG\r\n\x1a\n") or header.startswith(b"\xff\xd8\xff"), "PNG/JPEG capture bytes required")
    directory = root / "captures"
    directory.mkdir(exist_ok=True)
    require(directory.resolve() == directory and not directory.is_symlink(), "capture directory redirected")
    target = directory / (args.stage + (".png" if header.startswith(b"\x89PNG") else ".jpg"))
    require(not target.exists(), "do not overwrite a prior observation capture")
    shutil.copyfile(source, target)
    index = root / "captures.json"
    captures = json.loads(index.read_bytes()) if index.exists() else {}
    require(args.stage not in captures, "capture stage already recorded")
    captures[args.stage] = {"path": str(target), "sha256": sha(target), "attached_at": now()}
    write_json(index, captures)
    print("CAPTURE_ATTACHED: content/actual operator observation still requires independent review")
    return 0


def classify(args: argparse.Namespace) -> int:
    root, fixture = load_fixture(args.root)
    require(not (root / "active-launch.json").exists(), "both actual application processes must have exited")
    launches = json.loads((root / "launches.json").read_bytes())
    require(len(launches) == 2, "two real GUI process launch/exit receipts required")
    records = trace(root)
    task, starts, _ = first_session(root, fixture, launches, records)
    require(starts[0]["session"] != starts[1]["session"], "app restart must create a distinct process session")
    second = [item for item in records if item["session"] == starts[1]["session"]]
    require(not any(item["kind"] in {"submit", "process"} for item in second), "second launch must retrieve the prior result without resubmitting")
    collects = [item for item in second if item["kind"] == "collect"]
    require(bool(collects), "real main-webview collect command missing after restart")
    captures_path = root / "captures.json"
    if not captures_path.is_file():
        raise Blocked("actual pre/post-restart GUI captures and attributable observer receipt are pending")
    captures = json.loads(captures_path.read_bytes())
    require(set(captures) == {"first-processed", "second-collected"}, "both manual GUI capture stages required")
    for entry in captures.values():
        path = Path(entry["path"])
        require(path.parent == root / "captures" and path.is_file() and not path.is_symlink()
                and sha(path) == entry["sha256"], "manual capture content/digest mismatch")
    negative = fixture["case"] == "delete-store-control"
    deletion_path = root / "store-deletion.json"
    if negative:
        require(deletion_path.is_file(), "explicit owned-store deletion receipt required")
        deletion = json.loads(deletion_path.read_bytes())
        require(deletion["status"] == "OWNED_STORE_REMOVED" and deletion["path"] == str(store_file(root, required=False))
                and deletion["before_sha256"] == launches[0]["store_after_sha256"]
                and launches[1]["store_before_sha256"] is None, "control did not delete the exact closed test-app store")
    else:
        require(not deletion_path.exists() and launches[1]["store_before_sha256"] == launches[0]["store_after_sha256"],
                "positive store changed or disappeared between app processes")
    returned = [item for item in second if item["kind"] == "result" and item["task"] == task]
    missing = not returned
    oracle = {"case_id": fixture["case_id"], "candidate": fixture["candidate"],
              "contract_digest": fixture["contract_digest"], "task_id": task, "at": now(),
              "trace_sha256": sha(root / "app-trace.tsv"), "launches_sha256": sha(root / "launches.json"),
              "binary_sha256": fixture["binary"]["sha256"], "captures": captures,
              "trace_oracle": "FAIL" if missing else "PASS", "expected_negative": negative,
              "classifier": PROTOCOL["negative_classifier"] if missing else "OA15_PERSISTED_RESULT_RETRIEVED",
              "gui_certification": "PENDING_INDEPENDENT_OPERATOR_OBSERVATION_REVIEW",
              "limit": "Backend traces/capture hashes do not prove visual content or actual operator authority"}
    if missing:
        require(all(item["data"] == "0" for item in collects)
                and not any(item["kind"] == "result" for item in second), "missing task is not a clean empty retrieval control")
        write_json(root / "trace-oracle.json", oracle)
        print(f"FAIL: {PROTOCOL['negative_classifier']}; expected_negative={negative}; GUI review pending")
        return 1
    require(not negative, "owned-store deletion control was not detected")
    require(len(returned) == 1 and returned[0]["data"] == fixture["input"].upper()
            and [item["data"] for item in collects] == ["1", "0"], "exact persisted result/once-only retrieval oracle failed")
    write_json(root / "trace-oracle.json", oracle)
    print("TRACE_ORACLE_PASS: actual Tauri IPC restart retrieval; GUI certification requires independent manual review")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    build_parser = commands.add_parser("build")
    build_parser.add_argument("--output", type=Path, required=True)
    prepare_parser = commands.add_parser("prepare")
    prepare_parser.add_argument("--case", choices=tuple(PROTOCOL["gui_cases"]), required=True)
    prepare_parser.add_argument("--build-receipt", type=Path, required=True)
    prepare_parser.add_argument("--parent", type=Path, required=True)
    for name in ("launch", "delete-store", "classify", "capture"):
        command_parser = commands.add_parser(name)
        command_parser.add_argument("--root", type=Path, required=True)
        if name == "capture":
            command_parser.add_argument("--stage", choices=("first-processed", "second-collected"), required=True)
            command_parser.add_argument("--image", type=Path, required=True)
    args = parser.parse_args()
    try:
        return globals()[args.command.replace("-", "_")](args)
    except Blocked as error:
        print(f"BLOCKED: {error}", file=sys.stderr)
        return 2
    except (AssuranceError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
