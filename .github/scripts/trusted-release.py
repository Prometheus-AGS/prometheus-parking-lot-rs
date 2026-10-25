#!/usr/bin/env python3
"""Default non-publishing rehearsal; publish only from independently approved CI.

There is deliberately no --receipt-file, --approved or token CLI argument.
The trust policy is an operator-controlled public environment setting, not data
from the artifact/caller. GitHub source/API checks establish receipt provenance.
"""

from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import zipfile

sys.dont_write_bytecode = True
from release_evidence import (  # noqa: E402
    GitHubAPI, PACKAGES, RELEASE_WORKFLOW, SHA, ProvenanceUnavailable, ReleaseError,
    canonical, fetch_boundary, git, require, sha256, validate_approvals, validate_policy,
    validate_preparation, validate_receipt, validate_run, verify_candidate_tag,
)


def contract_digest(path: Path) -> str:
    contract = json.loads(path.read_bytes())
    normalized = copy.deepcopy(contract)
    normalized["identity"].pop("contractDigest", None)
    normalized["identity"].pop("planRevision", None)
    digest = sha256(canonical(normalized))
    require(contract["identity"]["contractDigest"] == digest, "adopted contract digest is invalid")
    return digest


def approved_publication_run(api: GitHubAPI, policy: dict, candidate: str) -> dict:
    require(os.environ.get("GITHUB_ACTIONS") == "true"
            and os.environ.get("GITHUB_REPOSITORY", "").casefold() == policy["repository"].casefold()
            and os.environ.get("GITHUB_SHA") == candidate
            and os.environ.get("GITHUB_REF") == f"refs/heads/{policy['branch']}",
            "publication requires the frozen candidate's trusted GitHub Actions context")
    workflow_ref = f"{policy['repository']}/{RELEASE_WORKFLOW}@refs/heads/{policy['branch']}"
    require(os.environ.get("GITHUB_WORKFLOW_REF", "").casefold() == workflow_ref.casefold(),
            "publication workflow identity mismatch")
    run_id = os.environ.get("GITHUB_RUN_ID", "")
    require(run_id.isdecimal() and int(run_id) > 0, "publication run ID is missing")
    run = api.get(f"/actions/runs/{run_id}")
    validate_run(run, policy, candidate, RELEASE_WORKFLOW, completed=False)
    require(api.workflow_digest(RELEASE_WORKFLOW, candidate) == policy["release_workflow_sha256"],
            "trusted release workflow source digest mismatch")
    validate_approvals(api.get(f"/actions/runs/{run_id}/approvals"), policy["publish_environment"])
    return {"run_id": int(run_id), "environment": policy["publish_environment"]["name"]}


def run(args: argparse.Namespace) -> None:
    workspace = args.workspace.resolve()
    output = args.output.resolve()
    require(not output.exists() and not output.is_relative_to(workspace),
            "use a fresh output directory outside the candidate checkout")
    output.mkdir(parents=True)
    require(SHA.fullmatch(args.candidate) is not None, "full frozen candidate commit ID required")
    require(git(workspace, "rev-parse", "HEAD") == args.candidate
            and git(workspace, "status", "--porcelain") == "", "candidate checkout must be exact and clean")
    policy = json.loads(args.policy.read_bytes())
    validate_policy(policy)
    digest = contract_digest(args.contract)
    require(digest == policy["contract_digest"], "operator-approved contract digest mismatch")
    api = GitHubAPI(policy["repository"])
    receipt, plan, provenance = fetch_boundary(api, args.artifact_id, policy, args.candidate)
    publication = args.mode in {"authorize", "publish"}
    version = policy["version"]
    validate_receipt(receipt, args.candidate, digest, args.tag, version, publication=publication)
    signed, trust_digest = verify_candidate_tag(workspace, policy, args.candidate, args.tag)
    expected_signature = {**signed, "public_trust_sha256": trust_digest}
    require(receipt["tag_verification"] == expected_signature and receipt["tag_object"] == signed["tag_object"],
            "boundary tag/signature/public-trust identity mismatch")
    # Only load the candidate's OA-02 manifest checker after its exact commit
    # has passed independent signature/identity and trusted boundary binding.
    validate_preparation(workspace, plan, args.candidate, version)
    publish_context = None
    if publication:
        require(args.tag == f"v{version}", "publication tag must name the exact lockstep version")
        publish_context = approved_publication_run(api, policy, args.candidate)
    command = ["cargo", "publish", "--workspace", "--exclude", "prometheus-parking-lot-ffi",
               "--locked", "--registry", "crates-io"]
    summary = {"mode": args.mode, "candidate": args.candidate, "contract_digest": digest,
               "tag": args.tag, "tag_object": signed["tag_object"], "version": version,
               "purpose": receipt["purpose"], "provenance": provenance,
               "trust_policy_sha256": sha256(canonical(policy)), "public_trust_sha256": trust_digest,
               "packages": list(PACKAGES), "publish_context": publish_context,
               "planned_publish_argv": command, "publish_invoked": False}
    (output / "release-gate.json").write_bytes(canonical(summary))
    if args.mode != "publish":
        print(json.dumps({"status": "REHEARSAL_VALIDATED" if args.mode == "rehearse" else "PUBLICATION_AUTHORIZED",
                          "candidate": args.candidate, "packages": list(PACKAGES),
                          "certification": receipt["purpose"] == "certification", "publish_invoked": False}, sort_keys=True))
        return

    # Only the workflow's pinned OIDC action supplies this per-step value. No
    # registry token secret, cached credential, --token argument or fallback.
    token = os.environ.get("RELEASE_OIDC_TOKEN")
    require(bool(token), "OIDC exchange output is missing; no credential fallback")
    require(bool(os.environ.get("ACTIONS_ID_TOKEN_REQUEST_URL"))
            and bool(os.environ.get("ACTIONS_ID_TOKEN_REQUEST_TOKEN")), "OIDC job permission/context is missing")
    retained = {"PATH", "HOME", "USER", "TMPDIR", "SYSTEMROOT", "COMSPEC", "LANG", "LC_ALL",
                "RUSTUP_HOME", "SSL_CERT_FILE", "SSL_CERT_DIR", "SDKROOT", "MACOSX_DEPLOYMENT_TARGET"}
    environment = {name: value for name, value in os.environ.items() if name in retained}
    with tempfile.TemporaryDirectory(prefix="oidc-cargo-", dir=output) as cargo_home:
        environment.update(CARGO_HOME=cargo_home, CARGO_REGISTRY_TOKEN=token,
                           CARGO_REGISTRIES_CRATES_IO_TOKEN=token,
                           RUSTUP_TOOLCHAIN="1.99.1", RUSTUP_AUTO_INSTALL="0")
        summary["publish_invoked"] = True
        (output / "release-gate.json").write_bytes(canonical(summary))
        result = subprocess.run(command, cwd=workspace, env=environment, check=False)
    require(result.returncode == 0, "cargo publish failed; inspect partial registry state before any retry")
    summary["publish_exit"] = result.returncode
    (output / "release-gate.json").write_bytes(canonical(summary))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--mode", choices=("rehearse", "authorize", "publish"), default="rehearse")
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--candidate", required=True)
    parser.add_argument("--tag", required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True, help="operator-controlled public trust policy")
    parser.add_argument("--artifact-id", type=int, required=True, help="server artifact ID, never a local receipt path")
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        run(args)
    except ProvenanceUnavailable as error:
        print(f"BLOCKED: {error}", file=sys.stderr)
        return 2
    except ReleaseError as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    except (OSError, ValueError, KeyError, TypeError, AttributeError, ImportError, zipfile.BadZipFile):
        # No input/environment/token dump or traceback of remote data.
        print("FAIL: malformed or unavailable release evidence/configuration", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
