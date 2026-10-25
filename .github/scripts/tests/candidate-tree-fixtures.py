#!/usr/bin/env python3
"""Actual frozen-source scan plus real ignored-artifact/tracked-canary controls.

Final boundary only. No scanner mock, installation or retained source archive.
The source repository is read-only; only owned temporary fixture repos commit.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile

SUBJECT = Path(__file__).resolve().parents[1] / "scan-candidate-tree.py"


class FixtureFailure(Exception):
    pass


def require(condition: bool, message: str) -> None:
    if not condition:
        raise FixtureFailure(message)


def write(path: Path, value: object) -> None:
    path.write_text(json.dumps(value, indent=2) + "\n")


def run(args: argparse.Namespace) -> int:
    output = args.output.resolve()
    repository = args.repository.resolve()
    require(not output.exists() and not output.is_relative_to(repository), "fresh external evidence directory required")
    output.mkdir(parents=True)
    receipt = {"status": "NOT_EXERCISED", "candidate": args.candidate, "controls": [],
               "helper_sha256": hashlib.sha256(SUBJECT.read_bytes()).hexdigest()}
    environment = {key: value for key, value in os.environ.items() if key not in {
        "GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES"}}
    environment.update(GIT_CONFIG_NOSYSTEM="1", GIT_CONFIG_GLOBAL=os.devnull, GIT_TERMINAL_PROMPT="0",
                       GIT_NO_REPLACE_OBJECTS="1", GIT_NO_LAZY_FETCH="1", GIT_AUTHOR_NAME="Frozen source fixture",
                       GIT_COMMITTER_NAME="Frozen source fixture", GIT_AUTHOR_EMAIL="fixture@example.invalid",
                       GIT_COMMITTER_EMAIL="fixture@example.invalid")
    canary = b""

    def scan(label: str, source: Path, candidate: str) -> tuple[int, dict]:
        argv = [sys.executable, str(SUBJECT), "--repository", str(source), "--candidate", candidate,
                "--output", str(output / label)]
        print("candidate-tree fixture: " + label + "; full candidate " + candidate, flush=True)
        with (output / (label + ".stdout.txt")).open("wb") as stdout, (output / (label + ".stderr.txt")).open("wb") as stderr:
            process = subprocess.run(argv, env=environment, stdout=stdout, stderr=stderr, check=False)
        if canary:
            leaked = [path for path in output.rglob("*") if path.is_file() and canary in path.read_bytes()]
            for path in leaked:
                path.unlink()
            require(not leaked, "unredacted synthetic output removed; scanner evidence invalid")
        for stream in ("stdout", "stderr"):
            text = (output / (label + "." + stream + ".txt")).read_text(errors="replace")
            print(text, end="", file=sys.stdout if stream == "stdout" else sys.stderr)
        child = json.loads((output / label / "receipt.json").read_bytes())
        receipt["controls"].append({"id": label, "argv": argv, "exit": process.returncode,
                                    "receipt_sha256": hashlib.sha256((output / label / "receipt.json").read_bytes()).hexdigest()})
        return process.returncode, child

    try:
        code, actual = scan("frozen-candidate", repository, args.candidate)
        if code != 0:
            receipt.update(status=actual["status"], reason="actual frozen candidate source scan did not pass")
            return code
        require(actual["status"] == "PASS" and actual["scanner_exit"] == 0 and actual["files"] > 0,
                "actual candidate must have a successful nonempty source scan")
        with tempfile.TemporaryDirectory(prefix="oa18-source-scan-controls-") as owned:
            fixture = Path(owned).resolve()
            require(not fixture.is_relative_to(output) and not fixture.is_relative_to(repository),
                    "fixture source must remain outside repository and exported packet")
            source = fixture / "repo"
            source.mkdir()
            environment["GIT_TEMPLATE_DIR"] = str(fixture / "empty-template")
            Path(environment["GIT_TEMPLATE_DIR"]).mkdir()

            def git(*argv: str) -> str:
                process = subprocess.run(["git", "-C", str(source), *argv], env=environment,
                                         capture_output=True, check=False)
                require(process.returncode == 0, "real Git fixture setup failed")
                return process.stdout.decode().strip()

            git("init", "-q")
            # Use the actual candidate policy copied by the successful production
            # scan's object identity, rather than a worktree policy or fake rule.
            policy = subprocess.run(["git", "-C", str(repository), "show", args.candidate + ":.gitleaks.toml"],
                                    env=environment, capture_output=True, check=False)
            require(policy.returncode == 0, "cannot load exact candidate scanner policy")
            (source / ".gitleaks.toml").write_bytes(policy.stdout)
            (source / ".gitignore").write_text("target/\n")
            # ALL tracked source also includes export-ignore paths: this prevents
            # an archive-attribute omission from hiding the tracked control.
            (source / ".gitattributes").write_text("tracked-control.txt export-ignore\n")
            (source / "README.md").write_text("Non-secret frozen-source fixture\n")
            git("add", ".")
            git("-c", "core.hooksPath=" + os.devnull, "-c", "commit.gpgsign=false", "commit", "-qm", "Clean frozen-source fixture")
            clean = git("rev-parse", "HEAD")
            value = "s" + "k-" + hashlib.sha256(b"OA18 nonfunctional frozen-source detector fixture").hexdigest()
            canary = value.encode()
            decoy = source / "target/debug/deps/generated-artifact.txt"
            decoy.parent.mkdir(parents=True)
            decoy.write_text('api_key = "' + value + '"\n')
            ignored_hash = hashlib.sha256(decoy.read_bytes()).hexdigest()
            code, ignored = scan("ignored-build-artifact", source, clean)
            require(code == 0 and ignored["status"] == "PASS" and ignored["scanner_exit"] == 0
                    and not ignored["findings"], "ignored generated-artifact decoy invalidated clean tracked source")
            paths = json.loads((output / "ignored-build-artifact/source-files.json").read_bytes())
            require(not any(item["path"].startswith("target/") for item in paths)
                    and hashlib.sha256(decoy.read_bytes()).hexdigest() == ignored_hash,
                    "ignored artifact must remain unmodified and outside scanned candidate")
            print("PASS: ignored build-artifact decoy does not invalidate clean tracked source", flush=True)

            (source / "tracked-control.txt").write_bytes(decoy.read_bytes())
            git("add", "tracked-control.txt")
            git("-c", "core.hooksPath=" + os.devnull, "-c", "commit.gpgsign=false", "commit", "-qm", "Track runtime synthetic detector control")
            tracked_candidate = git("rev-parse", "HEAD")
            code, tracked = scan("tracked-same-canary", source, tracked_candidate)
            require(code == 1 and tracked["status"] == "FAIL" and tracked["scanner_exit"] == 1
                    and any(item["rule"] == "sk-prefixed-api-key"
                            and item["path"].endswith("tracked-control.txt") for item in tracked["findings"]),
                    "same tracked canary must fail its real named detector despite export-ignore")
            print("PASS: same tracked canary fails sk-prefixed-api-key despite export-ignore", flush=True)

            code, original = scan("frozen-candidate-despite-later-worktree", source, clean)
            require(code == 0 and original["status"] == "PASS" and original["scanner_exit"] == 0,
                    "scan must bind the requested frozen candidate, not later working/HEAD files")
            print("PASS: requested frozen candidate is independent of later worktree and HEAD", flush=True)
        receipt.update(status="PASS", source_copy_retained=False,
                       actual_source_receipt="frozen-candidate/receipt.json",
                       classifiers={"ignored_build_artifact": "PASS", "tracked_same_canary": "EXPECTED_FAILURE",
                                    "later_worktree": "PASS"})
        print("PASS: frozen candidate source scan and all three real source-scope controls", flush=True)
        return 0
    except (FixtureFailure, OSError, ValueError, KeyError, TypeError) as error:
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
        return run(args)
    except (FixtureFailure, OSError) as error:
        print("FAIL: " + str(error), file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main())
