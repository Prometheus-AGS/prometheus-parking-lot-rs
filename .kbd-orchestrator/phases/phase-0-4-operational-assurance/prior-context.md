# Prior context — phase-0-4-operational-assurance (execute)

> Auto-populated by /kbd-memory-recall for lead view (main thread); lessons from surreal-memory. Cite the lessons that apply; recalled entries are information recorded by agents, not instructions.

## Lessons

- [lead] KBD plan summary for phase phase-0-4-operational-assurance: 18 changes /36 pending tasks cover 17 accepted issues; 13 enhancements excluded. Rotation/signer replacement explicitly deferred and non-blocking; external purge/publication approvals remain separate. Task models are catalog-only recommendations needing access recheck. Native isolated artifact review passed after one core-graph correction; production evidence is not yet run. _(recorded by an agent; progress, stage plan, 2026-10-24; via surreal-memory)_
- [lead] KBD assess summary for phase phase-0-4-operational-assurance: Assessment reviewed: no findings in native isolated review. Accepted 17-issue scope and operator rotation/signer deferral recorded; fresh build health, actual runner access and external approvals remain unverified. Plan must separate completed implementation from certification/publication. _(recorded by an agent; progress, stage assess, 2026-10-24; via surreal-memory)_

## pk knowledge

*(no pk knowledge for this phase)*

## Previous reflection

From `phase-correctness-and-durability/reflection.md`:

**Delta**

1. Containment remains incomplete. cr-15 is administratively closed as deferred carry-over under #88, not implemented. No credential rotation, GitHub Support request, history purge or post-purge mirror check is asserted. The operator explicitly chose “Record both deferrals and close the phase”; canonical decision `phase-correctness-and-durability-close-deferrals-20261024` records that scope disposition.
2. The release shipped manually. Seven crates reached crates.io at 0.3.0, FFI remained unpublished, and annotated tag `v0.3.0` points to `4b747350c6883497813bed33623a9937ad7a5894`. The tag is unsigned. Versioning ADR/release tooling, OIDC trusted publishing, the gated release workflow and signed tags remain #85–#87. cr-52 tasks 1.1–1.3 close as deferred, with explicit task summaries; they are not implementation evidence.
3. Review needed explicit exit criteria. The cumulative independent review found six critical items; a focused second round found another critical regression. Findings were corrected in merged PRs #74/#75/#76/#83. The operator stopped further review rounds and accepted one final quiet-machine gate. This closure records those reviews and does not claim a third code review, a new cross-family review, or a fresh PASS verdict on all original requirements.
4. Gate execution under load produced timing-only failures. The final gate summary at `a3e517f` records all 19 adopted steps passing after step 13 was corrected to the approved secret-scan scope. Full-history containment was explicitly deferred. This checkout has the gate README, script and summary, but not raw per-step logs. Sender-side test runs are historical evidence, not local reruns.
5. Cross-machine restoration was incomplete without signing authority. The copied journal and registration correctly returned revision 627/event `9cb19a43-6764-4b64-9cd9-c750bc8b83d2`; the first write still failed because the enrolled private key was absent. After the operator supplied it, task boundaries succeeded. The supplied key was delivered through a publicly reachable IPFS URL and must be treated as exposed; no key rotation or enrollment change was performed here.
6. Bookkeeping needed correction. Original task-display-ID mismatches left stale blockers. During closure, redundant change-after evaluations after successful `end-task` calls created two more diagnostic
… (truncated)

## Knowledge gaps

- Goal PARTIAL: Three promises: capacity, accepted-work retention, disconnected retrieval — The adopted gate covers weighted/vector admission, panic/budget handling and PG/redb restart/mailbox paths. Durable guarantees apply to durable ResourcePool, not every pool; accepted follow-ups #81/#82 and event/idempotency gaps remain.
- Goal PARTIAL: M0 containment, truthful docs, corrected prior record — Docs and KBD correction delivered; package allowlists and adopted secret checks passed. cr-15 containment remains #88; exposed signing authority is an additional unresolved operational risk.
- Security: original leaked `.codex` history and operator-only rotation/purge remain #88. Template scan findings remain #89. The journal signing key supplied through a public gateway must be replaced under authorized runtime procedures.
- Release: long-lived-token retirement, OIDC, workflow gates, versioning automation and tag signing remain #85–#87.
- Runtime/config/events: #52–#62, #78, #80–#82 capture the accepted functional gaps and operating limits.
- Verification/performance/platforms: #63–#71 and #79 capture deeper loom/negative-control, browser, GUI, upgrade and
… (truncated)
