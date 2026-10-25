#!/usr/bin/env python3
"""Scan every blob of a full frozen Git candidate, excluding working artifacts.

Materialization uses raw Git objects, so export-ignore, checkout filters and
ignored build/doc output cannot change the scanned set. No source copy or
archive is retained in the exported evidence directory.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath, PureWindowsPath
import re
import shutil
import subprocess
import sys
import tempfile


class ScanFailure(Exception):
    pass


class Unavailable(ScanFailure):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ScanFailure(message)


def sha(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2, ensure_ascii=True) + "\n")


def git(repository: Path, environment: dict, *args: str) -> bytes:
    result = subprocess.run(["git", "-C", str(repository), *args], env=environment,
                            capture_output=True, check=False)
    require(result.returncode == 0, "candidate-tree: Git object inspection failed")
    return result.stdout


def entries(repository: Path, candidate: str, environment: dict) -> list[dict]:
    resolved = git(repository, environment, "rev-parse", "--verify", candidate + "^{commit}").decode().strip()
    require(resolved == candidate, "candidate-tree: full commit identity must resolve exactly")
    result = []
    seen = set()
    for record in git(repository, environment, "ls-tree", "-r", "-z", "--full-tree", candidate).split(b"\0"):
        if not record:
            continue
        metadata, raw_path = record.split(b"\t", 1)
        mode, kind, oid = metadata.decode("ascii").split()
        require(kind == "blob" and mode in {"100644", "100755", "120000"},
                "candidate-tree: unsupported non-blob/gitlink; complete source unavailable")
        name = os.fsdecode(raw_path)
        path = PurePosixPath(name)
        require(name and not path.is_absolute() and not PureWindowsPath(name).drive
                and "\\" not in name and all(part not in {"", ".", ".."} for part in name.split("/"))
                and name not in seen, "candidate-tree: unsafe or duplicate tracked path")
        seen.add(name)
        result.append({"path": name, "mode": mode, "object": oid})
    require(result, "candidate-tree: empty tracked candidate cannot certify a source scan")
    policy = [entry for entry in result if entry["path"] == ".gitleaks.toml"]
    require(len(policy) == 1 and policy[0]["mode"] in {"100644", "100755"},
            "candidate-tree: candidate must contain its regular .gitleaks.toml policy")
    return result


def materialize(repository: Path, source: Path, files: list[dict], environment: dict, scratch: Path) -> None:
    # No tar extraction, checkout filters, LFS downloads or symlink traversal.
    # A symlink's Git blob is its tracked target text, scanned as a regular file.
    with (scratch / "cat-file.stderr").open("wb") as errors:
        with subprocess.Popen(["git", "-C", str(repository), "cat-file", "--batch"], env=environment,
                              stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=errors) as process:
            try:
                for entry in files:
                    process.stdin.write((entry["object"] + "\n").encode("ascii"))
                    process.stdin.flush()
                    header = process.stdout.readline().decode("ascii").strip().split()
                    require(len(header) == 3 and header[:2] == [entry["object"], "blob"],
                            "candidate-tree: incomplete Git blob response")
                    remaining = int(header[2])
                    require(remaining >= 0, "candidate-tree: invalid Git blob size")
                    entry["bytes"] = remaining
                    target = source.joinpath(*PurePosixPath(entry["path"]).parts)
                    target.parent.mkdir(parents=True, exist_ok=True)
                    digest = hashlib.sha256()
                    with target.open("xb") as output:
                        while remaining:
                            chunk = process.stdout.read(min(65536, remaining))
                            require(chunk, "candidate-tree: truncated Git blob")
                            output.write(chunk)
                            digest.update(chunk)
                            remaining -= len(chunk)
                    entry["sha256"] = digest.hexdigest()
                    require(process.stdout.read(1) == b"\n", "candidate-tree: missing Git blob terminator")
            finally:
                process.stdin.close()
            require(process.wait() == 0, "candidate-tree: Git blob materialization failed")


def scan(args: argparse.Namespace) -> int:
    require(re.fullmatch(r"(?:[0-9a-f]{40}|[0-9a-f]{64})", args.candidate),
            "candidate-tree: full lowercase commit identity required")
    repository = args.repository.resolve()
    output = args.output.resolve()
    require(not output.exists() and not output.is_relative_to(repository),
            "candidate-tree: fresh external evidence directory required")
    output.mkdir(parents=True)
    receipt = {"status": "NOT_EXERCISED", "candidate": args.candidate,
               "scope": "all tracked candidate blobs; symlink blobs scanned as link text",
               "source_copy_retained": False, "runner_sha256": sha(Path(__file__))}
    environment = {key: value for key, value in os.environ.items() if key not in {
        "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES"}}
    environment.update(GIT_NO_REPLACE_OBJECTS="1", GIT_NO_LAZY_FETCH="1",
                       GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull,
                       GIT_TERMINAL_PROMPT="0")
    try:
        for tool in ("git", "gitleaks"):
            if shutil.which(tool) is None:
                raise Unavailable("candidate-tree: installed " + tool + " required")
        version = subprocess.run(["gitleaks", "version"], env=environment, capture_output=True, check=False)
        require(version.returncode == 0, "candidate-tree: cannot inspect scanner version")
        receipt["gitleaks_version"] = version.stdout.decode(errors="replace").strip()
        if receipt["gitleaks_version"] != "8.30.1":
            raise Unavailable("candidate-tree: preinstalled gitleaks 8.30.1 required; no automatic install")
        files = entries(repository, args.candidate, environment)
        with tempfile.TemporaryDirectory(prefix="oa18-candidate-source-") as owned:
            scratch = Path(owned).resolve()
            require(not scratch.is_relative_to(output) and not scratch.is_relative_to(repository),
                    "candidate-tree: private temporary source must be outside repository and evidence packet")
            source = scratch / "source"
            source.mkdir()
            materialize(repository, source, files, environment, scratch)
            write(output / "source-files.json", files)
            receipt.update(files=len(files), bytes=sum(entry["bytes"] for entry in files),
                           source_manifest_sha256=sha(output / "source-files.json"),
                           policy_sha256=sha(source / ".gitleaks.toml"))
            report = scratch / "redacted-findings.json"
            argv = ["gitleaks", "detect", "--no-git", "--no-banner", "--no-color", "--redact=100",
                    "--config", str(source / ".gitleaks.toml"), "--source", str(source),
                    "--report-format=json", "--report-path", str(report)]
            receipt["argv"] = argv
            print("candidate-tree: scanning full candidate " + args.candidate
                  + f" ({len(files)} tracked blobs; generated worktree artifacts excluded)", flush=True)
            with (output / "scanner.stdout.txt").open("wb") as stdout, (output / "scanner.stderr.txt").open("wb") as stderr:
                result = subprocess.run(argv, cwd=source, env=environment, stdout=stdout, stderr=stderr, check=False)
            receipt["scanner_exit"] = result.returncode
            findings = json.loads(report.read_bytes()) if report.exists() else []
            require(isinstance(findings, list), "candidate-tree: malformed scanner finding report")
            # Export only finding identity; never source lines, matches or secret
            # fields. The scanner's fully redacted raw logs are retained below.
            receipt["findings"] = [{"rule": item["RuleID"], "path": item["File"]} for item in findings]
            for stream in ("stdout", "stderr"):
                log = output / ("scanner." + stream + ".txt")
                receipt[stream + "_sha256"] = sha(log)
                print(log.read_text(errors="replace"), end="", file=sys.stdout if stream == "stdout" else sys.stderr)
            require(result.returncode != 0 or not findings, "candidate-tree: scanner returned success with findings")
            receipt["status"] = "PASS" if result.returncode == 0 else "FAIL"
            print("candidate-tree: scanner exit " + str(result.returncode) + "; " + receipt["status"], flush=True)
            return result.returncode if result.returncode >= 0 else 1
    except Unavailable as error:
        receipt.update(status="BLOCKED", reason=str(error))
        print("BLOCKED: " + str(error), file=sys.stderr)
        return 2
    except (ScanFailure, OSError, ValueError, KeyError, TypeError) as error:
        receipt.update(status="FAIL", reason=str(error))
        print("FAIL: " + str(error), file=sys.stderr)
        return 1
    finally:
        write(output / "receipt.json", receipt)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repository", type=Path, default=Path.cwd())
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        return scan(args)
    except (ScanFailure, OSError) as error:
        print("FAIL: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
