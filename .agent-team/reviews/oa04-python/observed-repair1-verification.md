Observed integration repair1 verification, within existing cumulative review correction cycle1. This is not a second general R2 review.

Verdict: no actionable correctness issue found in the assigned candidate-tree scan helper, real fixture oracles, secrets-tree model input, or README. The source repair addresses the reported post-build generated-artifact findings without adding detector exceptions. Actual execution remains NOT_EXERCISED by this reviewer.

Review identity: `git rev-parse HEAD` returned `729cc42549defb5fcf69b432485224cb89da74bc`. The repair files/model were inspected as current working-tree source; this report does not claim that those edits are part of that existing commit. The parent must include them in the next frozen repair candidate before execution.

Scope read in full: `.github/scripts/scan-candidate-tree.py` (`git`, `entries`, `materialize`, `scan`, `main`); `.github/scripts/tests/candidate-tree-fixtures.py` (`run`, nested `scan`/`git`, `main`); `.github/scripts/tests/README-candidate-tree.md`; only `commands[id=secrets-tree]` in `.github/operational-assurance.json`; and the existing `sk-prefixed-api-key` rule in `.gitleaks.toml` needed to verify the canary classifier. Other model changes and prior subsystem findings were not reviewed again.

Fresh proof of exact candidate/source scope, `scan-candidate-tree.py:52-56`:

```python
resolved = git(repository, environment, "rev-parse", "--verify", candidate + "^{commit}").decode().strip()
require(resolved == candidate, "candidate-tree: full commit identity must resolve exactly")
```

The next loop reads `git ls-tree -r -z --full-tree candidate`; it does not consult the worktree/index or use Git archive/export-ignore. Lines61-73 require supported blobs, safe unique materializable paths, a nonempty set, and one regular candidate `.gitleaks.toml`. Unsupported gitlinks fail the operation rather than yield a partial PASS.

Fresh proof of raw-byte materialization, `scan-candidate-tree.py:85-104`:

```python
process.stdin.write((entry["object"] + "\n").encode("ascii"))
process.stdin.flush()
header = process.stdout.readline().decode("ascii").strip().split()
require(len(header) == 3 and header[:2] == [entry["object"], "blob"],
        "candidate-tree: incomplete Git blob response")
```

The complete loop reads exactly the reported blob length, checks the trailing LF, hashes every blob and writes each target using `open("xb")`. A tracked symlink is materialized from its blob as ordinary link-target text; no filesystem symlink is created or followed. The source materialization is created in `TemporaryDirectory` outside both repository and evidence output at lines136-142. The temporary source/full finding report are not exported.

Fresh proof of real scanner/policy/exit handling, `scan-candidate-tree.py:148-156`:

```python
argv = ["gitleaks", "detect", "--no-git", "--no-banner", "--no-color", "--redact=100",
        "--config", str(source / ".gitleaks.toml"), "--source", str(source),
        "--report-format=json", "--report-path", str(report)]
```

The helper requires installed gitleaks8.30.1, invokes this argv against the materialized source, records the actual `result.returncode`, hashes raw fully-redacted stdout/stderr, exports only finding rule/path metadata, and sets PASS only for scanner exit0. It rejects exit0 accompanied by reported findings. Nonzero scanner exits propagate; required tool/version absence is BLOCKED2. No allowlist, excluded path, export attribute, maximum-size skip or scanner mock was introduced.

Fresh proof that each fixture outcome is discriminating:

- `candidate-tree-fixtures.py:70-75` first invokes the actual requested candidate and requires PASS, scanner exit0 and a positive materialized-file count. It propagates a real candidate failure before exercising disposable controls.
- Lines108-118 place a runtime-assembled nonfunctional canary only in an ignored artifact, require helper/scanner exit0 with no findings, check its absence from source-files metadata, and verify its bytes remain unchanged.
- Lines121-129 commit exactly those same bytes as a tracked `export-ignore` file and require the real named detector, not an arbitrary failure:

```python
require(code == 1 and tracked["status"] == "FAIL" and tracked["scanner_exit"] == 1
        and any(item["rule"] == "sk-prefixed-api-key"
                and item["path"].endswith("tracked-control.txt") for item in tracked["findings"]),
        "same tracked canary must fail its real named detector despite export-ignore")
```

- Lines132-134 scan the old clean full commit while the later canary commit/worktree remains and require scanner exit0. This checks the selected immutable candidate independently of current HEAD/worktree contents.
- The named rule is present at `.gitleaks.toml:6-9`, with the `sk-` prefix and at least20 allowed suffix characters. The runtime-generated same-canary value satisfies this rule. The fixture requires the named rule/path in the scanner's actual finding report, so a missing report or unrelated command failure cannot satisfy this negative oracle.
- The wrapper checks retained outputs for the complete runtime canary before forwarding captured logs and exits0 only after all four invocations satisfy their actual predicates. No fixture commit changes the real source repository.

Fresh caller proof: `.github/operational-assurance.json:312-332` routes `secrets-tree` to `python3 .github/scripts/tests/candidate-tree-fixtures.py --repository {workspace} --candidate {candidate} --output {out}/secrets-tree`, retaining baseline/OA-05 ownership, explicit python3/git/gitleaks tools and the existing900-second outer timeout. The source/helper never receives a caller-generated success flag. The README describes this same finite fixture set and correctly labels authored commands as unexecuted.

Verification performed: targeted source/README/diff reads and read-only HEAD inspection. No analyzer, scanner, fixture, test, Cargo/build, Docker, GUI, network, source/index/HEAD change, KBD mutation, or subagent ran. The only file written is this requested owned review artifact.

Limits: the observed generated-artifact failure and prior source-scan passes are parent-supplied execution context, not independently rerun evidence. Scanner behavior/redaction, raw object materialization, all four real fixture exits, interruption cleanup, and the integrated final candidate must still be checked by the parent's serialized execution. This static verification does not certify OA-05, the full boundary, or publication.
