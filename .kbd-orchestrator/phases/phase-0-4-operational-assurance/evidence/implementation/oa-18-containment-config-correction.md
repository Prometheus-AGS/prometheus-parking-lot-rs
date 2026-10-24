# OA-18 correction cycle 1: candidate scanner policy

Status: **NOT_EXERCISED**. This is an authored correction/static source note,
not a scanner result or independent review. Driver assigned this bounded
correction under active OA-18 task 2 at revision 1118. The driver owns final
source/hash refresh, candidate freeze, runtime checks and bookkeeping.

## Reported failure and source trace

The reviewer reported that pinned gitleaks 8.30.1 can select inherited
`GITLEAKS_CONFIG` or `GITLEAKS_CONFIG_TOML` when no explicit config flag is given.
The OA-18 environment preserves those values. A permissive ambient policy could
therefore miss a synthetic or real historical finding while the separate tip
scan uses the repository's explicit policy. This contradicts release containment.

Fresh source reads before editing showed the complete
`gate_secrets_diff`/`gate_secrets_history` functions in
[the production script](../../../../../.github/scripts/supply-chain.sh).
The exact prior invocations were:

```text
supply-chain.sh:80
  gitleaks git --no-banner --redact --log-opts="$range" .
supply-chain.sh:94
  if gitleaks git --no-banner --redact --log-opts="--all HEAD --full-history" . ; then
```

The script changes to the candidate Git top-level directory before either
function. `gate_release_containment` calls `gate_secrets_history` after its
armed-root/full-history/credential/ancestry/path gates. The fixture previously
contained `unset GITLEAKS_CONFIG GITLEAKS_CONFIG_TOML` at line 18, so its isolated
real scan cases did not expose this inherited-policy scenario. The repository
[.gitleaks.toml](../../../../../.gitleaks.toml) extends default rules and adds the
existing `sk-prefixed-api-key` rule. No rule or allowlist changed here.

## Exact bounded change

| Owned path | Before SHA256 | After SHA256 |
|---|---|---|
| `.github/scripts/supply-chain.sh` | `144a0c3fd660d63b3b02ba318596508b1e2f20f35f1439c370912d4723ac012c` | `bb31ac9aba963518afa77bbcf80407e40046f6bc9aed79f8a2802fc72307647c` |
| `.github/scripts/tests/containment-fixtures.sh` | `6c037006881a963ae7365085b4cba7a4fdbdf8d1db1734c3fd9825aa1360ce6e` | `f44c45b4afbf47fddad14dfbdcd5e8a3ec56b468cb4f49a47931a02688d9ecf4` |

Both production scanner invocations now explicitly include
`--config .gitleaks.toml`, selecting the policy at the candidate repository root
instead of relying on ambient configuration discovery. The new locations are
`gate_secrets_diff` line 81 and `gate_secrets_history` line 95. Existing range,
redaction, full-history, arming, informational and error-propagation behavior
is preserved. No actual root list, history or operator setting changed.

The fixture preserves inherited scanner overrides. In its existing real
`secret-history` repository it constructs a permissive TOML policy in its owned
temporary directory, with one rule matching a marker absent from the repository
and no default-rule extension. It uses the existing synthetic canary, generated
only at fixture runtime, committed and removed from HEAD. No literal secret-like
canary was added to tracked source.

A direct real-gitleaks precondition uses the permissive policy explicitly on
that same commit range and must exit 0. This establishes that the conflicting
policy is valid and actually permits the synthetic history; a malformed-policy
error cannot substitute for this precondition. Five named production-path
controls then require failure:

| Case label | Controlled environment | Required subject result |
|---|---|---|
| `candidate history policy overrides ambient config file` | permissive `GITLEAKS_CONFIG`; inline variable empty | `secrets-history` exit 1 and `full-history secret scan found secrets or failed (exit 1)` |
| `candidate diff policy overrides ambient config file` | permissive `GITLEAKS_CONFIG`; inline variable empty | `secrets-diff <before-secret..HEAD>` exit 1 and `gitleaks found secrets` |
| `candidate history policy overrides ambient inline TOML` | file variable empty; permissive `GITLEAKS_CONFIG_TOML` | `secrets-history` exit 1 and its full-history exit-1 diagnostic |
| `candidate diff policy overrides ambient inline TOML` | file variable empty; permissive `GITLEAKS_CONFIG_TOML` | `secrets-diff <before-secret..HEAD>` exit 1 and `gitleaks found secrets` |
| `release containment rejects synthetic history despite both ambient overrides` | both controlled permissive overrides present | armed `release-containment` exit 1 at the full-history secret-scan diagnostic |

The single-variable assignments are temporary per fixture invocation; they do
not mutate operator configuration or depend on the other inherited variable.
The both-present case exercises the actual release-containment call chain.
Existing tool-error, roots, old ancestry, full clone, retained-ref, merge,
tracked-path and shallow-inspection controls remain in the same fixture.
A temporary scanner-failure shim is still used only by its existing explicit
error cases; these new policy controls use installed real gitleaks.

## Deferred final boundary

The existing final command remains:

```bash
bash .github/scripts/tests/containment-fixtures.sh
```

It has **not** been run for this correction. The final OA-18 owner must retain
actual tool version, argv/exit and complete redacted fixture stdout/stderr,
including the permissive-policy precondition and all five new labels. Failure
to match the exact expected status/diagnostic fails the fixture; a source patch
alone is not evidence that the policy precedence works in the installed tool.
The actual full candidate's armed history still requires the separately approved
G-02 roots/purge/cache evidence; this correction grants no certification or
publication authority. G-01 rotation/signer replacement remains deferred and
non-blocking.

Performed: fresh static reads, bounded edits to the two assigned scripts,
authored this note, and SHA256 reads. The review-verification-protocol skill was
read to ground the reported trace in same-turn source; no review was run or new
review verdict issued. No fixture/script invocation, test, `bash -n`, build,
Cargo, scanner execution, network, Docker/browser, key/account action, KBD,
checkbox edit, commit, push, dispatch or publication occurred. No file outside
the two assigned scripts and this evidence note was changed by this correction.
