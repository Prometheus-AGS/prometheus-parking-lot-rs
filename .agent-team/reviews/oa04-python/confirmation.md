Correction-only confirmation for candidate `729cc42549defb5fcf69b432485224cb89da74bc`. Observed `git rev-parse HEAD` matched that exact commit.

Prior finding 1 — OA-14 contract identity mismatch: RESOLVED by static inspection.

Fresh proof, `docker/postgres18/upgrade/rehearse.py:38-47`:

```python
normalized = copy.deepcopy(data)
normalized["identity"].pop("contractDigest", None)
normalized["identity"].pop("planRevision", None)
canonical = (json.dumps(normalized, sort_keys=True, separators=(",", ":"),
                        ensure_ascii=False) + "\n").encode()
digest = hashlib.sha256(canonical).hexdigest()
if digest != identity.get("contractDigest") or digest != expected_digest:
    raise RuntimeError("OA-14:contract-changed")
return {"contract_sha256": digest, "contract_identity": identity,
        "contract_raw_sha256": hashlib.sha256(raw).hexdigest()}
```

The complete `adopted_contract_identity()` validates the adopted identity before this normalization. `verify_inputs():135` stores its returned identity. The freshly reread caller at `.github/scripts/tests/negative-controls.py:328` still passes `manifest["contract_digest"]`; `classify_upgrade():345` still requires `receipt["contract_sha256"] == contract`. Caller, validator, and classifier now agree on the normalized digest; the raw-file digest is separate. Actual PG positive/control execution remains unverified.

Prior finding 2 — OA-02 preparation source versus final candidate: RESOLVED by static inspection, with supporting retained preparation evidence.

Fresh proof, `.github/operational-assurance.json:603-620`, routes `--candidate {preparation_source}` and separately `--final-candidate {candidate} --preparation-plan {preparation_plan}`.

`scripts/release/test-preparation.py:101-107` binds original plan inputs, rejects an identical source/final SHA, and calls:

```python
validate_preparation(source, bound_plan, args.final_candidate, args.version)
```

Its actual preparation parameters at lines109-112 continue using the pre-bump `args.candidate`. `compare_final_preparation():67-79` compares portable plan fields and tool identities, then requires:

```python
for name, expected in original["files_sha256"].items():
    if sha(final / name) != expected or sha(workspace / name) != expected:
        raise AssertionError("OA-02:final-preparation-mismatch: prepared/final bytes differ: " + name)
```

OA-18 now freezes the original plan and distinct ancestor at `operational-assurance.py:350-362`, rechecks original bytes before execution and collection, and requires collector plan bytes to match the baseline's exact original plan at lines799-805. The existing final-workspace0.4.0 and source-ancestry checks in `release_evidence.py:264-279` remain intact. The incompatible same-candidate requirement is removed without weakening final prepared-file binding.

The associated actual-preparation duplicate-heading repair is consistent with that route. Fresh proof, `scripts/release/prepare.py:249-260`:

```python
changelog = (f"# Changelog\n\n## [{current}] - {args.date}\n\n"
             + "\n".join(sections))
```

The real pinned set-version invocation follows, and only after its requested-heading check succeeds:

```python
(scratch / "CHANGELOG.md").write_text(
    result_changelog + "\n" + historical.removeprefix("# Changelog").lstrip(), encoding="utf-8")
```

Thus the parser receives only the staged new section; immutable historical release text is appended afterward. The fixture retains exact historical-body/heading checks and two-run plan/file identity checks at `test-preparation.py:132-153`.

Read artifacts establish preparation source `72adf5d8ad583c3b52b22194563e94f7895c5b3f`, seven0.4.0 publishable crates, FFI exclusion, and retained release-plz set-version exit0. `preparation.stdout.txt` records `PREPARED 0.4.0: 7 publishable crates; FFI excluded`. Fresh `shasum -a 256` reads of all13 files named in the original plan match its recorded hashes. The retained original plan hash is `b3bf274bc82c843285e7889877b0b84b98474ddf72d7747aadf663321e532fd1`. These are observed retained receipts and current-file hashes; this reviewer did not execute preparation.

Exact previous Ruff argv, retained from the original review tool call; NOT rerun:

```text
ruff check --no-cache .github/scripts/operational-assurance.py .github/scripts/release_evidence.py .github/scripts/trusted-release.py .github/scripts/verify-release-tag.py .github/scripts/tests/negative-controls.py .github/scripts/tests/boundary-packet.py .github/scripts/tests/browser-assurance.py .github/scripts/tests/trusted-publishing-fixtures.py scripts/release/prepare.py scripts/release/test-preparation.py benches/pg_measure.py bindings/tauri-desktop/assurance/desktop-assurance.py docker/postgres18/upgrade/rehearse.py
```

Its original observed exit was1, with935 diagnostics, predominantly formatting. No present-candidate lint result is claimed.

Confirmation verdict: both prior findings are resolved; no unresolved finding remains within this correction-only scope. This is not a full boundary PASS or release certification. Full final-mode OA02 reproduction/mismatch controls and real OA14 positive/negative routes remain unexecuted by this reviewer. No unrelated findings were sought or added. This confirmation used targeted reads, candidate inspection, and exact-file hashes only; no analyzers, tests, Cargo/builds, Docker/GUI/signing/network actions, source/index/HEAD changes, KBD mutations, or subagents.

The only files written by this reviewer are the requested exact initial report and this confirmation under `.agent-team/reviews/oa04-python/`.
