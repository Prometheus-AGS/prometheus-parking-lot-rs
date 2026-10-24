Correction-only confirmation for candidate `729cc42549defb5fcf69b432485224cb89da74bc`. Scope: the two initial findings and their relevant callers/fixtures; no fresh general review.

1. **OA-02 ancestor/final-candidate route — RESOLVED in source.** `.github/operational-assurance.json:603–620` now passes `"--candidate", "{preparation_source}"` separately from `"--final-candidate", "{candidate}"` and `"--preparation-plan", "{preparation_plan}"`. `operational-assurance.py:355–359` requires original-plan source/base/date equality, then:

   ```python
   require(plan["candidate"] != args.candidate, "OA-18:preparation-source-must-precede-final")
   validate_preparation(ROOT, plan, args.candidate, "0.4.0")
   ```

   The shared validator checks ancestor linkage (`release_evidence.py:274`) and exact final file hashes (`:279`). The fixture invokes preparation using the ancestor (`test-preparation.py:109–112`), compares original/reproduced portable plan fields and tool identities (`:67–76`), and checks both prepared and final bytes (`:77–79`). It includes altered-plan rejection controls (`:155–179`) and original-byte mutation rejection (`:223–225`). Collection revalidates the exported original plan (`operational-assurance.py:609–614`) and requires its digest to equal the collector’s plan (`:800–805`). Thus the final 0.4.0 candidate no longer must satisfy the preparer’s strictly-greater-than-current rule itself.

   Static observations: `git merge-base --is-ancestor 72adf5d8ad583c3b52b22194563e94f7895c5b3f HEAD` succeeded. That ancestor declares 0.3.0; current `Cargo.toml:19` declares 0.4.0. Original plan SHA256 is `b3bf274bc82c843285e7889877b0b84b98474ddf72d7747aadf663321e532fd1`; its source is that ancestor, and all 13 recorded file hashes matched the current files. Both release scripts are now tracked.

2. **Linux desktop OS/build-case binding — RESOLVED in source.** `.github/operational-assurance.json:2545–2564` now places `desktop-linux` in `"profile": "linux"` and requires `"os": "Linux", "case_id": "OA15-BUILD-LINUX"`. Distinct macOS and Windows rows require their corresponding pairs (`:2539–2541`, `:2585–2587`). `operational-assurance.py:94–95` enforces:

   ```python
   require(all(data.get(key) == value for key, value in row.get("receipt_identity", {}).items()),
           "OA-18:child-receipt-identity: required OS/build case differs")
   ```

   Execution calls this guard (`:517`); collection requires the child receipt and applies the checked definition’s identity guard (`:618–623`). Fixtures cover matching pairs, wrong OS, wrong case, another OS’s full pair, and three distinct profile mappings (`boundary-packet.py:89–116`).

Limits: this confirmation used source reads, Git identity/ancestry inspection and static file hashing only. No tests, Cargo/build commands, workflow execution or network mutation ran. The parent reports successful real-tool preparation; I did not rerun it. The final full baseline, corrected fixtures, three OS builds and complete certification remain unverified here. No production source, index or HEAD changed during this confirmation.

Created owned review artifacts: `.agent-team/reviews/oa04-contract/initial-report.md` preserves the initial report; `.agent-team/reviews/oa04-contract/correction-confirmation.md` preserves this confirmation.

## Review Summary

| Severity | Unresolved prior findings | Status |
|----------|---------------------------|--------|
| CRITICAL | 0 | pass |
| HIGH | 0 | pass |
| MEDIUM | 0 | pass |
| LOW | 0 | pass |

Verdict: APPROVE for the two source corrections. This is not release certification.
