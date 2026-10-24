#!/usr/bin/env python3
"""OA-04 real nonpublishing rehearsal plus discriminating negative controls.

Run only at the completed phase boundary. A real independently approved GitHub
artifact, exact clean candidate and operator public trust policy are mandatory.
No local fake approval/API server is accepted. This script never dispatches a
workflow, creates/imports a key, requests OIDC or invokes real Cargo/publication.
Missing real provenance is BLOCKED, not a mocked positive result.
"""

from __future__ import annotations

import argparse
import copy
import json
import os
from pathlib import Path
import shlex
import subprocess
import sys

sys.dont_write_bytecode = True
tools = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(tools))
from release_evidence import (  # noqa: E402
    BOUNDARY_WORKFLOW, GitHubAPI, ProvenanceUnavailable, ReleaseError, canonical,
    fetch_boundary, git, require, validate_approvals, validate_preparation,
    validate_policy, validate_receipt, validate_run,
)


def run(args: argparse.Namespace) -> None:
    workspace = args.workspace.resolve()
    output = args.output.resolve()
    require(not output.exists() and not output.is_relative_to(workspace), "fresh external fixture output required")
    output.mkdir(parents=True)
    policy = json.loads(args.policy.read_bytes())
    validate_policy(policy)
    candidate = git(workspace, "rev-parse", "HEAD")
    api = GitHubAPI(policy["repository"])
    # Real fixed-origin API/archive checks are mandatory before the suite can
    # claim a positive rehearsal or use the observed data for negative controls.
    receipt, plan, provenance = fetch_boundary(api, args.artifact_id, policy, candidate)
    version = policy["version"]
    tag = receipt["tag"]
    results: list[dict] = []
    cargo_log = output / "forbidden-cargo-invocations.txt"
    shim_directory = output / "bin"
    shim_directory.mkdir()
    shim = shim_directory / "cargo"
    shim.write_text("#!/bin/sh\nprintf '%s\\n' \"$*\" >> " + shlex.quote(str(cargo_log)) + "\nexit 93\n")
    shim.chmod(0o700)
    # Only the forbidden Cargo command is replaced. Git, signatures, registry
    # plan/manifest reading and GitHub transport/provenance stay real.
    environment = {key: value for key, value in os.environ.items()
                   if key != "RELEASE_OIDC_TOKEN" and not (key.startswith("CARGO_") and key.endswith("TOKEN"))}
    environment.update(PATH=str(shim_directory) + os.pathsep + os.environ.get("PATH", ""),
                       PYTHONDONTWRITEBYTECODE="1")

    def invoke(label: str, expected: int, oracle: str, *, selected_policy: dict | None = None,
               selected_tag: str | None = None, mode: str = "rehearse", no_actions: bool = False) -> None:
        index = len(results) + 1
        selected_policy_path = output / f"policy-{index}.json"
        selected_policy_path.write_bytes(canonical(selected_policy if selected_policy is not None else policy))
        argv = [sys.executable, str(tools / "trusted-release.py"), "--workspace", str(workspace),
                "--candidate", candidate, "--tag", selected_tag if selected_tag is not None else tag,
                "--contract", str(args.contract.resolve()), "--artifact-id", str(args.artifact_id),
                "--policy", str(selected_policy_path), "--output", str(output / f"gate-{index}"), "--mode", mode]
        invocation_env = dict(environment)
        if no_actions:
            invocation_env.pop("GITHUB_ACTIONS", None)
        result = subprocess.run(argv, cwd=workspace, env=invocation_env, capture_output=True, text=True, check=False)
        (output / f"case-{index}.txt").write_text(result.stdout + result.stderr)
        print(f"case {index}: {label}\nargv: {shlex.join(argv)}\nexit: {result.returncode}\n{result.stdout}{result.stderr}")
        if result.returncode == 2 and "BLOCKED:" in result.stderr:
            raise ProvenanceUnavailable("real GitHub rehearsal became unavailable")
        require(result.returncode == expected and oracle in result.stdout + result.stderr,
                f"{label}: required exit/oracle not observed")
        require(not cargo_log.exists(), f"{label}: forbidden Cargo invocation occurred")
        results.append({"case": label, "exit": result.returncode, "status": "PASS"})

    invoke("approved actual candidate rehearsal resolves seven crates without Cargo", 0, '"publish_invoked": false')
    proof = json.loads((output / "gate-1/release-gate.json").read_bytes())
    require(len(proof["packages"]) == 7 and proof["provenance"] == provenance,
            "real rehearsal did not bind exactly seven crates and observed provenance")
    invoke("wrong requested tag never reaches publish", 1, "boundary tag/version identity mismatch",
           selected_tag=tag + "-wrong-control")
    stale_policy = copy.deepcopy(policy)
    stale_policy["contract_digest"] = "1" * 64 if policy["contract_digest"] != "1" * 64 else "2" * 64
    invoke("stale approved contract never reaches publish", 1, "operator-approved contract digest mismatch",
           selected_policy=stale_policy, mode="publish")
    wrong_workflow = copy.deepcopy(policy)
    wrong_workflow["boundary_workflow_sha256"] = "1" * 64 if policy["boundary_workflow_sha256"] != "1" * 64 else "2" * 64
    invoke("unapproved boundary workflow source never reaches publish", 1, "trusted boundary workflow source digest mismatch",
           selected_policy=wrong_workflow, mode="publish")
    wrong_reviewer = copy.deepcopy(policy)
    wrong_reviewer["boundary_environment"]["reviewer_ids"] = [9223372036854775807]
    invoke("actual source review cannot authorize an unapproved reviewer", 1, "trusted environment reviewer is not authorized",
           selected_policy=wrong_reviewer, mode="publish")
    if receipt["purpose"] == "rehearsal":
        invoke("rehearsal purpose cannot be promoted into publish", 1, "rehearsal receipt cannot authorize publication", mode="publish")
    else:
        invoke("certification without actual publication context cannot publish", 1,
               "publication requires the frozen candidate's trusted GitHub Actions context", mode="publish", no_actions=True)

    def reject(label: str, action, oracle: str) -> None:
        try:
            action()
        except ReleaseError as error:
            require(oracle in str(error), f"{label}: wrong negative classifier")
            results.append({"case": label, "status": "PASS", "oracle": oracle})
            print(f"case {len(results)}: {label}: expected rejection: {error}")
        else:
            raise ReleaseError(f"{label}: broken control was accepted")
        require(not cargo_log.exists(), f"{label}: forbidden Cargo invocation occurred")

    # Content/metadata mutation controls share the production validators and
    # start from the independently fetched real packet above. They are never
    # substituted for the suite's real transport/signature/rehearsal invocation.
    stale = copy.deepcopy(receipt)
    stale["candidate"] = "0" * 40
    reject("stale receipt candidate", lambda: validate_receipt(stale, candidate, policy["contract_digest"], tag, version,
                                                             publication=False), "stale boundary candidate/contract")
    caller_approval = copy.deepcopy(receipt)
    caller_approval["approved"] = True
    reject("caller-authored approval field has no authority", lambda: validate_receipt(caller_approval, candidate,
           policy["contract_digest"], tag, version, publication=True), "shared protocol")
    incomplete = copy.deepcopy(receipt)
    incomplete["scenarios"].pop()
    reject("omitted mandatory scenario", lambda: validate_receipt(incomplete, candidate, policy["contract_digest"], tag,
           version, publication=False), "mandatory scenario set")
    def synthetic_certificate_control() -> dict:
        # In-memory negative input only, never saved/uploaded or passed to the
        # production CLI. Make other statuses formally PASS so each isolated
        # missing baseline/review/operator oracle is actually discriminating.
        control = copy.deepcopy(receipt)
        control.update(purpose="certification", status="PASS")
        for record in [*control["scenarios"], control["baseline"], control["review"], *control["operator_gates"].values()]:
            record.update(status="PASS", receipt_sha256=provenance["receipt_sha256"])
        return control

    false_certification = synthetic_certificate_control()
    false_certification["baseline"] = {"status": "NOT_EXERCISED"}
    reject("certification cannot contain an unexercised baseline", lambda: validate_receipt(false_certification,
           candidate, policy["contract_digest"], tag, version, publication=True), "mandatory boundary oracle is not PASS")
    missing_review = synthetic_certificate_control()
    missing_review["review"] = {"status": "NOT_EXERCISED"}
    reject("certification cannot omit independent review", lambda: validate_receipt(missing_review,
           candidate, policy["contract_digest"], tag, version, publication=True), "mandatory boundary oracle is not PASS")
    missing_operator = synthetic_certificate_control()
    missing_operator["operator_gates"]["G-04"] = {"status": "NOT_EXERCISED"}
    reject("certification cannot omit operator publisher evidence", lambda: validate_receipt(missing_operator,
           candidate, policy["contract_digest"], tag, version, publication=True), "mandatory boundary oracle is not PASS")
    source_run = api.get(f"/actions/runs/{provenance['run_id']}")
    rerun = copy.deepcopy(source_run)
    rerun["run_attempt"] = 2
    reject("approval history from a rerun is ambiguous", lambda: validate_run(rerun, policy, candidate,
           BOUNDARY_WORKFLOW, completed=True), "first-attempt workflow provenance required")
    foreign_run = copy.deepcopy(source_run)
    foreign_run["event"] = "pull_request"
    reject("pull-request source is not approved boundary provenance", lambda: validate_run(foreign_run, policy,
           candidate, BOUNDARY_WORKFLOW, completed=True), "unapproved workflow source/event")
    reject("missing environment approval", lambda: validate_approvals([], policy["boundary_environment"]),
           "trusted environment approval is absent")
    fewer_crates = copy.deepcopy(plan)
    fewer_crates["packages"].pop()
    reject("six-crate plan cannot publish", lambda: validate_preparation(workspace, fewer_crates, candidate, version),
           "exactly seven lockstep crates")
    ffi_plan = copy.deepcopy(plan)
    ffi_plan["packages"].append({"name": "prometheus-parking-lot-ffi", "version": version, "path": "crates/parking-lot-ffi"})
    reject("unpublished FFI cannot enter the publication set", lambda: validate_preparation(workspace, ffi_plan, candidate,
           version), "exactly seven lockstep crates")
    changed_lock = copy.deepcopy(plan)
    changed_lock["files_sha256"]["Cargo.lock"] = "0" * 64
    reject("prepared content cannot drift from frozen candidate", lambda: validate_preparation(workspace, changed_lock,
           candidate, version), "prepared manifest/lockfile/changelog content differs")
    require(not cargo_log.exists(), "Cargo was invoked during a nonpublishing fixture")
    summary = {"status": "PASS", "suite": "OA-04", "candidate": candidate, "purpose": receipt["purpose"],
               "real_provenance": provenance, "cases": results, "publish_invoked": False,
               "limit": "This validates rehearsal tooling, not operator publisher settings, real approval or publication"}
    (output / "fixture-summary.json").write_bytes(canonical(summary))
    print(f"PASS: OA-04 real trusted-source rehearsal and controls ({len(results)} cases; no Cargo/publication)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--workspace", type=Path, required=True)
    parser.add_argument("--contract", type=Path, required=True)
    parser.add_argument("--policy", type=Path, required=True)
    parser.add_argument("--artifact-id", type=int, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        run(args)
    except ProvenanceUnavailable as error:
        print(f"BLOCKED: {error}", file=sys.stderr)
        return 2
    except (ReleaseError, OSError, ValueError, KeyError, TypeError) as error:
        print(f"FAIL: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
