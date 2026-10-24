# Assessment: phase-0-4-operational-assurance

Project: prometheus_parking_lot  
Date: 2026-10-24  
Baseline: main at 92da0d3d28f7896ddc67062fa49ba40ea48d74a3, merged PR #93.  
Authorization: assess and plan 0.4.0; execution and operator-only actions are not authorized. The operator accepted the recommended scope and other gates, while explicitly deferring credential rotation and KBD signer replacement; see operator-gates.md.

## Observed baseline and cross-tool progress

The predecessor is COMPLETE at revision 876, last event 882072c6-10d7-4340-bf9f-a3a804b9218e. Its 37 changes and 132 task dispositions include seven explicit deferrals. The closure commit is on main; 173 audit checksums match, 37 archives and 30 canonical specs exist. The new phase was created through the supported lifecycle helper, not by editing projections. There are no active OpenSpec changes at assessment start. No other tool has claimed implementation in this phase.

The GitHub 0.4.0 milestone has 30 open issues. The dated snapshot is evidence/issue-inventory.json. Issue reports and static inspection are evidence of known gaps, not new reproductions. No fresh build, Docker, browser, desktop, benchmark or migration run occurred during assessment.

## Implementation status and spec alignment

| Area / canonical capability | Status | Evidence and gap |
|---|---|---|
| Core contracts, capacity-admission, fault-containment | PARTIAL | Released weighted/vector admission exists. #82 reports pre-populated TaskQueueAdapter entries bypass admission; #81 reports a discarded supervise future loses terminal handling. ResourcePool::launch passes an unpolled supervise future to Spawn; SettleOnDrop is constructed inside that async body (resource_pool.rs around 688–710). |
| Durable-dispatch and durable-mailbox-postgres | PARTIAL | PG/redb lease and deliver-before-ack paths exist. durable_pool/mod.rs around 1033–1091 emits the local delivery status; deliver returns bool and discards DeliveryOutcome. #78 requires stored-outcome terminal consistency after redelivery. |
| Result-delivery, embedded-queue, result-retrieval | PARTIAL | The mailbox conformance first_write_wins case explicitly treats the same idempotency key under another MailboxKey as Duplicate (contracts/conformance/mailbox.rs around 144–173). PG uniqueness is (mailbox_name, idempotency_key), embedded keys omit tenant scope. #80 is a deliberate contract/migration change, not a claim current behavior violates its present contract. Proposed 0.4 contract is tenant-scoped uniqueness; operator adoption pending. |
| Lifecycle-events and shutdown-cancellation | PARTIAL | queue.rs prune_expired returns only a count; ResourcePool pruning cannot emit per-task Expired (#57). #81's dropped future leaves its shutdown ledger unsettled. Do not broaden durable guarantees to native WorkerPool. |
| Supply-chain-safety | PARTIAL | Current tracked-path, diff and package checks exist. Full-history scans are informational and pre-purge is inert with no roots. #88 remains open; rotation/purge/cache-removal are unverified. The exposed journal signing authority remains a disclosed risk; replacement is operator-deferred and not a prerequisite. #89 records two template false positives. |
| Release-management | PARTIAL | 0.3.0 was manually published; annotated unsigned tag. OIDC workflow (#85), version ADR/release-plz/git-cliff (#86), signing (#87) remain missing. FFI stays unpublished. |
| Test-infrastructure / concurrency | PARTIAL | Existing loom models and controls exist; #64 adds ledger/kind-release models, #79 automates all non-loom negative controls. Compilation failures cannot count as successful negative-control detection. |
| Platform-targets | PARTIAL | wasm tests run in Node; headless browser evidence (#65) is missing. Tauri app launch/restart and Linux/Windows build coverage (#67) are missing. Existing Android/iOS compile claims remain historical. |
| Postgres-compatibility / postgres-test-substrate | PARTIAL | PG17/18 test paths and images exist. A populated 17→18 upgrade has not been exercised (#66). |
| Stream-replay / iggy-events | PARTIAL | Writer poisoning exists; real-server abandoned-chunk evidence is missing (#68). Restarted-publisher dedup limits remain #62, outside proposed scope. |
| Performance-evidence | PARTIAL | In-memory/redb evidence exists; PG contention/memory measurements missing (#71). Fast-path optimization #63 is separate optional work. |
| Configuration / documentation | PARTIAL | Configuration gaps #52–#56/#61 and ADR housekeeping #70 remain open; proposed outside this release-blocking phase, with documented limits retained. No feature completeness assertion for them. |
| Build-baseline, continuous-integration, crate-structure, backend-contracts, delivery-record, documentation-content/integrity/site, result slots | Existing / current health UNKNOWN | No new defect beyond the inventory established here. Existing canonical specs are the baseline, not fresh validation of every requirement. |

## Build health, coverage and constraints

- Build/check/clippy/tests: UNKNOWN for this receiving checkout; intentionally not run before the complete implementation boundary.
- Coverage: PARTIAL by the documented missing real-collaborator scenarios; numeric coverage UNKNOWN. A percentage would be invented.
- Historical sender gate: 19 adopted steps passed; transferred raw per-step logs are absent. Not 0.4.0 evidence.
- No Rust source, dependency, workflow or public API changed during this assessment.
- Use pinned Rust 1.99.1, MSRV 1.95, edition 2024 from real manifests/agent-context; project.json's 1.91.1/2021/source paths and old machine paths are stale metadata, not implementation authority.
- Preserve core's no-runtime/no-IO dependency boundary; every public contract change needs changelog and all-backend conformance.
- Single Cargo writer per target; managed OpenSpec operations serialized. Reviewers and production verification stay dormant until complete implementation. Artifact review now is planning review only.
- Never infer operator approval from prior 0.3.0 deferrals, a completed task counter, or this assessment.
- OpenSpec lifecycle refresh: 1.15.0, latestVerified=true, no authored paths changed. Generated integration changes are a separate tooling side effect with a backup receipt.
- Model preflight is degraded: cached 2026-10-22 result, missing resolver and unverified gateway identities. No provider/config mutation or remote judge dispatch is authorized. Use an explicitly labelled native isolated artifact reviewer; do not claim cross-model certification.
- The prior journal signer is still selected locally for planning journal writes; its replacement is not performed or certified by this phase setup.

## Outcome inputs and operator disposition

User operation: an operator can release seven 0.4.0 crates through an approval-gated, signed, credential-safe path; an application using the supported pools/backends can retrieve tenant-isolated results and terminal outcomes consistently after the named drop, expiry and crash scenarios.

Baseline: released 0.3.0 plus the concrete gaps above. UNKNOWN: fresh health, credential revocation, signer enrollment replacement, available desktop runners, migration compatibility and account permissions.

Desired result: the finite OA-01…OA-17 scenario set in the plan passes against the frozen candidate, while every operator release gate has attributable non-secret evidence. Shipping counts alone never satisfy the outcome.

Accepted supported environment scope (actual access unverified): native Tokio and native WorkerPool only within their existing contracts, PG17/18, redb, pinned Iggy, wasm headless Chromium plus existing Node coverage, and Tauri macOS/Linux/Windows. Android/iOS existing build checks remain regression gates. The exact runner/image/tool versions must be frozen before adoption; host installation and paid resources require separate approval.

Required safety: credential-path/diff/full-history/package checks after containment; recorded pre-purge roots and scratch-branch rejection; core/wasm dependency boundaries; no unsafe code; fresh real-backend/crash tests; signed immutable tag; release environment approval; never expose keys in evidence.

Accepted scope: #57, #64–#68, #71, #78–#82, #85–#89 (17 issues). Remaining 13 are explicitly unadopted optional work, listed in plan.md. No silent deferral of an adopted blocker on budget exhaustion. Credential/signer rotation is explicitly deferred by the operator and is not a blocker; do not infer any broader unrecorded risk acceptance.

Decision owner: the human operator GQAdonis. Asked once in the active chat; the operator replied: “Defer credential rotation for now and accept your recommendations on the rest.” operator-gates.md records the full reply and exact effects. Plan can bind the accepted scope, while separate history-rewrite/publication approvals and actual environment availability remain unresolved. Execute is not entered by this planning request.

## Goals

1. Assessment and plan with explicit gates: PARTIAL until artifact review and plan binding complete; operator response is recorded.
2. Accepted-work/result/event gaps: NOT MET in this phase; proposed changes identified.
3. Containment/release controls: NOT MET; operator-owned prerequisites outstanding.
4. Operational/platform assurance: NOT MET; finite missing evidence identified.

## Prior context and knowledge gaps

The refreshed prior-context.md reports no recalled lessons and no pk knowledge; do not fabricate lesson IDs. Its previous-reflection excerpt provides these applied constraints:
- “Administrative completion is not evidence those controls shipped”: keep #85–#88 and signer replacement as release gates.
- “Sender-side test runs are historical evidence, not local reruns”: require fresh, raw 0.4.0 evidence.
- “The supplied key was delivered through a publicly reachable IPFS URL and must be treated as exposed”: operator replacement gate precedes trusted execution.

Every visible Knowledge gaps entry is retained here:
1. Goal PARTIAL: three promises (capacity, accepted-work retention, disconnected retrieval); durable guarantees are scoped and #81/#82/event/idempotency gaps remain.
2. Goal PARTIAL: M0 containment, truthful docs and corrected prior record; containment and signer authority unresolved.
3. Security: leaked .codex history, operator rotation/purge #88, template findings #89 and exposed journal key.
4. Release: long-lived token retirement, OIDC, workflow gates, versioning and signing #85–#87.
5. Runtime/config/events: #52–#62, #78, #80–#82.
6. Verification/performance/platforms: #63–#71 and #79 (the recall excerpt is truncated; full predecessor reflection and issue inventory supply the source).

Optional learning routes, never invoked automatically: /learn-goal "accepted-work and durable result semantics"; /learn-goal "credential containment and signing authority"; /learn-goal "OIDC release approvals"; /learn-goal "runtime configuration and event contracts"; /learn-goal "concurrency and cross-platform production verification". These do not replace the concrete phase work.

## Uncomfortable constraint

The proposed assurance phase spans several operating systems, account authority and a public-history rewrite. A smaller code-only slice could ship faster but would leave the inherited release controls unresolved. A plan cannot claim the exposed signer has been neutralized or the 0.4.0 release is safe simply because all checkboxes are closed.

ASSESSMENT COMPLETE — accepted scope and explicit rotation deferral recorded; production outcomes remain unverified.
