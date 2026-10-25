#!/usr/bin/env python3
"""Transport public evidence from one pinned same-repository release asset.

This creates input bytes for upload-artifact, never certification authority.
"""
from __future__ import annotations

import argparse
import datetime as dt
import hashlib
import io
import json
import os
from pathlib import Path
import re
import stat
import sys
import urllib.request
import zipfile

from release_evidence import API_VERSION, GitHubAPI, ReleaseError, require

ARCHIVE_LIMIT = 32 * 1024 * 1024
EXPANDED_LIMIT = 64 * 1024 * 1024
MEMBER_LIMIT = 1000
RECEIPT = "seed-transport-receipt.json"
BASENAME = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]{0,127}")
HEX = re.compile(r"[0-9a-f]{64}")


def sha256(value: bytes) -> str:
    return hashlib.sha256(value).hexdigest()


def unpack(archive: bytes, destination: Path) -> dict[str, str]:
    """Decode bounded, flat regular files without executing any supplied content."""
    require(0 < len(archive) <= ARCHIVE_LIMIT, "seed archive exceeds download limit")
    require(not destination.exists() and not destination.is_symlink(), "fresh seed output required")
    with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
        members = bundle.infolist()
        require(0 < len(members) <= MEMBER_LIMIT, "empty or excessive seed member inventory")
        names: set[str] = set()
        expanded = 0
        for info in members:
            name = info.filename
            require(info.orig_filename == name and BASENAME.fullmatch(name) is not None
                    and name.casefold() != RECEIPT.casefold(),
                    "seed member must be a nonreserved safe basename")
            require(name.casefold() not in names, "duplicate or case-colliding seed member")
            names.add(name.casefold())
            kind = stat.S_IFMT(info.external_attr >> 16)
            require(not info.is_dir() and kind in (0, stat.S_IFREG)
                    and not info.flag_bits & 1, "seed member must be an unencrypted regular file")
            require(info.compress_type in (zipfile.ZIP_STORED, zipfile.ZIP_DEFLATED),
                    "unsupported seed compression")
            expanded += info.file_size
            require(expanded <= EXPANDED_LIMIT, "seed expanded size exceeds limit")
        destination.mkdir(mode=0o700, parents=True, exist_ok=False)
        hashes = {}
        for info in members:
            with bundle.open(info) as source:
                value = source.read(info.file_size + 1)
            require(len(value) == info.file_size, "seed member size differs")
            path = destination / info.filename
            with path.open("xb") as output:
                output.write(value)
            path.chmod(0o600)
            hashes[info.filename] = sha256(value)
        return hashes


def stage(repository: str, asset_id: int, expected: str, candidate: str,
          destination: Path) -> dict:
    require(re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository) is not None,
            "invalid current repository")
    require(type(asset_id) is int and asset_id > 0, "positive release asset ID required")
    require(HEX.fullmatch(expected) is not None, "exact seed archive SHA256 required")
    require(re.fullmatch(r"[0-9a-f]{40}", candidate) is not None, "full workflow candidate required")
    api = GitHubAPI(repository)
    suffix = f"/releases/assets/{asset_id}"
    url = f"https://api.github.com/repos/{repository}{suffix}"
    metadata = api.get(suffix)
    require(metadata.get("id") == asset_id and metadata.get("url") == url
            and metadata.get("state") == "uploaded", "same-repository release asset identity differs")
    require(type(metadata.get("size")) is int and 0 < metadata["size"] <= ARCHIVE_LIMIT,
            "release asset size exceeds limit")
    # The independent caller pin remains required even when GitHub supplies a digest.
    require(metadata.get("digest") == "sha256:" + expected, "release asset metadata digest differs")
    headers = {"Accept": "application/octet-stream", "X-GitHub-Api-Version": API_VERSION}
    if os.environ.get("GH_TOKEN"):
        headers["Authorization"] = "Bearer " + os.environ["GH_TOKEN"]
    request = urllib.request.Request(url, headers=headers)
    # GitHubAPI's SafeRedirect removes Authorization when leaving api.github.com.
    with api.opener.open(request, timeout=120) as response:
        archive = response.read(ARCHIVE_LIMIT + 1)
    require(len(archive) == metadata["size"] and len(archive) <= ARCHIVE_LIMIT,
            "release asset download size differs")
    require(sha256(archive) == expected, "release asset bytes differ from independent pin")
    files = unpack(archive, destination)
    receipt = {"schema_version": 1, "status": "TRANSPORT_ONLY", "repository": repository,
               "candidate": candidate, "release_asset_id": asset_id, "archive_sha256": expected,
               "observed_at": dt.datetime.now(dt.timezone.utc).isoformat(), "files_sha256": files,
               "authority": "NOT_ESTABLISHED", "publication": "NOT_EXERCISED"}
    with (destination / RECEIPT).open("x") as output:
        json.dump(receipt, output, indent=2, sort_keys=True)
        output.write("\n")
    (destination / RECEIPT).chmod(0o600)
    return receipt


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--asset-id", required=True, type=int)
    parser.add_argument("--sha256", required=True)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--out", required=True, type=Path)
    args = parser.parse_args()
    receipt = stage(os.environ.get("GITHUB_REPOSITORY", ""), args.asset_id,
                    args.sha256, args.candidate, args.out)
    print(json.dumps({"status": receipt["status"], "files": len(receipt["files_sha256"]),
                      "archive_sha256": receipt["archive_sha256"]}, sort_keys=True))


if __name__ == "__main__":
    try:
        main()
    except (ReleaseError, OSError, ValueError, KeyError, TypeError, zipfile.BadZipFile,
            NotImplementedError):
        # Network errors can contain signed storage URLs; supplied filenames/bytes
        # are also deliberately absent from diagnostics.
        print("assurance seed transport failed: invalid pin, archive, or unavailable source/output",
              file=sys.stderr)
        sys.exit(1)
