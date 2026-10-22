// Writes adopted-contract.json from plan-data.mjs. Adoption (digest + decision) is done
// by kbd-contract-adopt.sh, never by this script.
import { writeFileSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { PHASE, BOUNDARY, CHANGES } from './plan-data.mjs';

const root = process.argv[2];
const project = JSON.parse(readFileSync(join(root, '.prometheus/project.json'), 'utf8'));
const runId = process.argv[3];

const S = (id, name, ownerTask, productionEntry, realCollaborators, oracle, negativeControl, fixtures, evidence, classification) =>
  ({ id, name, ownerTask, productionEntry, realCollaborators, oracle, negativeControl: negativeControl.replaceAll('--cfg parking_lot_negative_control', `--cfg parking_lot_negative_control="${id.toLowerCase()}"`), fixtures, evidence, ...(classification ? { classification } : {}) });

const ev = 'argv, exit code, test names and pass/fail counts from stdout, git SHA, toolchain version';

const contract = {
  contractVersion: '1',
  kind: 'kbd.adopted-contract',
  identity: { projectId: project.projectId, runId, phaseId: PHASE, planRevision: null, contractDigest: null },
  digestNormalization: {
    algorithm: 'sha256',
    command: "jq -S -c 'del(.identity.contractDigest) | del(.identity.planRevision)'",
    rationale: 'contractDigest is excluded to avoid self-reference; planRevision is bound by the canonical decision.',
    stalenessRule: 'REJECT-STALE when the digest differs from the latest non-superseded adoption decision for this contract path.',
  },
  adoptionSource: 'Operator instructions 2026-10-22: adopt docs/update-plan-10-22-2026.md as the plan ("that is the plan for what we do next"); "run /kbd-assess"; decisions "1. Research what is correct and do it 2. Update the version of rust to the latest advisable version and pull it forward ... flint-forge ... postgres 18 3. DO what is most correct and do not be afraid to change things"; "Include in the plan a branded docusaurus site ... know-me ..."',
  decisionOwner: 'Operator (Travis James); merges are operator-only',
  amendmentPolicy: 'Material scope or acceptance changes require explicit operator adoption recorded as a new canonical decision with --supersedes; review suggestions never auto-enter task totals.',
  releaseClaim: {
    userOperation: 'A service, desktop app or mobile app submits weighted AI tasks to a pool; the pool never runs more than its capacity in any resource dimension, never silently loses an accepted task across executor panic, shutdown or process crash (Postgres lease queue or embedded store; the pgmq backend provides lease redelivery per S-18 but pool-level crash durability on pgmq is not claimed), and the caller retrieves the result later from the mailbox even after disconnecting. Documented on a branded site with cited theory and compiled examples.',
    supportedEnvironments: ['macOS aarch64 and Linux x86_64 (full tests)', 'Rust 1.99.1 (pinned) and MSRV 1.95', 'PostgreSQL 17 and 18 (project images, pgmq >= 1.8)', 'Tauri desktop via embedded redb store', 'wasm32-unknown-unknown (check + browser smoke)', 'Android arm64 and iOS device/simulator (check builds)'],
    currentCapabilities: 'As of main@90a6194 (assessment.md): capacity enforced only in ResourcePool; no panic containment; Postgres backends non-functional; JSONL embedded queue not crash-safe; hooks unwired; CI fully red; no docs site.',
    explicitUnknowns: ['Throughput/latency figures (benchmarks do not compile today)', 'pgmq extension build on PG18 inside the project image (unproven until cr-43)', 'UniFFI async support level for the chosen API surface', 'Iggy server version to pin for CI', `Token/cost budget for a ${CHANGES.length}-change multi-agent phase`],
  },
  statusVocabulary: ['PASS', 'FAIL', 'BLOCKED', 'NOT_EXERCISED'],
  // Negative controls use the test-only `--cfg parking_lot_negative_control` (never a Cargo feature,
  // never enabled by --all-features) unless stated otherwise.
  scenarioMatrix: [
    S('S-1', 'Capacity invariant (dispatcher + semaphore model)', 'cr-19-dispatcher-semaphore#3', 'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 cargo test -p prometheus-parking-lot-core --test loom_capacity --locked', ['dispatcher', 'unit-weighted semaphore (parking-lot-core, no tokio)'], 'No explored schedule has sum(active cost) > max_units and no accepted task is lost', 'With --cfg parking_lot_negative_control (semaphore acquire skipped) the model reports a violation', 'Pure in-memory; loom with bounded preemptions', ev),
    S('S-2', 'No lost wakeup under submit/complete races', 'cr-19-dispatcher-semaphore#3', 'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 cargo test -p prometheus-parking-lot-core --test loom_wakeup --locked', ['dispatcher', 'priority queue'], 'Every accepted task is dispatched; no task stays parked while sufficient capacity is free', 'With --cfg parking_lot_negative_control (depth check and enqueue under separate locks, as on main@90a6194) the model finds the stranded-task schedule', 'In-memory; loom', ev),
    S('S-3', 'Executor panic containment (native pools)', 'cr-21-panic-containment#2', 'cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(panic_)"', ['ResourcePool', 'WorkerPool native'], '1 panicking task of 100: 99 complete, capacity returns to 0, stats.failed == 1, panicked status in mailbox', 'With --cfg parking_lot_negative_control (catch_unwind skipped) capacity leaks and the test fails', 'In-memory', ev),
    S('S-4', 'Bounded execution in every pool', 'cr-19-dispatcher-semaphore#3+cr-38-wasm-target#2', 'cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(budget_)" && wasm-pack test --headless --chrome crates/parking-lot-wasm', ['ResourcePool', 'WorkerPool native', 'WorkerPool wasm'], 'A never-returning executor is cancelled at its budget; units released; timeout status delivered', 'A cancel witness (Drop fires only on cancellation) does not fire for in-budget tasks; with --cfg parking_lot_negative_control (budget not applied) the wedged-executor test fails', 'In-memory; TestClock', ev),
    S('S-5', 'Postgres lease redelivery', 'cr-26-pg-lease-queue#3', 'cargo nextest run -p prometheus-parking-lot-pg --locked -E "test(sigkill_redelivery)"', ['Postgres 18 project image', 'sqlx 0.9.0', 'child process consumer'], 'A consumer killed with SIGKILL mid-task: task redelivered after lease expiry and completed exactly once', 'With --cfg parking_lot_negative_control (lease never expires) no redelivery occurs and the test fails', 'Fresh container per test from tag parking-lot-pg:18 (image digest recorded)', ev),
    S('S-6', 'No double claim with concurrent dispatchers', 'cr-26-pg-lease-queue#3', 'cargo nextest run -p prometheus-parking-lot-pg --locked -E "test(concurrent_dispatchers)"', ['Postgres 18 project image'], 'With 8 dispatchers and 10k tasks, no task is leased by two dispatchers simultaneously', 'With --cfg parking_lot_negative_control (SKIP LOCKED/row lock omitted) duplicates are detected', 'Fresh container; image digest recorded', ev),
    S('S-7', 'Consume-once mailbox with first-write-wins', 'cr-27-pg-mailbox#2', 'cargo nextest run -p prometheus-parking-lot-pg --locked -E "test(mailbox_)"', ['Postgres 18 project image'], 'Two consume transactions coordinated by a barrier: exactly one receives the result; duplicate deliveries keep the first write; expired results are evicted', 'With --cfg parking_lot_negative_control (SELECT then DELETE in separate statements) the barrier-coordinated test observes double delivery deterministically', 'Fresh container', ev),
    S('S-8', 'Embedded store crash safety', 'cr-28-embedded-redb-queue#3', 'cargo nextest run -p prometheus-parking-lot-embedded --locked -E "test(crash_point_)"', ['redb 4.x on a temp dir', 'child process aborted at instrumented points'], 'After abort at every crash point and restart, each accepted task is present exactly once or redelivered per lease rules', 'With --cfg parking_lot_negative_control (enqueue commit skipped) a task is lost and the test fails', 'Per-test tempdir; file digests recorded', ev),
    S('S-9', 'No loss on shutdown; requeue across restart', 'cr-29-shutdown-cancellation#3', 'cargo nextest run --workspace --all-features --locked --no-tests=fail -E "test(shutdown_)"', ['ResourcePool', 'WorkerPool native', 'Postgres 18 image', 'redb'], 'Every accepted task is completed, captured by the cancellation sink, or requeued; requeued tasks run after restart', 'With --cfg parking_lot_negative_control (received task dropped on the shutdown flag) the loss is detected', 'Fresh container + tempdir', ev),
    S('S-10', 'Notifier failure never loses a stored result', 'cr-34-mailbox-composition#2', 'cargo nextest run -p prometheus-parking-lot-core --locked --no-tests=fail -E "test(notifier_failure)"', ['StorageMailbox', 'in-memory storage', 'failing notifier'], 'With a notifier failing every call, every result is stored and consumable', 'With --cfg parking_lot_negative_control (notify-before-store, aborting on notifier error) results are lost and the test fails', 'In-memory', ev),
    S('S-11', 'Iggy events: deterministic IDs and non-blocking publish', 'cr-32-iggy-events#2', 'cargo nextest run -p prometheus-parking-lot-iggy --locked -E "test(iggy_)"', ['pinned Iggy server container'], 'A subscriber receives mailbox.ready per completed task and replays from an earlier offset; publishing the same idempotency key twice yields one message ID; with the server paused, dispatch latency stays within the configured bound', 'With --cfg parking_lot_negative_control (random message IDs; synchronous publish) the ID and latency assertions fail', 'Pinned Iggy image digest recorded', ev),
    S('S-12', 'CI green on toolchain and Postgres matrix', 'cr-42-ci-rebuild#1+cr-36-pg-matrix#1', 'GitHub Actions ci.yml on main', ['GitHub Actions', 'PG 17 and 18 images'], 'All required jobs pass on 1.99.1 and 1.95 and on PG 17 and 18', 'A deliberately unformatted commit on a scratch branch fails the fmt job', 'CI run IDs recorded', 'run URLs and job conclusions', 'delivery'),
    S('S-13', 'Package allowlist, purged history, no secrets', 'cr-15-containment#2+cr-51-supply-chain-gates#1', 'git log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json; cargo package --workspace --locked; gitleaks detect --log-opts=--all', ['fresh mirror clone', 'cargo packaging'], 'The path history query is empty; each package lists only allowlisted paths; gitleaks reports no findings', 'A scratch branch created from pre-purge commit 88f805c fails the pre-purge gate; a scratch branch adding a fake key file fails gitleaks', 'Fresh mirror clone after purge', 'query output, package listings, gitleaks report digest', 'delivery'),
    S('S-14', 'Web, mobile and desktop reach', 'cr-38-wasm-target#2+cr-44-mobile-desktop-bindings#2', 'CI jobs wasm-check (incl. cargo tree core check), android-check, ios-check, tauri-example', ['wasm-bindgen-test headless browser', 'cargo-ndk', 'Xcode toolchain', 'Tauri'], 'wasm tests pass and the core wasm dependency graph contains no tokio; Android and iOS check builds succeed; Tauri example retrieves a result after restart', 'A scratch branch adding tokio to parking-lot-core fails the core graph check', 'CI runners; toolchain versions recorded', ev, 'delivery'),
    S('S-15', 'Branded documentation site with compiled examples', 'cr-45-docs-site#3+cr-47-docs-guides#3', 'npm --prefix website ci && npm --prefix website run build && npm --prefix website run verify', ['Docusaurus', 'headless browser', 'examples/ sources'], 'Build and link check pass; light/dark screenshots match know-me tokens; no serious a11y violations; every guide example compiles', 'A scratch page with a bordered card or a broken link fails verify', 'Frozen lockfile; screenshot digests', 'build log, link report, a11y report, screenshot paths', 'delivery'),
    S('S-16', 'Pool process killed mid-execution loses nothing (durable dispatch)', 'cr-49-durable-dispatch#3', 'cargo nextest run -p prometheus_parking_lot --features postgres,embedded --locked --no-tests=fail -E "test(pool_sigkill_)"', ['ResourcePool in a child process', 'Postgres 18 image', 'redb'], 'After SIGKILL of the pool process mid-execution (and between execution and mailbox write) and restart, the task completes and exactly one result is consumable, on PG and on redb', 'With --cfg parking_lot_negative_control (ack before mailbox write) the result is lost and the test fails', 'Fresh container + tempdir', ev),
    S('S-17', 'Async waiters use no blocking threads', 'cr-22-result-slots#2', 'cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(waiters_no_blocking_threads)"', ['ResourcePool', 'tokio runtime with max_blocking_threads(1)'], 'With the single blocking thread occupied, 10,000 retrieve_async waiters all resolve within the timeout; unretrieved results expire after TTL', 'With --cfg parking_lot_negative_control (spawn_blocking waiter) the test times out', 'In-memory', ev),
    S('S-18', 'pgmq backend lease and priority', 'cr-30-pgmq-sql-backend#2', 'cargo nextest run -p prometheus-parking-lot-pg --locked -E "test(pgmq_)"', ['Postgres 18 image with pgmq'], 'The queue conformance suite passes; higher-priority tasks are leased first; an unacked task reappears after its visibility timeout', 'With --cfg parking_lot_negative_control (single queue, no priority split) the priority-order case fails', 'Fresh container; pgmq version recorded', ev),
    S('S-19', 'Multi-resource admission', 'cr-23-multi-resource#2', 'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 cargo test -p prometheus-parking-lot-core --test loom_vector --locked', ['dispatcher', 'vector semaphore'], 'No schedule exceeds capacity in any dimension; a task short in one dimension waits', 'With --cfg parking_lot_negative_control (only the first dimension checked) a violation is reported', 'In-memory; loom', ev),
    S('S-20', 'Skip-ahead starvation bound', 'cr-20-skip-ahead#2', 'cargo nextest run -p prometheus-parking-lot-core --locked -E "test(starvation_)"', ['dispatcher'], 'A large head task starts within the configured number of dispatch cycles while small tasks keep arriving', 'With --cfg parking_lot_negative_control (guard disabled) the head starves past the bound', 'In-memory; deterministic arrival schedule', ev),
    S('S-21', 'Lifecycle event ordering', 'cr-31-hook-emission#2', 'cargo nextest run -p prometheus-parking-lot-runtime-tokio --locked -E "test(lifecycle_events_)"', ['ResourcePool', 'WorkerPool native', 'recording hook'], 'Every task emits submitted before started before exactly one terminal event, including cancelled and requeued paths', 'With --cfg parking_lot_negative_control (terminal event suppressed on cancellation) the ordering check fails', 'In-memory', ev),
    S('S-22', 'Config builds pools and rejects unknown fields', 'cr-37-config-schema#2', 'cargo nextest run -p prometheus_parking_lot --all-features --locked --no-tests=fail -E "test(config_)"', ['config loader', 'in-memory, Postgres and redb backends'], 'Every documented example config builds its pools; a misspelled field fails naming the field', 'Removing deny_unknown_fields makes the misspelling case fail', 'Example files digested', ev),
    S('S-24', 'Release 0.3.0 installable, tagged and gated', 'cr-52-release#5', 'cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded in a fresh project && cargo build; git tag -v v0.3.0', ['crates.io', 'GitHub release', 'release workflow'], 'All crates resolve at 0.3.0 from crates.io and the project builds; the tag verifies; the GitHub release carries the 0.3.0 changelog', 'Starting the publish workflow without a boundary PASS receipt refuses to publish', 'Fresh project directory; Cargo.lock digest', 'cargo output, tag verification, release URL, workflow run URL', 'delivery'),
    S('S-23', 'Core crate dependency boundary', 'cr-48-workspace-split#2', 'cargo tree -p prometheus-parking-lot-core -e normal', ['cargo metadata'], 'No tokio, sqlx, sea-orm, iggy, redb or reqwest in the core graph', 'A scratch branch adding sqlx to core fails the check', 'Cargo.lock digest', 'cargo tree output', 'delivery'),
  ],
  functionalScope: [
    'Unit-weighted (then multi-resource) admission enforced by one dispatcher shared by all pools',
    'Panic containment, bounded execution, async result slots with TTL',
    'Async lease/ack queue, async mailbox and notifier contracts with a conformance suite',
    'Postgres lease queue, Postgres mailbox, pgmq-over-SQL backend, embedded redb queue',
    'Shutdown policy with cancellation capture and durable requeue',
    'Lifecycle events and Iggy hook/notifier',
    'Typed configuration building pools and backends',
    'Workspace split with a wasm-safe core; wasm, mobile (UniFFI) and Tauri reach',
    'Branded Docusaurus site with cited theory/history chapters and compiled scenario guides',
    'Versioned 0.3.0 release: changelog, crates.io publication of every workspace crate in dependency order, signed v0.3.0 tag, GitHub release, docs deploy',
  ],
  resilienceScope: {
    policy: 'Adopted requirements never defer via budget overflow; exhaustion checkpoints and marks BLOCKED. Only never-adopted optional items may defer.',
    deferredOptional: ['cr-33-stream-replay (stretch)'],
  },
  exclusions: ['Multi-node scheduler coordination (one dispatcher per partition) — later phase', 'Exactly-once execution (at-least-once + idempotent executors per decision R7)', 'Flint-specific Postgres extensions from the flint-forge image', 'Pool-level crash durability on the pgmq backend (lease redelivery only)', 'Durable dispatch on the web target (no durable backend in the browser)'],
  requiredSafetyChecks: [
    'Exposed OpenAI key and Codex tokens rotated by the operator before history purge',
    'History purge only with recorded operator approval; no force-push without it',
    'No agent-tool credentials or runtime state tracked (constraints no-tracked-tool-credentials)',
    'cargo deny advisories clean; gitleaks clean across history',
    'No unsafe code outside generated FFI glue; crate-level deny(unsafe_code) kept in core',
    'Docs sanitizer rejects private paths, secrets and raw wiki content',
    'crates.io publication only after boundary PASS and explicit operator approval, via trusted publishing (no long-lived token in CI)',
  ],
  budgets: {
    timeMinutes: 14400,
    toolCalls: 12000,
    repairAttempts: 3,
    noProgressLimit: 2,
    tokenBudget: 'UNKNOWN',
    tokenFallback: 'Checkpoint at each milestone; stop and report BLOCKED after two consecutive rounds with no merged change.',
    exhaustion: 'Checkpoint, record BLOCKED with the exhausted dimension, never report PASS; docs and state may be saved without running product tests.',
  },
  evidenceInvalidation: 'Evidence is keyed by (scenario id, git SHA of the owning crate, toolchain version, image digest or fixture digest). A change invalidates only evidence whose key it alters; history is preserved and there is no blanket re-review.',
  workerRoutes: {
    agent: 'Claude Code subagents from team parking-lot-core (.claude/agents/<role>.md) dispatched via the Agent tool, one git worktree per role',
    model: 'anthropic/claude-opus-5-5 for tech-lead, scheduler-engineer, product-manager, reviewer; anthropic/claude-sonnet-5-5 for postgres-engineer, events-engineer, platform-engineer',
    billing: 'Operator Claude subscription via this Claude Code session; per-token cost UNKNOWN',
    subagents: 'Allowed for role work and independent review; reviewer gets diff + spec only; same-family review is recorded as a limitation until a cross-family judge is configured',
    reviewRoundsMax: 2,
  },
  boundaryPacket: {
    postImplementationOnly: true,
    singleRun: true,
    commands: BOUNDARY.commands,
    expectedEffects: ['All commands exit 0', 'Scenario S-1..S-23 evidence recorded (S-24 is recorded by cr-52 after release) under .kbd-orchestrator/phases/' + PHASE + '/evidence/<date>/ with a README', 'No files outside build/test output directories modified'],
    requiredApprovals: 'Operator approval for the run; Docker available for Postgres/Iggy containers; no publish, tag or push is part of the packet (publication is cr-52, after this packet passes)',
    receipts: ['evidence/<date>/README.md mapping each scenario to its command output', 'CI run URLs for S-12 and S-14', 'image digests for parking-lot-pg:17/18 and Iggy'],
    cleanup: ['docker rm -f test containers created by the run', 'cargo sweep is NOT run (build-dir reuse is intentional)'],
    stopConditions: ['Any command exits non-zero: stop, record FAIL or BLOCKED for the affected scenarios, do not continue to later commands', 'Docker unavailable: BLOCKED for S-5..S-11, S-16, S-18 and S-22', 'A secret is detected: stop and escalate to the operator'],
  },
  status: { implementationComplete: false },
};

writeFileSync(join(root, '.kbd-orchestrator/phases', PHASE, 'adopted-contract.json'), JSON.stringify(contract, null, 2) + '\n');
console.log('written');
