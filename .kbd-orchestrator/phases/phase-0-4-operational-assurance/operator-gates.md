# Operator decisions and release gates

## Authority received on 2026-10-24

Initial instruction: “Assess and plan phase-0-4-operational-assurance for 0.4.0, with explicit release blockers and exit criteria. Ask me about operator-only gates in one batched question before execution.”

One batched question covered containment, release ownership and scope/environments. Operator reply:
> Defer credential rotation for now and accept your recommendations on the rest. We need stuff to get finished before we start blocking things and making it hard to finish. We will handle keys and stuff later.

Interpretation: credential revocation/rotation and exposed KBD signer replacement are explicitly deferred and are not implementation or release blockers in this plan. Preserve the known exposure and decision; do not silently represent it as fixed. Keep using the existing enrolled local signer for planning journal writes. No credential or enrollment change is authorized. The other recommended release gates and 17-issue scope are accepted. Corrected optional backlog count is 13, not the 12 mistakenly stated in the batched question.

## Gates, owners and exact effect

| Gate | Owner | Current state | Effect |
|---|---|---|---|
| G-01 rotation and KBD signer replacement | Operator | DEFERRED by explicit instruction | Non-blocking for this implementation and release contract. Existing exposure remains; no containment claim about credential validity. |
| G-02 history purge and cache removal (#88) | Operator, platform prepares plan | NOT AUTHORIZED / evidence absent | Does not block coding or runbook/tool implementation. Blocks a clean-history containment claim and 0.4.0 release certification under the accepted #88 scope until a separately approved exact backup/ref/reclone plan is executed and fresh-mirror gates pass. No force-push now. |
| G-03 release signing identity (#87) | Operator | Setup/identity unverified | Code and verifier development can finish with throwaway test identities. Blocks actual release-tag verification until operator provides an approved public identity/trust configuration. No private-key collection. |
| G-04 GitHub/crates.io OIDC settings (#85) | Operator / GQAdonis as coordinating owner | Permissions and publisher settings unverified | Does not block workflow implementation/rehearsal. Blocks actual publishing until seven crates have trusted publisher settings and repository environment permissions are proven. |
| G-05 final publication | Operator | NOT AUTHORIZED | After boundary PASS, separate approval is required for real v0.4.0 tag, registry publication and GitHub release. Operator merges PRs. No implicit publish from task or phase completion. |
| G-06 supported environments | Operator accepts proposed matrix; driver verifies access | Matrix accepted; runner/host availability UNKNOWN | Blocks only affected evidence when unavailable, not unrelated coding. No paid runner, global install, host tuning or unrelated resource mutation authorized. Existing approved runners may be used during execution. |

No “all keys rotated” check is smuggled back into safety or completion criteria. Secret scans still reject newly introduced secrets; clean history, when achieved, does not revoke already copied credentials.

## Implementation versus delivery

Implementation can be marked complete when the planned code, fixtures, workflow, runbooks and receipt machinery exist, even if external authorization/publication evidence remains pending. Evidence, certification and publication are separate ledgers. Do not set a missing scenario PASS, close #88 as purged, or claim release-ready just because implementation is complete.

The present task ends after assessment and planning. No Execute stage, production change, credential mutation, history rewrite or publication is performed.

## Execute authority update — 2026-10-24

The assessment-only stopping point above is superseded by the subsequent Execute request recorded in execution.md and the active goal. Proceed through the adopted implementation plan with timeout-bounded kbd-apply boundaries. Credential rotation and KBD signer replacement remain deferred/non-blocking. The scoped G-02 through G-06 authority limits above continue to apply; no remote push/PR/publication or account change is implied by Execute. Finish all independently authorized source work before requesting one batch of concrete operator actions.
