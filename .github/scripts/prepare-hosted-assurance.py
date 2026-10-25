#!/usr/bin/env python3
"""Explicit hosted setup/staging, before the noninstalling assurance freeze.

Only independently configured immutable GitHub artifact IDs may supply evidence.
Authority-config bytes are never rewritten to accommodate runner paths.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import sys
import urllib.request
import zipfile

from release_evidence import API_VERSION, GitHubAPI, ReleaseError, require

PROFILES = {"baseline", "linux", "macos", "windows", "android", "ios", "collector"}
HEX = re.compile(r"[0-9a-f]{64}")
BASENAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")
LIMIT = 1024 ** 3


def digest(path: Path) -> str:
    with path.open("rb") as handle:
        return hashlib.file_digest(handle, "sha256").hexdigest()


def member_name(value: str) -> bool:
    return (isinstance(value, str) and bool(value) and "\\" not in value
            and "\x00" not in value and ":" not in value
            and not value.startswith("/")
            and all(p not in ("", ".", "..") for p in value.rstrip("/").split("/")))


def stage(profile: str, destination: Path) -> dict:
    manifest = json.loads(os.environ.get("OA18_MATERIALS_JSON") or "{}")
    require(manifest.get("schema_version") == 1 and isinstance(manifest.get("files"), dict),
            "independently configured OA18_MATERIALS_JSON is required")
    require(len(manifest["files"]) <= 1000, "too many public material entries")
    repository = os.environ.get("GITHUB_REPOSITORY", "")
    require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is not None,
            "invalid current repository")
    api = GitHubAPI(repository)
    destination.mkdir(mode=0o700, parents=True, exist_ok=False)
    receipts = []
    archives = {}
    for name, record in manifest["files"].items():
        require(BASENAME.fullmatch(name) is not None and name not in (".", ".."), "material must have a safe basename")
        require(name != "staging-receipt.json" and not name.startswith("artifact-"), "reserved material basename")
        require(isinstance(record, dict) and set(record) == {"artifact_id", "artifact_digest", "member", "sha256", "profiles"},
                "material fields differ from the public staging schema")
        require(isinstance(record["profiles"], list) and record["profiles"]
                and all(p in PROFILES for p in record["profiles"]), "invalid material profile scope")
        require(type(record["artifact_id"]) is int and record["artifact_id"] > 0
                and re.fullmatch(r"sha256:[0-9a-f]{64}", record["artifact_digest"]) is not None
                and HEX.fullmatch(record["sha256"]) is not None
                and member_name(record["member"]), "invalid immutable material pin")
        if profile not in record["profiles"]:
            continue
        artifact_id = record["artifact_id"]
        key = (artifact_id, record["artifact_digest"])
        if key not in archives:
            metadata = api.get(f"/actions/artifacts/{artifact_id}")
            require(metadata.get("id") == artifact_id and metadata.get("expired") is False
                    and metadata.get("digest") == record["artifact_digest"], "immutable artifact metadata/pin differs")
            require(type(metadata.get("size_in_bytes")) is int and 0 < metadata["size_in_bytes"] <= LIMIT,
                    "material artifact size exceeds limit")
            archive = destination / f"artifact-{artifact_id}.zip"
            require(not archive.exists(), "conflicting digest pins for one artifact ID")
            headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": API_VERSION}
            if os.environ.get("GH_TOKEN"):
                headers["Authorization"] = "Bearer " + os.environ["GH_TOKEN"]
            request = urllib.request.Request(f"https://api.github.com/repos/{repository}/actions/artifacts/{artifact_id}/zip", headers=headers)
            with api.opener.open(request, timeout=120) as response, archive.open("xb") as output:
                copied = 0
                while chunk := response.read(1024 * 1024):
                    copied += len(chunk)
                    require(copied <= LIMIT, "material artifact download exceeds limit")
                    output.write(chunk)
            require("sha256:" + digest(archive) == record["artifact_digest"], "immutable artifact archive digest differs")
            archives[key] = archive
        with zipfile.ZipFile(archives[key]) as bundle:
            infos = bundle.infolist()
            require(len(infos) <= 100000 and len({i.filename for i in infos}) == len(infos), "duplicate/excess material archive entries")
            require(sum(i.file_size for i in infos) <= 4 * LIMIT, "material archive expands beyond limit")
            for info in infos:
                mode = stat.S_IFMT(info.external_attr >> 16)
                require(member_name(info.filename) and mode in (0, stat.S_IFREG, stat.S_IFDIR)
                        and not info.flag_bits & 1, "unsafe/encrypted material archive member")
            info = bundle.getinfo(record["member"])
            require(not info.is_dir() and info.file_size <= LIMIT, "material is not a bounded regular file")
            target = destination / name
            with bundle.open(info) as source, target.open("xb") as output:
                shutil.copyfileobj(source, output)
            require(digest(target) == record["sha256"], "selected material bytes differ from independent pin")
        receipts.append({"name": name, **record})
    (destination / "staging-receipt.json").write_text(json.dumps({"schema_version": 1, "profile": profile, "materials": receipts}, indent=2) + "\n")
    return {r["name"]: destination / r["name"] for r in receipts}



def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--profile", required=True, choices=sorted(PROFILES))
    args = parser.parse_args()
    require(os.environ.get("GITHUB_ACTIONS") == "true"
            and os.environ.get("BOOTSTRAP_RUNNER_ENVIRONMENT") == "github-hosted",
            "explicit GitHub-hosted setup context required")
    require(os.environ.get("CANDIDATE") == os.environ.get("GITHUB_SHA")
            and re.fullmatch(r"[0-9a-f]{40}", os.environ.get("CANDIDATE", "")) is not None
            and os.environ.get("GITHUB_REF") == "refs/heads/main"
            and os.environ.get("GITHUB_RUN_ATTEMPT") == "1",
            "material staging requires the exact first-dispatch main candidate")
    temp = Path(os.environ["RUNNER_TEMP"]).resolve()
    materials = stage(args.profile, temp / "oa18-materials")
    mapping = json.loads(os.environ.get("OA18_INPUTS_JSON") or "{}")
    inputs = mapping.get(args.profile)
    require(isinstance(inputs, dict), "public profile input mapping missing")
    inputs = dict(inputs)
    for key, value in inputs.items():
        if isinstance(value, str) and value.startswith("material:"):
            name = value.removeprefix("material:")
            require(name in materials, "input references an unstaged material")
            inputs[key] = str(materials[name])
    if "authority_config" in inputs:
        pin = os.environ.get("OA18_AUTHORITY_SHA256", "")
        require(HEX.fullmatch(pin) is not None and digest(Path(inputs["authority_config"])) == pin,
                "staged authority-config bytes differ from independent environment pin")
    policy = os.environ.get("RELEASE_TRUST_POLICY_JSON") or "{}"
    policy_data = json.loads(policy)
    require(isinstance(policy_data, dict), "public trust policy must be a JSON object")
    policy_path = temp / "release-trust-policy.json"
    policy_path.write_text(policy)
    if args.profile in ("baseline", "collector"):
        subprocess.run([sys.executable, str(Path(__file__).with_name("stage-release-trust.py")),
                        "--policy", str(policy_path)], check=True)
    if args.profile in ("baseline", "collector"):
        inputs["trust_policy"] = str(temp / "release-trust-policy.json")
    if args.profile == "baseline":
        for key, binary in (("release_plz", "release-plz"), ("git_cliff", "git-cliff")):
            path = shutil.which(binary)
            require(path is not None, f"prepared {binary} is missing")
            inputs[key] = path
    if args.profile == "linux":
        inputs["chrome"] = os.environ["WASM_BROWSER_BIN"]
        inputs["chromedriver"] = os.environ["CHROMEDRIVER"]
        for key, name in (("pg17_image", "OA12_PG17_IMAGE"), ("pg18_image", "OA12_PG18_IMAGE"),
                          ("upgrade_image", "OA12_UPGRADE_IMAGE")):
            image = os.environ[name]
            require(re.fullmatch(r"sha256:[0-9a-f]{64}", image) is not None, "prepared backend image ID missing")
            inputs[key] = image
    if args.profile == "android":
        inputs["android_ndk"] = os.environ["ANDROID_NDK_HOME"]
    (temp / "oa18-inputs.json").write_text(json.dumps(inputs, indent=2) + "\n")


if __name__ == "__main__":
    try:
        main()
    except (ReleaseError, OSError, ValueError, KeyError, zipfile.BadZipFile) as error:
        # Avoid leaking signed artifact redirect URLs through urllib exceptions.
        if isinstance(error, OSError):
            raise SystemExit("hosted material staging failed: file/network operation unavailable") from None
        raise SystemExit(f"hosted material staging failed: {error}") from None
