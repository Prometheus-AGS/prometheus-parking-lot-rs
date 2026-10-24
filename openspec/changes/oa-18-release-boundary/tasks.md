# Tasks

Phase: phase-0-4-operational-assurance. Assignment source: .kbd-orchestrator/phases/phase-0-4-operational-assurance/plan.md, Task model assignments, this change's backend IDs 1 and 2. Display numbers 1.1/1.2 are not assumed canonical backend IDs until driver reconciliation.

## 1. Assemble and evaluate one candidate boundary packet

- [x] 1.1 Implement the candidate-bound boundary runner and receipt/approval verifier; verify candidate packet fixture/manifest coverage by static inspection
- [ ] 1.2 Assemble the completed candidate and record the final integration, review and publication dispositions; verify artifact links and contract/documentation handoff

Production fixtures and documentation are part of this coherent change. Final real verification is owned by oa-18 task 2, after all production edits; no per-edit Cargo/test loops. All OA scenarios and exact acceptance oracle are in design.md and the phase contract. Never check off either task by hand; begin/end through timeout-bounded kbd-apply with stdin closed.

## Workflow follow-up

- Run the final adopted packet at the completed implementation boundary, then attach the applicable receipt.
- Record implementation separately from external certification/publication evidence.
- Verify and archive only after the applicable gates. Do not rotate credentials, rewrite history, change external settings or publish without scoped authority.
