#!/usr/bin/env python3
"""OA-13 final-boundary real Node/Chromium runner; never installs a browser.

Run only after the coherent phase implementation set is complete. Browser
versions/paths are pre-provisioned environment inputs, not an installation task.
Build, exact nonzero discovery, real execution and expected defects are separate
oracles. Source/API pins and the complete test set live in the checked manifest.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import re
import shlex
import shutil
import signal
import subprocess
import sys
import tomllib


class AssuranceError(Exception):
    pass


class UnsupportedEnvironment(AssuranceError):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise AssuranceError(message)


def digest(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def run(args: argparse.Namespace) -> None:
    workspace = Path(__file__).resolve().parents[3]
    manifest_path = workspace / ".github/wasm-browser-assurance.json"
    manifest = json.loads(manifest_path.read_bytes())
    output = args.output.resolve()
    require(not output.exists() and not output.is_relative_to(workspace), "fresh external output directory required")
    output.mkdir(parents=True)
    environment = {key: value for key, value in os.environ.items()
                   if not key.startswith("WASM_BINDGEN_")
                   and not key.startswith(("GECKODRIVER", "SAFARIDRIVER", "CHROMEDRIVER", "MSEDGEDRIVER"))
                   and key not in {"NO_HEADLESS", "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS"}}
    environment.update(CARGO_TERM_COLOR="never", RUSTUP_AUTO_INSTALL="0", RUSTUP_TOOLCHAIN=args.toolchain,
                       CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="wasm-bindgen-test-runner",
                       WASM_BINDGEN_TEST_TIMEOUT="180", WASM_BINDGEN_TEST_DRIVER_TIMEOUT="30",
                       RUSTFLAGS="", RUSTDOCFLAGS="")
    command_index = 0
    commands: list[dict] = []

    def command(label: str, argv: list[str], timeout: int = 900) -> tuple[int, str]:
        nonlocal command_index
        command_index += 1
        print(f"argv: {shlex.join(argv)}", flush=True)
        log = output / f"{command_index:02}-{label}.txt"
        # Stream to disk so an external cancellation retains partial raw output.
        with log.open("w") as handle:
            try:
                with subprocess.Popen(argv, cwd=workspace, env=environment, stdout=handle,
                                      stderr=subprocess.STDOUT, text=True, start_new_session=True) as process:
                    try:
                        returncode = process.wait(timeout=timeout)
                    except subprocess.TimeoutExpired:
                        # Cancel only this invocation's owned process group;
                        # do not leave a runner/driver/browser after a watchdog.
                        try:
                            os.killpg(process.pid, signal.SIGTERM)
                            process.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
                        except ProcessLookupError:
                            pass
                        raise
            except subprocess.TimeoutExpired:
                commands.append({"argv": argv, "log": log.name, "status": "TIMEOUT"})
                (output / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
                raise AssuranceError(f"{label}: command timed out; not an expected-negative result") from None
        text = log.read_text()
        print(text, end="" if text.endswith("\n") else "\n", flush=True)
        print(f"exit: {returncode}", flush=True)
        commands.append({"argv": argv, "log": log.name, "exit": returncode})
        (output / "commands.json").write_text(json.dumps(commands, indent=2) + "\n")
        return returncode, text

    def installed_version(name: str, pattern: str) -> str:
        if shutil.which(name) is None:
            raise UnsupportedEnvironment(f"missing preinstalled {name}")
        rc, text = command(name.replace("/", "_") + "-version", [name, "--version"], timeout=30)
        match = re.search(pattern, text)
        require(rc == 0 and match is not None, f"{name}: version inspection failed")
        return match.group(1)

    if os.name != "posix":
        raise UnsupportedEnvironment("owned process-group cleanup requires a POSIX boundary runner")
    rc, candidate = command("candidate-before", ["git", "rev-parse", "HEAD"], timeout=30)
    require(rc == 0 and re.fullmatch(r"[0-9a-f]{40}\n?", candidate) is not None, "full candidate identity unavailable")
    rc, status = command("candidate-clean-before", ["git", "status", "--porcelain"], timeout=30)
    require(rc == 0 and not status.strip(), "clean frozen candidate checkout required")
    lock = tomllib.loads((workspace / "Cargo.lock").read_text())
    for name, key in (("wasm-bindgen", "wasm_bindgen"), ("wasm-bindgen-test", "wasm_bindgen_test")):
        found = [package["version"] for package in lock["package"] if package["name"] == name]
        require(found == [manifest[key]], f"{name}: manifest/lockfile pin mismatch")
    runner_version = installed_version("wasm-bindgen-test-runner", r"wasm-bindgen-test-runner (\S+)")
    require(runner_version == manifest["wasm_bindgen"], "installed runner does not match Cargo.lock")
    browser = args.mode != "node"
    versions = {"wasm_bindgen_test_runner": runner_version, "toolchain_selector": args.toolchain}
    if browser:
        if platform.system() != "Linux" or platform.machine() != "x86_64":
            raise UnsupportedEnvironment("browser receipt supports only pinned Linux x86_64 Chromium")
        require(args.toolchain == manifest["rust_toolchain"], "browser toolchain pin mismatch")
        for name, value in (("chrome", args.chrome), ("chromedriver", args.chromedriver)):
            if not value or not Path(value).is_absolute() or not os.access(value, os.X_OK):
                raise UnsupportedEnvironment(f"{name}: existing absolute executable path required")
            versions[name] = installed_version(value, r"\b(\d+\.\d+\.\d+\.\d+)\b")
            require(versions[name] == manifest["browser"]["version"], f"{name}: approved exact version mismatch")
        capabilities = output / "webdriver.json"
        capabilities.write_text(json.dumps({"goog:chromeOptions": {"binary": args.chrome}}) + "\n")
        environment.update(WASM_BINDGEN_USE_BROWSER="1", CHROMEDRIVER=args.chromedriver,
                           WASM_BINDGEN_TEST_WEBDRIVER_JSON=str(capabilities))
    else:
        versions["node"] = installed_version("node", r"v(\d+\.\d+\.\d+)")
        require(int(versions["node"].split(".")[0]) == manifest["node_major"], "Node major pin mismatch")
    negative = args.mode == "browser-negative"
    if negative:
        environment["RUSTFLAGS"] = f'--cfg parking_lot_negative_control="{manifest["negative_control"]["cfg"]}"'
    rc, _ = command("wasm-graph", ["bash", ".github/scripts/check-wasm-graph.sh"])
    require(rc == 0, "no-tokio WASM production graph gate failed")
    cargo = ["cargo", f"+{args.toolchain}"]
    for tool in ("cargo", "rustc"):
        rc, text = command(tool + "-identity", [tool, f"+{args.toolchain}", "--version", "--verbose"], timeout=30)
        require(rc == 0, f"{tool}: selected preinstalled toolchain unavailable")
        versions[tool] = text.strip()
    reports = []
    for target, expected in manifest["tests"].items():
        base = [*cargo, "test", "--locked", "--target", "wasm32-unknown-unknown",
                "-p", "prometheus-parking-lot-wasm", "--test", target]
        rc, _ = command(target + "-build", [*base, "--no-run"])
        require(rc == 0, f"{target}: build failure cannot satisfy a browser oracle")
        rc, discovered = command(target + "-discovery", [*base, "--", "--list"])
        names = [line.removesuffix(": test").split("::")[-1]
                 for line in discovered.splitlines() if line.endswith(": test")]
        require(rc == 0 and len(names) == len(expected) > 0 and set(names) == set(expected),
                f"{target}: exact nonzero exported WASM test discovery required")
        rc, executed = command(target + "-run", [*base, "--", "--nocapture"], timeout=360)
        if browser:
            require("Running headless tests in Chrome" in executed
                    and "OA13_BROWSER_CONTEXT document=9 user_agent=" in executed
                    and "HeadlessChrome/" in executed and "OA13_NODE_CONTEXT" not in executed,
                    f"{target}: actual headless browser context missing; Node cannot satisfy browser evidence")
        else:
            require("OA13_NODE_CONTEXT version=" in executed and "OA13_BROWSER_CONTEXT" not in executed,
                    f"{target}: actual Node context missing")
        outcomes = re.findall(r"^test (\S+) \.\.\. (ok|FAIL)$", executed, flags=re.MULTILINE)
        outcomes = [(name.split("::")[-1], status) for name, status in outcomes]
        require(len(outcomes) == len(expected) and {name for name, _ in outcomes} == set(expected),
                f"{target}: complete real execution results required")
        expected_failure = negative and target == manifest["negative_control"]["target"]
        failed = [name for name, status in outcomes if status == "FAIL"]
        if expected_failure:
            require(rc == 101 and failed == [manifest["negative_control"]["test"]]
                    and manifest["negative_control"]["diagnostic"] in executed
                    and f"test result: FAILED. {len(expected) - 1} passed; 1 failed; 0 ignored; 0 filtered out" in executed,
                    "budget-event control requires its named assertion, exactly one failure and all positive peers")
        else:
            require(rc == 0 and not failed
                    and f"test result: ok. {len(expected)} passed; 0 failed; 0 ignored; 0 filtered out" in executed,
                    f"{target}: real suite failed or did not fully execute")
        reports.append({"target": target, "discovered": names, "exit": rc,
                        "expected_negative": expected_failure, "outcomes": outcomes})
    rc, after = command("candidate-after", ["git", "rev-parse", "HEAD"], timeout=30)
    require(rc == 0 and after == candidate, "frozen candidate moved during browser assurance")
    rc, status = command("candidate-clean-after", ["git", "status", "--porcelain"], timeout=30)
    require(rc == 0 and not status.strip(), "candidate changed during browser assurance")
    contract_path = workspace / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json"
    contract = json.loads(contract_path.read_bytes())
    sources = [manifest_path, Path(__file__), workspace / ".github/workflows/ci.yml",
               workspace / "Cargo.lock", workspace / "rust-toolchain.toml", contract_path,
               *sorted((workspace / "crates/parking-lot-wasm/src").glob("*.rs")),
               *sorted((workspace / "crates/parking-lot-wasm/tests").rglob("*.rs"))]
    summary = {"status": "PASS", "suite": "OA-13", "mode": args.mode, "candidate": candidate.strip(),
               "contract_digest": contract["identity"]["contractDigest"], "versions": versions,
               "platform": platform.platform(), "machine": platform.machine(), "reports": reports,
               "manifest_sha256": digest(manifest_path),
               "sources_sha256": {str(path.relative_to(workspace)): digest(path) for path in sources},
               "selected_environment": {key: environment[key] for key in (
                   "RUSTFLAGS", "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER", "WASM_BINDGEN_TEST_TIMEOUT")},
               "browser_evidence": browser, "limit": "No durability or panic-containment claim; no installation performed"}
    if browser:
        summary["executables_sha256"] = {"chrome": digest(Path(args.chrome)), "chromedriver": digest(Path(args.chromedriver))}
    (output / "fixture-summary.json").write_text(json.dumps(summary, indent=2) + "\n")
    print(f"PASS: OA-13 {args.mode}: 21 real WASM tests classified; browser_evidence={browser}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("node", "browser", "browser-negative"), required=True)
    parser.add_argument("--toolchain", choices=("1.99.1", "1.95"), default="1.99.1")
    parser.add_argument("--chrome", default=os.environ.get("WASM_BROWSER_BIN"))
    parser.add_argument("--chromedriver", default=os.environ.get("CHROMEDRIVER"))
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        run(args)
    except UnsupportedEnvironment as error:
        print(f"BLOCKED: {error}", file=sys.stderr)
        return 2
    except (AssuranceError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
