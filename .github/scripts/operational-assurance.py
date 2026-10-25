#!/usr/bin/env python3
"""OA-18 full packet coordinator. Only the incumbent compact protocol certifies.

Authoring this file is not execution evidence. A local packet establishes no
remote authority. Tools/runners/images are preprovisioned; nothing installs,
publishes, signs production tags, dispatches or rewrites history. Review is an external input.
"""
from __future__ import annotations

import argparse
import copy
import datetime as dt
import hashlib
import importlib.util
import json
import ntpath
import os
import posixpath
from pathlib import Path
import platform
import re
import shutil
import signal
import stat
import subprocess
import sys
import time
import urllib.request
import zipfile

sys.dont_write_bytecode = True
from release_evidence import (  # noqa: E402
    BOUNDARY_WORKFLOW, GitHubAPI, ReleaseError, canonical, git, require,
    sha256, validate_approvals, validate_policy, validate_preparation,
    validate_receipt, validate_run, verify_candidate_tag,
)

ROOT = Path(__file__).resolve().parents[2]
DEFINITION = ROOT / ".github/operational-assurance.json"
STATES = {"PASS", "FAIL", "BLOCKED", "NOT_EXERCISED"}
INPUT_KEYS = {"base", "release_date", "release_plz", "git_cliff", "trust_policy", "rehearsal_artifact_id",
              "fresh_mirror", "pg17_image", "pg18_image", "upgrade_image", "chrome", "chromedriver",
              "android_ndk", "baseline_root", "quiet_attestation", "gui_positive", "gui_control",
              "authority_config", "rust_toolchain", "tag", "preparation_plan", "preparation_source"}
MANUAL_PROFILES = {"macos-gui": "OA15-GUI", "quiet": "OA17-QUIET"}
AUTOMATIC_PROFILES = {"baseline", "linux", "macos", "windows", "android", "ios"}
CONTRACT = ROOT / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/adopted-contract.json"
SUBSTITUTION = re.compile(r"\{([a-z_][a-z_0-9]*)\}")
TOOLS = {
    "python3": ([sys.executable, "--version"], "Python 3."),
    "git": (["git", "--version"], "git version"),
    "cargo": (["cargo", "--version"], "cargo 1.99.1"),
    "cargo_numeric": (["cargo", "+1.99.1", "--version"], "cargo 1.99.1"),
    "rustfmt": (["rustfmt", "--version"], "rustfmt"),
    "clippy": (["cargo", "clippy", "--version"], "clippy"),
    "msrv": (["rustc", "+1.95", "--version"], "rustc 1.95"),
    "nextest": (["cargo-nextest", "--version"], "cargo-nextest 0.9.149"),
    "deny": (["cargo-deny", "--version"], "cargo-deny"),
    "docker": (["docker", "version"], "Server:"),
    "gitleaks": (["gitleaks", "version"], "8.30.1"),
    "node": (["node", "--version"], "v24."),
    "npm": (["npm", "--version"], None),
    "wasm_runner": (["wasm-bindgen-test-runner", "--version"], "0.2.129"),
    "chrome": (["{chrome}", "--version"], "156.0.8078.47"),
    "chromedriver": (["{chromedriver}", "--version"], "156.0.8078.47"),
    "release_plz": (["{release_plz}", "--version"], "0.3.173"),
    "git_cliff": (["{git_cliff}", "--version"], "2.14.2"),
    "gpg": (["gpg", "--version"], "gpg"),
    "ssh-keygen": (["ssh", "-V"], "OpenSSH"),
    "ndk": (["cargo", "ndk", "--version"], "4.1.2"),
    "kotlinc": (["kotlinc", "-version"], "2.4.20"),
    "java": (["java", "-version"], "version"),
    "swift": (["swift", "--version"], "Swift version"),
}


def write(path: Path, value: object) -> None:
    path.write_bytes(canonical(value))


def digest(path: Path) -> str:
    return sha256(path.read_bytes())


def stamp() -> str:
    return dt.datetime.now(dt.timezone.utc).isoformat()


def state(records: list[dict]) -> str:
    values = {r["status"] for r in records}
    return next((s for s in ("FAIL", "BLOCKED", "NOT_EXERCISED") if s in values),
                "PASS" if records else "NOT_EXERCISED")


def validate_child_receipt(data: dict, row: dict, candidate: str, binding: str) -> None:
    require(data.get("status") == "PASS" and data.get("candidate") == candidate
            and data.get("contract_digest") == binding,
            "specialized receipt is missing/stale/non-PASS")
    require(all(data.get(key) == value for key, value in row.get("receipt_identity", {}).items()),
            "OA-18:child-receipt-identity: required OS/build case differs")


def definition() -> dict:
    value = json.loads(DEFINITION.read_bytes())
    ids = [row["id"] for row in value["commands"]]
    require(len(ids) == len(set(ids)), "OA-18:duplicate-command")
    matrix = json.loads((ROOT / ".github/negative-controls.json").read_bytes())
    expected = {"oa12-" + row["id"] for row in matrix["cases"]}
    actual = {name for name in ids if name.startswith("oa12-") and name != "oa12-aggregate"}
    require(actual == expected and len(expected) == 52, "OA-18:control-inventory")
    return value


def contract(path: Path) -> str:
    data = json.loads(path.read_bytes())
    normalized = copy.deepcopy(data)
    normalized["identity"].pop("contractDigest", None)
    normalized["identity"].pop("planRevision", None)
    result = sha256(canonical(normalized))
    require(result == data["identity"]["contractDigest"] and data["identity"]["planRevision"] == 1,
            "OA-18:stale-contract")
    require({r["id"] for r in data["scenarioMatrix"]} == {f"OA-{n:02}" for n in range(1, 18)},
            "OA-18:scenario-contract")
    return result


def sources() -> dict:
    # Git index modes distinguish a real symlink from Windows' regular-file
    # materialization of its target. Both bind the link bytes, never its referent.
    result = subprocess.run(["git", "ls-files", "--stage", "-z"], cwd=ROOT,
                            capture_output=True, check=True)
    values = {}
    for entry in result.stdout.split(b"\0"):
        if not entry:
            continue
        metadata, raw_name = entry.split(b"\t", 1)
        mode, _, stage = metadata.split()
        require(stage == b"0", "OA-18:unmerged-source-index")
        name = Path(os.fsdecode(raw_name))
        path = ROOT / name
        if mode == b"120000":
            require(path.is_symlink() or path.is_file(), "OA-18:missing-symlink-source")
            data = os.fsencode(os.readlink(path)) if path.is_symlink() else path.read_bytes()
            values[name.as_posix()] = sha256(data)
        elif path.is_file():
            require(not path.is_symlink(), "OA-18:unexpected-source-symlink")
            values[name.as_posix()] = digest(path)
    return values


def clean(candidate: str) -> None:
    require(re.fullmatch(r"[0-9a-f]{40}", candidate) is not None, "full candidate SHA required")
    require(git(ROOT, "rev-parse", "HEAD") == candidate and not git(ROOT, "status", "--porcelain"),
            "OA-18:changed-candidate: exact clean committed verification checkout required")


def expand(value: str, inputs: dict) -> str:
    def replace(match):
        name = match[1]
        require(name in inputs and isinstance(inputs[name], str) and inputs[name],
                f"missing required environment input: {name}")
        return inputs[name]
    return SUBSTITUTION.sub(replace, value)


def environment() -> dict:
    # Do not retain a config dump or credentials in any manifest or command log.
    env = {k: v for k, v in os.environ.items() if k not in {
        "RUSTFLAGS", "RUSTDOCFLAGS", "CARGO_ENCODED_RUSTFLAGS", "PROFILE",
        "RUSTC", "RUSTDOC", "RUSTC_WRAPPER", "RUSTC_WORKSPACE_WRAPPER", "CARGO_BUILD_TARGET"}
        and not k.startswith(("NEXTEST_", "WASM_BINDGEN_", "CHROMEDRIVER", "GECKODRIVER",
                              "SAFARIDRIVER", "MSEDGEDRIVER"))}
    env.update(RUSTUP_AUTO_INSTALL="0", RUSTUP_TOOLCHAIN="1.99.1", CARGO_TERM_COLOR="never",
               CARGO_NET_OFFLINE="true", RUSTFLAGS="", RUSTDOCFLAGS="", PROFILE="debug",
               PYTHONDONTWRITEBYTECODE="1", GIT_NO_REPLACE_OBJECTS="1", GIT_TERMINAL_PROMPT="0")
    return env


def execute(argv: list[str], folder: Path, label: str, cwd: Path, env: dict, timeout: int) -> dict:
    record = {"argv": argv, "cwd": str(cwd), "started": stamp(),
              "stdout": label + ".stdout.txt", "stderr": label + ".stderr.txt",
              "environment": {k: env[k] for k in ("RUSTUP_AUTO_INSTALL", "RUSTUP_TOOLCHAIN", "RUSTFLAGS",
                  "RUSTDOCFLAGS", "CARGO_TARGET_DIR", "CARGO_NET_OFFLINE", "PARKING_LOT_PG_MAJOR",
                  "WASM_BROWSER_BIN", "CHROMEDRIVER", "ANDROID_NDK_HOME", "VERIFY_SKIP_INSTALL") if k in env}}
    start = time.monotonic()
    with (folder / record["stdout"]).open("wb") as out, (folder / record["stderr"]).open("wb") as err:
        try:
            with subprocess.Popen(argv, cwd=cwd, env=env, stdout=out, stderr=err,
                                  start_new_session=os.name == "posix") as process:
                try:
                    record["exit"] = process.wait(timeout=timeout)
                except subprocess.TimeoutExpired:
                    if os.name == "posix":
                        os.killpg(process.pid, signal.SIGTERM)
                        try:
                            process.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            os.killpg(process.pid, signal.SIGKILL)
                            process.wait()
                    else:
                        process.kill()
                        process.wait()
                    record["exit"] = "TIMEOUT"
        except OSError as error:
            err.write(str(error).encode())
            record["exit"] = "UNAVAILABLE"
    record.update(finished=stamp(), elapsed_seconds=time.monotonic() - start)
    for stream in ("stdout", "stderr"):
        record[stream + "_sha256"] = digest(folder / record[stream])
    return record


def text(folder: Path, record: dict) -> str:
    return "\n".join((folder / record[k]).read_text(errors="replace") for k in ("stdout", "stderr"))


def authority_path(config_path: Path, value: str) -> Path:
    """Portable immutable material names are confined to the config directory."""
    require(isinstance(value, str) and bool(value), "authority material path missing")
    path = Path(value)
    if path.is_absolute():
        return path
    require(value not in {".", ".."} and "/" not in value and "\\" not in value
            and ":" not in value, "authority relative path must be a material basename")
    result = config_path.resolve().parent / value
    require(not result.is_symlink() and result.resolve().parent == config_path.resolve().parent,
            "authority material escapes config directory")
    return result


def authority_configuration(inputs: dict, candidate: str, binding: str) -> tuple[Path, dict, str]:
    path = Path(inputs["authority_config"])
    pin = os.environ.get("OA18_AUTHORITY_SHA256", "")
    require(re.fullmatch(r"[0-9a-f]{64}", pin) is not None and pin != "0" * 64
            and digest(path) == pin, "independently approved authority-config digest absent/mismatched")
    config = json.loads(path.read_bytes())
    require(config["candidate"] == candidate and config["contract_digest"] == binding,
            "stale independently approved authority configuration")
    age_limit = config.get("max_age_seconds")
    require(type(age_limit) is int and 0 < age_limit <= 604800,
            "explicit manual evidence freshness limit missing")
    return path, config, pin


def scoped_containment(report: dict, proofs: list[dict], binding: str) -> None:
    model = definition()
    if model.get("containment_scope") != "current-source-and-fixtures":
        require(all(re.fullmatch(r"[0-9a-f]{64}", report.get(k, "")) for k in
                    ("approved_rewrite_plan_sha256", "ref_plan_sha256", "cache_removal_evidence_sha256")),
                "exact separately approved rewrite/refplan/cache evidence missing")
        return
    amendment = ROOT / ".kbd-orchestrator/phases/phase-0-4-operational-assurance/evidence/oa20/publication-preparation/scope-amendment.json"
    pin = model.get("containment_scope_amendment_sha256", "")
    require(re.fullmatch(r"[0-9a-f]{64}", pin) is not None and digest(amendment) == pin,
            "exact adopted scope amendment source missing/different")
    manifest = json.loads(amendment.read_bytes())
    require(manifest.get("newContractDigest") == binding
            and manifest.get("action") == "amend-contract"
            and manifest.get("scenarioChanges") == {"added": [], "modified": ["OA-01"], "removed": []},
            "containment amendment contract/scenario binding differs")
    evidence = {item["sha256"] for item in proofs}
    require(report.get("scope") == "current-source-and-fixtures"
            and report.get("history_cleanup") == "DEFERRED"
            and report.get("credential_rotation") == "DEFERRED"
            and report.get("scope_amendment_sha256") == pin and pin in evidence,
            "explicit cleanup/rotation deferral and adopted amendment evidence missing")
    source_pin = report.get("current_source_evidence_sha256", "")
    require(re.fullmatch(r"[0-9a-f]{64}", source_pin) is not None and source_pin in evidence,
            "pinned current-source/fixture evidence missing")


def authority(subject: str, inputs: dict, candidate: str, binding: str) -> dict:
    """An independently pinned report digest is authority; caller JSON alone isn't."""
    try:
        path, config, pin = authority_configuration(inputs, candidate, binding)
        age_limit = config["max_age_seconds"]
        entry = config["subjects"][subject]
        report_path = authority_path(path, entry["report"])
        require(report_path.is_absolute() and digest(report_path) == entry["sha256"]
                and entry["source"], "approved report bytes/source differ")
        report = json.loads(report_path.read_bytes())
        require(report.get("candidate") == candidate and report.get("contract_digest") == binding,
                "report candidate/contract differs")
        require(isinstance(report.get("observed_at"), str), "manual report timestamp missing")
        observed = dt.datetime.fromisoformat(report["observed_at"].replace("Z", "+00:00"))
        require(observed.tzinfo is not None and 0 <= (dt.datetime.now(dt.timezone.utc) - observed).total_seconds() <= age_limit,
                "stale/future manual report")
        require(entry["evidence"], "actual raw evidence bytes required, not an approval flag")
        if subject in {"G-02", "G-03", "G-04"}:
            require(report.get("status") == "PASS", "approved source report does not establish a passing gate")
        if subject == "OA15-GUI":
            require(report.get("verdict") == "PASS" and report.get("positive_observation")
                    and report.get("negative_observation"), "actual independent GUI observations missing")
        proofs = []
        for item in entry["evidence"]:
            file = authority_path(path, item["path"])
            require(file.is_absolute() and digest(file) == item["sha256"], "raw approved evidence differs")
            proofs.append({"path": str(file), "sha256": item["sha256"]})
        if subject == "review":
            require(report.get("route") == "R2" and report.get("verdict") == "PASS"
                    and type(report.get("correction_cycles")) is int
                    and 0 <= report["correction_cycles"] <= 1
                    and report.get("open_critical_findings") == 0,
                    "actual independent R2 acceptance/correction budget not established")
        elif subject == "G-02":
            scoped_containment(report, proofs, binding)
        elif subject == "G-04":
            require(all(re.fullmatch(r"[0-9a-f]{64}", report.get(k, "")) for k in
                        ("settings_evidence_sha256", "environment_receipt_sha256")),
                    "authenticated publisher settings/environment evidence missing")
        elif subject == "OA17-QUIET":
            require(digest(Path(inputs["quiet_attestation"])) in {p["sha256"] for p in proofs},
                    "quiet-machine attestation has no independent approved binding")
        return {"status": "PASS", "subject": subject, "report_sha256": entry["sha256"],
                "source": entry["source"], "authority_config_sha256": pin,
                "report_path": str(report_path), "raw": proofs}
    except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        return {"status": "BLOCKED", "subject": subject, "reason": str(error)}


def retain_authority(output: Path, approved: dict) -> None:
    for subject, record in approved.items():
        if record["status"] != "PASS":
            continue
        folder = output / "authority" / subject
        folder.mkdir(parents=True, exist_ok=True)
        items = [{"path": record["report_path"], "sha256": record["report_sha256"]}, *record["raw"]]
        record["retained"] = []
        for number, item in enumerate(items):
            source = Path(item["path"])
            require(digest(source) == item["sha256"], "approved evidence changed before retention")
            target = folder / (str(number) + source.suffix)
            shutil.copyfile(source, target)
            record["retained"].append({"path": target.relative_to(output).as_posix(), "sha256": digest(target)})


def make_freeze(args, model: dict, output: Path) -> dict:
    clean(args.candidate)
    binding = contract(args.contract)
    require(binding == model["contract_digest"], "OA-18:manifest-contract")
    supplied = json.loads(args.inputs.read_bytes()) if args.inputs else {}
    require(set(supplied) <= INPUT_KEYS and all(isinstance(v, str) for v in supplied.values()),
            "inputs are only checked public string paths/identities, never credentials/commands")
    write(output / "profile-inputs.json", supplied)
    inputs = {**supplied, "workspace": str(ROOT), "candidate": args.candidate,
              "contract": str(args.contract.resolve()), "out": str(output)}
    env = environment()
    env["RUSTUP_TOOLCHAIN"] = inputs.get("rust_toolchain", "1.99.1")
    for key, source in (("WASM_BROWSER_BIN", "chrome"), ("CHROMEDRIVER", "chromedriver"),
                        ("ANDROID_NDK_HOME", "android_ndk")):
        if source in inputs:
            env[key] = inputs[source]
    rows = []
    for row in model["commands"]:
        if row["profile"] != args.profile:
            continue
        resolved = copy.deepcopy(row)
        try:
            resolved["argv"] = [expand(v, inputs) for v in row["argv"]]
            if row.get("discovery_argv"):
                resolved["discovery_argv"] = [expand(v, inputs) for v in row["discovery_argv"]]
            resolved["cwd"] = str((ROOT / expand(row.get("cwd", "."), inputs)).resolve())
            resolved["env"] = {k: expand(v, inputs) for k, v in row.get("env", {}).items()}
            resolved["paths"] = [str((ROOT / expand(v, inputs)).resolve()) for v in row.get("paths", [])]
            resolved["images"] = [expand(v, inputs) for v in row.get("images", [])]
            if row.get("receipt"):
                resolved["receipt"] = expand(row["receipt"], inputs)
        except ReleaseError as error:
            resolved["blocked"] = str(error)
        rows.append(resolved)
    observed = {}
    compatible = model["profiles"][args.profile] in {"native", platform.system()}
    for name in sorted({tool for row in rows for tool in row["tools"]}):
        try:
            argv, expected = TOOLS[name]
            argv = [expand(v, inputs) for v in argv]
            # Cargo sets the plugin environment required by cargo-ndk. Retain
            # the plugin's own identity rather than hashing the Cargo launcher.
            location = shutil.which("cargo-ndk" if name == "ndk" else argv[0])
            require(location is not None, "executable not on installed PATH")
            record = {"path": str(Path(location).resolve()), "sha256": digest(Path(location)),
                      "presence": "PRESENT", "version_argv": argv}
            if not compatible:
                record.update(status="BLOCKED", functional_health="NOT_EXERCISED", reason="wrong OS profile")
            else:
                proof = execute(argv, output, "tool-" + name, ROOT, env, 60)
                record.update(proof=proof, status="PASS" if proof["exit"] == 0 and
                              (expected is None or expected in text(output, proof)) else "BLOCKED")
                # ssh -V and java -version legitimately report identity on stderr.
                record["functional_health"] = "PASS" if record["status"] == "PASS" else "UNAVAILABLE_OR_WRONG_VERSION"
                if name in {"cargo", "cargo_numeric", "msrv"} and record["status"] == "PASS":
                    selector = ["+1.99.1"] if name == "cargo_numeric" else ["+1.95"] if name == "msrv" else []
                    expected_rustc = "rustc 1.95" if name == "msrv" else "rustc 1.99.1"
                    version = execute(["rustc", *selector, "--version", "--verbose"], output, "rustc-selected-" + name, ROOT, env, 60)
                    sysroot = execute(["rustc", *selector, "--print", "sysroot"], output, "rustc-sysroot-" + name, ROOT, env, 60)
                    require(version["exit"] == 0 and expected_rustc in text(output, version)
                            and sysroot["exit"] == 0, "selected compiler version differs")
                    install = Path((output / sysroot["stdout"]).read_text().strip())
                    executable = install / "bin" / ("rustc.exe" if os.name == "nt" else "rustc")
                    record.update(selected_rustc={"path": str(executable), "sha256": digest(executable)},
                                  rustc_version=version, sysroot=sysroot,
                                  explicit_selector=env["RUSTUP_TOOLCHAIN"])
            observed[name] = record
        except (ReleaseError, OSError, KeyError) as error:
            observed[name] = {"status": "BLOCKED", "presence": "ABSENT_OR_UNRESOLVED",
                              "functional_health": "NOT_EXERCISED", "reason": str(error)}
    images = {}
    for image in sorted({v for row in rows if not row.get("blocked") for v in row.get("images", [])}):
        if compatible and observed.get("docker", {}).get("status") == "PASS":
            proof = execute(["docker", "image", "inspect", "--format", "{{.Id}}", image], output,
                            "image-" + sha256(image.encode())[:12], ROOT, env, 60)
            info = {"proof": proof, "status": "BLOCKED"}
            if proof["exit"] == 0:
                image_id = (output / proof["stdout"]).read_text().strip()
                if re.fullmatch(r"sha256:[0-9a-f]{64}", image_id):
                    info.update(status="PASS", id=image_id, requested_reference=image)
            images[image] = info
        else:
            images[image] = {"status": "BLOCKED", "reason": "existing functional Docker unavailable; no pull/build"}
    fixture_inputs = {}
    if args.profile == "baseline":
        try:
            plan_path = Path(inputs["preparation_plan"]).resolve()
            plan_bytes = plan_path.read_bytes()
            plan = json.loads(plan_bytes)
            require(plan.get("candidate") == inputs["preparation_source"]
                    and plan.get("base") == inputs["base"]
                    and plan.get("date") == inputs["release_date"], "OA-18:preparation-input-binding")
            require(plan["candidate"] != args.candidate, "OA-18:preparation-source-must-precede-final")
            validate_preparation(ROOT, plan, args.candidate, "0.4.0")
            fixture_inputs["preparation_plan"] = {"status": "PASS", "path": str(plan_path),
                "sha256": sha256(plan_bytes), "source": plan["candidate"], "final_candidate": args.candidate}
            (output / "original-release-plan.json").write_bytes(plan_bytes)
        except (ReleaseError, OSError, ValueError, KeyError) as error:
            fixture_inputs["preparation_plan"] = {"status": "BLOCKED", "reason": str(error)}
    if inputs.get("fresh_mirror"):
        try:
            mirror = Path(inputs["fresh_mirror"]).resolve()
            require(mirror != ROOT and not mirror.is_relative_to(ROOT), "fresh mirror must be an isolated external worktree")
            require(git(mirror, "rev-parse", "HEAD") == args.candidate
                    and not git(mirror, "status", "--porcelain")
                    and git(mirror, "rev-parse", "--is-shallow-repository") == "false",
                    "fresh mirror candidate/clean full-history identity differs")
            origin = git(mirror, "remote", "get-url", "origin").rstrip("/")
            require(origin in {"https://github.com/Prometheus-AGS/prometheus-parking-lot-rs",
                               "https://github.com/Prometheus-AGS/prometheus-parking-lot-rs.git",
                               "git@github.com:Prometheus-AGS/prometheus-parking-lot-rs.git"},
                    "fresh mirror is not from the approved repository")
            roots = ".github/pre-purge-roots.txt"
            require(digest(mirror / roots) == digest(ROOT / roots), "mirror containment roots differ from candidate")
            fixture_inputs["fresh_mirror"] = {"status": "PASS", "candidate": args.candidate,
                                              "roots_sha256": digest(mirror / roots)}
        except (ReleaseError, OSError) as error:
            fixture_inputs["fresh_mirror"] = {"status": "BLOCKED", "reason": str(error)}
    approved = {key: authority(key, inputs, args.candidate, binding) for key in model["required_authority"]}
    if supplied.get("quiet_attestation"):
        approved["OA17-QUIET"] = authority("OA17-QUIET", inputs, args.candidate, binding)
    retain_authority(output, approved)
    manifest = {"schema_version": 1, "candidate": args.candidate, "contract_digest": binding,
                "definition_sha256": digest(DEFINITION), "source_sha256": sources(),
                "profile": args.profile, "os": platform.platform(), "system": platform.system(),
                "machine": platform.machine(), "host": platform.node(),
                "python": sys.version, "frozen_at": stamp(), "commands": rows,
                "tools": observed, "images": images, "authority": approved, "fixture_inputs": fixture_inputs,
                "inputs": inputs, "inputs_sha256": digest(args.inputs) if args.inputs else None,
                "budgets": model["budgets"], "environment": {k: env[k] for k in
                    ("RUSTUP_AUTO_INSTALL", "RUSTUP_TOOLCHAIN", "CARGO_NET_OFFLINE", "CARGO_TERM_COLOR")}}
    write(output / "checked-manifest.json", manifest)
    return manifest


def discovered(value: str, kind: str | None) -> int:
    if kind == "nextest":
        return sum(map(int, re.findall(r"(\d+) tests? run", value)))
    if kind == "libtest":
        return sum(map(int, re.findall(r"test result: ok\. (\d+) passed", value)))
    if kind == "unittest":
        return sum(map(int, re.findall(r"Ran (\d+) tests?", value)))
    return 0


def discovery_count(value: str, row: dict) -> int:
    if row["discovery_kind"] == "nextest":
        suites = json.loads(value)["rust-suites"].values()
        return sum(1 for suite in suites if suite.get("status") == "listed"
                   for test in suite["testcases"].values()
                   if row.get("include_ignored") or not test.get("ignored"))
    return len(re.findall(r"^.+: test$", value, re.M))


def run(args) -> int:
    output = args.out.resolve()
    require(not output.exists() and not output.is_relative_to(ROOT), "fresh external owned --out required")
    output.mkdir(parents=True)
    model = definition()
    require(args.profile in model["profiles"], "unknown checked profile")
    manifest = make_freeze(args, model, output)
    if args.freeze_only:
        write(output / "job-receipt.json", {"status": "NOT_EXERCISED", "candidate": args.candidate,
              "contract_digest": manifest["contract_digest"], "profile": args.profile,
              "definition_sha256": digest(DEFINITION), "manifest_sha256": digest(output / "checked-manifest.json"),
              "records": [{"id": row["id"], "scenarios": row["scenarios"], "status": "NOT_EXERCISED",
                           "reason": "freeze-only; no production gate executed"} for row in manifest["commands"]],
              "reason": "freeze-only; no production gate executed"})
        seal(output)
        return 2
    env = environment()
    inputs = manifest["inputs"]
    env["RUSTUP_TOOLCHAIN"] = inputs.get("rust_toolchain", "1.99.1")
    for key, source in (("WASM_BROWSER_BIN", "chrome"), ("CHROMEDRIVER", "chromedriver"),
                        ("ANDROID_NDK_HOME", "android_ndk")):
        if source in inputs:
            env[key] = inputs[source]
    records = []
    lock = Path(os.environ.get("CARGO_TARGET_DIR", str(ROOT / "target"))) / ".oa18-writer-lock"
    lock.parent.mkdir(parents=True, exist_ok=True)
    acquired = False
    try:
        lock.mkdir()
        acquired = True
        write(lock / "owner.json", {"pid": os.getpid(), "candidate": args.candidate, "out": str(output)})
    except FileExistsError:
        pass
    start = time.monotonic()
    aborted = False
    for row in manifest["commands"]:
        record = {"id": row["id"], "scenarios": row["scenarios"], "status": "NOT_EXERCISED"}
        try:
            require(not aborted, "earlier timeout/source change; remaining commands not attempted")
            require(acquired, "concurrent/stale build lock; owner intervention required")
            require(model["profiles"][args.profile] in {"native", platform.system()}, "required OS unavailable")
            require(not row.get("blocked"), row.get("blocked", ""))
            require(time.monotonic() - start < model["budgets"]["active_minutes"] * 60
                    and len(records) < model["budgets"]["commands"], "budget exhausted; checkpoint")
            require(all(manifest["tools"][tool]["status"] == "PASS" for tool in row["tools"]),
                    "required installed tool/version/functional health unavailable")
            require(all(manifest["images"][image]["status"] == "PASS" for image in row.get("images", [])),
                    "pre-existing image identity unavailable; no build/pull authorized")
            require(all(Path(path).exists() for path in row.get("paths", [])), "preprovisioned path unavailable")
            require(all(any(r["id"] == dep and r["status"] == "PASS" for r in records)
                        for dep in row.get("depends", [])), "required earlier command did not PASS")
            if row.get("authority"):
                require(manifest["authority"][row["authority"]]["status"] == "PASS", "independent authority absent")
            if row["id"] == "oa02":
                preparation = manifest["fixture_inputs"]["preparation_plan"]
                require(preparation["status"] == "PASS", "bound original OA02 plan/ancestor unavailable")
                require(digest(Path(preparation["path"])) == preparation["sha256"],
                        "OA-18:changed-preparation-plan")
            if row["id"] == "containment-real":
                require(manifest["fixture_inputs"].get("fresh_mirror", {}).get("status") == "PASS",
                        "actual approved fresh-mirror identity unavailable")
            clean(args.candidate)
            require(sources() == manifest["source_sha256"], "OA-18:changed-source")
            for tool in row["tools"]:
                item = manifest["tools"][tool]
                require(digest(Path(item["path"])) == item["sha256"], "frozen tool executable changed")
                if item.get("selected_rustc"):
                    compiler = item["selected_rustc"]
                    require(digest(Path(compiler["path"])) == compiler["sha256"], "frozen compiler changed")
            if row.get("discovery_argv"):
                listing = execute(row["discovery_argv"], output, row["id"] + "-discovery", Path(row["cwd"]),
                                  {**env, **row.get("env", {})}, row["timeout"])
                record["discovery"] = listing
                require(listing["exit"] == 0, "build/discovery prerequisite failed")
                count = discovery_count((output / listing["stdout"]).read_text(), row)
                require(count > 0, "OA-18:zero-discovery: selected tests absent")
                record["selected_tests_before_run"] = count
                clean(args.candidate)
                require(sources() == manifest["source_sha256"], "OA-18:changed-source-after-discovery")
            proof = execute(row["argv"], output, row["id"], Path(row["cwd"]),
                            {**env, **row.get("env", {})}, row["timeout"])
            value = text(output, proof)
            record.update(command=proof, status="FAIL")
            if proof["exit"] in {"TIMEOUT", "UNAVAILABLE"}:
                aborted = True
                record["reason"] = "timeout/tool availability is not an assertion oracle"
            elif proof["exit"] != row.get("expected_exit", 0):
                record.update(status="BLOCKED" if re.search(r"\bBLOCKED[: ]", value) else "FAIL",
                              reason="command exit did not match checked expectation")
            elif row.get("contains") and row["contains"] not in value:
                record["reason"] = "designated oracle diagnostic absent"
            elif row.get("tests") and discovered(value, row["tests"]) == 0:
                record["reason"] = "OA-18:zero-discovery: no classified tests actually ran"
            else:
                record.update(status="PASS", discovered_tests=discovered(value, row.get("tests")))
                if row.get("receipt"):
                    child = Path(row["receipt"])
                    data = json.loads(child.read_bytes())
                    validate_child_receipt(data, row, args.candidate, manifest["contract_digest"])
                    record.update(child_receipt=child.relative_to(output).as_posix(), child_sha256=digest(child))
            clean(args.candidate)
            require(sources() == manifest["source_sha256"], "OA-18:changed-source")
        except (ReleaseError, OSError, ValueError, KeyError) as error:
            record.update(status="FAIL" if record.get("command") else
                          "NOT_EXERCISED" if aborted else "BLOCKED", reason=str(error))
            if "changed-" in str(error):
                aborted = True
        records.append(record)
        write(output / "progress.json", records)
    if acquired and not aborted:
        (lock / "owner.json").unlink()
        lock.rmdir()
    # A timeout keeps the lock as an explicit checkpoint; never delete live locks.
    receipt = {"schema_version": 1, "status": state(records), "candidate": args.candidate,
               "contract_digest": manifest["contract_digest"], "profile": args.profile,
               "definition_sha256": digest(DEFINITION), "manifest_sha256": digest(output / "checked-manifest.json"),
               "records": records, "finished": stamp(), "elapsed_seconds": time.monotonic() - start,
               "repair_attempts": 0, "automatic_repairs": False,
               "tool_call_accounting": "child internal calls not measurable; driver owns cumulative 3000-call budget"}
    write(output / "job-receipt.json", receipt)
    seal(output)
    return 0 if receipt["status"] == "PASS" else 2


def retained(path: Path, root: Path) -> bool:
    relative = path.relative_to(root)
    return not any(part in {".git", "workspace", "node_modules", "target"} for part in relative.parts)


def seal(root: Path) -> None:
    files = {p.relative_to(root).as_posix(): digest(p) for p in sorted(root.rglob("*"))
             if p.is_file() and not p.is_symlink() and p.name != "files-sha256.json" and retained(p, root)}
    write(root / "files-sha256.json", files)


def export(args) -> int:
    require(not args.out.exists(), "fresh export directory required")
    args.out.mkdir(parents=True)
    files = json.loads((args.receipts / "files-sha256.json").read_bytes())
    for name, expected in files.items():
        source = safe(args.receipts, name)
        require(digest(source) == expected, "raw job bytes changed before export")
        target = safe(args.out, name)
        target.parent.mkdir(parents=True, exist_ok=True)
        shutil.copyfile(source, target)
    shutil.copyfile(args.receipts / "files-sha256.json", args.out / "files-sha256.json")
    return 0


def safe(root: Path, name: str) -> Path:
    result = (root / name).resolve()
    require(result.is_relative_to(root.resolve()), "artifact path escapes owned packet")
    return result


def validate_frozen_command(row: dict, checked: dict, frozen: dict) -> None:
    """A receipt cannot replace a checked oracle/argv in its own frozen copy."""
    inputs = frozen["inputs"]
    require(set(inputs) <= INPUT_KEYS | {"workspace", "candidate", "contract", "out"}
            and all(isinstance(value, str) for value in inputs.values())
            and inputs["candidate"] == frozen["candidate"], "OA-18:changed-frozen-inputs")
    paths = ntpath if frozen["system"] == "Windows" else posixpath
    expected = copy.deepcopy(checked)
    expected["argv"] = [expand(value, inputs) for value in checked["argv"]]
    if checked.get("discovery_argv"):
        expected["discovery_argv"] = [expand(value, inputs) for value in checked["discovery_argv"]]
    expected["cwd"] = paths.normpath(paths.join(inputs["workspace"], expand(checked.get("cwd", "."), inputs)))
    expected["env"] = {key: expand(value, inputs) for key, value in checked.get("env", {}).items()}
    expected["paths"] = [paths.normpath(paths.join(inputs["workspace"], expand(value, inputs)))
                         for value in checked.get("paths", [])]
    expected["images"] = [expand(value, inputs) for value in checked.get("images", [])]
    if checked.get("receipt"):
        expected["receipt"] = expand(checked["receipt"], inputs)
    require(row == expected, "OA-18:changed-frozen-command")


def verify_job(folder: Path, model: dict, candidate: str, binding: str) -> dict:
    require(not any(p.is_symlink() for p in folder.rglob("*")), "OA-18:linked-raw-evidence")
    files = json.loads((folder / "files-sha256.json").read_bytes())
    require(set(files) == {p.relative_to(folder).as_posix() for p in folder.rglob("*")
                          if p.is_file() and p.name != "files-sha256.json"}, "OA-18:omitted-raw-evidence")
    for name, expected in files.items():
        require(digest(safe(folder, name)) == expected, "OA-18:altered-raw-evidence")
    receipt = json.loads((folder / "job-receipt.json").read_bytes())
    require(receipt["candidate"] == candidate and receipt["contract_digest"] == binding
            and receipt["definition_sha256"] == digest(DEFINITION), "OA-18:stale-receipt")
    frozen = json.loads((folder / "checked-manifest.json").read_bytes())
    require(digest(folder / "checked-manifest.json") == receipt["manifest_sha256"]
            and frozen["candidate"] == candidate and frozen["contract_digest"] == binding
            and frozen["source_sha256"] == sources(), "OA-18:stale-freeze")
    profile = receipt["profile"]
    require(profile in model["profiles"] and frozen["profile"] == profile
            and frozen.get("system") in {"Linux", "Darwin", "Windows"}
            and model["profiles"][profile] in {"native", frozen["system"]}, "OA-18:wrong-os-profile")
    required = {r["id"] for r in model["commands"] if r["profile"] == profile}
    require(len(frozen["commands"]) == len(required)
            and {r["id"] for r in frozen["commands"]} == required, "OA-18:omitted-frozen-command")
    records = receipt["records"]
    require(len(records) == len(required) and {r["id"] for r in records} == required,
            "OA-18:omitted-command")
    for record in records:
        checked_row = next(r for r in model["commands"] if r["id"] == record["id"])
        require(record["scenarios"] == checked_row["scenarios"], "OA-18:changed-command-scenarios")
        require(record["status"] in STATES, "OA-18:skipped-packet")
        if record["status"] != "PASS":
            continue
        row = next(r for r in frozen["commands"] if r["id"] == record["id"])
        validate_frozen_command(row, checked_row, frozen)
        command = record["command"]
        require(command["argv"] == row["argv"] and command["cwd"] == row["cwd"]
                and command["exit"] == row.get("expected_exit", 0), "OA-18:false-command-pass")
        for key in ("stdout", "stderr"):
            require(digest(safe(folder, command[key])) == command[key + "_sha256"], "OA-18:missing-log")
        if row.get("discovery_argv"):
            listing = record["discovery"]
            require(listing["argv"] == row["discovery_argv"] and listing["exit"] == 0,
                    "OA-18:missing-discovery")
            for stream in ("stdout", "stderr"):
                require(digest(safe(folder, listing[stream])) == listing[stream + "_sha256"], "OA-18:changed-discovery")
            count = discovery_count((folder / listing["stdout"]).read_text(), row)
            require(count > 0 and record["selected_tests_before_run"] == count, "OA-18:zero-discovery")
        if record["id"] == "oa02":
            preparation = frozen["fixture_inputs"]["preparation_plan"]
            original = folder / "original-release-plan.json"
            require(preparation["status"] == "PASS" and digest(original) == preparation["sha256"],
                    "OA-18:changed-preparation-plan")
            validate_preparation(ROOT, json.loads(original.read_bytes()), candidate, "0.4.0")
        output = text(folder, command)
        require(not row.get("contains") or row["contains"] in output, "OA-18:missing-oracle")
        require(not row.get("tests") or discovered(output, row["tests"]) > 0, "OA-18:zero-discovery")
        checked_row = next(r for r in model["commands"] if r["id"] == record["id"])
        if checked_row.get("receipt"):
            require(bool(record.get("child_receipt")), "OA-18:omitted-child-receipt")
            path = safe(folder, record["child_receipt"])
            require(digest(path) == record["child_sha256"], "OA-18:changed-child-receipt")
            validate_child_receipt(json.loads(path.read_bytes()), checked_row, candidate, binding)
    require(receipt["status"] == state(records), "OA-18:false-job-pass")
    return receipt


def validate_packet(packet: dict, model: dict, candidate: str, binding: str) -> None:
    require(packet["candidate"] == candidate and packet["contract_digest"] == binding,
            "OA-18:stale-packet")
    require(set(packet["profiles"]) == set(model["profiles"]), "OA-18:omitted-os-receipt")
    require(set(packet["scenarios"]) == set(model["required_scenarios"]), "OA-18:omitted-scenario")
    records = [*packet["profiles"].values(), *packet["scenarios"].values(), packet["baseline"],
               packet["review"], *packet["operator_gates"].values(), packet["provenance"]]
    require(all(r["status"] in STATES for r in records), "OA-18:skipped-packet")
    require(set(packet["operator_gates"]) == {"G-02", "G-03", "G-04"}, "OA-18:omitted-authority")
    require(packet["status"] == state(records), "OA-18:false-packet-pass")
    if packet["status"] == "PASS":
        require(all(r["status"] == "PASS" for r in records), "OA-18:nonpass-required")


def extract_archive(archive: Path, folder: Path) -> None:
    """Untrusted immutable artifact boundary: only bounded regular files/directories."""
    require(archive.stat().st_size <= 1024 ** 3 and not folder.exists(), "unsafe archive size/output")
    with zipfile.ZipFile(archive) as bundle:
        members = bundle.infolist()
        require(len(members) <= 20000, "raw archive member limit")
        seen = set()
        total = 0
        for info in members:
            name = info.filename.rstrip("/")
            require(name and "\\" not in name and ":" not in name and not name.startswith("/")
                    and all(part not in {"", ".", ".."} for part in name.split("/")),
                    "unsafe archive member path")
            require(name not in seen, "duplicate archive member")
            seen.add(name)
            kind = stat.S_IFMT(info.external_attr >> 16)
            require(kind in ({0, stat.S_IFDIR} if info.is_dir() else {0, stat.S_IFREG})
                    and not info.flag_bits & 1, "unsafe raw archive link/type/encryption")
            total += info.file_size
            require(total <= 4 * 1024 ** 3, "unsafe raw archive expanded size")
        folder.mkdir(parents=True)
        for info in members:
            target = safe(folder, info.filename)
            if info.is_dir():
                target.mkdir(parents=True, exist_ok=True)
            else:
                target.parent.mkdir(parents=True, exist_ok=True)
                with bundle.open(info) as source, target.open("xb") as dest:
                    shutil.copyfileobj(source, dest)


def profile_sources(model: dict) -> dict:
    expected = {**{p: "github-actions" for p in AUTOMATIC_PROFILES},
                **{p: "approved-manual-bundle" for p in MANUAL_PROFILES}}
    require(set(model["profiles"]) == set(expected) and model.get("profile_sources") == expected,
            "OA-18:changed-profile-sources")
    return expected


def import_manual_jobs(output: Path, model: dict, inputs: dict, candidate: str, binding: str) -> dict:
    """Import only independently pinned local bundles; no manual automatic overrides."""
    provenance = {}
    for profile, subject in MANUAL_PROFILES.items():
        try:
            require(profile_sources(model)[profile] == "approved-manual-bundle", "manual source differs")
            config_path, config, pin = authority_configuration(inputs, candidate, binding)
            entries = config["manual_profiles"]
            require(set(entries) == set(MANUAL_PROFILES), "manual profile inventory differs")
            entry = entries[profile]
            require(set(entry) == {"path", "sha256", "subject"} and entry["subject"] == subject,
                    "manual profile subject differs")
            archive = authority_path(config_path, entry["path"])
            require(not archive.is_symlink() and archive.is_file()
                    and digest(archive) == entry["sha256"], "manual archive digest differs")
            approved = authority(subject, inputs, candidate, binding)
            require(approved["status"] == "PASS" and entry["sha256"] in
                    {proof["sha256"] for proof in approved["raw"]},
                    "actual manual archive bytes lack independent authority coverage")
            retained_archive = output / (profile + ".zip")
            require(not retained_archive.exists(), "manual archive output already exists")
            shutil.copyfile(archive, retained_archive)
            require(digest(retained_archive) == entry["sha256"], "manual archive changed before import")
            folder = output / profile
            extract_archive(retained_archive, folder)
            require(digest(archive) == entry["sha256"], "manual archive changed during import")
            job = verify_job(folder, model, candidate, binding)
            require(job["profile"] == profile, "manual profile receipt differs")
            frozen = json.loads((folder / "checked-manifest.json").read_bytes())
            require(isinstance(frozen.get("frozen_at"), str) and isinstance(job.get("finished"), str),
                    "manual execution timestamps missing")
            frozen_at = dt.datetime.fromisoformat(frozen["frozen_at"].replace("Z", "+00:00"))
            finished = dt.datetime.fromisoformat(job["finished"].replace("Z", "+00:00"))
            now = dt.datetime.now(dt.timezone.utc)
            require(frozen_at.tzinfo is not None and finished.tzinfo is not None
                    and frozen_at <= finished <= now
                    and 0 <= (now - frozen_at).total_seconds() <= config["max_age_seconds"],
                    "manual bundle stale/future execution interval")
            require(frozen.get("host") and frozen.get("machine") and frozen.get("os"), "manual host identity missing")
            report = json.loads(Path(approved["report_path"]).read_bytes())
            require(report.get("profile") == profile and report.get("machine") == frozen["machine"]
                    and report.get("os") == frozen["os"] and report.get("host") == frozen["host"]
                    and dt.datetime.fromisoformat(report["observed_at"].replace("Z", "+00:00")) >= finished
                    and report.get("archive_sha256") == entry["sha256"],
                    "manual authority host/profile/archive binding differs")
            retain_authority(output, {subject: approved})
            provenance[profile] = {"status": "PASS", "source": "approved-manual-bundle",
                                   "archive_sha256": entry["sha256"], "authority_config_sha256": pin,
                                   "authority": approved}
        except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
            provenance[profile] = {"status": "BLOCKED", "reason": str(error)}
    return provenance


def fetch_jobs(output: Path, model: dict, candidate: str, policy: dict) -> dict:
    """Read only authenticated immutable artifacts from this approved first run."""
    require(os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("GITHUB_SHA") == candidate
            and os.environ.get("GITHUB_REF") == "refs/heads/main", "trusted protected-main context required")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    require(run_id.isdecimal(), "authenticated workflow run ID missing")
    api = GitHubAPI(policy["repository"])
    current = api.get(f"/actions/runs/{run_id}")
    validate_run(current, policy, candidate, BOUNDARY_WORKFLOW, completed=False)
    trust = {"status": "BLOCKED"}
    try:
        validate_policy(policy)
        require(api.workflow_digest(BOUNDARY_WORKFLOW, candidate) == policy["boundary_workflow_sha256"],
                "operator-approved boundary source digest differs")
        validate_approvals(api.get(f"/actions/runs/{run_id}/approvals"), policy["boundary_environment"])
        trust = {"status": "PASS", "workflow_sha256": policy["boundary_workflow_sha256"],
                 "environment": policy["boundary_environment"]["name"], "run_id": int(run_id)}
    except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        trust["reason"] = str(error)
    artifacts = []
    page = 1
    while True:
        batch = api.get(f"/actions/runs/{run_id}/artifacts?per_page=100&page={page}")["artifacts"]
        artifacts.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    provenance = {"trust": trust}
    for profile, source in profile_sources(model).items():
        if source != "github-actions":
            continue
        matches = [a for a in artifacts if a["name"] == "oa18-" + profile and not a["expired"]]
        if len(matches) != 1:
            provenance[profile] = {"status": "BLOCKED", "reason": "OA-18:omitted-os-artifact: " + profile}
            continue
        item = matches[0]
        artifact_id = item["id"]
        require(type(artifact_id) is int and item.get("workflow_run", {}).get("id") == int(run_id),
                "raw artifact run provenance differs")
        # Same fixed GitHub origin/redirect policy as release_evidence; raw logs
        # may be larger than its deliberately compact 32MiB consumer limit.
        url = "https://api.github.com/repos/" + policy["repository"] + f"/actions/artifacts/{artifact_id}/zip"
        headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": "2026-03-10"}
        if os.environ.get("GH_TOKEN"):
            headers["Authorization"] = "Bearer " + os.environ["GH_TOKEN"]
        request = urllib.request.Request(url, headers=headers)
        archive = output / (profile + ".zip")
        with api.opener.open(request, timeout=60) as response, archive.open("wb") as target:
            copied = 0
            while chunk := response.read(1024 * 1024):
                copied += len(chunk)
                require(copied <= 1024 ** 3, "raw artifact exceeds 1GiB owned packet limit")
                target.write(chunk)
        require(item.get("digest") == "sha256:" + digest(archive), "server raw artifact digest differs")
        folder = output / profile
        extract_archive(archive, folder)
        provenance[profile] = {"status": "PASS", "artifact_id": artifact_id, "run_id": int(run_id),
                               "archive_sha256": digest(archive), "source": BOUNDARY_WORKFLOW}
    return provenance


def assemble(args) -> int:
    clean(args.candidate)
    model = definition()
    binding = contract(args.contract)
    require(not args.out.exists() and not args.out.resolve().is_relative_to(ROOT), "fresh external packet output required")
    args.out.mkdir(parents=True)
    policy = json.loads(args.policy.read_bytes()) if args.policy else None
    provenance = {}
    inputs = json.loads(args.inputs.read_bytes()) if args.inputs else {}
    source = args.receipts
    provenance_error = None
    if args.trusted:
        try:
            if policy is None:
                policy = {}
            policy = {"repository": "Prometheus-AGS/prometheus-parking-lot-rs", "branch": "main", **policy}
            source = args.out / "jobs"
            source.mkdir()
            provenance = fetch_jobs(source, model, args.candidate, policy)
            provenance.update(import_manual_jobs(source, model, inputs, args.candidate, binding))
        except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
            provenance_error = str(error)
    profiles = {}
    records = []
    for profile in model["profiles"]:
        try:
            require(source is not None, "per-job evidence unavailable")
            job = verify_job(source / profile, model, args.candidate, binding)
            require(job["profile"] == profile, "OA-18:profile-receipt-differs")
            profiles[profile] = {"status": job["status"], "receipt_sha256": digest(source / profile / "job-receipt.json")}
            records += job["records"]
        except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
            profiles[profile] = {"status": "BLOCKED", "reason": str(error)}
    approved = {s: authority(s, inputs, args.candidate, binding) for s in model["required_authority"]}
    signed = None
    trust = None
    try:
        require(policy is not None and args.tag, "actual signed candidate tag/public trust unavailable")
        validate_policy(policy)
        signed, trust = verify_candidate_tag(ROOT, policy, args.candidate, args.tag)
        require(approved["G-03"]["status"] == "PASS", "independent G03 identity authority missing")
        report = json.loads(Path(approved["G-03"]["report_path"]).read_bytes())
        require(report.get("tag") == args.tag and report.get("public_trust_sha256") == trust,
                "G03 approved tag/public trust differs")
        write(args.out / "tag-verification.json", {**signed, "public_trust_sha256": trust})
    except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        approved["G-03"] = {"status": "BLOCKED", "subject": "G-03", "reason": str(error)}
    retain_authority(args.out, approved)
    scenarios = {}
    for scenario in model["required_scenarios"]:
        required = [r for r in model["commands"] if scenario in r["scenarios"]]
        actual = [r for r in records if scenario in r["scenarios"]]
        status = state(actual) if len(required) == len(actual) else "BLOCKED"
        scenarios[scenario] = {"status": status, "required_commands": [r["id"] for r in required],
                               "observed_commands": [r["id"] for r in actual]}
    for scenario, subject in (("OA-01", "G-02"), ("OA-03", "G-03"), ("OA-04", "G-04"),
                              ("OA-15", "OA15-GUI")):
        scenarios[scenario]["status"] = state([scenarios[scenario], approved[subject]])
    baseline_rows = [r for r in records if not r["scenarios"]]
    baseline_required = [r for r in model["commands"] if not r["scenarios"]]
    packet = {"schema_version": 1, "candidate": args.candidate, "contract_digest": binding,
              "definition_sha256": digest(DEFINITION), "profiles": profiles, "scenarios": scenarios,
              "public_trust_policy_sha256": digest(args.policy) if args.policy else None,
              "baseline": {"status": state(baseline_rows) if len(baseline_rows) == len(baseline_required) else "BLOCKED"},
              "review": approved["review"], "operator_gates": {s: approved[s] for s in ("G-02", "G-03", "G-04")},
              "provenance": {"status": "PASS" if provenance and not provenance_error
                             and all(p["status"] == "PASS" for p in provenance.values()) else "BLOCKED",
                             "artifacts": provenance, "reason": provenance_error or
                             (None if args.trusted and provenance.get("trust", {}).get("status") == "PASS"
                              else "operator-pinned remote authority unavailable; local data alone establishes none")},
              "implementation": "SOURCE_DELIVERED", "publication": "NOT_EXERCISED",
              "rotation": "G-01 operator-deferred/nonblocking; no credential retirement claimed",
              "created": stamp(), "budgets": model["budgets"]}
    packet["status"] = state([*profiles.values(), *scenarios.values(), packet["baseline"], packet["review"],
                              *packet["operator_gates"].values(), packet["provenance"]])
    validate_packet(packet, model, args.candidate, binding)
    write(args.out / "packet.json", packet)
    compact_reason = "no compact artifact: real tag/preparation/provenance unavailable"
    try:
        require(args.trusted and packet["provenance"]["status"] == "PASS", compact_reason)
        require(policy["contract_digest"] == binding, "operator-approved contract digest differs")
        require(args.tag and args.preparation_plan and policy is not None, compact_reason)
        require(all(approved[s]["status"] == "PASS" for s in ("G-02", "G-03", "G-04")),
                "compact artifact blocked: independently authenticated G02/G03/G04 authority missing")
        require(signed is not None and trust is not None, "actual signed candidate verification missing")
        plan_bytes = args.preparation_plan.read_bytes()
        baseline_freeze = json.loads((source / "baseline" / "checked-manifest.json").read_bytes())
        preparation = baseline_freeze["fixture_inputs"]["preparation_plan"]
        require(preparation["status"] == "PASS" and preparation["sha256"] == sha256(plan_bytes)
                and digest(source / "baseline" / "original-release-plan.json") == sha256(plan_bytes),
                "OA-18:changed-preparation-plan: collector must retain the baseline's exact original plan")
        validate_preparation(ROOT, json.loads(plan_bytes), args.candidate, "0.4.0")
        require(args.purpose == "rehearsal" or packet["status"] == "PASS", "required full packet is not PASS")
        checked = {p: digest(source / p / "checked-manifest.json") for p in profiles}
        write(args.out / "checked-environment-manifests.json", checked)
        observed = args.out / "observed-receipts"
        observed.mkdir()
        serial = 0
        def oracle(value):
            nonlocal serial
            serial += 1
            path = observed / f"{serial:02}.json"
            write(path, value)
            # The shared compact rehearsal vocabulary cannot encode BLOCKED.
            # Preserve full state in packet.json; non-PASS is NOT_EXERCISED here,
            # and rehearsal never authorizes publication.
            status = "PASS" if value["status"] == "PASS" else "NOT_EXERCISED"
            return {"status": status, **({"receipt_sha256": digest(path)} if status == "PASS" else {})}
        receipt = {"schema_version": 1, "purpose": args.purpose, "candidate": args.candidate,
                   "contract_digest": binding, "tag": args.tag, "tag_object": signed["tag_object"],
                   "version": "0.4.0", "manifest_sha256": digest(args.out / "checked-environment-manifests.json"),
                   "preparation_plan_sha256": sha256(plan_bytes),
                   "tag_verification": {**signed, "public_trust_sha256": trust},
                   "scenarios": [{"id": s, **oracle(scenarios[s])} for s in model["required_scenarios"]],
                   "baseline": oracle(packet["baseline"]), "review": oracle(packet["review"]),
                   "operator_gates": {s: oracle(packet["operator_gates"][s]) for s in ("G-02", "G-03", "G-04")},
                   "status": "PASS" if args.purpose == "certification" else "NOT_EXERCISED"}
        validate_receipt(receipt, args.candidate, binding, args.tag, "0.4.0", publication=args.purpose == "certification")
        compact = args.out / "compact"
        compact.mkdir()
        write(compact / "boundary-receipt.json", receipt)
        (compact / "release-plan.json").write_bytes(plan_bytes)
        compact_reason = "compact " + args.purpose + " artifact authored from observed packet; publication NOT_EXERCISED"
    except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        compact_reason = str(error)
    write(args.out / "disposition.json", {"packet_status": packet["status"], "compact": compact_reason,
          "compact_present": (args.out / "compact").exists(), "publication": "NOT_EXERCISED"})
    seal(args.out)
    return 0 if (args.out / "compact").exists() else 2


def observe_quiet(args) -> int:
    """Observe an already built, dedicated host; never stop/tune any process."""
    require(platform.system() in {"Linux", "Darwin"}, "quiet measurement supports Linux/macOS")
    binding = contract(args.contract)
    clean(args.candidate)
    inputs = json.loads(args.inputs.read_bytes())
    path = ROOT / "benches/pg_measure.py"
    spec = importlib.util.spec_from_file_location("oa17_measure", path)
    require(spec is not None and spec.loader is not None, "OA17 source helper absent")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    snapshot = args.out.with_suffix(".ps")
    observed = module.quiet_snapshot(snapshot)
    processes = snapshot.read_text()
    names = [Path(parts[2]).name.lower() for line in processes.splitlines()
             if len(parts := line.strip().split(None, 2)) == 3]
    require(not any(name.startswith(("pg_backend", "components-", "throughput-", "latency-", "utilization-"))
                    for name in names), "other benchmark process present; no process is killed")
    require(not set(names) & {"cpupower", "tuned", "taskset", "numactl"},
            "known host-tuning process present; no host settings are changed")
    if inputs.get("quiet_attestation"):
        approved = authority("OA17-QUIET", inputs, args.candidate, binding)
        require(approved["status"] == "PASS", "external quiet evidence lacks independent binding")
        attestation = json.loads(Path(inputs["quiet_attestation"]).read_bytes())
        attestation["external_authority"] = approved
    else:
        expected = "Prometheus-AGS/prometheus-parking-lot-rs/" + BOUNDARY_WORKFLOW + "@refs/heads/main"
        require(os.environ.get("GITHUB_ACTIONS") == "true" and os.environ.get("GITHUB_SHA") == args.candidate
                and os.environ.get("GITHUB_REF") == "refs/heads/main"
                and os.environ.get("GITHUB_RUN_ATTEMPT") == "1"
                and os.environ.get("GITHUB_WORKFLOW_REF") == expected,
                "automatic quiet observations require the dedicated same-run workflow; local external evidence must be bound")
        # These are derived observations under approved workflow source, not
        # caller approval flags. pg_measure rechecks process/load snapshots for
        # every actual sample; unknown/wrapped agents remain a declared limit.
        attestation = {"operator": "workflow:" + os.environ["GITHUB_REPOSITORY"] + "/actions/runs/" + os.environ["GITHUB_RUN_ID"],
                       "machine_id": os.environ.get("RUNNER_NAME", platform.node()) + ":" + platform.node(),
                       "no_active_agents": True, "no_concurrent_builds": True,
                       "no_other_benchmarks": True, "no_host_tuning": True,
                       "source": "actual process snapshot after builds plus dedicated workflow no-tuning source",
                       "no_host_tuning_scope": "no setting mutation by this workflow and no known tuning process observed",
                       "limit": "known process names/snapshots cannot prove absence of every wrapper or between-snapshot activity"}
    attestation.update(candidate=args.candidate, contract_digest=binding, observed_at=stamp(),
                       process_observation=observed, process_snapshot_sha256=digest(snapshot),
                       measurement_source_sha256=digest(path), workflow_job=os.environ.get("GITHUB_JOB"))
    write(args.out, attestation)
    print("OBSERVED: quiet attestation from actual host/process evidence; measurement remains unexecuted")
    return 0


def android_elf() -> int:
    path = ROOT / "target/aarch64-linux-android/debug/libparking_lot_ffi.so"
    ndk = Path(os.environ["ANDROID_NDK_HOME"])
    result = subprocess.run(["file", str(path)], capture_output=True, text=True, check=False)
    print(result.stdout, end="")
    require(result.returncode == 0 and "ARM aarch64" in result.stdout, "Android ARM ELF missing")
    tool = ndk / "toolchains/llvm/prebuilt/linux-x86_64/bin/llvm-readelf"
    result = subprocess.run([str(tool), "--dyn-syms", "--wide", str(path)], capture_output=True, text=True, check=False)
    require(result.returncode == 0 and "uniffi_parking_lot_ffi_fn_func_open_store" in result.stdout,
            "Android UniFFI export missing")
    subprocess.run([str(tool), "-l", str(path)], check=True)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="operation", required=True)
    run_parser = sub.add_parser("run")
    run_parser.add_argument("--candidate", required=True)
    run_parser.add_argument("--contract", type=Path, default=CONTRACT)
    run_parser.add_argument("--out", type=Path, required=True)
    run_parser.add_argument("--profile", default="baseline")
    run_parser.add_argument("--inputs", type=Path)
    run_parser.add_argument("--freeze-only", action="store_true")
    collect = sub.add_parser("assemble")
    for name in ("candidate",):
        collect.add_argument("--" + name, required=True)
    collect.add_argument("--contract", type=Path, default=CONTRACT)
    collect.add_argument("--out", type=Path, required=True)
    collect.add_argument("--receipts", type=Path)
    collect.add_argument("--inputs", type=Path)
    collect.add_argument("--policy", type=Path)
    collect.add_argument("--preparation-plan", type=Path)
    collect.add_argument("--tag")
    collect.add_argument("--trusted", action="store_true")
    collect.add_argument("--purpose", choices=("rehearsal", "certification"), default="rehearsal")
    copy_parser = sub.add_parser("export")
    copy_parser.add_argument("--receipts", type=Path, required=True)
    copy_parser.add_argument("--out", type=Path, required=True)
    quiet_parser = sub.add_parser("observe-quiet")
    quiet_parser.add_argument("--candidate", required=True)
    quiet_parser.add_argument("--contract", type=Path, default=CONTRACT)
    quiet_parser.add_argument("--inputs", type=Path, required=True)
    quiet_parser.add_argument("--out", type=Path, required=True)
    sub.add_parser("android-elf")
    args = parser.parse_args()
    try:
        return {"run": run, "assemble": assemble, "export": export,
                "android-elf": lambda _: android_elf(), "observe-quiet": observe_quiet}[args.operation](args)
    except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile) as error:
        print("OA-18 BLOCKED: " + str(error), file=sys.stderr)
        return 2


if __name__ == "__main__":
    sys.exit(main())
