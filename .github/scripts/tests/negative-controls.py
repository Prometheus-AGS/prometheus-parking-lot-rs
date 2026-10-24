#!/usr/bin/env python3
"""OA-12 final-boundary runner. Authoring this file is not execution evidence.

Every Cargo operation is sequential. A clean build, exact test discovery, normal
execution, designated mutant assertion and unaffected mutant companion are
different receipts. An exit code alone is never a negative-control oracle.
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
import uuid

ROOT = Path(__file__).resolve().parents[3]
MANIFEST = ROOT / ".github/negative-controls.json"
CONTRACT = ROOT / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json"
RAW_START = re.compile(r'r(#+)?"')
CHAR_START = re.compile(r"'(?:\\(?:x[0-9A-Fa-f]{2}|u\{[0-9A-Fa-f_]+\}|.)|[^'\\\n])'")


class Failure(Exception):
    pass


class Blocked(Failure):
    pass


def require(ok: bool, message: str) -> None:
    if not ok:
        raise Failure(message)


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def uncomment(text: str) -> str:
    """Remove nested Rust comments while preserving quoted/raw-string contents."""
    out = []
    i = 0
    while i < len(text):
        if text.startswith("//", i):
            end = text.find("\n", i)
            i = len(text) if end < 0 else end
        elif text.startswith("/*", i):
            depth = 1
            i += 2
            while i < len(text) and depth:
                if text.startswith("/*", i):
                    depth += 1
                    i += 2
                elif text.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    out.append("\n" if text[i] == "\n" else " ")
                    i += 1
            require(depth == 0, "unterminated Rust comment in inventory")
        elif match := RAW_START.match(text, i):
            terminator = '"' + (match.group(1) or "")
            end = text.find(terminator, match.end())
            require(end >= 0, "unterminated raw string in inventory")
            end += len(terminator)
            out.append(text[i:end])
            i = end
        elif match := CHAR_START.match(text, i):
            out.append(match.group())
            i = match.end()
        elif text[i] == '"':
            end = i + 1
            while end < len(text):
                if text[end] == "\\":
                    end += 2
                elif text[end] == '"':
                    end += 1
                    break
                else:
                    end += 1
            out.append(text[i:end])
            i = end
        else:
            out.append(text[i])
            i += 1
    return "".join(out)


def inventory(manifest: dict) -> dict:
    # A clean frozen Git candidate excludes ignored targets/generated outputs.
    result = subprocess.run(["git", "ls-files", "-z", "--", "*.rs"], cwd=ROOT,
                            capture_output=True, check=True)
    sites: dict[str, set[str]] = {}
    for name in result.stdout.decode().split("\0"):
        if name:
            source = uncomment((ROOT / name).read_text())
            for control in re.findall(r'\bparking_lot_negative_control\s*=\s*"([a-z0-9-]+)"', source):
                sites.setdefault(control, set()).add(name)
    actual = {key: sorted(value) for key, value in sorted(sites.items())}
    require(actual == manifest["source_sites"], "OA-12:inventory: cfg IDs/sites changed or were omitted")
    require(set(actual) == {case["control"] for case in manifest["cases"]},
            "OA-12:inventory: every real cfg requires an execution case")
    ids = [case["id"] for case in manifest["cases"]]
    require(len(ids) == len(set(ids)) > 0, "duplicate or empty execution matrix")
    for name, expected in manifest["source_sha256"].items():
        require(sha(ROOT / name) == expected, f"OA-12:source identity changed: {name}")
    return actual


class Runner:
    def __init__(self, output: Path):
        self.output = output.resolve()
        require(not self.output.exists() and not self.output.is_relative_to(ROOT),
                "fresh external output directory required")
        self.output.mkdir(parents=True)
        self.commands: list[dict] = []
        self.env = {key: value for key, value in os.environ.items()
                    if key not in {"RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS"}
                    and not key.startswith(("NEXTEST_", "WASM_BINDGEN_", "CHROMEDRIVER",
                                            "GECKODRIVER", "SAFARIDRIVER", "MSEDGEDRIVER"))}
        self.env.update(CARGO_TERM_COLOR="never", RUSTFLAGS="", RUSTDOCFLAGS="",
                        RUSTUP_TOOLCHAIN="1.99.1", RUSTUP_AUTO_INSTALL="0", LOOM_MAX_PREEMPTIONS="3")
        self.proofs: list[dict] = []
        self.identities: dict = {"python": sys.version}

    def command(self, label: str, argv: list[str], timeout: int = 1200) -> dict:
        number = len(self.commands) + 1
        entry = {"label": label, "argv": argv,
                 "stdout": f"{number:03}-{label}.stdout.txt",
                 "stderr": f"{number:03}-{label}.stderr.txt",
                 "environment": {key: self.env[key] for key in (
                     "RUSTFLAGS", "CARGO_TARGET_DIR", "RUSTUP_TOOLCHAIN", "LOOM_MAX_PREEMPTIONS",
                     "PARKING_LOT_PG_MAJOR", "WASM_BINDGEN_USE_BROWSER", "CHROMEDRIVER",
                     "CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER") if key in self.env}}
        print(shlex.join(argv), flush=True)
        with (self.output / entry["stdout"]).open("w") as stdout, (self.output / entry["stderr"]).open("w") as stderr:
            with subprocess.Popen(argv, cwd=ROOT, env=self.env, stdout=stdout, stderr=stderr,
                                  start_new_session=True, text=True) as process:
                try:
                    entry["exit"] = process.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    os.killpg(process.pid, signal.SIGTERM)
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        os.killpg(process.pid, signal.SIGKILL)
                        process.wait()
                    entry["exit"] = "TIMEOUT"
        for stream in ("stdout", "stderr"):
            entry[stream + "_sha256"] = sha(self.output / entry[stream])
        self.commands.append(entry)
        write(self.output / "commands.json", self.commands)
        require(entry["exit"] != "TIMEOUT", f"{label}: watchdog is not a designated assertion")
        return entry

    def text(self, command: dict) -> str:
        return "\n".join((self.output / command[key]).read_text() for key in ("stdout", "stderr"))

    def passed(self, label: str, argv: list[str], timeout: int = 1200) -> dict:
        command = self.command(label, argv, timeout)
        require(command["exit"] == 0, f"{label}: prerequisite/build/discovery/normal execution failed")
        return command

    def prerequisite(self, label: str, argv: list[str], timeout: int = 30) -> dict:
        command = self.command(label, argv, timeout)
        if command["exit"] != 0:
            raise Blocked(f"{label}: required installed tool/collaborator unavailable")
        return command


def filter_for(test: dict) -> str:
    return f'binary(={test["target"]}) & test(={test["name"]})'


def native(runner: Runner, case: dict) -> None:
    package = case["package"]
    tests = case["oracles"] + case["companions"]
    for mode in ("normal", "mutant"):
        flags = "--cfg parking_lot_loom" if case["adapter"] == "loom" else ""
        if mode == "mutant":
            flags += f' --cfg parking_lot_negative_control="{case["control"]}"'
        runner.env["RUSTFLAGS"] = flags.strip()
        runner.env["CARGO_TARGET_DIR"] = str(ROOT / "target/oa12" / case["id"] / mode)
        base = ["cargo", "+1.99.1", "nextest"]
        select = ["-p", package, "--locked", "--all-features"]
        targets = sorted({test["target"] for test in tests})
        for target in targets:
            select += ["--lib"] if target == package.replace("-", "_") else ["--test", target]
        build = runner.passed(mode + "-build", base + ["run", *select, "--no-run"])
        discovery = runner.passed(mode + "-discovery", base + ["list", *select, "--message-format", "json"])
        listing = json.loads((runner.output / discovery["stdout"]).read_text())
        actual = {(suite["binary-name"], name)
                  for suite in listing["rust-suites"].values()
                  if suite.get("status") == "listed" and suite["package-name"] == package
                  for name in suite["testcases"]}
        require(actual and all((test["target"], test["name"]) in actual for test in tests),
                "OA-12:discovery: exact oracle/companion identities missing")
        for index, test in enumerate(tests):
            expected_negative = mode == "mutant" and index < len(case["oracles"])
            command = runner.command(f"{mode}-test-{index}", base + ["run", *select,
                "--run-ignored", "all", "--no-fail-fast", "--no-tests=fail", "--test-threads=1",
                "--retries=0", "--status-level", "all", "--failure-output", "immediate",
                "--success-output", "immediate", "--show-progress", "none", "-E", filter_for(test)], timeout=600)
            proof = {"kind": "native", "mode": mode, "test": test, "command": command,
                     "discovery": discovery, "build": build,
                     "negative": expected_negative}
            # The successful build is also retained in commands.json; never
            # infer a build from a later runtime status.
            classify_native(command["exit"], runner.text(command), test, expected_negative)
            runner.proofs.append(proof)


def classify_native(code: object, text: str, test: dict, negative: bool) -> None:
    status = "FAIL" if negative else "PASS"
    require(code == (100 if negative else 0), "OA-12:exit: wrong runtime exit (including defect pass)")
    require(re.search(rf"^\s*{status}\s+\[[^\n]+\][^\n]*\s{re.escape(test['name'])}\s*$", text, re.M),
            "OA-12:identity: designated process result missing")
    require(re.search(r"Summary[^\n]*\b1 test run:", text), "OA-12:execution: exactly one real test required")
    require("1 failed" in text if negative else "1 passed" in text, "OA-12:execution: result count mismatch")
    if negative:
        require("panicked at " in text and all(fragment in text for fragment in test["diagnostic"]),
                "OA-12:oracle: designated assertion missing; backend/setup errors do not count")
        require(not test.get("diagnostic_any") or any(fragment in text for fragment in test["diagnostic_any"]),
                "OA-12:oracle: designated alternative assertion missing")


def browser(runner: Runner, case: dict) -> None:
    policy = json.loads((ROOT / ".github/wasm-browser-assurance.json").read_bytes())
    if platform.system() != "Linux" or platform.machine() != "x86_64":
        raise Blocked("only pinned Linux x86_64 Chromium is supported")
    paths = {"chrome": os.environ.get("WASM_BROWSER_BIN"), "driver": os.environ.get("CHROMEDRIVER")}
    for key, path in paths.items():
        if not path or not Path(path).is_absolute() or not os.access(path, os.X_OK):
            raise Blocked(f"existing absolute {key} path required; no browser install performed")
        command = runner.passed(key + "-version", [path, "--version"], 30)
        require(policy["browser"]["version"] in runner.text(command), "browser/driver version pin mismatch")
        runner.identities[key] = {"path": path, "sha256": sha(Path(path)), "version": runner.text(command).strip()}
    if shutil.which("wasm-bindgen-test-runner") is None:
        raise Blocked("preinstalled wasm-bindgen-test-runner required")
    command = runner.prerequisite("wasm-runner-version", ["wasm-bindgen-test-runner", "--version"])
    require("wasm-bindgen-test-runner " + policy["wasm_bindgen"] in runner.text(command), "WASM runner lock pin mismatch")
    capabilities = runner.output / "webdriver.json"
    write(capabilities, {"goog:chromeOptions": {"binary": paths["chrome"]}})
    runner.env.pop("NO_HEADLESS", None)
    runner.env.update(WASM_BINDGEN_USE_BROWSER="1", CHROMEDRIVER=paths["driver"],
                      WASM_BINDGEN_TEST_WEBDRIVER_JSON=str(capabilities),
                      CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER="wasm-bindgen-test-runner",
                      WASM_BINDGEN_TEST_TIMEOUT="180", WASM_BINDGEN_TEST_DRIVER_TIMEOUT="30")
    runner.passed("wasm-graph", ["bash", ".github/scripts/check-wasm-graph.sh"])
    for mode in ("normal", "mutant"):
        runner.env["RUSTFLAGS"] = "" if mode == "normal" else f'--cfg parking_lot_negative_control="{case["control"]}"'
        runner.env["CARGO_TARGET_DIR"] = str(ROOT / "target/oa12" / case["id"] / mode)
        for index, test in enumerate(case["oracles"] + case["companions"]):
            negative = mode == "mutant" and index < len(case["oracles"])
            base = ["cargo", "+1.99.1", "test", "--locked", "--target", "wasm32-unknown-unknown",
                    "-p", case["package"], "--test", test["target"]]
            build = runner.passed(f"{mode}-{index}-build", [*base, "--no-run"])
            discovery = runner.passed(f"{mode}-{index}-discovery", [*base, "--", "--list"])
            names = [line.removesuffix(": test").split("::")[-1]
                     for line in runner.text(discovery).splitlines() if line.endswith(": test")]
            require(len(names) == len(policy["tests"][test["target"]]) > 0
                    and set(names) == set(policy["tests"][test["target"]])
                    and test["name"] in names, "OA-12:browser-discovery: exact real exports required")
            # The pinned runner's substring filter is followed by an exact
            # one-result classifier; matching another export cannot pass.
            command = runner.command(f"{mode}-{index}-run", [*base, "--", test["name"], "--nocapture"], 360)
            text = runner.text(command)
            classify_browser(command["exit"], text, test, negative)
            runner.proofs.append({"kind": "browser", "mode": mode, "negative": negative,
                                  "test": test, "command": command, "discovery": discovery, "build": build})


def classify_browser(code: object, text: str, test: dict, negative: bool) -> None:
    require(code == (101 if negative else 0), "OA-12:browser-exit: wrong runtime exit")
    outcomes = [(name.split("::")[-1], status)
                for name, status in re.findall(r"^test (\S+) \.\.\. (ok|FAIL)$", text, re.M)]
    require(outcomes == [(test["name"], "FAIL" if negative else "ok")],
            "OA-12:browser-identity: exactly one designated real test required")
    # Context marker is emitted by smoke in the same real target. The selected
    # tests need not print it: runner's actual Chrome launch and document-using
    # wasm-bindgen runtime establish this invocation's browser mode.
    require("Running headless tests in Chrome" in text and "OA13_NODE_CONTEXT" not in text,
            "OA-12:browser-context: Node-only discovery/execution is insufficient")
    require(("test result: FAILED. 0 passed; 1 failed;" if negative else
             "test result: ok. 1 passed; 0 failed;") in text, "OA-12:browser-summary missing")
    if negative:
        require(all(fragment in text for fragment in test["diagnostic"]), "OA-12:browser-oracle missing")


def upgrade(runner: Runner, case: dict, args: argparse.Namespace, manifest: dict) -> None:
    for name in ("pg17_image", "pg18_image", "upgrade_image"):
        if not re.fullmatch(r"sha256:[0-9a-f]{64}", getattr(args, name) or ""):
            raise Blocked("OA-14 requires approved prebuilt local PG17/PG18/dual-version image IDs")
    candidate = runner.text(runner.passed("upgrade-candidate", ["git", "rev-parse", "HEAD"], 30)).strip()
    run_nonce = uuid.uuid4().hex[:8]
    for mode in ("normal", "mutant"):
        out = runner.output / ("upgrade-" + mode)
        runner.env["CARGO_TARGET_DIR"] = str(ROOT / "target/oa12" / case["id"] / mode)
        runner.env["RUSTFLAGS"] = "" if mode == "normal" else '--cfg parking_lot_negative_control="oa-14-missing-result"'
        cargo = ["cargo", "+1.99.1", "nextest"]
        select = ["-p", case["package"], "--all-features", "--locked", "--test", "postgres_upgrade"]
        build = runner.passed(mode + "-upgrade-build", [*cargo, "run", *select, "--no-run"])
        discovery = runner.passed(mode + "-upgrade-discovery", [*cargo, "list", *select, "--message-format", "json"])
        listing = json.loads((runner.output / discovery["stdout"]).read_bytes())
        found = [(suite["binary-name"], name) for suite in listing["rust-suites"].values()
                 if suite.get("status") == "listed" for name in suite["testcases"]]
        require(found == [("postgres_upgrade", "postgres_upgrade_populated_0_3_to_schema3")],
                "OA-12:upgrade-discovery: exact one real upgrade fixture required")
        # The owner adapter refuses inherited injection and supplies its own
        # single control only after it has created the marked upgraded cluster.
        runner.env["RUSTFLAGS"] = ""
        argv = ["bash", "scripts/rehearse-postgres-upgrade.sh", "--out", str(out),
                "--run-id", "oa12-" + candidate[:12] + "-" + run_nonce + "-" + mode,
                "--candidate", candidate, "--contract-sha256", manifest["contract_digest"],
                "--pg17-image", args.pg17_image, "--pg18-image", args.pg18_image,
                "--upgrade-image", args.upgrade_image]
        if mode == "mutant":
            argv += ["--control", "missing-result"]
        command = runner.passed("upgrade-" + mode, argv, 7200)
        receipt = json.loads((out / "receipt.json").read_bytes())
        classify_upgrade(out, receipt, candidate, manifest["contract_digest"], mode == "mutant")
        runner.proofs.append({"kind": "upgrade", "mode": mode, "negative": mode == "mutant",
                              "command": command, "build": build, "discovery": discovery,
                              "receipt": str(out.relative_to(runner.output) / "receipt.json"),
                              "files_sha256": {str(path.relative_to(runner.output)): sha(path)
                                               for path in sorted(out.rglob("*")) if path.is_file()}})


def classify_upgrade(folder: Path, receipt: dict, candidate: str, contract: str, negative: bool) -> None:
    require(receipt["scenario"] == "OA-14" and receipt["candidate"] == candidate
            and receipt["contract_sha256"] == contract
            and receipt["control"] == ("missing-result" if negative else "none")
            and receipt["status"] == ("EXPECTED_FAILURE" if negative else "PASS"),
            "OA-12:upgrade: actual run-bound upgraded-cluster result required")
    suites = [step for step in receipt["steps"] if step["name"] == "schema3-and-real-upgraded-suites"]
    require(len(suites) == 1 and suites[0]["exit"] == (100 if negative else 0), "OA-12:upgrade: wrong suite exit")
    step = suites[0]
    text = "\n".join((folder / step[stream]).read_text() for stream in ("stdout", "stderr"))
    text = re.sub(r"\x1b\[[0-9;]*m", "", text)
    name = "postgres_upgrade_populated_0_3_to_schema3"
    require(name in text, "OA-12:upgrade: designated real test missing")
    if negative:
        require(receipt["discovered_tests"] == 1 and receipt["oracle"] == "OA-14:seeded-result"
                and "OA-14:seeded-result" in text and "panicked at " in text
                and re.search(rf"^\s*FAIL\s+\[[^\n]+\][^\n]*\s{re.escape(name)}\s*$", text, re.M),
                "OA-12:upgrade: designated seeded-result assertion required")
    else:
        require(receipt["discovered_tests"] > 0 and "PASS: independent PG17 copy" in receipt["backup_restore"],
                "OA-12:upgrade: populated upgrade/restore baseline missing")


def run_case(args: argparse.Namespace, manifest: dict) -> None:
    if os.name != "posix":
        raise Blocked("POSIX process groups and Linux collaborator lanes required")
    case = next((case for case in manifest["cases"] if case["id"] == args.case), None)
    require(case is not None, "unknown case; cannot manufacture an unlisted row")
    runner = Runner(args.output)
    try:
        runner.passed("clean-before", ["git", "diff", "--exit-code", "HEAD"], 30)
        status = runner.passed("untracked-before", ["git", "status", "--porcelain"], 30)
        require(not runner.text(status).strip(), "clean frozen candidate required")
        inventory(manifest)
        candidate = runner.text(runner.passed("candidate-before", ["git", "rev-parse", "HEAD"], 30)).strip()
        require(re.fullmatch(r"[0-9a-f]{40}", candidate), "full candidate required")
        contract = json.loads(CONTRACT.read_bytes())
        require(contract["identity"]["contractDigest"] == manifest["contract_digest"], "adopted contract changed")
        for tool, argv in (("rustc", ["rustc", "+1.99.1", "--version", "--verbose"]),
                           ("cargo", ["cargo", "+1.99.1", "--version", "--verbose"]),
                           ("nextest", ["cargo", "+1.99.1", "nextest", "--version"])):
            if shutil.which(argv[0]) is None:
                raise Blocked(f"missing preinstalled {argv[0]}")
            identity = runner.prerequisite(tool + "-identity", argv)
            if tool == "nextest":
                require("cargo-nextest " + manifest["nextest"] in runner.text(identity), "nextest version pin mismatch")
        if case.get("pg"):
            runner.env["PARKING_LOT_PG_MAJOR"] = case["pg"]
        if case["adapter"] in ("pg", "iggy", "upgrade"):
            if shutil.which("docker") is None:
                raise Blocked("real Docker collaborator required")
            runner.prerequisite("docker-identity", ["docker", "version"])
        if case["adapter"] == "pg":
            runner.prerequisite("project-pg-image", ["docker", "image", "inspect", "parking-lot-pg:" + case["pg"],
                                "--format", '{{.Id}} {{json .RepoDigests}} {{json .Config.Labels}}'])
        if case["adapter"] == "browser":
            browser(runner, case)
        elif case["adapter"] == "upgrade":
            upgrade(runner, case, args, manifest)
        else:
            native(runner, case)
        if case["adapter"] == "iggy":
            runner.passed("iggy-image-identity", ["docker", "image", "inspect", manifest["iggy_image"],
                          "--format", '{{.Id}} {{json .RepoDigests}} {{json .Config.Labels}}'], 30)
        after = runner.text(runner.passed("candidate-after", ["git", "rev-parse", "HEAD"], 30)).strip()
        status = runner.passed("clean-after", ["git", "status", "--porcelain"], 30)
        require(candidate == after and not runner.text(status).strip(), "candidate changed")
        receipt = {"status": "PASS", "scenario": "OA-12", "case": case["id"],
                   "control": case["control"], "candidate": candidate, "contract_digest": manifest["contract_digest"],
                   "manifest_sha256": sha(MANIFEST), "runner_sha256": sha(Path(__file__)),
                   "platform": platform.platform(), "machine": platform.machine(),
                   "identities": runner.identities, "proofs": runner.proofs, "commands": runner.commands}
        write(runner.output / "receipt.json", receipt)
    except (Failure, OSError, ValueError, KeyError) as error:
        write(runner.output / "receipt.json", {"status": "BLOCKED" if isinstance(error, Blocked) else "FAIL",
              "case": args.case, "reason": str(error), "commands": runner.commands})
        raise


def verify_receipt(folder: Path, case: dict, manifest: dict, candidate: str) -> dict:
    receipt = json.loads((folder / "receipt.json").read_bytes())
    require(receipt["status"] == "PASS" and receipt["case"] == case["id"]
            and receipt["control"] == case["control"] and receipt["candidate"] == candidate
            and receipt["manifest_sha256"] == sha(MANIFEST)
            and receipt["runner_sha256"] == sha(Path(__file__))
            and receipt["contract_digest"] == manifest["contract_digest"], "OA-12:receipt binding failed")
    commands = receipt["commands"]
    require(commands and all(command["exit"] in (0, 100, 101) for command in commands), "invalid/timeout command")
    for command in commands:
        for stream in ("stdout", "stderr"):
            path = (folder / command[stream]).resolve()
            require(path.is_relative_to(folder.resolve()) and sha(path) == command[stream + "_sha256"], "raw log changed/missing")
    expected = [(mode, test, mode == "mutant" and index < len(case["oracles"]))
                for mode in ("normal", "mutant")
                for index, test in enumerate(case.get("oracles", []) + case.get("companions", []))]
    if case["adapter"] == "upgrade":
        require([proof["mode"] for proof in receipt["proofs"]] == ["normal", "mutant"], "upgrade modes missing")
        for proof in receipt["proofs"]:
            external = (folder / proof["receipt"]).resolve()
            require(external.is_relative_to(folder.resolve()), "upgrade receipt path escapes artifact")
            upgrade_receipt = json.loads(external.read_bytes())
            require(proof["command"] in commands and proof["command"]["exit"] == 0, "unbound upgrade invocation")
            require(proof["build"] in commands and proof["build"]["exit"] == 0
                    and "--no-run" in proof["build"]["argv"] and proof["discovery"] in commands
                    and proof["discovery"]["exit"] == 0, "upgrade compile/discovery missing")
            for relative, digest in proof["files_sha256"].items():
                path = (folder / relative).resolve()
                require(path.is_relative_to(folder.resolve()) and sha(path) == digest, "upgrade raw evidence changed")
            classify_upgrade(external.parent, upgrade_receipt, candidate, manifest["contract_digest"], proof["negative"])
    else:
        proofs = receipt["proofs"]
        require([(proof["mode"], proof["test"], proof["negative"]) for proof in proofs] == expected,
                "OA-12:omission: exact normal/mutant oracle/companion proofs required")
        for proof in proofs:
            command = proof["command"]
            require(command in commands and proof["discovery"] in commands
                    and proof["discovery"]["exit"] == 0 and proof["build"] in commands
                    and proof["build"]["exit"] == 0 and "--no-run" in proof["build"]["argv"],
                    "unbound command/build/discovery")
            flags = "--cfg parking_lot_loom" if case["adapter"] == "loom" else ""
            if proof["mode"] == "mutant":
                flags += f' --cfg parking_lot_negative_control="{case["control"]}"'
            require(all(item["environment"]["RUSTFLAGS"] == flags.strip()
                        for item in (command, proof["build"], proof["discovery"])),
                    "OA-12:injection: exact isolated normal/mutant flags required")
            text = "\n".join((folder / command[stream]).read_text() for stream in ("stdout", "stderr"))
            classifier = classify_browser if case["adapter"] == "browser" else classify_native
            classifier(command["exit"], text, proof["test"], proof["negative"])
    return receipt


def complete(folders: dict, expected: set[str]) -> None:
    require(set(folders) == expected, "OA-12:omission: exact complete matrix receipt set required")


def aggregate(args: argparse.Namespace, manifest: dict) -> None:
    inventory(manifest)
    require(re.fullmatch(r"[0-9a-f]{40}", args.candidate or ""), "full frozen candidate required")
    folders = {path.parent.name: path.parent for path in args.receipts.glob("*/receipt.json")}
    expected = {case["id"] for case in manifest["cases"]}
    complete(folders, expected)
    receipts = [verify_receipt(folders[case["id"]], case, manifest, args.candidate) for case in manifest["cases"]]
    # Discriminating controls reuse actual successful matrix evidence. A missing
    # row and an undetected mutant must both be rejected by the same predicates.
    missing = dict(folders)
    missing.pop(next(iter(expected)))
    try:
        complete(missing, expected)
    except Failure:
        omission_rejected = True
    else:
        omission_rejected = False
    # Substitute the actual same-oracle NORMAL PASS into an expected-mutant
    # decision. Preserve the real argv/status/log; do not manufacture exit 0
    # or a synthetic successful test. An undetected defect has this outcome.
    source_receipt = next(receipt for receipt in receipts if any(
        proof["kind"] == "native" and proof["negative"] for proof in receipt["proofs"]))
    designated = next(proof["test"] for proof in source_receipt["proofs"] if proof["negative"])
    proof = next(proof for proof in source_receipt["proofs"]
                 if proof["mode"] == "normal" and proof["test"] == designated)
    command = proof["command"]
    folder = folders[source_receipt["case"]]
    text = "\n".join((folder / command[stream]).read_text() for stream in ("stdout", "stderr"))
    try:
        classify_native(command["exit"], text, proof["test"], True)
    except Failure as error:
        defect_pass_rejected = True
        defect_pass_reason = str(error)
    else:
        defect_pass_rejected = False
    require(omission_rejected and defect_pass_rejected, "OA-12:meta-control: verifier accepted omission/defect pass")
    require(not args.output.exists(), "fresh aggregate receipt path required")
    write(args.output, {"status": "PASS", "scenario": "OA-12", "candidate": args.candidate,
          "contract_digest": manifest["contract_digest"], "manifest_sha256": sha(MANIFEST),
          "cases": sorted(expected), "controls": sorted(manifest["source_sites"]),
          "meta_controls": {"omitted_row": {"status": "EXPECTED_FAILURE", "diagnostic": "OA-12:omission"},
                            "defect_pass": {"status": "EXPECTED_FAILURE", "diagnostic": defect_pass_reason,
                                            "source_case": source_receipt["case"], "actual_exit": command["exit"],
                                            "actual_normal_oracle": proof["test"]["name"],
                                            "stdout_sha256": command["stdout_sha256"]}},
          "receipts_sha256": {name: sha(folder / "receipt.json") for name, folder in folders.items()}})


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    sub.add_parser("matrix")
    run = sub.add_parser("run")
    run.add_argument("--case", required=True)
    run.add_argument("--output", type=Path, required=True)
    for name in ("pg17-image", "pg18-image", "upgrade-image"):
        run.add_argument("--" + name, default=os.environ.get("OA12_" + name.upper().replace("-", "_")))
    collect = sub.add_parser("aggregate")
    collect.add_argument("--receipts", type=Path, required=True)
    collect.add_argument("--candidate", required=True)
    collect.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        manifest = json.loads(MANIFEST.read_bytes())
        if args.operation == "matrix":
            inventory(manifest)
            print(json.dumps({"include": [{key: case.get(key, "") for key in ("id", "adapter", "pg")}
                                          for case in manifest["cases"]]}, separators=(",", ":")))
        elif args.operation == "run":
            run_case(args, manifest)
        else:
            aggregate(args, manifest)
    except Blocked as error:
        print(f"BLOCKED: {error}", file=sys.stderr)
        return 2
    except (Failure, OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
