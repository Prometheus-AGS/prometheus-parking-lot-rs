// One-shot revision applying plan review round 2 + the operator's release requirement
// to plan-data.mjs and contract-generate.mjs. Kept for provenance; idempotence is not required.
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';

const dir = process.argv[2];
const edit = (file, fn) => {
  const p = join(dir, file);
  writeFileSync(p, fn(readFileSync(p, 'utf8')));
};
const swap = (s, from, to) => {
  if (!s.includes(from)) throw new Error(`anchor missing: ${from.slice(0, 80)}`);
  return s.replace(from, to);
};

// ── package names: crates.io treats - and _ alike; parking-lot-core is Amanieu's parking_lot_core ──
const renamePkgs = (s) => s
  .replaceAll('-p parking-lot-', '-p prometheus-parking-lot-')
  .replaceAll('cargo tree -p parking-lot-core', 'cargo tree -p prometheus-parking-lot-core');

edit('plan-data.mjs', (s0) => {
  let s = renamePkgs(s0);

  // cr-15: operator/driver-only containment; CI gates move to cr-51.
  s = swap(s, `      t('After the plan branch merges and with explicit operator approval: in a fresh mirror clone with no worktrees, purge .codex/{auth.json,config.toml,history.jsonl,version.json,sessions/} with git filter-repo, force-push, record old/new main SHAs, and have every collaborator re-clone', 'platform-engineer', 'destructive git operation; approval string recorded in execution.md'),`,
    `      t('After the plan branch merges and with explicit operator approval: the KBD driver (no team worktree) purges .codex/{auth.json,config.toml,history.jsonl,version.json,sessions/} with git filter-repo in a fresh mirror clone, force-pushes, records old/new main SHAs and the pre-purge root commit set, and the operator re-clones every checkout', 'operator', 'destructive git operation run by the driver in a mirror clone; approval string recorded in execution.md'),`);
  s = swap(s, `      t('Hand tech-lead the Cargo.toml include allowlist diff and confirm it is applied', 'platform-engineer', 'cross-owner handoff; small edit'),
      t('Add CI gates: gitleaks (full history), the path-based history check, the pre-purge-ancestor rejection, and the cargo package --list allowlist check', 'platform-engineer', 'CI YAML; deterministic checks'),
      review('secret exposure, purge completeness, allowlist'),`,
    `      t('Verify on a fresh mirror clone that the path-history query is empty and record the output', 'operator', 'driver-run verification; no worktree'),`);
  s = swap(s, `id: 'cr-15-containment', milestone: 'M0', owner: 'platform-engineer'`, `id: 'cr-15-containment', milestone: 'M0', owner: 'operator'`);
  s = swap(s, `        sc('Pre-purge history rejected', 'a branch whose history contains the recorded pre-purge main SHA opens a pull request', 'the CI gate \`git merge-base --is-ancestor <old-main-sha> HEAD\` succeeds and the job fails'),
        sc('Package allowlist', '\`cargo package --list\` runs in CI for each published crate', 'every listed path matches that crate include allowlist and the job fails on any other path'),`, '');

  // cr-41: check-cfg declarations; baseline waiver note.
  s = swap(s, `      t('Make benches/queue_bench.rs and both examples compile (idempotency_key, missing main), remove #![deny(warnings)], fix repository/homepage URLs', 'tech-lead', 'small edits across targets'),`,
    `      t('Make benches/queue_bench.rs and both examples compile (idempotency_key, missing main), remove #![deny(warnings)], fix repository/homepage URLs; declare check-cfg for parking_lot_loom and keyed parking_lot_negative_control values under [lints.rust] unexpected_cfgs', 'tech-lead', 'small edits across targets + lint config'),`);

  // cr-48: package names.
  s = swap(s, `the repository root stays the published facade prometheus_parking_lot, re-exporting behind features.`,
    `the repository root stays the published facade prometheus_parking_lot, re-exporting behind features. Directory names stay short (crates/parking-lot-*) but every published package is named prometheus-parking-lot-* because crates.io treats - and _ alike and parking-lot-core/parking_lot_core belongs to the parking_lot project (checked 2026-10-22).`);

  // cr-43 after cr-42 (testcontainers version settled first).
  s = swap(s, `id: 'cr-43-postgres18-image', milestone: 'M0', owner: 'postgres-engineer', deps: ['cr-48-workspace-split']`,
    `id: 'cr-43-postgres18-image', milestone: 'M0', owner: 'postgres-engineer', deps: ['cr-48-workspace-split', 'cr-42-ci-rebuild']`);

  // cr-16 after the API-breaking contracts.
  s = swap(s, `id: 'cr-16-docs-truth', milestone: 'M0', owner: 'product-manager', deps: ['cr-41-toolchain-baseline']`,
    `id: 'cr-16-docs-truth', milestone: 'M0', owner: 'product-manager', deps: ['cr-48-workspace-split', 'cr-18-core-contracts']`);

  // cr-19: budget tests in task 3.
  s = swap(s, `proptest suites for the tokio pools; miri on atomics paths', 'scheduler-engineer', 'model checking'),`,
    `proptest suites for the tokio pools; budget_ tests (wedged executor, cancel witness) for ResourcePool and the native WorkerPool; miri on atomics paths', 'scheduler-engineer', 'model checking + budget tests'),`);

  // cr-25: StorageMailbox type only; composition behaviour belongs to cr-34.
  s = swap(s, `      t('In-memory reference implementations and StorageMailbox<S, N> composition', 'tech-lead', 'implementation'),`,
    `      t('In-memory reference implementations; StorageMailbox<S, N> type signature only (behaviour in cr-34)', 'tech-lead', 'implementation'),`);
  s = swap(s, `      t('StorageMailbox<S, N> with off-path bounded notification', 'events-engineer', 'async composition'),
      t('Failure-injection tests against PG and in-memory storage', 'events-engineer', 'tests'),`,
    `      t('StorageMailbox<S, N> behaviour in crates/parking-lot-core/src/mailbox/ with off-path bounded notification', 'events-engineer', 'async composition'),
      t('Failure-injection tests in prometheus-parking-lot-core with in-memory storage and a failing notifier', 'events-engineer', 'tests'),`);

  // cr-49: depends on durable mailbox and composition.
  s = swap(s, `id: 'cr-49-durable-dispatch', milestone: 'M2', owner: 'scheduler-engineer', deps: ['cr-21-panic-containment', 'cr-26-pg-lease-queue', 'cr-28-embedded-redb-queue']`,
    `id: 'cr-49-durable-dispatch', milestone: 'M2', owner: 'scheduler-engineer', deps: ['cr-21-panic-containment', 'cr-26-pg-lease-queue', 'cr-27-pg-mailbox', 'cr-28-embedded-redb-queue', 'cr-34-mailbox-composition']`);

  // cr-47: wait for the APIs its guides show; streaming guide conditional.
  s = swap(s, `deps: ['cr-46-docs-theory', 'cr-37-config-schema', 'cr-29-shutdown-cancellation', 'cr-44-mobile-desktop-bindings']`,
    `deps: ['cr-46-docs-theory', 'cr-37-config-schema', 'cr-29-shutdown-cancellation', 'cr-44-mobile-desktop-bindings', 'cr-32-iggy-events', 'cr-23-multi-resource']`);
  s = swap(s, `streaming replay, shutdown/requeue, Iggy events, configuration;`,
    `streaming replay (only if stretch cr-33 merged), shutdown/requeue, Iggy events, configuration, release/upgrade notes;`);

  // New changes: cr-51 supply-chain gates, cr-53 wasm parity, cr-52 release (last).
  const gates = `  {
    id: 'cr-51-supply-chain-gates', milestone: 'M0', owner: 'platform-engineer', deps: ['cr-15-containment', 'cr-42-ci-rebuild', 'cr-48-workspace-split'], size: 'S', complexity: 'Low', cls: 'medium', value: 'HIGH',
    title: 'CI gates: secrets, purged paths, pre-purge history and package allowlists',
    why: 'Containment (cr-15) must stay contained: a branch carrying any pre-purge commit would reintroduce the credential history on merge, and packaging could again ship agent tooling.',
    details: 'Created from post-purge main only. CI with fetch-depth 0 runs gitleaks over full history, the path-history query (must be empty), a rejection of any branch containing a commit from the recorded pre-purge root set, and cargo package --list per published crate against its include allowlist.',
    capability: 'supply-chain-safety',
    requirements: [
      req('Contained history and packages', 'CI SHALL fail any pull request whose history contains a pre-purge commit or a credential path, or whose packages include a path outside the allowlist.', [
        sc('Pre-purge branch rejected', 'a scratch branch created from pre-purge commit 88f805c opens a pull request', 'the pre-purge gate fails the job'),
        sc('Path history', 'the path-history query runs on the PR head with full history', 'it is empty, and a scratch branch re-adding .codex/auth.json fails the job'),
        sc('Package allowlist', 'cargo package --list runs for each published crate', 'only allowlisted paths are listed'),
      ]),
    ],
    tasks: [
      t('Add CI jobs (fetch-depth 0): gitleaks full history, path-history emptiness assertion, pre-purge root-set ancestry rejection, per-crate cargo package --list allowlist check', 'platform-engineer', 'CI YAML; deterministic checks'),
      t('Prove each gate with a negative-control scratch branch and record the failing run URLs', 'platform-engineer', 'CI evidence'),
      review('gate completeness, shallow-clone pitfalls'),
    ],
  },
`;
  s = swap(s, `  {
    id: 'cr-48-workspace-split',`, gates + `  {
    id: 'cr-48-workspace-split',`);

  const parity = `  {
    id: 'cr-53-wasm-parity', milestone: 'M4', owner: 'platform-engineer', deps: ['cr-38-wasm-target', 'cr-23-multi-resource', 'cr-31-hook-emission'], size: 'S', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'WASM pool parity with later dispatcher changes',
    why: 'cr-20/23/31 change the shared dispatcher after cr-38 ports the WASM pool; without a follow-up the WASM pool silently misses vector admission and lifecycle events (plan review round 2).',
    details: 'Bring the WASM pool to parity: vector admission, skip-ahead, lifecycle events. Durable dispatch does not apply on the web target (no durable backend in the browser); this is documented.',
    capability: 'platform-targets',
    requirements: [
      req('WASM parity', 'The WASM pool SHALL enforce vector admission and emit the same lifecycle events as the native pools.', [
        sc('Parity tests', 'wasm-bindgen-test runs the shared admission and event-ordering cases', 'all pass'),
      ]),
    ],
    tasks: [
      t('Adapt the WASM pool to vector admission, skip-ahead and lifecycle events', 'platform-engineer', 'wasm'),
      t('Shared admission/event cases under wasm-bindgen-test', 'platform-engineer', 'tests'),
      review('parity with native pools'),
    ],
  },
`;
  const release = `  {
    id: 'cr-52-release', milestone: 'M5', owner: 'platform-engineer', deps: ['cr-39-dependency-hygiene', 'cr-40-benchmarks', 'cr-47-docs-guides', 'cr-53-wasm-parity', 'cr-51-supply-chain-gates', 'cr-36-pg-matrix', 'cr-30-pgmq-sql-backend', 'cr-17-kbd-correction', 'cr-16-docs-truth'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Version, publish and tag 0.3.0 (crates.io trusted publishing, signed tag, GitHub release, docs deploy)',
    why: 'The operator requires the library versioned, its crates published and the version tagged. No version of prometheus_parking_lot has ever been published (crates.io checked 2026-10-22); 0.3.0 is the first release and is a breaking change from the 0.2.0 sources.',
    details: 'Versioning ADR (SemVer; workspace.package version 0.3.0 for every crate; inter-crate dependencies pinned to the same minor; MSRV policy). release-plz + git-cliff generate the changelog and release PR. Publishing runs in CI only after the boundary packet passes, using crates.io trusted publishing (OIDC, no long-lived token) configured by the operator per crate, in dependency order: core, runtime-tokio, pg, embedded, iggy, wasm, ffi, facade. Signed annotated tag v0.3.0, GitHub release with notes, docs site deployed for the tag. The long-lived CARGO_REGISTRY_TOKEN in the operator shell is retired.',
    capability: 'release-management',
    requirements: [
      req('Reproducible, approved release', 'Release 0.3.0 SHALL be published only after the boundary packet passes and the operator approves, every crate SHALL be installable from crates.io at 0.3.0, and the repository SHALL carry a signed v0.3.0 tag pointing at the released commit.', [
        sc('Install from crates.io', 'a fresh project runs \\\`cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded\\\` and builds', 'it resolves only 0.3.0 workspace crates from crates.io and compiles'),
        sc('Tag and release', 'the release workflow completes', '\\\`git tag -v v0.3.0\\\` verifies and the GitHub release lists the changelog for 0.3.0'),
        sc('Gate enforced', 'the publish workflow is started without a recorded boundary PASS receipt', 'it refuses to publish'),
      ]),
    ],
    tasks: [
      t('Versioning ADR (SemVer, workspace version 0.3.0, inter-crate pins, MSRV policy); release-plz.toml and cliff.toml', 'platform-engineer', 'release engineering'),
      t('Operator configures crates.io trusted publishing for each prometheus-parking-lot* crate and retires CARGO_REGISTRY_TOKEN from the shell profile', 'operator', 'account-bound; crates.io and GitHub settings'),
      t('Release workflow: requires boundary receipt + operator approval environment, publishes in dependency order via OIDC, creates signed tag v0.3.0 and GitHub release, deploys docs', 'platform-engineer', 'CI YAML; outward-facing'),
      t('Operator approves and triggers the 0.3.0 release; record crate versions, tag SHA and release URL', 'operator', 'irreversible publication'),
      t('Post-release verification: fresh-project install for each feature set; docs site reachable at the tag', 'platform-engineer', 'verification'),
      review('release gating, publish order, tag integrity'),
    ],
  },
`;
  s = swap(s, `];

// Phase-level boundary integration gate`, parity + release + `];

// Phase-level boundary integration gate`);

  // Boundary commands: assertions, keyed loom, features.
  s = swap(s, `    'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 cargo test -p prometheus-parking-lot-core --test loom_capacity --test loom_wakeup --locked',`,
    `    'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 cargo test -p prometheus-parking-lot-core --test loom_capacity --test loom_wakeup --test loom_vector --locked',`);
  s = swap(s, `    'git log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json',
    'cargo tree -p prometheus-parking-lot-core --target wasm32-unknown-unknown -e normal',`,
    `    'rm -rf "$TMPDIR/pl-mirror" && git clone --mirror https://github.com/Prometheus-AGS/prometheus-parking-lot-rs.git "$TMPDIR/pl-mirror" && test -z "$(git -C "$TMPDIR/pl-mirror" log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json)"',
    '! cargo tree -p prometheus-parking-lot-core -e normal | grep -E " (tokio|sqlx|sea-orm|iggy|redb|reqwest) v"',
    '! cargo tree -p prometheus-parking-lot-core --target wasm32-unknown-unknown -e normal | grep -E " tokio v"',`);
  s = swap(s, `    'cargo nextest run --workspace --all-features --locked',`, `    'cargo nextest run --workspace --all-features --locked --no-tests=fail',`);
  return s;
});

edit('contract-generate.mjs', (s0) => {
  let s = renamePkgs(s0);
  s = swap(s, `import { PHASE, BOUNDARY } from './plan-data.mjs';`, `import { PHASE, BOUNDARY, CHANGES } from './plan-data.mjs';`);
  // Keyed negative controls: one cfg value per scenario.
  s = swap(s, `  ({ id, name, ownerTask, productionEntry, realCollaborators, oracle, negativeControl, fixtures, evidence, ...(classification ? { classification } : {}) });`,
    `  ({ id, name, ownerTask, productionEntry, realCollaborators, oracle, negativeControl: negativeControl.replaceAll('--cfg parking_lot_negative_control', \`--cfg parking_lot_negative_control="\${id.toLowerCase()}"\`), fixtures, evidence, ...(classification ? { classification } : {}) });`);
  s = swap(s, `'Token/cost budget for a 33-change multi-agent phase'`, '`Token/cost budget for a ${CHANGES.length}-change multi-agent phase`');
  s = swap(s, `(Postgres, pgmq or embedded store)`, `(Postgres lease queue or embedded store; the pgmq backend provides lease redelivery per S-18 but pool-level crash durability on pgmq is not claimed)`);
  s = swap(s, `S('S-4', 'Bounded execution in every pool', 'cr-19-dispatcher-semaphore#2+cr-38-wasm-target#1',`, `S('S-4', 'Bounded execution in every pool', 'cr-19-dispatcher-semaphore#3+cr-38-wasm-target#2',`);
  s = swap(s, `S('S-9', 'No loss on shutdown; requeue across restart', 'cr-29-shutdown-cancellation#3', 'cargo nextest run --workspace --locked -E "test(shutdown_)"'`, `S('S-9', 'No loss on shutdown; requeue across restart', 'cr-29-shutdown-cancellation#3', 'cargo nextest run --workspace --all-features --locked --no-tests=fail -E "test(shutdown_)"'`);
  s = swap(s, `'cargo nextest run -p prometheus-parking-lot-pg --locked -E "test(notifier_failure)"', ['StorageMailbox', 'Postgres storage', 'failing notifier']`, `'cargo nextest run -p prometheus-parking-lot-core --locked --no-tests=fail -E "test(notifier_failure)"', ['StorageMailbox', 'in-memory storage', 'failing notifier']`);
  s = swap(s, `'Fresh container', ev),
    S('S-11'`, `'In-memory', ev),
    S('S-11'`);
  s = swap(s, `'cargo nextest run -p prometheus_parking_lot --locked -E "test(pool_sigkill_)"'`, `'cargo nextest run -p prometheus_parking_lot --features postgres,embedded --locked --no-tests=fail -E "test(pool_sigkill_)"'`);
  s = swap(s, `'cargo nextest run -p prometheus_parking_lot --locked -E "test(config_)"'`, `'cargo nextest run -p prometheus_parking_lot --all-features --locked --no-tests=fail -E "test(config_)"'`);
  s = swap(s, `'cr-15-containment#2+cr-15-containment#5'`, `'cr-15-containment#2+cr-51-supply-chain-gates#1'`);
  s = swap(s, `'A scratch branch based on the pre-purge main SHA fails the ancestor gate; a scratch branch adding a fake key file fails gitleaks'`, `'A scratch branch created from pre-purge commit 88f805c fails the pre-purge gate; a scratch branch adding a fake key file fails gitleaks'`);
  s = swap(s, `    S('S-23', 'Core crate dependency boundary',`, `    S('S-24', 'Release 0.3.0 installable, tagged and gated', 'cr-52-release#5', 'cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded in a fresh project && cargo build; git tag -v v0.3.0', ['crates.io', 'GitHub release', 'release workflow'], 'All crates resolve at 0.3.0 from crates.io and the project builds; the tag verifies; the GitHub release carries the 0.3.0 changelog', 'Starting the publish workflow without a boundary PASS receipt refuses to publish', 'Fresh project directory; Cargo.lock digest', 'cargo output, tag verification, release URL, workflow run URL', 'delivery'),
    S('S-23', 'Core crate dependency boundary',`);
  s = swap(s, `'Scenario S-1..S-15 evidence recorded under`, `'Scenario S-1..S-23 evidence recorded (S-24 is recorded by cr-52 after release) under`);
  s = swap(s, `'Docker unavailable: BLOCKED for S-5..S-11'`, `'Docker unavailable: BLOCKED for S-5..S-11, S-16, S-18 and S-22'`);
  s = swap(s, `exclusions: ['Multi-node scheduler coordination (one dispatcher per partition) — later phase', 'Exactly-once execution (at-least-once + idempotent executors per decision R7)', 'Flint-specific Postgres extensions from the flint-forge image', 'Publishing 0.3.0 to crates.io (separate operator action after the phase)'],`,
    `exclusions: ['Multi-node scheduler coordination (one dispatcher per partition) — later phase', 'Exactly-once execution (at-least-once + idempotent executors per decision R7)', 'Flint-specific Postgres extensions from the flint-forge image', 'Pool-level crash durability on the pgmq backend (lease redelivery only)', 'Durable dispatch on the web target (no durable backend in the browser)'],`);
  s = swap(s, `    'Branded Docusaurus site with cited theory/history chapters and compiled scenario guides',`, `    'Branded Docusaurus site with cited theory/history chapters and compiled scenario guides',
    'Versioned 0.3.0 release: changelog, crates.io publication of every workspace crate in dependency order, signed v0.3.0 tag, GitHub release, docs deploy',`);
  s = swap(s, `    'Docs sanitizer rejects private paths, secrets and raw wiki content',`, `    'Docs sanitizer rejects private paths, secrets and raw wiki content',
    'crates.io publication only after boundary PASS and explicit operator approval, via trusted publishing (no long-lived token in CI)',`);
  s = swap(s, `requiredApprovals: 'Operator approval for the run; Docker available for Postgres/Iggy containers; no publish, tag or push is part of the packet',`, `requiredApprovals: 'Operator approval for the run; Docker available for Postgres/Iggy containers; no publish, tag or push is part of the packet (publication is cr-52, after this packet passes)',`);
  return s;
});
console.log('revised');
