// Writes execution.md (dispatch contract) and execute-dispatch.json (dispatch receipt).
// Usage: node execution-generate.mjs <repo-root> <dispatched-change-id,...>
import { writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { PHASE, ROUTES, CHANGES } from './plan-data.mjs';

const [root, dispatchedArg = ''] = process.argv.slice(2);
const dispatched = dispatchedArg.split(',').filter(Boolean);
const phaseDir = join(root, '.kbd-orchestrator/phases', PHASE);
const now = new Date().toISOString();
const DEFERRED = new Set(['cr-15-containment']);
const deferredTasks = { 'cr-52-release': [2] };

const scope = CHANGES.map((c) => `- ${c.id}: ${c.title}${DEFERRED.has(c.id) ? ' — DEFERRED (operator decision phase-correctness-and-durability-defer-token-plans)' : ''}`).join('\n');

const contracts = CHANGES.flatMap((c) => c.tasks.map((tk, i) => {
  const r = ROUTES[tk.role];
  const id = i + 1;
  const deferred = DEFERRED.has(c.id) || (deferredTasks[c.id] ?? []).includes(id);
  const isRev = tk.role === 'reviewer';
  const isOp = tk.role === 'operator';
  return `- ${PHASE} / ${c.id} / ${id} → ${isOp ? 'operator' : 'Claude Code (this session)'}${deferred ? ' — DEFERRED' : ''}
  Assignment: plan.md Task model assignments row \`${PHASE} | ${c.id} | ${id}\`
  Entry: ${isOp ? 'operator action recorded here' : `Agent tool subagent_type=\`${tk.role}\`; worktree /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/${isRev ? 'reviewer' : tk.role} (scripts/agent-worktree.sh add ${isRev ? 'reviewer' : tk.role} ${c.id}); skills per .agent-team/parking-lot-core/team.json; scope: openspec/changes/${c.id}/ task 1.${id}`}
  Concrete model: ${r.model}; reasoning effort: ${r.effort}
  Route: ${isOp ? 'n/a' : 'native Claude Code subagent (subagent type listed in this session; first invocation pending)'}
  Model rationale: per plan assignment (role tier set in .claude/agents/${tk.role}.md, 2026-10-22)
  Native alternative: ${isRev ? 'Codex reviewer for cross-family review — unresolved (no model configured)' : 'Codex/OpenCode/Kimi definitions of the same role (source-verified only)'}
  Prerequisites: ${isRev ? 'all production changes complete (cumulative review at the phase boundary)' : c.deps.length ? `merged to main: ${c.deps.filter((d) => !DEFERRED.has(d)).join(', ') || 'none'}` : 'none'}
  Handoff: Return artifacts and evidence to the KBD driver; do not update task state`;
})).join('\n\n');

const ledger = CHANGES.map((c) => `- [${DEFERRED.has(c.id) ? 'BLOCKED' : dispatched.includes(c.id) ? 'IN_PROGRESS' : 'PENDING'}] ${c.id} — ${c.owner}`).join('\n');

writeFileSync(join(phaseDir, 'execution.md'), `EXECUTION: ${PHASE}
Project: prometheus_parking_lot
Date: ${now.slice(0, 10)}
Selected backend: openspec (driven task-by-task through kbd-apply; detect → openspec)
Dispatched to: Claude Code subagents of team parking-lot-core (SELF as KBD driver)
Backend rationale: 37 OpenSpec changes exist and validate strictly; the harness exposes every team role as a native subagent; KBD keeps task ownership through kbd-apply begin-task/end-task.
Backend entrypoint: kbd-apply.sh begin-task/end-task per task; Agent tool per role for implementation
OpenSpec available: YES
Source plan: .kbd-orchestrator/phases/${PHASE}/plan.md

## Operator decisions in force

- 2026-10-22 "Don't worry about token plans for now. this is being restructured. go directly to execute." → cr-15-containment (key rotation, history purge, GitHub cache purge) and cr-52 task 2 (trusted publishing / token retirement) are DEFERRED and parked as blocked; they no longer gate cr-41, cr-17 or cr-51. Recorded as decision \`phase-correctness-and-durability-defer-token-plans\` and blocker \`cr-15-token-plans\`.
- Merges to main are operator-only; each change is delivered as one PR from its role branch.

## Protocol notes

- Per the execute protocol, reviewer tasks ("Independent review at the merge gate", the last task of most changes) are NOT run per change. Reviewers stay dormant; one cumulative review covers the whole phase diff at the boundary, then those review tasks are closed with its receipt. Operator PR review is the only per-change gate.
- No per-task test loops. Workers verify at their change boundary with the narrowest real command their acceptance needs (cr-41's acceptance is itself a build gate). The contract boundary packet runs once after all production changes.
- Recalled lessons passed to every worker: real-service tests for external integrations; boundary gates build the packaged artifact; tests discover their environment (no primary-checkout or warm build-dir assumptions); operator-only actions are asked once; check harness configs before removing worktrees.

EXECUTION SCOPE

${scope}

DISPATCH CONTRACTS

${contracts}

APPROVAL GATES

- Operator merges every change PR.
- cr-52 publication requires the boundary PASS receipt plus explicit operator approval.
- Deferred: cr-15 history purge approval (on hold).

FALLBACK CONDITIONS

- A role subagent cannot be invoked or fails twice on the same task: record the failure here and execute that task's instructions in the driver context, labelled as such (never as independent review).
- A change exceeds its repair budget (3 attempts) or makes no progress for 2 rounds: mark the change BLOCKED and report.

VERIFICATION REQUIREMENTS

- Change boundary (author): the narrowest real command that proves the change's own acceptance, e.g. cr-41 \`cargo check --workspace --all-targets --all-features --locked\` on 1.99.1 and 1.95, \`cargo clippy ... -D warnings\`, \`cargo fmt --all -- --check\`.
- Phase boundary (driver): the adopted contract boundaryPacket, once.

PROGRESS LEDGER

${ledger}

OUTPUTS

- Per change: a branch team/<role>/<change-id>, a PR to main, and a hand-back with changed files, commands and output.

BLOCKERS

- cr-15-token-plans (deferred by operator)

PLANNED REFLECTION INPUTS

- Hand-backs, PR links and merge SHAs per change; the boundary packet evidence folder; review receipts; deviations recorded above (deferred containment, cumulative review instead of per-change review, same-family review).

DISPATCH READY — EXECUTE REMAINS ACTIVE
`);

writeFileSync(join(phaseDir, 'execute-dispatch.json'), JSON.stringify({
  phase: PHASE,
  dispatchedAt: now,
  backend: 'openspec',
  driver: 'Claude Code session (KBD driver)',
  dispatched: dispatched.map((id) => ({ change: id, role: CHANGES.find((c) => c.id === id).owner })),
  deferred: [...DEFERRED],
  pending: CHANGES.filter((c) => !dispatched.includes(c.id) && !DEFERRED.has(c.id)).map((c) => c.id),
}, null, 2) + '\n');
console.log(JSON.stringify({ dispatched, pending: CHANGES.length - dispatched.length - DEFERRED.size }));
