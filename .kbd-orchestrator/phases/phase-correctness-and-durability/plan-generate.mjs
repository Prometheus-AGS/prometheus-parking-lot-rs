// Generates openspec/changes/<id>/* and plan.md from plan-data.mjs.
// Usage: node plan-generate.mjs <repo-root>
import { mkdirSync, writeFileSync, existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { PHASE, ROUTES, CHANGES, BOUNDARY } from './plan-data.mjs';

const root = process.argv[2];
if (!root) throw new Error('usage: node plan-generate.mjs <repo-root>');
const phaseDir = join(root, '.kbd-orchestrator/phases', PHASE);
const today = '2026-10-22';

// ── validate graph ──
const byId = new Map(CHANGES.map((c) => [c.id, c]));
if (byId.size !== CHANGES.length) throw new Error('duplicate change id');
for (const c of CHANGES) for (const d of c.deps) if (!byId.has(d)) throw new Error(`${c.id}: unknown dep ${d}`);
const round = new Map();
const visiting = new Set();
const level = (id) => {
  if (round.has(id)) return round.get(id);
  if (visiting.has(id)) throw new Error(`cycle at ${id}`);
  visiting.add(id);
  const c = byId.get(id);
  const r = c.deps.length ? 1 + Math.max(...c.deps.map(level)) : 1;
  visiting.delete(id);
  round.set(id, r);
  return r;
};
CHANGES.forEach((c) => level(c.id));
// Owner serialization: one role works one change at a time; push a change to a later round
// when its owner already has a change in the same round (keeps rounds honest for one agent per role).
// List scheduling: each round, every owner takes its highest-value ready change
// (dependencies finished in earlier rounds); ties keep data-file order.
const finalRound = new Map();
const valueRank = { HIGH: 0, MEDIUM: 1, LOW: 2 };
for (let r = 1; finalRound.size < CHANGES.length; r += 1) {
  if (r > CHANGES.length * 2) throw new Error('scheduling did not converge');
  const ready = CHANGES.filter((c) => !finalRound.has(c.id) && c.deps.every((d) => finalRound.has(d) && finalRound.get(d) < r))
    .sort((a, b) => valueRank[a.value] - valueRank[b.value] || CHANGES.indexOf(a) - CHANGES.indexOf(b));
  const taken = new Set();
  for (const c of ready) if (!taken.has(c.owner)) { taken.add(c.owner); finalRound.set(c.id, r); }
}
const order = [...CHANGES].sort((a, b) => finalRound.get(a.id) - finalRound.get(b.id) || CHANGES.indexOf(a) - CHANGES.indexOf(b));

// ── openspec changes ──
for (const c of CHANGES) {
  const dir = join(root, 'openspec/changes', c.id);
  mkdirSync(join(dir, 'specs', c.capability), { recursive: true });
  if (!existsSync(join(dir, '.openspec.yaml'))) writeFileSync(join(dir, '.openspec.yaml'), `schema: spec-driven\ncreated: ${today}\n`);
  writeFileSync(join(dir, 'README.md'), `# ${c.id}\n\n${c.title}\n`);
  writeFileSync(join(dir, 'proposal.md'), `## Why

${c.why}

## What Changes

${c.details}${c.stretch ? '\n\n**Stretch:** never adopted as required scope; deferrable without blocking the phase.' : ''}

## Capabilities

### New Capabilities

- \`${c.capability}\`: ${c.requirements.map((r) => r.name).join('; ')}

### Modified Capabilities

None (no canonical specs exist yet; this phase establishes them).

## Impact

- Milestone ${c.milestone}; owner role \`${c.owner}\` (team \`parking-lot-core\`, docs/agent-team.md).
- Depends on: ${c.deps.length ? c.deps.map((d) => `\`${d}\``).join(', ') : 'none'}.
- Plan of record: \`.kbd-orchestrator/phases/${PHASE}/plan.md\`; source audit \`docs/update-plan-10-22-2026.md\`.
`);
  const spec = c.requirements.map((r) => `### Requirement: ${r.name}

${r.shall}

${r.scenarios.map((s) => `#### Scenario: ${s.name}

- **WHEN** ${s.when}
- **THEN** ${s.then}`).join('\n\n')}`).join('\n\n');
  writeFileSync(join(dir, 'specs', c.capability, 'spec.md'), `## ADDED Requirements\n\n${spec}\n`);
  writeFileSync(join(dir, 'tasks.md'), `Model and worker assignments for these tasks: \`.kbd-orchestrator/phases/${PHASE}/plan.md\` → Task model assignments, rows keyed \`${PHASE} / ${c.id} / <task id>\`.

## 1. ${c.title}

${c.tasks.map((tk, i) => `- [ ] 1.${i + 1} ${tk.title}`).join('\n')}
`);
}

// ── plan.md ──
const priorLessons = [
  ['External integrations require at least one real-service test', 'CR-26/27/30 run against the project Postgres 18 image; CR-32/33 against a pinned Iggy container; mocks are not completion evidence.'],
  ['Boundary gates must launch the packaged artifact', 'The boundary packet runs `cargo package --workspace --locked`, which packages and verify-builds every crate from its tarball (intra-workspace dependencies resolve through a temporary local registry), and builds the wasm/mobile targets and the docs site — not only `--lib --tests`.'],
  ['Discover test environment state instead of encoding checkout assumptions', 'CR-43 test helper builds or reuses the image by tag and never assumes the primary checkout path or a warm build-dir (one worktree per role).'],
  ['Ask once at phase boundary for operator-only actions', 'Operator-only actions are collected up front: key rotation and history-purge approval (CR-15) before any worktree exists; merges stay operator-owned.'],
  ['Check harness configs before removing worktrees', 'Applies when roles finish and `scripts/agent-worktree.sh remove` runs.'],
];

const changeList = order.map((c, i) => `${i + 1}. ${c.id}: ${c.title}
   - Milestone: ${c.milestone} · Round: ${finalRound.get(c.id)}${c.stretch ? ' · STRETCH (deferredOptional)' : ''}
   - Scope: ${c.capability}
   - Depends on: ${c.deps.length ? c.deps.join(', ') : 'NONE'}
   - Recommended agent: Claude Code subagent \`${c.owner}\` (parking-lot-core)
   - Est. complexity: ${c.size}
   - Complexity score: ${c.complexity}
   - Model class: ${c.cls}
   - Customer value: ${c.value}
   - Details: ${c.details}`).join('\n\n');

const rows = [];
for (const c of order) c.tasks.forEach((tk, i) => {
  const r = ROUTES[tk.role];
  const isOp = tk.role === 'operator';
  const isRev = tk.role === 'reviewer';
  rows.push(`| ${PHASE} | ${c.id} | ${i + 1} | ${tk.needs} | ${r.model} | ${r.effort} | ${isOp ? 'Account-bound action; only the operator can perform it.' : isRev ? 'Independent review needs strongest reasoning; same-family as builders (no cross-family judge configured, preflight `degraded` 2026-10-22) — recorded as a limitation.' : `Role \`${tk.role}\` model set in .claude/agents/${tk.role}.md (team manifest 2026-10-22); tier ${c.cls === 'frontier' ? 'hard' : 'medium'} matches ${c.complexity.toLowerCase()} complexity.`} | ${isOp ? 'none (human)' : 'Claude Code (this session); native'} | ${isOp ? 'Operator records confirmation in execution.md' : `Agent tool, subagent_type=\`${tk.role}\`; worktree /Volumes/T7/Projects/prometheus/prometheus-parking-lot.worktrees/${isRev ? '<author-role>' : tk.role} (branch team/${isRev ? '<author-role>' : tk.role}/${c.id}); skills per team manifest; result → hand-back to the KBD driver, recorded in execution.md`} | ${isOp ? 'n/a' : isRev ? 'Codex `reviewer` (.codex/agents/reviewer.toml) for cross-family review — unresolved: model not configured' : `Codex/OpenCode/Kimi \`${tk.role}\` definitions (source-verified only)`} | ${isOp ? 'operator availability' : 'Subagent type listed by the harness in this session (configured); not yet invoked (unverified)'} | ${isRev ? 'Cross-family judge needs a liter-llm provider (preflight degraded)' : c.deps.length ? `Merged: ${c.deps.join(', ')}` : 'none'} |`);
});

const rounds = [...new Set(order.map((c) => finalRound.get(c.id)))].map((r) => `Round ${r}${order.filter((c) => finalRound.get(c.id) === r).length > 1 ? ' (parallel)' : ''}: ${order.filter((c) => finalRound.get(c.id) === r).map((c) => c.id).join(', ')}`).join('\n');

const contractPath = `.kbd-orchestrator/phases/${PHASE}/adopted-contract.json`;
let digest = 'PENDING (written by kbd-contract-adopt.sh)';
let planRevision = 'PENDING';
const cp = join(root, contractPath);
if (existsSync(cp)) {
  const j = JSON.parse(readFileSync(cp, 'utf8'));
  if (j.identity?.contractDigest) digest = j.identity.contractDigest;
  if (j.identity?.planRevision != null) planRevision = String(j.identity.planRevision);
}

const totalTasks = CHANGES.reduce((n, c) => n + c.tasks.length, 0);
writeFileSync(join(phaseDir, 'plan.md'), `PLAN: ${PHASE}
Project: prometheus_parking_lot
Date: ${today}
OpenSpec available: YES
Changes to implement: ${CHANGES.length} (${CHANGES.filter((c) => c.stretch).length} stretch) · Tasks: ${totalTasks}

Generated from \`plan-data.mjs\` by \`plan-generate.mjs\` — edit the data file and regenerate; do not hand-edit.

## Inputs and operator decisions

- Assessment: \`assessment.md\` (adversarial review PASS 0/7/2, all warnings applied).
- Plan of record: \`docs/update-plan-10-22-2026.md\` §11, extended per the assessment and these operator decisions (2026-10-22, recorded in \`.kbd-orchestrator/constraints.md\` → Phase decisions):
  1. \`.codex\` credential/runtime state untracked and gitignored now (this plan's branch); constraints amended; CR-15 rotates keys and purges history with operator approval.
  2. Toolchain pinned to Rust 1.99.1 (latest stable, released 2026-10-13); MSRV 1.95; edition 2024 — pulled into M0 (\`cr-41\`). The pgmq client crate is dropped; pgmq is reached through SQL (\`cr-30\`). Postgres tests use a project Postgres 18 image modelled on flint-forge \`images/postgres18/Dockerfile\` (\`cr-43\`).
  3. Correctness over compatibility: breaking API ships as 0.3.0; CI may stay red until the M0 baseline lands.
  4. Added scope: a branded Docusaurus site with know-me branding (\`cr-45\`) and extensive theory/history/usage documentation (\`cr-46\`, \`cr-47\`).

## Recalled lessons applied

${priorLessons.map(([l, how]) => `- **${l}** — ${how}`).join('\n')}

## Changes vs. the audit plan

- **Added:** \`cr-41-toolchain-baseline\` (toolchain/edition/lock/all-targets compile), \`cr-42-ci-rebuild\`, \`cr-43-postgres18-image\`, \`cr-48-workspace-split\`, \`cr-49-durable-dispatch\` (pools persist, lease, ack-after-delivery and extend through the durable queue — the crash-durability promise end to end), \`cr-44-mobile-desktop-bindings\`, \`cr-45/46/47\` documentation.
- **Plan review round 1 (BLOCK, 3 critical, 14 warnings) applied:** durable dispatch added; history purge moved after this plan merges, run from a fresh mirror clone, with a CI gate rejecting pre-purge history; negative controls use the test-only \`--cfg parking_lot_negative_control\`, never a Cargo feature; loom runs under \`--cfg parking_lot_loom\` on the tokio-free core with bounded preemptions; scenario entry points re-targeted to the crates that hold the code; the cost-vector type moved into cr-18; hook emission moved after shutdown/cancellation and owned by the scheduler; config schema scoped to merged backends; sea-orm removal moved to cr-27; WASM pool ported by its owner in cr-38; cr-43/cr-45 wait for the split.
- **Moved earlier:** CR-35 test targets into M0 (evidence depends on it); CR-25 contracts into M1 (backends fan out against merged contracts); the pgmq decision into M0 (blocking constraints use \`--all-features\`).
- **Folded:** CR-24 FIFO sequencing into \`cr-18-core-contracts\`; WorkerPool execution budgets (assessment finding) into \`cr-19\`; native WorkerPool shutdown loss into \`cr-29\`.
- **Decided:** keep WASM (\`cr-38\`, core becomes wasm-safe through the split) rather than drop it; pgmq kept via SQL rather than removed.
- **Scope cut / deferred:** \`cr-33-stream-replay\` is stretch (deferredOptional). Multi-node coordination stays out of scope.

- **Plan review round 2 (BLOCK, 1 critical, 15 warnings, 2 suggestions) applied:** team ownership rewritten for the workspace (contracts module → tech-lead; dispatch/semaphore and runtime-tokio → scheduler-engineer; hooks/mailbox composition, embedded, iggy → events-engineer; wasm/ffi → platform-engineer; per-crate manifests, Cargo.lock and toolchain → tech-lead) and reinstalled for all six harnesses; negative controls keyed per scenario (\`--cfg parking_lot_negative_control="s-N"\`) and declared to check-cfg in cr-41; cr-49 now depends on the durable mailbox and composition; boundary assertions can fail (mirror-clone path history, cargo-tree greps, \`--no-tests=fail\`) and cover S-1..S-23 including loom_vector; the pre-purge gate rejects any pre-purge commit with full-depth history (cr-51); cr-15 is driver/operator-only in a mirror clone; cr-47 waits for Iggy and multi-resource; cr-34 owns StorageMailbox behaviour in core; the pgmq crash-durability claim narrowed; cr-16 waits for the new API; cr-53 keeps the WASM pool at parity; pre-baseline waiver recorded; cr-43 after cr-42.
- **Operator addition (2026-10-22): release.** \`cr-52-release\` versions every crate at 0.3.0 (first crates.io publication — no prior version exists), generates the changelog (release-plz + git-cliff), publishes in dependency order through crates.io trusted publishing after the boundary passes and the operator approves, creates a signed \`v0.3.0\` tag and GitHub release, and deploys the docs. Published names are \`prometheus-parking-lot-*\` because \`parking-lot-core\` is the parking_lot project's crate.

## Trade-offs and risks

- **Size:** ${CHANGES.length} changes / ${totalTasks} tasks is several weeks of agent work; multi-agent runs cost ~15× single-session tokens (docs/agent-team.md). Rounds keep one change per role at a time.
- **Workspace split (\`cr-48\`) is the riskiest single move:** it touches every file once. It is scheduled after the baseline and before any fan-out so no branch rebases across it.
- **History purge (\`cr-15\`)** force-pushes public \`main\` after this plan branch merges, from a fresh mirror clone with no worktrees; every later branch is created from post-purge main and CI rejects any branch containing the old main SHA. GitHub may still serve old commits through \`refs/pull/*\` or forks, so key rotation — not the purge — is the containment guarantee.
- **Review independence is weaker than required:** no cross-family judge is configured; reviews run as same-family subagents and say so.

CHANGE LIST (ordered)
${changeList}

TASK MODEL ASSIGNMENTS

Key = phase path + change ID + backend task ID (the OpenSpec ordinal reported by \`kbd-apply list <change>\`). Discovery 2026-10-22: harness Claude Code (this session) lists subagent types tech-lead, scheduler-engineer, postgres-engineer, events-engineer, platform-engineer, product-manager and reviewer; their models come from \`.claude/agents/*.md\` frontmatter (\`opus\` → anthropic/claude-opus-5-5, \`sonnet\` → anthropic/claude-sonnet-5-5). No per-subagent reasoning-effort control is exposed. liter-llm has no configured providers (preflight \`degraded\`), so no liter-llm route is available.

| Phase path | Change ID | Backend task ID | Requirements | Provider/model | Reasoning effort | Rationale and dated evidence | Harness and route | Worker launch and handoff | Native alternative | Availability and verification | Prerequisites |
|---|---|---|---|---|---|---|---|---|---|---|---|
${rows.join('\n')}

EXECUTION ROUND ORDER
${rounds}

Rounds are computed from dependencies and serialized per owner role (one change per role at a time). A change starts only after every dependency is merged to main by the operator.

COMMANDS TO RUN
OpenSpec changes were generated directly into \`openspec/changes/<id>/\` (proposal.md, specs/<capability>/spec.md, tasks.md) — equivalent to \`/opsx:new <id>\` + artifact authoring:
${order.map((c) => `/opsx:new ${c.id}`).join('\n')}

## Contract binding

- Contract: \`${contractPath}\`
- Digest (sha256, normalization \`jq -S -c 'del(.identity.contractDigest) | del(.identity.planRevision)'\`): \`${digest}\`
- Plan revision at adoption: ${planRevision}
- Scenario → task coverage: see \`scenarioMatrix[].ownerTask\` in the contract (every scenario names its owning change/task).
- Worker routes, budgets, freshness keys and the single boundary packet are defined in the contract; the boundary packet runs once, after all production changes:

\`\`\`text
${BOUNDARY.commands.join('\n')}
\`\`\`

## Unresolved review findings

The two-round review cap is reached (round 1 BLOCK 3/14/1, round 2 BLOCK 1/15/2; all findings applied). Remaining, carried to execute:

- **Review independence:** every review, including both plan rounds, ran as a same-family subagent (\`same-model-collision\`). The packet builder is blocked by the modern-python \`python3\` shim and no cross-family judge is configured in liter-llm. Configure one before the first merge gate or record each review as same-family.
- **Round-2 fixes were not re-reviewed.** The first execute-stage review gate should spot-check ownership in team.json, the keyed negative controls and the cr-15/cr-51 split.
- **Native team definitions are source-verified only** in all six harnesses; no role has been invoked.

PLAN COMPLETE
`);

// tasks.json: scenario-coverage index for kbd-phase-readiness-check (ids = <change>#<ordinal>).
writeFileSync(join(phaseDir, 'tasks.json'), JSON.stringify({
  phase: PHASE,
  generatedBy: 'plan-generate.mjs',
  tasks: order.flatMap((c) => c.tasks.map((tk, i) => ({ id: `${c.id}#${i + 1}`, change: c.id, ordinal: i + 1, title: tk.title, role: tk.role }))),
}, null, 2) + '\n');

writeFileSync(join(phaseDir, 'plan-index.json'), JSON.stringify(order.map((c, i) => ({ seq: i + 1, id: c.id, title: c.title, round: finalRound.get(c.id), owner: c.owner, tasks: c.tasks.map((t) => t.title) })), null, 2));
console.log(JSON.stringify({ changes: CHANGES.length, tasks: totalTasks, rounds: Math.max(...finalRound.values()) }));
