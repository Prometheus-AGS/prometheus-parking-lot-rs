Correction-only verdict: **Resolved in source** at candidate `729cc42549defb5fcf69b432485224cb89da74bc`. Runtime confirmation remains **NOT_EXERCISED**.

Fresh exact proof:

- `.github/scripts/supply-chain.sh:81`: `gitleaks git --config .gitleaks.toml --no-banner --redact --log-opts="$range" .`
- `.github/scripts/supply-chain.sh:95`: `if gitleaks git --config .gitleaks.toml --no-banner --redact --log-opts="--all HEAD --full-history" . ; then`

The script first changes to the candidate Git top-level directory at line 18. The complete reviewed diff/history functions now explicitly select that repository’s scanner policy. The `release-containment` caller reaches the corrected history function at line 159; dispatch still reaches the corrected diff/history functions at lines 183–184. Inherited scanner configuration therefore no longer selects their detection policy under the previously verified pinned-tool precedence.

The reviewed correction fixture preserves ambient overrides at lines 18–19. Its explicit permissive-policy precondition at lines 174–177 runs `gitleaks git --config "$ambient_config"` on the same synthetic-history range and requires exit 0 before proceeding. Lines 179–183 then exercise five actual production-path cases: file override/history, file override/diff, inline override/history, inline override/diff, and both overrides/release-containment. Every case requires exit 1 and its designated scanner-failure diagnostic through the complete `expect_gate` function at lines 49–65. The canary was committed and removed from the tip at lines 150–157, so these controls address the reported historical-secret scenario.

Read the correction evidence note `.kbd-orchestrator/phases/phase-0-4-operational-assurance/evidence/implementation/oa-18-containment-config-correction.md`; its NOT_EXERCISED disposition matches this review. The requested production fix and conflicting-configuration controls are present. No unresolved source issue remains from the sole reported finding.

Limits: confirmation is restricted to the prior inherited-Gitleaks-configuration finding. Only source/evidence reads and reviewer report writes occurred. No fixture, scanner, test, Cargo/build, network mutation, destructive operation, secret read, source/index/HEAD/KBD mutation or subagent ran. The parent owns serialized execution and must retain the actual permissive-policy precondition plus all five case results; this confirmation claims no containment-fixture PASS, release certification or publication authority. Credential rotation and signer replacement remain operator-deferred and nonblocking.

Created reviewer artifacts containing the exact initial report and this confirmation:

- `.agent-team/reviews/oa04-security/initial-report.md`
- `.agent-team/reviews/oa04-security/correction-confirmation.md`
