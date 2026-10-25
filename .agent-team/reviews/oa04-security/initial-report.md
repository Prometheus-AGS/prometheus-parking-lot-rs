Verdict: **Important correction required before security acceptance. No critical issue identified in this review.**

**[.github/scripts/supply-chain.sh:94] Important — ambient configuration can weaken the release history scan**

Exact reviewed line:

`if gitleaks git --no-banner --redact --log-opts="--all HEAD --full-history" . ; then`

`gate_secrets_diff` likewise omits `--config` at line 80. The boundary runner’s complete `environment()` function preserves `GITLEAKS_CONFIG` and `GITLEAKS_CONFIG_TOML` (`operational-assurance.py:136–146`). Gitleaks v8.30.1 explicitly gives those settings precedence over repository configuration when `--config` is absent. [Pinned upstream implementation](https://raw.githubusercontent.com/gitleaks/gitleaks/v8.30.1/cmd/root.go).

Named failure scenario: an approved self-hosted runner retains a permissive scanner configuration. A secret introduced into ordinary historical source and removed from the tip escapes that configuration; the explicitly configured tip scan passes, and `release-containment` can report clean history despite bypassing the candidate’s scanner rules. Existing containment fixtures mask this scenario by explicitly unsetting both variables (`containment-fixtures.sh:18`).

Fix: explicitly select the candidate repository’s `.gitleaks.toml` in both production invocations. Add a synthetic final-boundary control that supplies conflicting ambient configuration and still requires detection. This corrects an existing scanner trust boundary; it does not add a credential-rotation gate.

Strengths observed:

- Exact annotated tag headers bind candidate and tag name (`verify-release-tag.py:60`: `require(headers[:3] == …)`); signer fingerprint and public trust are independently checked.
- Artifact acceptance verifies run provenance, workflow source, approval, freshness and server archive digest (`release_evidence.py:185–200`).
- Candidate preparation code loads after signature and receipt binding (`trusted-release.py:75–82`).
- Raw archives reject symlinks and escaping paths (`operational-assurance.py:667–669`: `require(… "unsafe raw archive size/symlink"); dest = safe(folder, info.filename)`).
- Desktop deletion is limited to the owned control store after process and digest checks (`desktop-assurance.py:341–357`: `store.unlink()`).

Limitations: static review only, against the dirty candidate over `92da0d3d28f7896ddc67062fa49ba40ea48d74a3`. No tests, builds, Cargo commands, actual deletion, account changes or secret reads occurred. Live GitHub approval/OIDC behavior and operator-provisioned trust remain unverified. Credential rotation and signer replacement remain explicitly deferred and nonblocking. No files changed.
