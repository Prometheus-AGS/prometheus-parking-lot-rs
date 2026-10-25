Strengths: public API and changelog document the breaking pruning/duplicate-status contracts and migration limits. Release tooling separates preparation, certification, operator authorization and OIDC publication; rehearsal cannot authorize publishing.

[HIGH] [.github/operational-assurance.json:603] OA-02 cannot pass on the final prepared candidate

Exact artifact:

```json
"--candidate",
"{candidate}",
"--base",
"{base}",
"--version",
"0.4.0",
```

`make_freeze` binds `{candidate}` to the final candidate SHA (`operational-assurance.py:268`). But `scripts/release/prepare.py:222–226` reads that commit’s version and requires:

```python
require(tuple(map(int, args.version.split("."))) > tuple(map(int, baseline_version.split("."))),
        "release version must be greater than the candidate's current version")
```

For a prepared 0.4.0 candidate, mandatory OA-02 therefore fails. Keeping the candidate at 0.3.0 cannot avoid this: compact assembly calls `validate_preparation(..., "0.4.0")` at `operational-assurance.py:765`, whose checker requires the integrated workspace to be 0.4.0.

Fix: distinguish the pre-bump preparation source from the final candidate. Reproduce preparation from the explicitly bound ancestor and compare its outputs with the final candidate, while retaining final-candidate identity on the boundary receipt.

[HIGH] [.github/operational-assurance.json:2538] Linux desktop certification can use a macOS build

Exact artifact:

```json
"id": "desktop-linux",
"profile": "baseline",
```

The baseline profile is `"native"` (`:6`), and the runner accepts it on any OS:

```python
require(model["profiles"][args.profile] in {"native", platform.system()}, "required OS unavailable")
```

The desktop builder selects the actual host OS (`desktop-assurance.py:99`) and emits that OS’s `case_id` and `os` (`:174–176`). Child validation checks only PASS, candidate and contract (`operational-assurance.py:489–490`).

Consequently, a successful macOS baseline can satisfy `desktop-linux`; the Linux profile contains no replacement desktop build. After other gates pass, OA-15 can be certified without the contract-required Linux Tauri build.

Fix: move `desktop-linux` to the Linux profile and verify the expected OS/build-case identity when accepting its child receipt.

Reviewed complete preparation/check/test-fixture functions, boundary freeze/run/assembly/verification functions, release consumers/workflows, desktop build function, relevant specifications, API/changelog integration and manifests. Rust edition is workspace-inherited `"2024"`; neither finding depends on Rust edition semantics.

Limitations: no builds, tests, Cargo commands, remote workflow execution or account verification ran. Rust concurrency, migration correctness and Python security internals belong to the other reviewers. The two ignored release scripts require the parent’s planned explicit force-add during candidate assembly; this is an assembly verification item, not a separate finding.

## Review Summary

| Severity | Count | Status |
|----------|-------|--------|
| CRITICAL | 0 | pass |
| HIGH | 2 | warn |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: WARNING — resolve both integration findings before release certification.
