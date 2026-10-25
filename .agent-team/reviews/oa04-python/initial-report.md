Verdict: **Block — two important orchestration defects. No critical finding in this assigned scope.**

[HIGH] [docker/postgres18/upgrade/rehearse.py:103] OA-14 rejects the contract identity supplied by its production caller

Freshly read proof:

```python
if hashlib.sha256(contract.read_bytes()).hexdigest() != self.args.contract_sha256:
    raise RuntimeError("OA-14:contract-changed")
```

The verified caller, `.github/scripts/tests/negative-controls.py:328`, supplies:

```python
"--candidate", candidate, "--contract-sha256", manifest["contract_digest"],
```

`upgrade()`, `run_case()`, `classify_upgrade()`, and `Rehearsal.verify_inputs()` were read in full. The checked OA-18 command `oa12-oa-14-missing-result` invokes this delegate.

Observed raw file hash:

```text
4b1ba3d9755c7b3dfe8efa5120ae62e707b7b478d3e503f3a071d1eb60d58056
```

The manifest passes the normalized adopted digest:

```text
e77aa4ac91ad92082a6cef288cd5f9b9636b6101faa65237dbe16116fff64257
```

Failure scenario: with valid tools, images, and the correct committed contract, the normal OA-14 delegate rejects the input before reaching Docker. Consequently OA-12 aggregation and OA-14 cannot pass.

Fix: use one explicitly defined contract identity throughout the caller, runner, and receipt classifier. Prefer the validated normalized adopted digest; retain a separate raw-file hash if needed.

[HIGH] [scripts/release/prepare.py:225] Release preparation and final-candidate validation require incompatible versions

Freshly read proof:

```python
require(tuple(map(int, args.version.split("."))) > tuple(map(int, baseline_version.split("."))),
        "release version must be greater than the candidate's current version")
```

The OA-18 `oa02` command supplies the same frozen candidate and `--version 0.4.0`. Meanwhile `.github/scripts/operational-assurance.py:765` requires:

```python
validate_preparation(ROOT, json.loads(plan_bytes), args.candidate, "0.4.0")
```

That reaches `.github/scripts/release_evidence.py:264`:

```python
module.check(workspace, version)
```

`prepare()`, `check()`, `test-preparation.main()`, `validate_preparation()`, and `assemble()` were read in full.

Failure scenario: a pre-bump 0.3.0 candidate permits preparation but fails compact-artifact validation against the actual workspace. Committing the prepared 0.4.0 files makes final validation possible, but the mandatory OA-02 command then rejects preparing 0.4.0 from that same candidate. No frozen candidate satisfies both paths.

Fix: distinguish the preparation source from the final prepared candidate. On the final candidate, validate/reproduce the approved preparation plan and deterministic output rather than requiring another version increase.

Strengths: native/browser controls distinguish compilation, discovery, execution, and designated assertions; receipts retain command/log hashes; publication checks independently bind remote provenance, signature identity, and approval; desktop tooling explicitly distinguishes build evidence from manual GUI evidence.

Verification: read-only source/caller/spec inspection; raw contract hash calculation; scoped `ruff check --no-cache` exited **1**, reporting **935 diagnostics**, predominantly formatting. These lint diagnostics were not promoted into correctness findings.

Limitations: no tests, builds, Cargo commands, GUI operations, Docker execution, signing, or network actions were run. Runtime behavior and timeout cleanup remain unverified. No files or KBD state were changed. Confirmation review should cover only the two corrections above.
