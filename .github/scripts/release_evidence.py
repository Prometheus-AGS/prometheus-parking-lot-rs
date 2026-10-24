"""Shared OA-04/OA-18 boundary receipt protocol and trusted GitHub source checks.

Only fetch_boundary establishes remote provenance. validate_receipt checks content
but cannot authorize a local JSON file. OA-18 must produce this same schema from
observed receipts; rehearsal purpose can never authorize publication.
"""

from __future__ import annotations

import base64
import datetime as dt
import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request
import zipfile


SCHEMA_VERSION = 1
SCENARIOS = tuple(f"OA-{number:02}" for number in range(1, 18))
PACKAGES = {
    "prometheus-parking-lot-core": "crates/parking-lot-core",
    "prometheus-parking-lot-runtime-tokio": "crates/parking-lot-runtime-tokio",
    "prometheus-parking-lot-pg": "crates/parking-lot-pg",
    "prometheus-parking-lot-embedded": "crates/parking-lot-embedded",
    "prometheus-parking-lot-iggy": "crates/parking-lot-iggy",
    "prometheus-parking-lot-wasm": "crates/parking-lot-wasm",
    "prometheus_parking_lot": ".",
}
FFI = "prometheus-parking-lot-ffi"
API_VERSION = "2026-03-10"
ARTIFACT_NAME = "operational-assurance-boundary"
BOUNDARY_WORKFLOW = ".github/workflows/operational-assurance.yml"
RELEASE_WORKFLOW = ".github/workflows/release.yml"
SHA = re.compile(r"[0-9a-f]{40}")
DIGEST = re.compile(r"[0-9a-f]{64}")


class ReleaseError(Exception):
    """An observed policy, identity or receipt failure."""


class ProvenanceUnavailable(ReleaseError):
    """Required external authority is unavailable; never a passing result."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise ReleaseError(message)


def canonical(value: object) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def valid_digest(value: object) -> bool:
    return isinstance(value, str) and DIGEST.fullmatch(value) is not None and value != "0" * 64


def git(workspace: Path, *args: str) -> str:
    result = subprocess.run(["git", "-C", str(workspace), *args], capture_output=True,
                            text=True, env={**os.environ, "GIT_NO_REPLACE_OBJECTS": "1"}, check=False)
    require(result.returncode == 0, "candidate Git inspection failed")
    return result.stdout.strip()


def validate_policy(policy: dict) -> None:
    require(type(policy.get("schema_version")) is int and policy["schema_version"] == SCHEMA_VERSION,
            "unsupported release trust policy")
    require(policy.get("repository") == "Prometheus-AGS/prometheus-parking-lot-rs",
            "unapproved repository")
    require(policy.get("branch") == "main", "unapproved release branch")
    require(policy.get("version") == "0.4.0", "unapproved phase release version")
    for key in ("boundary_workflow_sha256", "release_workflow_sha256", "contract_digest"):
        require(valid_digest(policy.get(key)) and policy[key] != "0" * 64,
                f"operator-pinned {key} is missing")
    require(type(policy.get("max_age_seconds")) is int and 0 < policy["max_age_seconds"] <= 604800,
            "explicit receipt freshness policy required (at most seven days)")
    for name in ("boundary_environment", "publish_environment"):
        environment = policy.get(name, {})
        expected = "release-certification" if name == "boundary_environment" else "release-crates-io"
        require(environment.get("name") == expected and type(environment.get("id")) is int
                and environment["id"] > 0, f"operator-approved {name} is missing")
        reviewers = environment.get("reviewer_ids")
        require(isinstance(reviewers, list) and bool(reviewers)
                and all(type(value) is int and value > 0 for value in reviewers),
                f"operator-approved reviewers for {name} are missing")
    require(valid_digest(policy.get("signature", {}).get("public_trust_sha256")),
            "approved public trust configuration digest required")


def validate_approvals(reviews: list, environment: dict) -> None:
    matching = [review for review in reviews if any(
        item.get("id") == environment["id"] and item.get("name") == environment["name"]
        for item in review.get("environments", []))]
    # The API has no review timestamp/attempt binding. Ambiguous pending/rejected
    # histories are refused, and validate_run requires a fresh first attempt.
    require(bool(matching) and all(review.get("state") == "approved" for review in matching),
            "trusted environment approval is absent or ambiguous")
    require(any(review.get("user", {}).get("id") in environment["reviewer_ids"]
                for review in matching), "trusted environment reviewer is not authorized")


def validate_run(run: dict, policy: dict, candidate: str, workflow: str, completed: bool) -> None:
    repository = run.get("repository", {})
    require(repository.get("full_name", "").casefold() == policy["repository"].casefold()
            and run.get("head_repository", {}).get("id") == repository.get("id")
            and type(repository.get("id")) is int, "workflow repository/fork provenance mismatch")
    require(run.get("head_sha") == candidate and run.get("head_branch") == policy["branch"],
            "stale workflow candidate or unapproved branch")
    require(run.get("path") == workflow and run.get("event") == "workflow_dispatch",
            "unapproved workflow source/event")
    require(run.get("run_attempt") == 1, "first-attempt workflow provenance required; rerun approvals are ambiguous")
    if completed:
        require(run.get("status") == "completed" and run.get("conclusion") == "success",
                "trusted boundary workflow did not complete successfully")
    else:
        require(run.get("status") == "in_progress", "publication run is not active")


class SafeRedirect(urllib.request.HTTPRedirectHandler):
    """Never forward the Actions read token to an artifact storage redirect."""

    def redirect_request(self, request, fp, code, message, headers, new_url):
        require(urllib.parse.urlsplit(new_url).scheme == "https", "non-HTTPS artifact redirect refused")
        redirected = super().redirect_request(request, fp, code, message, headers, new_url)
        if redirected is not None and urllib.parse.urlsplit(new_url).netloc != "api.github.com":
            redirected.remove_header("Authorization")
        return redirected


class GitHubAPI:
    """Fixed-origin read-only GitHub REST collaborator; no arbitrary API URL input."""

    def __init__(self, repository: str):
        self.prefix = f"/repos/{repository}"
        self.opener = urllib.request.build_opener(SafeRedirect())

    def raw(self, suffix: str) -> bytes:
        headers = {"Accept": "application/vnd.github+json", "X-GitHub-Api-Version": API_VERSION}
        token = os.environ.get("GH_TOKEN")
        if token:
            headers["Authorization"] = f"Bearer {token}"
        request = urllib.request.Request("https://api.github.com" + self.prefix + suffix, headers=headers)
        try:
            with self.opener.open(request, timeout=60) as response:
                data = response.read(32 * 1024 * 1024 + 1)
            require(len(data) <= 32 * 1024 * 1024, "boundary artifact/API response exceeds size limit")
            return data
        except (urllib.error.URLError, TimeoutError) as error:
            # Do not log token headers, signed redirect URLs or response bodies.
            code = getattr(error, "code", "unavailable")
            raise ProvenanceUnavailable(f"trusted GitHub source unavailable ({code})") from None

    def get(self, suffix: str):
        return json.loads(self.raw(suffix))

    def workflow_digest(self, workflow: str, candidate: str) -> str:
        data = self.get(f"/contents/{workflow}?ref={candidate}")
        require(data.get("encoding") == "base64" and data.get("type") == "file",
                "trusted workflow file is unavailable")
        return sha256(base64.b64decode(data["content"], validate=False))


def fetch_boundary(api: GitHubAPI, artifact_id: int, policy: dict, candidate: str) -> tuple[dict, dict, dict]:
    require(type(artifact_id) is int and artifact_id > 0, "numeric trusted artifact ID required; local receipt files are not accepted")
    metadata = api.get(f"/actions/artifacts/{artifact_id}")
    require(metadata.get("id") == artifact_id and metadata.get("name") == ARTIFACT_NAME
            and metadata.get("expired") is False, "wrong or expired boundary artifact")
    run_id = metadata.get("workflow_run", {}).get("id")
    require(type(run_id) is int and run_id > 0, "artifact has no workflow provenance")
    run = api.get(f"/actions/runs/{run_id}")
    validate_run(run, policy, candidate, BOUNDARY_WORKFLOW, completed=True)
    artifact_run = metadata["workflow_run"]
    require(artifact_run.get("head_sha") == candidate
            and artifact_run.get("repository_id") == run["repository"]["id"]
            and artifact_run.get("head_repository_id") == run["repository"]["id"],
            "artifact run/candidate provenance mismatch")
    require(api.workflow_digest(BOUNDARY_WORKFLOW, candidate) == policy["boundary_workflow_sha256"],
            "trusted boundary workflow source digest mismatch")
    validate_approvals(api.get(f"/actions/runs/{run_id}/approvals"), policy["boundary_environment"])
    created = dt.datetime.fromisoformat(metadata["created_at"].replace("Z", "+00:00"))
    expires = dt.datetime.fromisoformat(metadata["expires_at"].replace("Z", "+00:00"))
    now = dt.datetime.now(dt.timezone.utc)
    require(0 <= (now - created).total_seconds() <= policy["max_age_seconds"] and now < expires,
            "stale boundary artifact")
    archive = api.raw(f"/actions/artifacts/{artifact_id}/zip")
    require(metadata.get("digest") == "sha256:" + sha256(archive), "server boundary archive digest mismatch")
    with zipfile.ZipFile(io.BytesIO(archive)) as bundle:
        require(bundle.namelist() == list(dict.fromkeys(bundle.namelist()))
                and set(bundle.namelist()) == {"boundary-receipt.json", "release-plan.json"},
                "boundary artifact must contain exactly the two protocol files")
        require(all(item.file_size <= 8 * 1024 * 1024 for item in bundle.infolist()),
                "boundary protocol file exceeds size limit")
        receipt_bytes = bundle.read("boundary-receipt.json")
        plan_bytes = bundle.read("release-plan.json")
    receipt = json.loads(receipt_bytes)
    plan = json.loads(plan_bytes)
    require(receipt.get("preparation_plan_sha256") == sha256(plan_bytes), "preparation plan digest mismatch")
    provenance = {"repository": policy["repository"], "run_id": run_id, "run_attempt": 1,
                  "artifact_id": artifact_id, "archive_sha256": sha256(archive),
                  "receipt_sha256": sha256(receipt_bytes), "plan_sha256": sha256(plan_bytes),
                  "boundary_workflow_sha256": policy["boundary_workflow_sha256"]}
    return receipt, plan, provenance


def validate_receipt(receipt: dict, candidate: str, contract_digest: str, tag: str, version: str,
                     *, publication: bool) -> None:
    fields = {"schema_version", "purpose", "candidate", "contract_digest", "tag", "tag_object", "version",
              "manifest_sha256", "preparation_plan_sha256", "tag_verification", "scenarios", "baseline",
              "review", "operator_gates", "status"}
    require(set(receipt) == fields, "boundary receipt fields differ from the shared protocol")
    require(type(receipt.get("schema_version")) is int and receipt["schema_version"] == SCHEMA_VERSION,
            "unsupported boundary receipt schema")
    purpose = receipt.get("purpose")
    require(purpose in {"rehearsal", "certification"}, "boundary purpose is missing")
    require(not publication or purpose == "certification", "rehearsal receipt cannot authorize publication")
    require(receipt.get("candidate") == candidate and receipt.get("contract_digest") == contract_digest,
            "stale boundary candidate/contract")
    require(receipt.get("tag") == tag and receipt.get("version") == version
            and SHA.fullmatch(receipt.get("tag_object", "")) is not None,
            "boundary tag/version identity mismatch")
    require(valid_digest(receipt.get("manifest_sha256")), "checked boundary manifest digest required")
    scenarios = receipt.get("scenarios", [])
    require(len(scenarios) == len(SCENARIOS) and {item.get("id") for item in scenarios} == set(SCENARIOS),
            "mandatory scenario set is incomplete or duplicated")
    gates = receipt.get("operator_gates", {})
    require(set(gates) == {"G-02", "G-03", "G-04"}, "mandatory operator receipt set is incomplete")
    records = [*scenarios, receipt.get("baseline", {}), receipt.get("review", {}), *gates.values()]
    certification = purpose == "certification"
    require(receipt.get("status") == ("PASS" if certification else "NOT_EXERCISED"),
            "boundary status does not match its purpose")
    for record in records:
        require(set(record) <= {"id", "status", "receipt_sha256"}, "oracle fields differ from the shared protocol")
        require(record.get("status") in ({"PASS"} if certification else {"PASS", "NOT_EXERCISED"}),
                "mandatory boundary oracle is not PASS")
        if record["status"] == "PASS":
            require(valid_digest(record.get("receipt_sha256")), "PASS oracle has no observed receipt digest")
    require(valid_digest(receipt.get("tag_verification", {}).get("public_trust_sha256")),
            "tag verification public trust binding is missing")


def validate_preparation(workspace: Path, plan: dict, candidate: str, version: str) -> None:
    # Reuse OA-02's actual manifest/lockfile policy; no Cargo process is started.
    location = workspace / "scripts/release/prepare.py"
    spec = importlib.util.spec_from_file_location("release_preparation", location)
    require(spec is not None and spec.loader is not None, "OA-02 preparation policy is unavailable")
    module = importlib.util.module_from_spec(spec)
    sys.dont_write_bytecode = True
    spec.loader.exec_module(module)
    try:
        module.check(workspace, version)
    except module.PreparationError as error:
        raise ReleaseError(f"seven-crate preparation policy failed: {error}") from None
    expected = [{"name": name, "path": path, "version": version} for name, path in PACKAGES.items()]
    require(plan.get("schema_version") == 1 and plan.get("policy") == "lockstep"
            and plan.get("publish") is False and plan.get("version") == version
            and plan.get("packages") == expected and plan.get("excluded") == [FFI],
            "preparation must resolve exactly seven lockstep crates and exclude FFI")
    source = plan.get("candidate", "")
    require(SHA.fullmatch(source) is not None, "preparation source candidate is missing")
    git(workspace, "merge-base", "--is-ancestor", source, candidate)
    paths = {"Cargo.toml", "Cargo.lock", "CHANGELOG.md", "release-plz.toml", "cliff.toml", "release-tools.toml",
             "crates/parking-lot-ffi/Cargo.toml", *(f"{path}/Cargo.toml" for path in PACKAGES.values() if path != ".")}
    require(set(plan.get("files_sha256", {})) == paths, "preparation file binding is incomplete")
    for path in paths:
        require(sha256((workspace / path).read_bytes()) == plan["files_sha256"][path],
                "prepared manifest/lockfile/changelog content differs from candidate")


def verify_candidate_tag(workspace: Path, policy: dict, candidate: str, tag: str) -> tuple[dict, str]:
    signature = policy["signature"]
    fmt = signature.get("format")
    argv = ["python3", str(Path(__file__).with_name("verify-release-tag.py")),
            "--candidate", candidate, "--tag", tag, "--format", str(fmt),
            "--fingerprint", signature.get("fingerprint", "")]
    public = {"format": fmt, "fingerprint": signature.get("fingerprint")}
    if fmt == "ssh":
        signers = Path(signature["allowed_signers_file"])
        require(signers.is_absolute(), "approved SSH trust path must be absolute")
        public.update(identity=signature.get("identity"), allowed_signers_sha256=sha256(signers.read_bytes()))
        revocations = signature.get("revocations_file")
        if revocations is None:
            result = subprocess.run(["git", "-C", str(workspace), "config", "--path", "--get", "gpg.ssh.revocationFile"],
                                    capture_output=True, text=True, check=False)
            require(result.returncode in {0, 1}, "effective SSH revocation configuration is unavailable")
            revocations = result.stdout.strip() if result.returncode == 0 else None
        if revocations:
            revocation_path = Path(revocations)
            if not revocation_path.is_absolute():
                revocation_path = workspace / revocation_path
            revocations = str(revocation_path.resolve())
        public["revocations_sha256"] = sha256(Path(revocations).read_bytes()) if revocations else None
        argv += ["--identity", signature.get("identity", ""), "--allowed-signers", str(signers)]
        if revocations:
            argv += ["--revocations", revocations]
    elif fmt == "openpgp":
        home = Path(signature["gnupg_home"])
        keyring = Path(signature["public_keyring_file"])
        require(home.is_absolute() and keyring.is_absolute()
                and keyring.name in {"pubring.kbx", "pubring.gpg"} and keyring.resolve().parent == home.resolve(),
                "only an existing public GnuPG keyring may be digested")
        public["fingerprint"] = str(public["fingerprint"]).upper()
        public["public_keyring_sha256"] = sha256(keyring.read_bytes())
        argv += ["--gnupg-home", str(home)]
    else:
        raise ReleaseError("operator signature policy is missing")
    trust_digest = sha256(canonical(public))
    require(trust_digest == signature["public_trust_sha256"], "approved public trust configuration digest mismatch")
    result = subprocess.run(argv, cwd=workspace, capture_output=True, text=True, check=False)
    require(result.returncode == 0, "candidate tag/signing identity verification failed")
    return json.loads(result.stdout), trust_digest
