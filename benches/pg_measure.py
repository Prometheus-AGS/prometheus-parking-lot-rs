#!/usr/bin/env python3
"""OA-17 disposable PG sampling and strict report tooling. Execution: OA-18 only.

No build or measurement is performed on import. Stage/build happen before the
quiet interval. Measurement invokes already-built clients, never Cargo; every
sample gets a fresh Docker database. No image pull/build, host tuning or live
store is supported. RSS is exact client-process wait4 peak, not DB/VM memory.
"""

import argparse
import difflib
import hashlib
import json
import math
import os
from pathlib import Path
import platform
import random
import secrets
import signal
import subprocess
import sys
import time

BASELINE = "92da0d3d28f7896ddc67062fa49ba40ea48d74a3"
CASES = ("queue", "mailbox_unique", "mailbox_duplicate", "durable")
CLIENTS = (1, 8)
FEATURES = ("postgres", "tokio-runtime")
OPERATIONS = 1024
SLOW_US = 100_000
STANZA = '[[bench]]\nname = "pg_backend"\nharness = false\nrequired-features = ["tokio-runtime", "postgres"]\n'
ANCHOR = '[[bench]]\nname = "utilization"\nharness = false\n'
BUILD_ARGV = ["cargo", "bench", "-p", "prometheus_parking_lot", "--locked",
              "--no-default-features", "--features", "tokio-runtime,postgres",
              "--bench", "pg_backend", "--no-run", "--message-format=json"]
FORBIDDEN = {"cargo", "rustc", "rustdoc", "clippy-driver", "cargo-nextest", "codex", "claude", "aider"}
RSS_SCOPE = "benchmark client process including startup/migration/warmup/shutdown; excludes PostgreSQL/Docker/runner"


def require(condition, code, message):
    if not condition:
        raise ValueError(f"OA-17:{code}: {message}")


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), allow_nan=False).encode()


def digest(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def save(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True, allow_nan=False) + "\n")


def load(path):
    return json.loads(Path(path).read_text())


def command(argv, cwd=None, env=None, strip=True):
    output = subprocess.run(argv, cwd=cwd, env=env, check=True, capture_output=True, text=True, timeout=120).stdout
    return output.strip() if strip else output


def git(root, *args):
    return command(["git", "-C", str(root), *args])


def stage_baseline(args):
    root = Path(args.root).resolve()
    require(git(root, "rev-parse", "HEAD") == BASELINE, "baseline", "not the frozen 0.3 commit")
    require(not git(root, "status", "--porcelain", "--untracked-files=all"), "baseline", "scratch baseline must start clean")
    original = (root / "Cargo.toml").read_text()
    require(original.count(ANCHOR) == 1 and STANZA not in original, "baseline", "unexpected manifest base")
    source = Path(args.candidate_root).resolve() / "benches/pg_backend.rs"
    destination = root / "benches/pg_backend.rs"
    require(not destination.exists(), "baseline", "benchmark destination already exists")
    amended = original.replace(ANCHOR, ANCHOR + "\n" + STANZA, 1)
    destination.write_bytes(source.read_bytes())
    (root / "Cargo.toml").write_text(amended)
    patch = "".join(difflib.unified_diff(original.splitlines(True), amended.splitlines(True),
                                       fromfile="a/Cargo.toml", tofile="b/Cargo.toml"))
    save(args.out, {"schema": 1, "baseline": BASELINE, "harness_sha256": digest(destination),
                    "manifest_patch": patch, "manifest_sha256": digest(root / "Cargo.toml"),
                    "production_code_modified": False})


def source_identity(root, arm, revision):
    require(git(root, "rev-parse", "HEAD") == revision and len(revision) == 40,
            "revision", "freeze exact full source commit")
    changed = set(git(root, "diff", "--name-only", "HEAD").splitlines())
    untracked = set(git(root, "ls-files", "--others", "--exclude-standard").splitlines())
    if arm == "baseline":
        require(revision == BASELINE, "baseline", "baseline production revision differs")
        require(changed == {"Cargo.toml"} and untracked == {"benches/pg_backend.rs"},
                "baseline", "only the declared harness/manifest injection is allowed")
        original = command(["git", "-C", str(root), "show", "HEAD:Cargo.toml"], strip=False)
        require(original.count(ANCHOR) == 1, "baseline", "manifest anchor changed")
        require((root / "Cargo.toml").read_text() == original.replace(ANCHOR, ANCHOR + "\n" + STANZA, 1),
                "baseline", "baseline manifest has an undeclared change")
    else:
        require(not changed and not untracked, "candidate", "candidate must be frozen and clean")
    return {"revision": revision, "arm": arm, "root": str(root),
            "harness_sha256": digest(root / "benches/pg_backend.rs"),
            "manifest_sha256": digest(root / "Cargo.toml"), "lock_sha256": digest(root / "Cargo.lock"),
            "production_diff": git(root, "diff", "HEAD", "--", "Cargo.toml")}


def build(args):
    root = Path(args.root).resolve()
    identity = source_identity(root, args.arm, args.revision)
    require(not os.environ.get("RUSTFLAGS") and not os.environ.get("CARGO_ENCODED_RUSTFLAGS"),
            "cfg", "model/negative/ambient Rust flags are forbidden")
    out = Path(args.out).resolve()
    log = out.with_suffix(".cargo.jsonl")
    stderr = out.with_suffix(".cargo.stderr")
    require(not out.exists() and not log.exists() and not stderr.exists(), "overwrite", "new build receipt paths required")
    with log.open("w") as stdout, stderr.open("w") as errors:
        env = os.environ.copy()
        env["CARGO_TARGET_DIR"] = str(root / "target/oa17-build")
        result = subprocess.run(BUILD_ARGV, cwd=root, stdout=stdout, stderr=errors, env=env, check=False)
    require(result.returncode == 0, "build", f"compile failed ({result.returncode}); retain {stderr}")
    require(source_identity(root, args.arm, args.revision) == identity, "build-binding", "source changed during compilation")
    artifacts = []
    for line in log.read_text().splitlines():
        item = json.loads(line)
        if item.get("reason") == "compiler-artifact" and item["target"]["name"] == "pg_backend" and item.get("executable"):
            artifacts.append(item)
    require(len(artifacts) == 1, "build", "exactly one linked benchmark artifact required")
    artifact = artifacts[0]
    require(sorted(artifact["features"]) == list(FEATURES), "features", "unexpected compiled feature set")
    executable = Path(artifact["executable"]).resolve()
    identity.update({"schema": 1, "argv": BUILD_ARGV, "exit": result.returncode,
                     "executable": str(executable), "executable_sha256": digest(executable),
                     "features": artifact["features"], "profile": artifact["profile"],
                     "target_dir": str(root / "target/oa17-build"), "rustflags": "",
                     "rustc": command(["rustc", "-Vv"], root), "cargo": command(["cargo", "-V"], root),
                     "tool_sha256": digest(__file__), "build_log": str(log), "build_log_sha256": digest(log),
                     "build_stderr": str(stderr), "build_stderr_sha256": digest(stderr)})
    require("rustc 1.99.1 " in identity["rustc"], "toolchain", "pinned toolchain required")
    save(out, identity)


def cpu_identity():
    if platform.system() == "Darwin":
        return {"model": command(["sysctl", "-n", "machdep.cpu.brand_string"]),
                "hardware": command(["sysctl", "-n", "hw.model"]),
                "memory_bytes": int(command(["sysctl", "-n", "hw.memsize"]))}
    require(platform.system() == "Linux", "host", "wait4 RSS collection supports Linux/macOS only")
    fields = {}
    for line in Path("/proc/cpuinfo").read_text().splitlines():
        if ":" in line:
            key, value = (part.strip() for part in line.split(":", 1))
            if key in {"model name", "CPU implementer", "CPU architecture", "CPU part", "Hardware"}:
                fields.setdefault(key, set()).add(value)
    require(bool(fields), "host", "CPU identity unavailable")
    return {"cpu_fields": {key: sorted(values) for key, values in fields.items()},
            "memory_bytes": os.sysconf("SC_PHYS_PAGES") * os.sysconf("SC_PAGE_SIZE")}


def machine(image):
    inspected = json.loads(command(["docker", "image", "inspect", image]))[0]
    info = json.loads(command(["docker", "info", "--format", "{{json .}}"] ))
    return {"os": platform.platform(), "uname": list(platform.uname()), "cpu": cpu_identity(),
            "logical_cpus": os.cpu_count(), "affinity": sorted(os.sched_getaffinity(0)) if hasattr(os, "sched_getaffinity") else None,
            "python": sys.version, "docker_version": command(["docker", "version"]),
            "docker_vm": {key: info.get(key) for key in ("Architecture", "OSType", "OperatingSystem", "NCPU", "MemTotal", "KernelVersion", "ServerVersion", "Name", "Driver")},
            "image": {"id": inspected["Id"], "repo_digests": sorted(inspected.get("RepoDigests") or []),
                      "os": inspected["Os"], "architecture": inspected["Architecture"]}}


def quiet_snapshot(path):
    snapshot = command(["ps", "-axo", "pid=,ppid=,comm="])
    Path(path).write_text(snapshot + "\n")
    require_quiet_processes(snapshot)
    return {"path": Path(path).name, "sha256": digest(path), "load_average": list(os.getloadavg())}


def require_quiet_processes(snapshot):
    found = []
    for line in snapshot.splitlines():
        fields = line.strip().split(None, 2)
        if len(fields) == 3 and Path(fields[2]).name.lower() in FORBIDDEN:
            found.append(fields[2])
    require(not found, "quiet", f"agent/build process present: {found}; never kill it automatically")


def checked_receipt(path):
    receipt = load(path)
    for key, file_key in (("executable", "executable_sha256"), ("build_log", "build_log_sha256"), ("build_stderr", "build_stderr_sha256")):
        require(digest(receipt[key]) == receipt[file_key], "build-binding", f"changed {key}")
    require(receipt["tool_sha256"] == digest(__file__), "build-binding", "measurement tool changed")
    root = Path(receipt["root"])
    require(source_identity(root, receipt["arm"], receipt["revision"])["harness_sha256"] == receipt["harness_sha256"],
            "build-binding", "source changed since build")
    return receipt


def child_sample(executable, case, clients, delay, image_id, directory, stem, capture_deadline):
    password = secrets.token_hex(24)
    container = command(["docker", "run", "--pull=never", "--detach", "--publish", "127.0.0.1::5432",
                         "--env", "POSTGRES_PASSWORD", image_id],
                        env=dict(os.environ, POSTGRES_PASSWORD=password))
    try:
        port = int(command(["docker", "port", container, "5432/tcp"]).split(":")[-1])
        url = f"postgres://postgres:{password}@127.0.0.1:{port}/postgres"
        inspected = json.loads(command(["docker", "inspect", container]))[0]
        resources = {key: inspected["HostConfig"].get(key) for key in
                     ("NanoCpus", "CpuQuota", "CpuPeriod", "CpusetCpus", "Memory", "MemorySwap", "ShmSize")}
        postgres = command(["docker", "exec", container, "postgres", "--version"])
        stdout_path, stderr_path = directory / f"{stem}.stdout", directory / f"{stem}.stderr"
        argv = [executable, "--case", case, "--clients", str(clients)]
        env = os.environ.copy()
        env.update(OA17_DATABASE_URL=url, OA17_CONTROL_DELAY_US=str(delay))
        started_ns = time.time_ns()
        require(time.monotonic() < capture_deadline, "capture-timeout", "capture resource envelope exhausted")
        with stdout_path.open("w") as stdout, stderr_path.open("w") as stderr:
            process = subprocess.Popen(argv, stdout=stdout, stderr=stderr, env=env, start_new_session=True)
            deadline = min(time.monotonic() + 2400, capture_deadline)
            timed_out = False
            while True:
                pid, status, usage = os.wait4(process.pid, os.WNOHANG)
                if pid:
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                if time.monotonic() >= deadline:
                    timed_out = True
                    os.killpg(process.pid, signal.SIGKILL)
                    _, status, usage = os.wait4(process.pid, 0)
                    process.returncode = os.waitstatus_to_exitcode(status)
                    break
                time.sleep(0.05)
        for path in (stdout_path, stderr_path):
            path.write_text(path.read_text().replace(url, "<disposable-db-url>").replace(password, "<redacted>"))
        require(not timed_out and process.returncode == 0, "sample-process", f"{stem}: exit {process.returncode}; timeout={timed_out}")
        raw = json.loads(stdout_path.read_text())
        return {"raw": raw, "argv": argv, "exit": process.returncode, "timed_out": timed_out,
                "client_started_unix_ns": started_ns, "client_ended_unix_ns": time.time_ns(),
                "peak_rss_bytes": usage.ru_maxrss * (1024 if platform.system() == "Linux" else 1),
                "rss_raw": {"ru_maxrss": usage.ru_maxrss, "native_unit": "KiB" if platform.system() == "Linux" else "bytes", "method": "wait4", "pid": process.pid},
                "rss_scope": RSS_SCOPE,
                "postgres": postgres, "container_id": container, "container_resources": resources,
                "stdout": stdout_path.name, "stdout_sha256": digest(stdout_path),
                "stderr": stderr_path.name, "stderr_sha256": digest(stderr_path)}
    finally:
        command(["docker", "rm", "--force", container])


def measure(args):
    require(args.samples >= 5, "sample-count", "at least five pairs are mandatory")
    require(args.max_wall_seconds > 0, "capture-timeout", "positive bounded capture envelope required")
    require(hasattr(os, "wait4"), "rss", "per-child wait4 peak RSS unavailable")
    baseline, candidate = checked_receipt(args.baseline_receipt), checked_receipt(args.candidate_receipt)
    require(baseline["arm"] == "baseline" and candidate["arm"] == "candidate", "arms", "receipt arms reversed")
    for field in ("harness_sha256", "features", "profile", "rustc", "cargo", "tool_sha256"):
        require(baseline[field] == candidate[field], "build-binding", f"incomparable {field}")
    attestation = load(args.quiet_attestation)
    require(all(attestation.get(key) is True for key in ("no_active_agents", "no_concurrent_builds", "no_other_benchmarks", "no_host_tuning"))
            and bool(attestation.get("operator")) and bool(attestation.get("machine_id")), "quiet", "explicit quiet-machine attestation required")
    directory = Path(args.out).resolve()
    directory.mkdir(parents=True, exist_ok=False)
    capture_deadline = time.monotonic() + args.max_wall_seconds
    identity = machine(args.image)
    cases = ["mailbox_duplicate"] if args.slow_control else list(CASES)
    clients = [8] if args.slow_control else list(CLIENTS)
    manifest = {"schema": 1, "mode": "slow-control" if args.slow_control else "normal", "samples": args.samples,
                "cases": cases, "clients": clients, "machine": identity, "attestation": attestation,
                "baseline": baseline, "candidate": candidate, "records": [], "build_logs": {},
                "started_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
                "max_wall_seconds": args.max_wall_seconds, "sample_watchdog_seconds": 2400,
                "docker_command_watchdog_seconds": 120}
    for arm, receipt in (("baseline", baseline), ("candidate", candidate)):
        manifest["build_logs"][arm] = {}
        for field in ("build_log", "build_stderr"):
            destination = directory / f"{arm}-{field}.log"
            destination.write_bytes(Path(receipt[field]).read_bytes())
            manifest["build_logs"][arm][field] = {"path": destination.name, "sha256": digest(destination)}
    try:
        for case in cases:
            for count in clients:
                for pair in range(args.samples):
                    # Alternate pair order; each arm still uses a fresh DB.
                    arms = [("baseline", baseline), ("candidate", candidate)]
                    if pair % 2:
                        arms.reverse()
                    for arm, receipt in arms:
                        require(time.monotonic() < capture_deadline, "capture-timeout", "capture resource envelope exhausted")
                        stem = f"{case}-{count}-{pair}-{arm}"
                        before = quiet_snapshot(directory / f"{stem}.before.ps")
                        require(machine(args.image) == identity, "machine-identity", "machine/image changed during collection")
                        delay = SLOW_US if args.slow_control and arm == "candidate" else 0
                        row = child_sample(receipt["executable"], case, count, delay, identity["image"]["id"], directory, stem, capture_deadline)
                        after = quiet_snapshot(directory / f"{stem}.after.ps")
                        require(machine(args.image) == identity, "machine-identity", "machine/image changed during sample")
                        row.update(arm=arm, case=case, clients=count, pair=pair, machine=identity,
                                   executable_sha256=receipt["executable_sha256"], before=before, after=after)
                        file = directory / f"{stem}.json"
                        save(file, row)
                        manifest["records"].append({"path": file.name, "sha256": digest(file)})
        require(time.monotonic() < capture_deadline, "capture-timeout", "capture resource envelope exhausted")
        manifest["ended_utc"] = time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())
        save(directory / "manifest.json", manifest)
    except Exception as error:
        save(directory / "failure.json", {"disposition": "NOT_CERTIFIED", "error": str(error), "partial_manifest": manifest})
        raise


def percentile(values, fraction):
    values = sorted(values)
    return values[max(0, math.ceil(len(values) * fraction) - 1)]


def expected_workload(case, clients):
    return {"case": case, "clients": clients, "operations": OPERATIONS, "warmup": 128,
            "payload_bytes": 256, "runtime_threads": 4, "max_units": 16, "queue_max_depth": 1_000_000,
            "lease_ms": 30_000, "poll_ms": 1, "priority": "Normal", "cost_units": 1,
            "sqlx_max_connections": 10, "seed": "sequential-ids-v1"}


def validate_machine(identity):
    require(identity.get("os") and identity.get("python") and len(identity.get("uname", [])) == 6 and
            identity["uname"][0] in {"Linux", "Darwin"} and identity.get("logical_cpus", 0) > 0,
            "machine-identity", "OS/Python/CPU-thread identity incomplete")
    cpu, vm, image = identity.get("cpu", {}), identity.get("docker_vm", {}), identity.get("image", {})
    require(cpu.get("memory_bytes", 0) > 0 and (cpu.get("model") or cpu.get("cpu_fields")) and
            vm.get("NCPU", 0) > 0 and vm.get("MemTotal", 0) > 0 and identity.get("docker_version"),
            "machine-identity", "CPU/memory/Docker VM identity incomplete")
    require(image.get("id", "").startswith("sha256:") and len(image["id"]) == 71 and image.get("os") and image.get("architecture") and
            isinstance(image.get("repo_digests"), list), "machine-identity", "immutable image identity incomplete")


def validate(directory, allow_slow=False):
    directory = Path(directory)
    manifest = load(directory / "manifest.json")
    slow = manifest["mode"] == "slow-control"
    require(manifest["mode"] in {"normal", "slow-control"} and (not slow or allow_slow), "control", "normal report cannot certify a perturbed workload")
    require(manifest["samples"] >= 5, "sample-count", "fewer than five pairs")
    validate_machine(manifest["machine"])
    require(manifest["baseline"]["revision"] == BASELINE and len(manifest["candidate"]["revision"]) == 40,
            "revision", "baseline/candidate identity missing")
    require(all(manifest["attestation"].get(key) is True for key in
                ("no_active_agents", "no_concurrent_builds", "no_other_benchmarks", "no_host_tuning")) and
            manifest["attestation"].get("operator") and manifest["attestation"].get("machine_id"),
            "quiet", "quiet-machine attestation missing")
    for field in ("harness_sha256", "features", "profile", "rustc", "cargo", "tool_sha256"):
        require(manifest["baseline"][field] == manifest["candidate"][field], "build-binding", f"incomparable {field}")
    require(sorted(manifest["baseline"]["features"]) == list(FEATURES), "features", "feature identity changed")
    require(manifest["baseline"]["tool_sha256"] == digest(__file__) and
            "rustc 1.99.1 " in manifest["baseline"]["rustc"], "build-binding", "tool/source pin differs")
    for arm in ("baseline", "candidate"):
        for field in ("build_log", "build_stderr"):
            record = manifest["build_logs"][arm][field]
            file = directory / record["path"]
            require(file.is_file() and digest(file) == record["sha256"] == manifest[arm][field + "_sha256"],
                    "build-binding", "raw build log missing/changed")
    require(manifest["cases"] == (["mailbox_duplicate"] if slow else list(CASES)) and
            manifest["clients"] == ([8] if slow else list(CLIENTS)), "workload-identity", "matrix changed")
    expected = {(case, clients, pair, arm) for case in manifest["cases"] for clients in manifest["clients"]
                for pair in range(manifest["samples"]) for arm in ("baseline", "candidate")}
    require(len(manifest["records"]) == len(expected), "sample-count", "raw sample dropped or added")
    rows, seen = [], set()
    backend_identity = None
    for record in manifest["records"]:
        file = directory / record["path"]
        require(file.is_file(), "missing-raw", f"missing {file.name}")
        require(digest(file) == record["sha256"], "raw-hash", f"changed {file.name}")
        row = load(file)
        key = (row["case"], row["clients"], row["pair"], row["arm"])
        require(key in expected and key not in seen, "sample-count", f"duplicate/unexpected sample {key}")
        seen.add(key)
        require(row["machine"] == manifest["machine"], "machine-identity", "sample machine/image differs")
        require(row["executable_sha256"] == manifest[row["arm"]]["executable_sha256"], "build-binding", "binary changed")
        require(row["exit"] == 0 and row["timed_out"] is False, "sample-process", "failed sample cannot be reported")
        require(type(row["peak_rss_bytes"]) is int and row["peak_rss_bytes"] > 0, "rss", "peak RSS unavailable")
        rss_unit = "KiB" if manifest["machine"]["uname"][0] == "Linux" else "bytes"
        require(row["rss_scope"] == RSS_SCOPE and row["rss_raw"]["method"] == "wait4" and row["rss_raw"]["native_unit"] == rss_unit and
                row["peak_rss_bytes"] == row["rss_raw"]["ru_maxrss"] * (1024 if rss_unit == "KiB" else 1),
                "rss", "RSS scope/native units disagree")
        raw = row["raw"]
        require(raw["workload"] == expected_workload(row["case"], row["clients"]), "workload-identity", "workload/connection count differs")
        require(raw["control_delay_us"] == (SLOW_US if slow and row["arm"] == "candidate" else 0), "control", "unrecorded perturbation")
        require(raw["verified_operations"] == OPERATIONS and len(raw["latencies_ns"]) == OPERATIONS and
                all(type(value) is int and value > 0 for value in raw["latencies_ns"]) and
                type(raw["elapsed_ns"]) is int and raw["elapsed_ns"] >= max(raw["latencies_ns"]),
                "raw-latencies", "incomplete raw operation samples")
        for field in ("stdout", "stderr"):
            path = directory / row[field]
            require(path.is_file() and digest(path) == row[field + "_sha256"], "missing-raw", f"missing/changed {field}")
        require(json.loads((directory / row["stdout"]).read_text()) == raw, "raw-binding", "raw stdout disagrees with sample")
        for field in ("before", "after"):
            path = directory / row[field]["path"]
            require(path.is_file() and digest(path) == row[field]["sha256"], "quiet", "process snapshot missing/changed")
            require_quiet_processes(path.read_text())
        backend = (row["postgres"], row["container_resources"])
        if backend_identity is None:
            backend_identity = backend
        require(backend == backend_identity, "machine-identity", "Postgres/resource identity differs")
        rows.append(row)
    require(seen == expected, "sample-count", "matrix incomplete")
    return manifest, rows


def paired_interval(ratios):
    # Deterministic paired nonparametric bootstrap, no invented gain budget.
    rng = random.Random(0x5EED)
    logs = [math.log(value) for value in ratios]
    means = [math.exp(sum(rng.choice(logs) for _ in logs) / len(logs)) for _ in range(10_000)]
    return [percentile(means, 0.025), percentile(means, 0.975)]


def summarize(manifest, rows):
    output = []
    for case in manifest["cases"]:
        for clients in manifest["clients"]:
            samples = {arm: sorted((row for row in rows if row["case"] == case and row["clients"] == clients and row["arm"] == arm),
                                   key=lambda row: row["pair"]) for arm in ("baseline", "candidate")}
            metrics = {}
            for metric in ("throughput_ops_s", "p50_ns", "p95_ns", "p99_ns", "peak_rss_bytes"):
                values = {}
                for arm, selected in samples.items():
                    values[arm] = []
                    for row in selected:
                        raw = row["raw"]
                        if metric == "throughput_ops_s":
                            value = OPERATIONS * 1_000_000_000 / raw["elapsed_ns"]
                        elif metric == "peak_rss_bytes":
                            value = row[metric]
                        else:
                            value = percentile(raw["latencies_ns"], {"p50_ns": 0.5, "p95_ns": 0.95, "p99_ns": 0.99}[metric])
                        values[arm].append(value)
                ratios = [candidate / baseline for baseline, candidate in zip(values["baseline"], values["candidate"])]
                costs = [1 / ratio for ratio in ratios] if metric == "throughput_ops_s" else ratios
                interval = paired_interval(costs)
                classification = "regression-observed" if interval[0] > 1 else "improvement-observed" if interval[1] < 1 else "inconclusive-noise-overlap"
                metrics[metric] = {"raw_samples": values, "candidate_to_baseline": ratios,
                                   "cost_ratio_95pct_paired_bootstrap": interval, "classification": classification}
            output.append({"case": case, "clients": clients, "metrics": metrics})
    return output


def report(args):
    manifest, rows = validate(args.capture, args.allow_slow_control)
    summaries = summarize(manifest, rows)
    if manifest["mode"] == "slow-control":
        require(any(item["metrics"][metric]["classification"] == "regression-observed" for item in summaries
                    for metric in ("throughput_ops_s", "p50_ns", "p95_ns", "p99_ns")),
                "slow-classifier", "deliberate slowdown was not classified as a regression")
    save(args.out, {"schema": 1, "disposition": "MEASURED_CONTROL_ONLY" if manifest["mode"] == "slow-control" else "MEASURED_REPORT_ONLY",
                    "manifest_sha256": digest(Path(args.capture) / "manifest.json"), "source": {arm: manifest[arm]["revision"] for arm in ("baseline", "candidate")},
                    "machine": manifest["machine"], "attestation": manifest["attestation"], "summaries": summaries,
                    "notes": ["No release pass/fail or gain threshold is inferred.",
                              "Overlapping intervals are inconclusive; any observed regression is explicit for operator decision.",
                              "Peak RSS is benchmark-client process peak, including setup and warmup; excludes PostgreSQL/Docker/runner.",
                              "Bootstrap intervals with five pairs are supporting evidence, not a universal performance guarantee."]})


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    sub = parser.add_subparsers(dest="action", required=True)
    stage = sub.add_parser("stage-baseline")
    for key in ("root", "candidate-root", "out"):
        stage.add_argument("--" + key, required=True)
    stage.set_defaults(function=stage_baseline)
    compile_parser = sub.add_parser("build")
    for key in ("root", "revision", "out"):
        compile_parser.add_argument("--" + key, required=True)
    compile_parser.add_argument("--arm", choices=("baseline", "candidate"), required=True)
    compile_parser.set_defaults(function=build)
    measurement = sub.add_parser("measure")
    for key in ("baseline-receipt", "candidate-receipt", "image", "quiet-attestation", "out"):
        measurement.add_argument("--" + key, required=True)
    measurement.add_argument("--samples", type=int, default=5)
    measurement.add_argument("--max-wall-seconds", type=int, default=7200)
    measurement.add_argument("--slow-control", action="store_true")
    measurement.set_defaults(function=measure)
    reporting = sub.add_parser("report")
    reporting.add_argument("--capture", required=True)
    reporting.add_argument("--out", required=True)
    reporting.add_argument("--allow-slow-control", action="store_true")
    reporting.set_defaults(function=report)
    args = parser.parse_args()
    args.function(args)


if __name__ == "__main__":
    main()
