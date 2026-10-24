#!/usr/bin/env python3
"""Provision the checked CfT pin in a temporary GitHub-hosted Linux runner.

This is an explicit normal-CI setup step, separate from assurance/certification
validators. Explicit runner or executable configurations bypass it in ci.yml.
"""

from __future__ import annotations

import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import subprocess
import tempfile
import urllib.request


def main() -> None:
    if (os.environ.get("GITHUB_ACTIONS") != "true"
            or os.environ.get("BOOTSTRAP_RUNNER_ENVIRONMENT") != "github-hosted"
            or os.environ.get("RUNNER_OS") != "Linux"
            or os.environ.get("RUNNER_ARCH") != "X64"):
        raise SystemExit("bootstrap requires an explicit GitHub-hosted Linux X64 setup step")
    if os.environ.get("WASM_BROWSER_BIN") or os.environ.get("CHROMEDRIVER"):
        raise SystemExit("bootstrap must not replace configured browser executables")

    root = Path(__file__).resolve().parents[2]
    browser = json.loads((root / ".github/wasm-browser-assurance.json").read_bytes())["browser"]
    version = browser["version"]
    if browser["platform"] != "linux64" or not re.fullmatch(r"\d+\.\d+\.\d+\.\d+", version):
        raise SystemExit("unsupported checked browser pin")
    # CfT publishes immutable per-version metadata. Never select a moving channel.
    metadata_url = f"https://googlechromelabs.github.io/chrome-for-testing/{version}.json"
    with urllib.request.urlopen(metadata_url, timeout=60) as response:
        metadata_bytes = response.read()
    metadata = json.loads(metadata_bytes)
    if metadata["version"] != version or metadata["revision"] != browser["revision"]:
        raise SystemExit("official CfT version/revision differs from the checked pin")

    downloads = {}
    for product, key in (("chrome", "chrome_url"), ("chromedriver", "chromedriver_url")):
        expected = f"https://storage.googleapis.com/chrome-for-testing-public/{version}/linux64/{product}-linux64.zip"
        published = [entry["url"] for entry in metadata["downloads"][product]
                     if entry["platform"] == "linux64"]
        if browser[key] != expected or published != [expected]:
            raise SystemExit(f"{product}: official download differs from the checked URL")
        downloads[product] = expected

    destination = Path(tempfile.mkdtemp(prefix="oa19-cft-", dir=os.environ["RUNNER_TEMP"])).resolve()
    (destination / "metadata.json").write_bytes(metadata_bytes)
    executables = {}
    for product, url in downloads.items():
        archive = destination / f"{product}-linux64.zip"
        with urllib.request.urlopen(url, timeout=120) as response, archive.open("wb") as handle:
            shutil.copyfileobj(response, handle)
        with archive.open("rb") as handle:
            print(f"{hashlib.file_digest(handle, 'sha256').hexdigest()}  {archive}", flush=True)
        # unzip preserves CfT executable modes, including Chrome's helper processes.
        subprocess.run(["unzip", "-q", str(archive), "-d", str(destination)], check=True)
        binary = destination / f"{product}-linux64" / product
        result = subprocess.run([str(binary), "--version"], check=True, capture_output=True, text=True)
        print(result.stdout, end="", flush=True)
        found = re.search(r"\b(\d+\.\d+\.\d+\.\d+)\b", result.stdout)
        if not found or found.group(1) != version:
            raise SystemExit(f"{product}: downloaded executable version differs from the checked pin")
        executables["WASM_BROWSER_BIN" if product == "chrome" else "CHROMEDRIVER"] = str(binary)
    with Path(os.environ["GITHUB_ENV"]).open("a") as handle:
        for name, value in executables.items():
            if "\n" in value or "\r" in value:
                raise SystemExit("invalid executable path for GITHUB_ENV")
            handle.write(f"{name}={value}\n")


if __name__ == "__main__":
    main()
