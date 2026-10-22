// Single source for plan.md, openspec/changes/*, and KBD change/task registration.
// Edit here, then re-run plan-generate.mjs. Do not hand-edit the generated files.

export const PHASE = 'phase-correctness-and-durability';

// role -> Claude Code subagent + model (native route verified in this session:
// subagent types listed by the harness; models are the opus/sonnet aliases set in
// .claude/agents/<role>.md frontmatter).
export const ROUTES = {
  'product-manager': { model: 'anthropic/claude-opus-5-5', effort: 'harness default (no per-subagent effort control exposed)' },
  'tech-lead': { model: 'anthropic/claude-opus-5-5', effort: 'harness default' },
  'scheduler-engineer': { model: 'anthropic/claude-opus-5-5', effort: 'harness default' },
  'postgres-engineer': { model: 'anthropic/claude-sonnet-5-5', effort: 'harness default' },
  'events-engineer': { model: 'anthropic/claude-sonnet-5-5', effort: 'harness default' },
  'platform-engineer': { model: 'anthropic/claude-sonnet-5-5', effort: 'harness default' },
  reviewer: { model: 'anthropic/claude-opus-5-5', effort: 'harness default' },
  operator: { model: 'n/a (human)', effort: 'n/a' },
};

const req = (name, shall, scenarios) => ({ name, shall, scenarios });
const sc = (name, when, then) => ({ name, when, then });
const t = (title, role, needs) => ({ title, role, needs });
const review = (focus) => t(`Independent review at the merge gate (${focus})`, 'reviewer', 'high reasoning; reads diff + this change spec only; runs acceptance commands; must not share builder context');

export const CHANGES = [
  // ───────────────────────── M0 — containment and build baseline ─────────────────────────
  {
    id: 'cr-15-containment', milestone: 'M0', owner: 'operator', deps: [], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Contain leaked agent credentials and ship only allowlisted files',
    why: '.codex/auth.json (OPENAI_API_KEY, OAuth tokens) was committed in ec6b4b4 to a public repository; Cargo.toml uses an exclude list that would publish agent tooling. Untracking landed with this plan; rotation and history purge remain. Rotation, not the purge, is the containment guarantee: GitHub keeps refs/pull/* and forks may hold the old commits.',
    details: 'Operator rotates the exposed keys first. After this plan branch is merged and with recorded operator approval, purge the files from history in a fresh mirror clone (no worktrees exist), force-push, record the old main SHA, and ask GitHub Support to purge cached PR refs. Every later branch is created from post-purge main; a CI gate rejects any branch that still contains the old main SHA. Switch Cargo.toml to an include allowlist; add gitleaks and a cargo package --list allowlist check to CI.',
    capability: 'supply-chain-safety',
    requirements: [
      req('No credentials in history or package', 'No reachable commit on any branch SHALL contain an agent-tool credential or runtime-state path, no branch SHALL reintroduce pre-purge history, and `cargo package` SHALL include only allowlisted paths.', [
        sc('Path-based history check', '`git log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json` runs on a fresh mirror clone after the purge', 'the output is empty'),

      ]),
    ],
    tasks: [
      t('Operator rotates OPENAI_API_KEY and Codex OAuth tokens exposed in .codex/auth.json and records confirmation (no secret values) in execution.md', 'operator', 'account access; cannot be delegated'),
      t('After the plan branch merges and with explicit operator approval: the KBD driver (no team worktree) purges .codex/{auth.json,config.toml,history.jsonl,version.json,sessions/} with git filter-repo in a fresh mirror clone, force-pushes, records old/new main SHAs and the pre-purge root commit set, and the operator re-clones every checkout', 'operator', 'destructive git operation run by the driver in a mirror clone; approval string recorded in execution.md'),
      t('Operator asks GitHub Support to purge cached refs/pull/* for the removed paths and records the ticket ID', 'operator', 'account-bound request'),
      t('Verify on a fresh mirror clone that the path-history query is empty and record the output', 'operator', 'driver-run verification; no worktree'),
    ],
  },
  {
    id: 'cr-41-toolchain-baseline', milestone: 'M0', owner: 'tech-lead', deps: ['cr-15-containment'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Rust 1.99.1 / MSRV 1.95 / edition 2024 build baseline where every target compiles',
    why: 'CI is fully red: --all-features cannot resolve on 1.91.1 (pgmq alpha -> sqlx 0.9 needs 1.94), Cargo.lock is gitignored so resolution drifts, benches and examples do not compile, 355 fmt hunks and 157 clippy findings exist. No later change can produce gate evidence until this lands.',
    details: 'Pin rust-toolchain.toml to 1.99.1, set rust-version 1.95 and edition 2024 (cargo fix --edition), commit Cargo.lock, drop the pgmq crate dependency, remove #![deny(warnings)], fix repository/homepage, make benches and examples compile, run one rustfmt pass and resolve clippy findings (fix, or #[expect] with reason).',
    capability: 'build-baseline',
    requirements: [
      req('Reproducible build on the pinned toolchain', 'The crate SHALL build, lint and test with `--locked` on Rust 1.99.1 and on the declared MSRV 1.95, for all targets and all features.', [
        sc('All targets compile', '`cargo check --workspace --all-targets --all-features --locked` runs on 1.99.1 and on 1.95', 'both exit 0'),
        sc('Lint and format clean', '`cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` and `cargo fmt --all -- --check` run', 'both exit 0'),
      ]),
    ],
    tasks: [
      t('Pin rust-toolchain.toml to 1.99.1, set rust-version = "1.95" and edition = "2024", run cargo fix --edition, and commit Cargo.lock (remove it from .gitignore)', 'tech-lead', 'whole-crate mechanical migration; single writer'),
      t('Drop the pgmq client crate and pgmq-ext feature wiring; keep sea-orm 1.x/sqlx 0.8 until cr-27 removes them; update constraints.md checks for MSRV 1.95 (operator-adopted amendment) and the role prompts via the team manifest', 'tech-lead', 'Cargo.toml + cfg-gated code + constraint/prompt text'),
      t('Make benches/queue_bench.rs and both examples compile (idempotency_key, missing main), remove #![deny(warnings)], fix repository/homepage URLs; declare check-cfg for parking_lot_loom and keyed parking_lot_negative_control values under [lints.rust] unexpected_cfgs', 'tech-lead', 'small edits across targets + lint config'),
      t('Single rustfmt pass, then resolve clippy -D warnings findings (fix; #[expect(..., reason)] only where a lint is wrong for this crate)', 'tech-lead', '157 findings; mechanical but judgement on expectations'),
      review('MSRV correctness, no behaviour change hidden in lint fixes'),
    ],
  },
  {
    id: 'cr-17-kbd-correction', milestone: 'M0', owner: 'product-manager', deps: ['cr-15-containment'], size: 'S', complexity: 'Low', cls: 'medium', value: 'MEDIUM',
    title: 'Correct the certification phase record through superseding KBD decisions',
    why: 'phase-certification-remediation is 14/14 COMPLETE, but hooks, cancellation capture and shutdown policy exist only as unwired types (assessment). Generated progress cannot be hand-edited.',
    details: 'Record superseding canonical decisions that state, per overstated change (CR-03, CR-07, CR-13, CR-14), what shipped and which CR in this phase finishes it.',
    capability: 'delivery-record',
    requirements: [
      req('Truthful delivery record', 'The canonical KBD audit SHALL record, for every prior change whose completion overstated delivery, a superseding decision naming the gap and the finishing change.', [
        sc('Audit shows correction', '`prometheus kbd audit` is read after this change', 'decisions for CR-03, CR-07, CR-13 and CR-14 each reference the finishing change in this phase'),
      ]),
    ],
    tasks: [
      t('Verify by reading code which certification-phase changes are unwired (CR-03, CR-07, CR-13, CR-14 at minimum)', 'product-manager', 'code reading; no edits'),
      t('Record superseding decisions with prometheus kbd decision record --supersedes for each overstated change', 'product-manager', 'typed KBD command only'),
      review('record accuracy vs code'),
    ],
  },
  {
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
  {
    id: 'cr-48-workspace-split', milestone: 'M0', owner: 'tech-lead', deps: ['cr-41-toolchain-baseline'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Split into a Cargo workspace with a wasm-safe core and per-backend crates',
    why: 'Backends drag sea-orm/sqlx/iggy/uniffi into every build; the core must build for wasm32 without tokio; four implementers need disjoint ownership. Research (docs/build-performance.md) and the operator decision favour a split; doing it before fan-out avoids every branch rebasing across file moves.',
    details: 'ADR, then one mechanical split: crates/parking-lot-core (dispatcher, semaphore, contracts; no IO, no tokio), crates/parking-lot-runtime-tokio (ResourcePool, native WorkerPool), crates/parking-lot-pg, crates/parking-lot-embedded, crates/parking-lot-iggy, crates/parking-lot-ffi, crates/parking-lot-wasm; the repository root stays the published facade prometheus_parking_lot, re-exporting behind features. Directory names stay short (crates/parking-lot-*) but every published package is named prometheus-parking-lot-* because crates.io treats - and _ alike and parking-lot-core/parking_lot_core belongs to the parking_lot project (checked 2026-10-22). Per-crate include allowlists replace the single exclude list. Constraint checks move from src/ greps to crate-aware checks (cargo tree on core; unsafe grep over crates/** with a documented exemption for generated UniFFI glue in parking-lot-ffi).',
    capability: 'crate-structure',
    requirements: [
      req('Layered workspace', 'The core crate SHALL compile for wasm32-unknown-unknown with no default features and SHALL NOT depend on any IO, database, network or runtime crate; the facade SHALL preserve the published crate name.', [
        sc('Core dependency boundary', '`cargo tree -p prometheus-parking-lot-core -e normal` is inspected', 'no tokio, sqlx, sea-orm, iggy, redb or reqwest appears'),
        sc('Facade compatibility', 'a downstream crate depends on prometheus_parking_lot with default features', 'it compiles and reaches the core types through the facade'),
      ]),
    ],
    tasks: [
      t('Write docs/adr/0001-workspace-split.md (crate map, dependency rules, feature mapping, MSRV per crate)', 'tech-lead', 'architecture judgement'),
      t('Perform the split in one change: move modules and tests to crates/*, workspace Cargo.toml, facade re-exports, per-crate include allowlists; no behaviour change', 'tech-lead', 'large mechanical move; single writer; sequential'),
      t('Rewrite constraints.md checks for the workspace (core boundary via cargo tree, unsafe grep over crates/** with the FFI exemption) as an operator-adopted amendment', 'tech-lead', 'constraint text + commands'),
      review('dependency boundaries, no behaviour drift'),
    ],
  },
  {
    id: 'cr-42-ci-rebuild', milestone: 'M0', owner: 'platform-engineer', deps: ['cr-48-workspace-split'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Green, cached, locked CI on stable and MSRV',
    why: 'Every CI job fails on main; caching and locking are absent; cargo-deny fails on RUSTSEC-2025-0111 (tokio-tar via testcontainers) and licenses.',
    details: 'Swatinem/rust-cache@v2 (save-if main), --locked everywhere, nextest, jobs for fmt/clippy/test(1.99.1, 1.95)/docs/deny/coverage; upgrade testcontainers to clear the advisory and fix deny.toml licenses; per-role aliases updated for the workspace.',
    capability: 'continuous-integration',
    requirements: [
      req('Required CI jobs pass on main', 'CI SHALL run fmt, clippy, tests on 1.99.1 and 1.95, docs, cargo-deny and coverage with --locked, and all SHALL pass on main.', [
        sc('Deny clean', '`cargo deny check` runs', 'advisories, bans, licenses and sources all pass'),
        sc('Cache reuse', 'CI runs twice on the same commit', 'the second run restores the cache and skips dependency compilation'),
      ]),
    ],
    tasks: [
      t('Rewrite .github/workflows/ci.yml: rust-cache, --locked, nextest, toolchain matrix 1.99.1 + 1.95', 'platform-engineer', 'CI YAML'),
      t('Resolve cargo-deny: upgrade testcontainers (tokio-tar RUSTSEC-2025-0111) via tech-lead handoff; fix deny.toml license policy', 'platform-engineer', 'supply-chain policy; Cargo.toml via handoff'),
      t('Update .cargo/config.toml aliases and docs/build-performance.md for the workspace', 'platform-engineer', 'config + doc'),
      review('CI covers every required job; no job silently skipped'),
    ],
  },
  {
    id: 'cr-43-postgres18-image', milestone: 'M0', owner: 'postgres-engineer', deps: ['cr-48-workspace-split', 'cr-42-ci-rebuild'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Project Postgres 18 image (flint-forge pattern) with pgmq for tests and local dev',
    why: 'Postgres backends have no running tests; pgmq must be reached through the extension. flint-forge images/postgres18/Dockerfile is the operator-designated basis (PGDG 18, multi-stage, pinned extension sources).',
    details: 'docker/postgres18/Dockerfile: PGDG Postgres 18 base, pgmq built from a pinned source tag, tuned postgresql.conf for queue workloads, init scripts; a pg17 variant for the matrix; compose service; testcontainers helper that builds or reuses the tagged image; CI image build with layer cache.',
    capability: 'postgres-test-substrate',
    requirements: [
      req('Reproducible Postgres substrate', 'The project SHALL provide a pinned Postgres 18 (and 17) image with the pgmq extension available, used by every Postgres test locally and in CI.', [
        sc('Extension present', 'the image starts and `CREATE EXTENSION pgmq` runs', 'it succeeds and `SELECT version()` reports PostgreSQL 18'),
        sc('Tests use the image', 'the Postgres integration suite runs', 'containers are created from the project image tag, not a floating upstream tag'),
      ]),
    ],
    tasks: [
      t('Write docker/postgres18/Dockerfile (+ pg17 variant) modelled on flint-forge images/postgres18: PGDG base, pgmq from pinned tag, tuned conf, init scripts; no Flint-specific extensions', 'postgres-engineer', 'Docker multi-stage; extension build'),
      t('Add a testcontainers helper that builds/reuses the image and exposes a pool; add a compose service for local use', 'postgres-engineer', 'Rust test support'),
      t('Hand platform-engineer the CI image-build step with layer caching', 'postgres-engineer', 'cross-owner handoff'),
      review('image pinning, extension availability'),
    ],
  },
  {
    id: 'cr-35-test-targets', milestone: 'M0', owner: 'platform-engineer', deps: ['cr-48-workspace-split', 'cr-43-postgres18-image'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Unit and integration suites become real test targets on the current testcontainers API',
    why: 'tests/unit and tests/integration are never compiled and use the removed testcontainers clients::Cli/RunnableImage API; the coverage baseline overstates.',
    details: 'Register the suites as test targets in their crates, port to the current testcontainers API using the CR-43 image, quarantine (with issue links) any test that encodes the old broken behaviour rather than deleting it.',
    capability: 'test-infrastructure',
    requirements: [
      req('All suites run', 'Every test file in the repository SHALL belong to a compiled test target that CI executes.', [
        sc('No orphan tests', 'a script lists *.rs under tests/ and crates/*/tests/', 'each file is a target root or is reachable from one'),
      ]),
    ],
    tasks: [
      t('Register unit/integration suites as test targets in their crates', 'platform-engineer', 'Cargo target wiring'),
      t('Port Postgres integration tests to the current testcontainers API and the CR-43 image (with postgres-engineer)', 'platform-engineer', 'test code; real Postgres'),
      t('Add the orphan-test check script to CI', 'platform-engineer', 'small script'),
      review('no test silently dropped'),
    ],
  },
  {
    id: 'cr-16-docs-truth', milestone: 'M0', owner: 'product-manager', deps: ['cr-48-workspace-split', 'cr-18-core-contracts'], size: 'M', complexity: 'Low', cls: 'medium', value: 'MEDIUM',
    title: 'Archive invalid docs and make README examples compile',
    why: '~30% of docs/ describes a different crate (Mutex/RwLock/Once/Condvar specs); README samples use non-existent APIs; the old CLAUDE.md content lives on in docs.',
    details: 'Move invalid docs to docs/archive/ with a one-line reason each, reset TODO/API, and make every README code block a compiled doctest (doc = include_str! on the facade).',
    capability: 'documentation-integrity',
    requirements: [
      req('Docs describe this crate', 'Every Rust code sample in README.md SHALL compile as a doctest, and docs/ outside docs/archive/ SHALL describe only APIs that exist.', [
        sc('README doctests', '`cargo test --doc -p prometheus_parking_lot` runs', 'README samples are executed and pass'),
      ]),
    ],
    tasks: [
      t('Archive invalid docs (PLAN-/SPEC-/TASKS-*, primitives docs, stale reports) to docs/archive/ with an index of reasons', 'product-manager', 'doc triage'),
      t('Rewrite README.md against the real API and wire it as a doctest on the facade (tech-lead applies the lib.rs attribute)', 'product-manager', 'docs + cross-owner handoff'),
      review('no stale API claims remain'),
    ],
  },
  {
    id: 'cr-45-docs-site', milestone: 'M0', owner: 'product-manager', deps: ['cr-48-workspace-split'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Branded Docusaurus documentation site (know-me brand, Flat 2.0)',
    why: 'The operator requires a branded documentation site; there is none. know-me-system docs/knowme-ui-ux-standard.md defines the brand (Flat 2.0, Ember #FF6A3D / Cyan #00C2DC, dark and light token tables, typography, logo).',
    details: 'Scaffold website/ with the build-branded-docusaurus skill: pinned Docusaurus, know-me tokens as CSS variables (dark + light), logo from know-me branding assets, Mermaid, local search, sanitizer, SITE_URL/BASE_URL, GitHub Pages workflow, non-root container; verify build, links, responsive light/dark screenshots and accessibility.',
    capability: 'documentation-site',
    requirements: [
      req('Branded, verified documentation site', 'The project SHALL publish a Docusaurus site whose theme uses the know-me brand tokens and Flat 2.0 rules in light and dark modes and passes build, link and accessibility checks.', [
        sc('Production build', 'the site builds with a frozen install', 'the build exits 0 with no broken links'),
        sc('Brand fidelity', 'light and dark screenshots of the home and a docs page are taken', 'canvas, surface, text, Ember and Cyan values match the know-me token table; no borders, gradients or decorative shadows'),
        sc('Accessibility', 'an automated accessibility check runs on representative routes', 'no serious or critical violations; keyboard focus is visible'),
      ]),
      req('Published on GitHub Pages and linked from the repository', 'A GitHub Actions workflow SHALL build the site on every pull request and deploy it to GitHub Pages from main, and the repository About section SHALL link to the published site (operator request 2026-10-22).', [
        sc('PR build', 'a pull request changes website/', 'the docs workflow builds the site and fails the check on a build or link error, without deploying'),
        sc('Deploy from main', 'a commit lands on main', 'the workflow deploys with actions/upload-pages-artifact and actions/deploy-pages, and https://prometheus-ags.github.io/prometheus-parking-lot-rs/ serves the new build (HTTP 200, expected title)'),
        sc('About link', 'the repository page is viewed', 'the About homepage is https://prometheus-ags.github.io/prometheus-parking-lot-rs/'),
      ]),
    ],
    tasks: [
      t('Scaffold website/ with build-branded-docusaurus; pin versions; map know-me dark/light tokens, typography and logo', 'product-manager', 'frontend scaffolding; design tokens'),
      t('Add Mermaid, local search, sanitizer (no private paths/secrets/raw wiki), SITE_URL/BASE_URL, container', 'product-manager', 'site config'),
      t('Vendor the build-branded-docusaurus verify script as `npm run verify` (build, links, light/dark screenshots, a11y) and hand platform-engineer the GitHub Pages workflow and verify job', 'product-manager', 'site tooling + cross-owner handoff'),
      review('brand fidelity, a11y, sanitizer'),
      t('Add .github/workflows/docs.yml: build website/ on pull requests (no deploy); on main build with SITE_URL=https://prometheus-ags.github.io BASE_URL=/prometheus-parking-lot-rs/ and deploy via actions/upload-pages-artifact + actions/deploy-pages (permissions pages: write, id-token: write; concurrency group pages)', 'platform-engineer', 'CI YAML; GitHub Pages deployment'),
      t('Enable GitHub Pages with source "GitHub Actions" (gh api repos/Prometheus-AGS/prometheus-parking-lot-rs/pages); after the first successful deploy returns HTTP 200, set the repository About homepage to the docs URL (gh repo edit --homepage) and record both in execution.md', 'operator', 'repository settings; outward-facing; authorized by the operator request 2026-10-22'),
    ],
  },

  // ───────────────────────── M1 — core rework ─────────────────────────
  {
    id: 'cr-18-core-contracts', milestone: 'M1', owner: 'tech-lead', deps: ['cr-48-workspace-split'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Core contracts: Arc<Inner>, Clock, Result executors, error taxonomy',
    why: 'Implementers must build against merged contracts; ResourcePool threads many Arcs through static futures; sync_wake_worker_loop is public and discards work; executors cannot report failure; time is untestable without a Clock.',
    details: 'Introduce Arc<Inner> for ResourcePool; delete sync_wake_worker_loop; Clock trait (monotonic + wall) with a test clock; TaskExecutor returns Result<T, TaskError>; extend the #[non_exhaustive] error taxonomy (ExecutorPanicked, Timeout, Cancelled, CapacityExceeded); monotonic task sequence for FIFO (folds in CR-24); the cost type is defined now as a resource vector (a scalar cost is the one-dimension case) so backends, config and FFI never build on a scalar that cr-23 would break.',
    capability: 'core-contracts',
    requirements: [
      req('Executors report failure', 'Task execution SHALL produce a typed result distinguishing success, executor error, timeout, panic and cancellation, delivered to the mailbox.', [
        sc('Executor error', 'an executor returns an error', 'the mailbox records a failed status carrying the error, and capacity is released'),
      ]),
      req('Deterministic ordering', 'Tasks of equal priority SHALL be dispatched in submission order using a monotonic sequence, with Eq/Ord consistent.', [
        sc('FIFO within priority', '1,000 equal-priority tasks are submitted from one thread to a one-unit pool', 'they complete in submission order'),
      ]),
    ],
    tasks: [
      t('Arc<Inner> refactor of ResourcePool; delete sync_wake_worker_loop; Clock trait + TestClock', 'tech-lead', 'core refactor; concurrency-sensitive'),
      t('Result-returning TaskExecutor contract and error taxonomy; monotonic FIFO sequence with consistent Eq/Ord; resource-vector cost type (serde, one-dimension default)', 'tech-lead', 'public API design'),
      t('Migrate existing tests and examples to the new contracts; CHANGELOG 0.3.0 entry', 'tech-lead', 'mechanical follow-through'),
      review('API soundness, Send/Sync bounds, semver notes'),
    ],
  },
  {
    id: 'cr-25-backend-contracts', milestone: 'M1', owner: 'tech-lead', deps: ['cr-18-core-contracts'], size: 'M', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Async lease/ack queue, async mailbox and Notifier contracts with a conformance suite',
    why: 'Postgres backends block_on inside the runtime and panic; the queue is at-most-once; no Notifier type exists although CR-32/34 need one. Pulled forward from M2 so backends can fan out in parallel.',
    details: 'Async TaskQueue with lease, ack, nack(requeue|dead-letter), extend; async Mailbox with deliver/consume-once/TTL; Notifier trait; StorageMailbox<S, N> composition type; in-memory reference implementations; a backend conformance test suite every backend must pass.',
    capability: 'backend-contracts',
    requirements: [
      req('Lease semantics', 'A leased task SHALL be invisible to other consumers until acked, nacked or its lease expires, after which it SHALL be redelivered.', [
        sc('Lease expiry', 'a consumer leases a task and never acks it', 'after the lease duration the task is delivered to another consumer'),
      ]),
      req('Conformance suite', 'Every queue and mailbox backend SHALL pass one shared conformance suite.', [
        sc('Reference backend passes', 'the conformance suite runs against the in-memory backends', 'all cases pass'),
      ]),
    ],
    tasks: [
      t('Define async TaskQueue (lease/ack/nack/extend), async Mailbox (deliver/consume-once/TTL) and Notifier traits', 'tech-lead', 'contract design'),
      t('In-memory reference implementations; StorageMailbox<S, N> type signature only (behaviour in cr-34)', 'tech-lead', 'implementation'),
      t('Backend conformance test suite (generic over backend) published for backend crates', 'tech-lead', 'test design'),
      review('contract completeness, cancellation safety of async fns'),
    ],
  },
  {
    id: 'cr-19-dispatcher-semaphore', milestone: 'M1', owner: 'scheduler-engineer', deps: ['cr-18-core-contracts'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Unit-weighted semaphore and priority dispatcher shared by every pool',
    why: 'WorkerPool (native and WASM) never enforces max_units (C1); ResourcePool has a lost wakeup (C3), pop-then-push task loss (C4) and a max_queue_depth TOCTOU; WorkerPool executions are unbounded.',
    details: 'One dispatcher (parking-lot-core, no tokio) owns the priority queue and a unit-weighted semaphore; submit performs depth check + enqueue atomically; dispatch peeks and pops only when the head fits. ResourcePool and the native WorkerPool (parking-lot-runtime-tokio) use it, and native WorkerPool executions get the same timeout/deadline budget as ResourcePool. The WASM pool is ported by platform-engineer in cr-38. Loom models run under `--cfg parking_lot_loom` on the core crate only; negative controls use the test-only `--cfg parking_lot_negative_control`, never a Cargo feature.',
    capability: 'capacity-admission',
    requirements: [
      req('Capacity invariant', 'At every instant the sum of the costs of running tasks SHALL be at most max_units, for every pool type.', [
        sc('Model check', 'the loom models of the dispatcher and semaphore run with LOOM_MAX_PREEMPTIONS=3', 'no explored schedule exceeds max_units and no accepted task is lost'),
        sc('Property test', 'random submit/complete sequences run against ResourcePool and the native WorkerPool', 'the invariant and queue-depth limit hold'),
        sc('Negative control', 'the same models run with `--cfg parking_lot_negative_control` (semaphore acquire skipped)', 'the model reports a violation'),
      ]),
      req('No lost wakeup', 'A parked task SHALL be dispatched when capacity sufficient for it becomes free.', [
        sc('Completion races submit', 'a task completes while another is being parked', 'the parked task is dispatched without waiting for a later completion'),
      ]),
      req('Bounded execution', 'Every pool SHALL bound each execution by the task deadline or the pool default timeout.', [
        sc('Wedged executor', 'an executor never returns in a WorkerPool', 'it is cancelled at the budget, its units are released, and the mailbox records a timeout'),
      ]),
    ],
    tasks: [
      t('Implement the dispatcher + unit-weighted semaphore; atomic depth-check/enqueue; peek-then-pop', 'scheduler-engineer', 'lock-free/locking design; memory ordering'),
      t('Port ResourcePool and the native WorkerPool onto the dispatcher; add execution budgets to the native WorkerPool', 'scheduler-engineer', 'cross-module refactor'),
      t('loom models (loom_capacity, loom_wakeup) on parking-lot-core under --cfg parking_lot_loom with negative controls under --cfg parking_lot_negative_control; proptest suites for the tokio pools; budget_ tests (wedged executor, cancel witness) for ResourcePool and the native WorkerPool; miri on atomics paths', 'scheduler-engineer', 'model checking + budget tests'),
      review('memory ordering, lost-wakeup proof, negative control'),
    ],
  },
  {
    id: 'cr-21-panic-containment', milestone: 'M1', owner: 'scheduler-engineer', deps: ['cr-19-dispatcher-semaphore'], size: 'M', complexity: 'Medium', cls: 'frontier', value: 'HIGH',
    title: 'Executor panics never leak capacity or kill the pool',
    why: 'No catch_unwind exists on any execution path (C2): a panic leaks units, strands result slots and kills WorkerPool workers.',
    details: 'Native pools only (ResourcePool, native WorkerPool): catch_unwind around executor futures, RAII semaphore permits so release happens on every exit path, worker respawn, failed_tasks accounting, panic delivered as a typed failure. On wasm32-unknown-unknown panics abort the module on stable Rust; cr-38 documents that limit.',
    capability: 'fault-containment',
    requirements: [
      req('Panic containment', 'An executor panic SHALL release the task capacity, deliver a panicked status, increment failure counters and leave the pool able to run further tasks.', [
        sc('Panicking executor', 'one of 100 tasks panics in a one-worker pool', 'the other 99 complete, capacity returns to zero, and stats report one failure'),
      ]),
    ],
    tasks: [
      t('catch_unwind + RAII permits on every execution path; worker respawn; counters', 'scheduler-engineer', 'unwind safety'),
      t('Panic tests for ResourcePool and the native WorkerPool, with a negative control under --cfg parking_lot_negative_control (catch_unwind skipped)', 'scheduler-engineer', 'tests'),
      review('unwind safety, permit release on all paths'),
    ],
  },
  {
    id: 'cr-22-result-slots', milestone: 'M1', owner: 'scheduler-engineer', deps: ['cr-19-dispatcher-semaphore'], size: 'M', complexity: 'Medium', cls: 'frontier', value: 'HIGH',
    title: 'Async result slots with TTL that use no blocking threads',
    why: 'retrieve_async parks a spawn_blocking thread per waiter; a few hundred waiters exhaust the blocking pool; unretrieved results live forever.',
    details: 'Per-slot oneshot/Notify for async waiters, blocking_recv for the sync API, u64 keys, TTL sweeper defaulting to the task timeout.',
    capability: 'result-retrieval',
    requirements: [
      req('Non-blocking retrieval', 'Waiting for a result asynchronously SHALL NOT occupy a blocking thread, and unretrieved results SHALL expire after their TTL.', [
        sc('Many waiters', 'a runtime built with max_blocking_threads(1) has that thread occupied, and 10,000 concurrent retrieve_async calls wait for results that are then delivered', 'all 10,000 calls resolve within the test timeout'),
        sc('TTL expiry', 'a result is never retrieved', 'it is evicted after its TTL and a later retrieve reports expired'),
      ]),
    ],
    tasks: [
      t('Replace condvar slots with oneshot/Notify slots; blocking API via blocking_recv; TTL sweeper', 'scheduler-engineer', 'async design'),
      t('Load test for blocking-thread usage and TTL tests', 'scheduler-engineer', 'tests'),
      review('no blocking in async context'),
    ],
  },
  {
    id: 'cr-20-skip-ahead', milestone: 'M1', owner: 'scheduler-engineer', deps: ['cr-19-dispatcher-semaphore', 'cr-31-hook-emission'], size: 'M', complexity: 'High', cls: 'frontier', value: 'MEDIUM',
    title: 'Skip-ahead bin-packing with a starvation guard',
    why: 'Strict head-of-line dispatch wastes capacity when a large task waits behind free units (PARKING_LOT.md section 3.3).',
    details: 'Dispatch a smaller fitting task when the head does not fit, bounded by a configurable starvation guard that reserves capacity for the head after N skips.',
    capability: 'capacity-admission',
    requirements: [
      req('Bounded skip-ahead', 'The dispatcher SHALL dispatch fitting tasks behind a non-fitting head, and the head SHALL run within the configured number of dispatch cycles.', [
        sc('Head not starved', 'small tasks keep arriving while a large head task waits', 'the head starts within N dispatch cycles'),
        sc('Utilization', 'the mixed-size benchmark runs', 'utilization exceeds strict FIFO dispatch'),
      ]),
    ],
    tasks: [
      t('Skip-ahead policy with starvation guard (config field via tech-lead handoff); emit lifecycle events for skipped/promoted dispatch', 'scheduler-engineer', 'scheduling algorithm'),
      t('Starvation and utilization tests/benchmarks; event-ordering test still passes', 'scheduler-engineer', 'tests + bench'),
      review('starvation bound proof'),
    ],
  },
  {
    id: 'cr-23-multi-resource', milestone: 'M1', owner: 'scheduler-engineer', deps: ['cr-20-skip-ahead'], size: 'M', complexity: 'High', cls: 'frontier', value: 'MEDIUM',
    title: 'Multi-resource cost vectors (e.g. VRAM + worker slots)',
    why: 'A single unit count cannot express a task needing 8 GB VRAM and one worker slot (PARKING_LOT.md section 5).',
    details: 'The resource-vector cost type already exists (cr-18); this change makes the semaphore and dispatcher admit by every dimension, adds multi-dimension pool limits to config, and emits capacity events per dimension.',
    capability: 'capacity-admission',
    requirements: [
      req('Multi-dimensional admission', 'A task SHALL be admitted only when every resource dimension of its cost fits within remaining capacity.', [
        sc('One dimension short', 'a task needs (8 GB, 1 slot) while 8 GB is free and 0 slots are free', 'the task waits'),
      ]),
    ],
    tasks: [
      t('Vector-aware semaphore/dispatcher admission; multi-dimension limits via tech-lead config handoff; lifecycle events per dimension', 'scheduler-engineer', 'algorithm'),
      t('Extend loom/proptest invariants and negative controls to vectors', 'scheduler-engineer', 'tests'),
      review('invariant per dimension'),
    ],
  },

  // ───────────────────────── M2 — durable backends ─────────────────────────
  {
    id: 'cr-26-pg-lease-queue', milestone: 'M2', owner: 'postgres-engineer', deps: ['cr-25-backend-contracts', 'cr-35-test-targets'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Postgres lease queue on raw sqlx with SKIP LOCKED, NOTIFY and a reaper',
    why: 'The current queue panics inside the runtime (block_on), deletes on dequeue (at-most-once) and claims SKIP LOCKED while using plain FOR UPDATE.',
    details: 'Single-statement lease with FOR UPDATE SKIP LOCKED (plan section 7.4), ack/nack/extend, LISTEN/NOTIFY wake-up, lease reaper, not_before backoff, migrations; raw sqlx 0.9.0 (stable 2026-05-21, rust-version 1.94 <= MSRV 1.95; re-check at implementation and record the date); passes the conformance suite. sea-orm stays until cr-27 ports the mailbox.',
    capability: 'durable-queue-postgres',
    requirements: [
      req('At-least-once with leases', 'A task leased from Postgres SHALL be redelivered after lease expiry if its consumer dies, and concurrent dispatchers SHALL never hold the same lease.', [
        sc('Crash redelivery', 'a consumer process is killed with SIGKILL mid-task', 'the task is redelivered after the lease expires'),
        sc('No double claim', 'eight dispatchers lease concurrently from one queue', 'no task is leased by two dispatchers at once'),
      ]),
    ],
    tasks: [
      t('Lease/ack/nack/extend SQL and migrations; LISTEN/NOTIFY wake-up; reaper; backoff', 'postgres-engineer', 'SQL concurrency'),
      t('Async sqlx 0.9 implementation of the TaskQueue contract; remove block_on from the queue', 'postgres-engineer', 'async Rust'),
      t('Conformance suite + SIGKILL redelivery + concurrent dispatcher tests on the PG18 image; negative controls under --cfg parking_lot_negative_control (lease never expires; SKIP LOCKED omitted)', 'postgres-engineer', 'real-service tests'),
      review('SQL correctness, isolation level, lease races'),
    ],
  },
  {
    id: 'cr-27-pg-mailbox', milestone: 'M2', owner: 'postgres-engineer', deps: ['cr-26-pg-lease-queue'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Postgres mailbox: consume-once, TTL eviction, first-write-wins idempotency',
    why: 'The mailbox block_ons inside the runtime and cannot be read back.',
    details: 'consume as DELETE ... RETURNING, TTL eviction, ON CONFLICT on the idempotency key, async sqlx; passes the mailbox conformance suite.',
    capability: 'durable-mailbox-postgres',
    requirements: [
      req('Consume-once delivery', 'A stored result SHALL be returned to exactly one consume call, the first write for an idempotency key SHALL win, and expired results SHALL be evicted.', [
        sc('Concurrent consume', 'two clients consume the same key concurrently', 'exactly one receives the result'),
        sc('Duplicate write', 'a retried task delivers twice with one idempotency key', 'the first stored result is kept'),
      ]),
    ],
    tasks: [
      t('Mailbox SQL (DELETE ... RETURNING, TTL, ON CONFLICT) and migration', 'postgres-engineer', 'SQL'),
      t('Async implementation + conformance tests on PG18; consume-once race test with two transactions coordinated by a barrier; remove sea-orm/sqlx 0.8 from parking-lot-pg via tech-lead Cargo handoff', 'postgres-engineer', 'real-service tests + dependency removal'),
      review('consume-once race, TTL'),
    ],
  },
  {
    id: 'cr-30-pgmq-sql-backend', milestone: 'M2', owner: 'postgres-engineer', deps: ['cr-26-pg-lease-queue', 'cr-43-postgres18-image'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'pgmq backend through SQL (one queue per priority), no client crate',
    why: 'Operator decision: drop the alpha pgmq client crate that broke MSRV; reach the pgmq extension (>= 1.8, PG18) through SQL.',
    details: 'Implement the TaskQueue contract with pgmq.send/read/delete/archive and visibility timeouts as leases; one pgmq queue per priority level; passes the conformance suite on the PG18 image.',
    capability: 'durable-queue-pgmq',
    requirements: [
      req('pgmq-backed queue', 'The pgmq backend SHALL satisfy the queue conformance suite using visibility timeouts as leases and per-priority queues.', [
        sc('Priority order', 'tasks of three priorities are enqueued', 'higher-priority tasks are leased first'),
      ]),
    ],
    tasks: [
      t('SQL implementation over pgmq functions with visibility-timeout leases and per-priority queues; add the pgmq config section and example test (tech-lead config handoff)', 'postgres-engineer', 'SQL + async Rust'),
      t('Conformance suite against the PG18 image with pgmq', 'postgres-engineer', 'real-service tests'),
      review('lease mapping to visibility timeout'),
    ],
  },
  {
    id: 'cr-28-embedded-redb-queue', milestone: 'M2', owner: 'events-engineer', deps: ['cr-25-backend-contracts'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Crash-safe embedded queue (redb) replacing the JSONL Yaque queue',
    why: 'The JSONL queue is not crash-safe and ignores priority; desktop/Tauri needs durable local parking.',
    details: 'redb-backed lease queue and mailbox (one-day rustqueue spike recorded first), priority honoured, fault-injection test at every crash point; Yaque code removed.',
    capability: 'embedded-queue',
    requirements: [
      req('Crash safety', 'An accepted task SHALL survive a process crash at any point of enqueue, lease or ack and be redelivered after restart.', [
        sc('Fault injection', 'the process is aborted at each instrumented crash point', 'after restart every accepted task is present exactly once or redelivered per lease rules'),
      ]),
    ],
    tasks: [
      t('One-day spike: redb vs rustqueue; record the decision in docs/adr', 'events-engineer', 'research'),
      t('redb lease queue + mailbox implementing the contracts; remove Yaque', 'events-engineer', 'storage engine'),
      t('Conformance suite + crash-point fault-injection tests; negative control under --cfg parking_lot_negative_control skips the redb commit and must lose a task', 'events-engineer', 'fault injection'),
      review('durability under crash'),
    ],
  },
  {
    id: 'cr-49-durable-dispatch', milestone: 'M2', owner: 'scheduler-engineer', deps: ['cr-21-panic-containment', 'cr-26-pg-lease-queue', 'cr-27-pg-mailbox', 'cr-28-embedded-redb-queue', 'cr-34-mailbox-composition'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Pools persist accepted work through the durable queue (persist, lease, ack after delivery, extend)',
    why: 'The phase promise is that accepted work survives a process crash, but today ResourcePool enqueues to TaskQueue only when it parks a task, so immediately admitted tasks are never persisted; backends tested in isolation do not prove the pool uses them correctly (plan review CRITICAL).',
    details: 'With a durable backend configured, submit persists before acknowledging acceptance; the dispatcher sources work from TaskQueue::lease; the task is acked only after the mailbox write; long tasks extend their lease with a heartbeat; on restart, unacked work is redelivered by lease expiry. Contract support (any trait additions) via tech-lead handoff; no backend-crate edits.',
    capability: 'durable-dispatch',
    requirements: [
      req('Crash-durable acceptance', 'With a durable backend, once submit reports a task accepted, the task SHALL complete exactly once from the caller perspective (result consumable) even if the pool process is killed at any point during its execution.', [
        sc('Pool killed mid-execution (Postgres)', 'a process running ResourcePool on the Postgres backend is killed with SIGKILL while a task executes, and a new process starts', 'the task re-runs after lease expiry and exactly one result is consumable'),
        sc('Pool killed mid-execution (redb)', 'the same test runs on the embedded redb backend', 'the task re-runs after restart and exactly one result is consumable'),
        sc('Ack after delivery', 'the process is killed after execution but before the mailbox write', 'the task is redelivered rather than lost'),
        sc('Negative control', 'the test runs with `--cfg parking_lot_negative_control` (ack before mailbox write)', 'the task result is lost and the test detects it'),
      ]),
      req('Lease extension', 'A task running longer than the lease duration SHALL extend its lease and SHALL NOT be redelivered while alive.', [
        sc('Long task', 'a task runs for three lease durations', 'it is never leased by a second dispatcher'),
      ]),
    ],
    tasks: [
      t('Dispatcher sources from TaskQueue::lease when durable; persist-before-accept on submit; ack after mailbox write; lease heartbeat', 'scheduler-engineer', 'concurrency + durability design'),
      t('Contract additions (if any) via tech-lead handoff; facade config wiring', 'scheduler-engineer', 'cross-owner handoff'),
      t('Child-process SIGKILL tests for ResourcePool on PG18 and redb, ack-ordering test, long-task lease test, negative control', 'scheduler-engineer', 'real-service fault tests'),
      review('durability ordering: persist, lease, deliver, ack'),
    ],
  },
  {
    id: 'cr-29-shutdown-cancellation', milestone: 'M2', owner: 'scheduler-engineer', deps: ['cr-49-durable-dispatch'], size: 'M', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Wire shutdown policy and cancellation capture into both pools',
    why: 'ShutdownPolicy, CancellationSink and SinkOutcome::Requeue exist only as types; the native WorkerPool drops a received task on shutdown (native.rs:603-616).',
    details: 'DrainThenCancel default; stragglers captured to the CancellationSink; Requeue maps to TaskQueue::nack (contract call only, no backend-crate edits); ResourcePool and the native WorkerPool covered; restart redelivery on PG and redb.',
    capability: 'shutdown-cancellation',
    requirements: [
      req('No loss on shutdown', 'On shutdown every accepted task SHALL either complete, be captured by the cancellation sink, or be requeued to a durable backend.', [
        sc('Shutdown mid-dispatch', 'shutdown is signalled while tasks are queued and one has been received by a worker', 'every task is completed, captured or requeued; none is silently dropped'),
        sc('Requeue across restart', 'a captured task is requeued to Postgres or redb and the process restarts', 'the task runs after restart'),
      ]),
    ],
    tasks: [
      t('Wire ShutdownPolicy + cancellation channel into ResourcePool and WorkerPool; fix received-task drop', 'scheduler-engineer', 'concurrency + lifecycle'),
      t('Requeue -> TaskQueue::nack through the contract (no edits inside parking-lot-pg or parking-lot-embedded)', 'scheduler-engineer', 'contract-level integration'),
      t('Shutdown and restart tests on PG18 and redb; negative control under --cfg parking_lot_negative_control drops a received task and must be detected', 'scheduler-engineer', 'real-service tests'),
      review('no silent loss on any shutdown path'),
    ],
  },

  // ───────────────────────── M3 — events ─────────────────────────
  {
    id: 'cr-31-hook-emission', milestone: 'M3', owner: 'scheduler-engineer', deps: ['cr-29-shutdown-cancellation'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'Emit lifecycle events at every transition',
    why: 'HookBus and emit_* are exported but never called by any pool. Emission points live in the scheduler-owned dispatcher and pools, so the scheduler owner writes them after every transition (including cancellation and requeue) exists.',
    details: 'Emit submitted/parked/started/completed/failed/cancelled/requeued/rejected from the dispatcher and pools; hooks registered on the builder (tech-lead handoff); hook API changes in hooks.rs via events-engineer handoff; zero overhead when no hooks are registered.',
    capability: 'lifecycle-events',
    requirements: [
      req('Ordered lifecycle events', 'Each task SHALL emit its lifecycle events in causal order, and pools with no hooks SHALL incur no measurable overhead.', [
        sc('Event ordering', 'a recording hook observes 1,000 tasks', 'each task shows submitted before started before a terminal event'),
        sc('Zero overhead', 'the dispatch benchmark runs with and without the hook feature', 'no regression beyond noise'),
      ]),
    ],
    tasks: [
      t('Emit events from the dispatcher and pools; builder registration (tech-lead handoff); hooks.rs API adjustments via events-engineer handoff', 'scheduler-engineer', 'integration'),
      t('Ordering tests and overhead benchmark', 'scheduler-engineer', 'tests + bench'),
      review('event completeness, overhead'),
    ],
  },
  {
    id: 'cr-34-mailbox-composition', milestone: 'M3', owner: 'events-engineer', deps: ['cr-25-backend-contracts', 'cr-27-pg-mailbox'], size: 'S', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'Composable mailbox: storage plus optional notifier',
    why: 'Notifier failure must never lose a stored result; there is no composition today.',
    details: 'StorageMailbox<S, N> stores first, notifies off-path with bounded retries; notifier failure is logged and surfaced, never fatal to storage.',
    capability: 'result-delivery',
    requirements: [
      req('Storage before notification', 'A result SHALL be durably stored before any notification, and a notifier failure SHALL NOT remove or block the stored result.', [
        sc('Notifier down', 'the notifier fails every call', 'results are still stored and consumable'),
      ]),
    ],
    tasks: [
      t('StorageMailbox<S, N> behaviour in crates/parking-lot-core/src/mailbox/ with off-path bounded notification', 'events-engineer', 'async composition'),
      t('Failure-injection tests in prometheus-parking-lot-core with in-memory storage and a failing notifier', 'events-engineer', 'tests'),
      review('ordering of store vs notify'),
    ],
  },
  {
    id: 'cr-32-iggy-events', milestone: 'M3', owner: 'events-engineer', deps: ['cr-31-hook-emission', 'cr-34-mailbox-composition'], size: 'L', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'Iggy hook and notifier (events only, not storage or work queue)',
    why: 'Plan section 8: Iggy fits lifecycle-event propagation and mailbox notification; it is not suited to mailbox storage or the priority work queue.',
    details: 'iggy feature/crate: IggyHook and IggyNotifier, UUIDv5 message IDs from the idempotency key, bounded off-path publisher; integration test against a pinned Iggy container.',
    capability: 'iggy-events',
    requirements: [
      req('Event delivery via Iggy', 'With the iggy feature, lifecycle events and mailbox-ready notifications SHALL be published to Iggy with deterministic message IDs, without blocking dispatch.', [
        sc('Subscriber receives', 'a subscriber reads the stream after a task completes', 'it receives mailbox.ready for that task and can replay from an earlier offset'),
      ]),
    ],
    tasks: [
      t('IggyHook + IggyNotifier with bounded publisher and UUIDv5 IDs; iggy config section + example test (tech-lead config handoff)', 'events-engineer', 'integration'),
      t('Integration tests against a pinned Iggy 0.11-compatible server container: same idempotency key published twice yields one message ID; a paused server does not raise dispatch latency beyond the bound', 'events-engineer', 'real-service test'),
      review('backpressure, ID determinism'),
    ],
  },
  {
    id: 'cr-33-stream-replay', milestone: 'M3', owner: 'events-engineer', deps: ['cr-32-iggy-events'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'LOW', stretch: true,
    title: '(Stretch) Streaming-token replay for reconnecting clients',
    why: 'Long LLM streams outlive client connections.',
    details: 'IggyStreamSink publishes streamed tokens; a reconnecting client resumes from its last offset.',
    capability: 'stream-replay',
    requirements: [
      req('Gap-free resume', 'A client that disconnects mid-stream SHALL be able to resume and receive the remaining tokens with no gap or duplicate.', [
        sc('Reconnect mid-stream', 'a client disconnects after token 50 of 200 and reconnects', 'it receives tokens 51-200 exactly once'),
      ]),
    ],
    tasks: [
      t('IggyStreamSink and resume API', 'events-engineer', 'streaming'),
      t('Disconnect/reconnect test', 'events-engineer', 'real-service test'),
      review('gap/duplicate proof'),
    ],
  },

  // ───────────────────────── M4 — platform, CI, quality ─────────────────────────
  {
    id: 'cr-36-pg-matrix', milestone: 'M4', owner: 'postgres-engineer', deps: ['cr-26-pg-lease-queue', 'cr-42-ci-rebuild'], size: 'S', complexity: 'Low', cls: 'medium', value: 'MEDIUM',
    title: 'Postgres 17 + 18 CI matrix, autovacuum tuning and PG18 runbook',
    why: 'Supported environments are PG 17 and 18; queue tables churn and need per-table autovacuum.',
    details: 'CI matrix on the project 17/18 images; per-table autovacuum settings in migrations; docs/runbooks/postgres18.md (checksums, volume path, SCRAM).',
    capability: 'postgres-compatibility',
    requirements: [
      req('Two-version support', 'All Postgres suites SHALL pass on PostgreSQL 17 and 18 in CI.', [
        sc('Matrix', 'CI runs the Postgres suites', 'jobs for 17 and 18 both pass'),
      ]),
    ],
    tasks: [
      t('CI matrix job (handoff to platform-engineer) and per-table autovacuum migration', 'postgres-engineer', 'CI + SQL'),
      t('PG18 runbook', 'postgres-engineer', 'docs'),
      review('matrix coverage'),
    ],
  },
  {
    id: 'cr-37-config-schema', milestone: 'M4', owner: 'tech-lead', deps: ['cr-26-pg-lease-queue', 'cr-28-embedded-redb-queue'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'Config schema carrying backend parameters and a real build_pools_from_scheduler_config',
    why: 'Docs advertise config-driven pools that do not exist; backend parameters (queue_name, path, lease_secs, notifier) have no schema.',
    details: 'Typed config with deny_unknown_fields, backend sections, validation; a builder that constructs pools from config; every JSON example in docs deserializes in a test.',
    capability: 'configuration',
    requirements: [
      req('Config-driven pools', 'A valid scheduler configuration SHALL build every configured pool and backend, and invalid configuration SHALL be rejected with a precise error.', [
        sc('Merged backends', 'configs for in-memory, Postgres and redb backends are loaded', 'each builds its pool; later backends (pgmq, Iggy) add their own section and example test in their changes'),
        sc('Unknown field', 'a config contains a misspelled field', 'loading fails naming the field'),
      ]),
    ],
    tasks: [
      t('Config types + validation + builder', 'tech-lead', 'API'),
      t('Doc-example deserialization tests', 'tech-lead', 'tests'),
      review('schema strictness'),
    ],
  },
  {
    id: 'cr-38-wasm-target', milestone: 'M4', owner: 'platform-engineer', deps: ['cr-19-dispatcher-semaphore'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'WASM pool on spawn_local with a CI check job',
    why: 'The WASM pool uses tokio::spawn and has never been built for wasm32.',
    details: 'Port the WASM WorkerPool onto the shared dispatcher in parking-lot-wasm with wasm-bindgen-futures::spawn_local and JS timers, including capacity admission (fixes C1 at wasm.rs:287) and execution budgets; getrandom wasm_js config; document that executor panics abort the module on wasm32 stable; CI check, a core dependency check (`cargo tree -p prometheus-parking-lot-core --target wasm32-unknown-unknown -e normal` contains no tokio), and a wasm-bindgen-test smoke/budget test.',
    capability: 'platform-targets',
    requirements: [
      req('Web target', 'The core and wasm crates SHALL build for wasm32-unknown-unknown, the wasm pool SHALL enforce capacity and execution budgets, and the core crate SHALL have no tokio in its wasm dependency graph.', [
        sc('Wasm check', 'CI builds the wasm crates and runs wasm-bindgen-test in a headless browser', 'the build passes, and smoke, capacity and budget tests pass'),
        sc('Core graph', 'the core dependency check runs', 'no tokio appears; a scratch branch adding tokio to core fails the check'),
      ]),
    ],
    tasks: [
      t('Port the wasm pool onto the dispatcher in parking-lot-wasm (spawn_local + JS timers, capacity admission, budgets); getrandom config; document panic=abort limit', 'platform-engineer', 'wasm'),
      t('CI wasm check, core dependency-graph check, and wasm-bindgen-test smoke/capacity/budget job', 'platform-engineer', 'CI'),
      review('no tokio runtime on wasm'),
    ],
  },
  {
    id: 'cr-44-mobile-desktop-bindings', milestone: 'M4', owner: 'platform-engineer', deps: ['cr-25-backend-contracts', 'cr-37-config-schema', 'cr-28-embedded-redb-queue'], size: 'L', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'Mobile (UniFFI) bindings and desktop (Tauri) integration',
    why: 'Mobile and desktop reach is a phase goal; no bindings or examples exist.',
    details: 'parking-lot-ffi with UniFFI async exports over the embedded backend; cargo-ndk Android and iOS (device + sim) check jobs; a Tauri example using the embedded queue.',
    capability: 'platform-targets',
    requirements: [
      req('Mobile and desktop reach', 'The FFI crate SHALL build for Android and iOS targets, and a Tauri example SHALL run the embedded queue end to end.', [
        sc('Mobile builds', 'CI builds aarch64-linux-android and aarch64-apple-ios(-sim)', 'both succeed and Swift/Kotlin bindings are generated'),
        sc('Desktop example', 'the Tauri example submits and retrieves a task across an app restart', 'the result is retrieved after restart'),
      ]),
    ],
    tasks: [
      t('UniFFI crate over the embedded backend; generated bindings', 'platform-engineer', 'FFI'),
      t('Android/iOS check jobs; Tauri example with restart test', 'platform-engineer', 'CI + example'),
      review('FFI safety (crate denies unsafe outside generated code)'),
    ],
  },
  {
    id: 'cr-39-dependency-hygiene', milestone: 'M4', owner: 'platform-engineer', deps: ['cr-26-pg-lease-queue', 'cr-28-embedded-redb-queue', 'cr-32-iggy-events'], size: 'S', complexity: 'Low', cls: 'medium', value: 'LOW',
    title: 'Dependency prune and semver checks',
    why: 'Unused dependencies slow builds; a published crate needs semver discipline.',
    details: 'cargo udeps clean, remove leftovers (sea-orm if unused, yaque), cargo-semver-checks in CI.',
    capability: 'continuous-integration',
    requirements: [
      req('Lean, semver-checked dependency graph', 'The workspace SHALL have no unused dependencies, and CI SHALL run semver checks on published crates.', [
        sc('udeps', 'cargo udeps runs', 'no unused dependency is reported'),
      ]),
    ],
    tasks: [
      t('Prune dependencies (handoff Cargo.toml edits to tech-lead)', 'platform-engineer', 'deps'),
      t('cargo-semver-checks + udeps CI jobs', 'platform-engineer', 'CI'),
      review('no needed dependency removed'),
    ],
  },
  {
    id: 'cr-40-benchmarks', milestone: 'M4', owner: 'scheduler-engineer', deps: ['cr-21-panic-containment', 'cr-22-result-slots', 'cr-20-skip-ahead'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'MEDIUM',
    title: 'CI-reproducible benchmarks and published numbers',
    why: 'Benchmarks do not compile; ARCHITECTURE.md numbers are unverifiable.',
    details: 'Criterion suites for dispatch throughput, latency and utilization; CI job storing results; ARCHITECTURE.md section 7 rewritten from them (tech-lead).',
    capability: 'performance-evidence',
    requirements: [
      req('Reproducible numbers', 'Published performance figures SHALL come from benchmarks a CI job runs and records.', [
        sc('Bench job', 'the benchmark job runs on main', 'results are stored as an artifact and match the documented figures'),
      ]),
    ],
    tasks: [
      t('Criterion suites (throughput, latency, utilization)', 'scheduler-engineer', 'benchmarks'),
      t('Bench CI job and ARCHITECTURE.md numbers (tech-lead)', 'scheduler-engineer', 'CI + docs handoff'),
      review('benchmark validity'),
    ],
  },

  // ───────────────────────── Documentation content ─────────────────────────
  {
    id: 'cr-46-docs-theory', milestone: 'M4', owner: 'product-manager', deps: ['cr-45-docs-site'], size: 'L', complexity: 'High', cls: 'frontier', value: 'HIGH',
    title: 'Concepts: theory and history of parking lots, admission, mailboxes and worker pools',
    why: 'The operator requires extensive documentation of the theory and history behind the library and its value for agentic software.',
    details: 'Cited concept chapters: parking lots (futex 2002, WebKit WTF::ParkingLot 2015, Rust parking_lot) versus this crate\'s admission-control parking lot; admission control and bin packing; priority scheduling and starvation; leases (Gray & Cheriton 1989) and at-least-once delivery; SKIP LOCKED queues; mailboxes and the actor model (Hewitt 1973, Erlang); thread and worker pools, work stealing (Cilk), Tokio; idempotency and outbox; event logs (Kafka, Iggy); why agentic workloads (GPU VRAM, long tasks, disconnecting clients) need all of them.',
    capability: 'documentation-content',
    requirements: [
      req('Cited concept documentation', 'The site SHALL contain concept chapters covering the listed topics, each with primary-source citations and diagrams, reviewed for factual accuracy.', [
        sc('Coverage', 'the concepts section is reviewed against the topic list', 'every topic has a chapter with at least one primary-source citation'),
        sc('Fact check', 'an independent reviewer checks historical claims', 'no claim lacks a source'),
      ]),
    ],
    tasks: [
      t('Research and outline concept chapters with primary sources (deep-research skill)', 'product-manager', 'research; citations'),
      t('Write concept chapters with Mermaid diagrams', 'product-manager', 'long-form technical writing'),
      review('factual accuracy and citations'),
    ],
  },
  {
    id: 'cr-47-docs-guides', milestone: 'M4', owner: 'product-manager', deps: ['cr-46-docs-theory', 'cr-37-config-schema', 'cr-29-shutdown-cancellation', 'cr-44-mobile-desktop-bindings', 'cr-32-iggy-events', 'cr-23-multi-resource'], size: 'L', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Guides and scenarios with compiled examples and agentic value',
    why: 'Users need explicit, working examples per deployment scenario and a clear statement of value for agentic software development.',
    details: 'Guides: GPU-VRAM inference pool, Tauri local LLM with embedded queue, cloud multi-tenant Postgres, disconnected-client retrieval, streaming replay (only if stretch cr-33 merged), shutdown/requeue, Iggy events, configuration, release/upgrade notes; an agentic-value chapter; rustdoc API reference linked; every code sample compiled in CI (examples/ or doctests).',
    capability: 'documentation-content',
    requirements: [
      req('Working examples per scenario', 'Every guide code sample SHALL be compiled in CI from a real source file, and each scenario SHALL state when to use it and the value it delivers.', [
        sc('Examples compile', 'the docs example job runs', 'every referenced example builds and its smoke run passes'),
      ]),
    ],
    tasks: [
      t('Write scenario guides backed by examples/ sources; every JSON/TOML config shown is added to the config example test (tech-lead reviews API usage)', 'product-manager', 'technical writing + examples'),
      t('Agentic-value chapter and API reference integration', 'product-manager', 'writing'),
      t('Hand platform-engineer the docs-example CI job', 'product-manager', 'cross-owner handoff'),
      review('examples compile and match the API'),
    ],
  },
  {
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
  {
    id: 'cr-52-release', milestone: 'M5', owner: 'platform-engineer', deps: ['cr-39-dependency-hygiene', 'cr-40-benchmarks', 'cr-47-docs-guides', 'cr-53-wasm-parity', 'cr-51-supply-chain-gates', 'cr-36-pg-matrix', 'cr-30-pgmq-sql-backend', 'cr-17-kbd-correction', 'cr-16-docs-truth'], size: 'M', complexity: 'Medium', cls: 'medium', value: 'HIGH',
    title: 'Version, publish and tag 0.3.0 (crates.io trusted publishing, signed tag, GitHub release, docs deploy)',
    why: 'The operator requires the library versioned, its crates published and the version tagged. No version of prometheus_parking_lot has ever been published (crates.io checked 2026-10-22); 0.3.0 is the first release and is a breaking change from the 0.2.0 sources.',
    details: 'Versioning ADR (SemVer; workspace.package version 0.3.0 for every crate; inter-crate dependencies pinned to the same minor; MSRV policy). release-plz + git-cliff generate the changelog and release PR. Publishing runs in CI only after the boundary packet passes, using crates.io trusted publishing (OIDC, no long-lived token) configured by the operator per crate, in dependency order: core, runtime-tokio, pg, embedded, iggy, wasm, ffi, facade. Signed annotated tag v0.3.0, GitHub release with notes, docs site deployed for the tag. The long-lived CARGO_REGISTRY_TOKEN in the operator shell is retired.',
    capability: 'release-management',
    requirements: [
      req('Reproducible, approved release', 'Release 0.3.0 SHALL be published only after the boundary packet passes and the operator approves, every crate SHALL be installable from crates.io at 0.3.0, and the repository SHALL carry a signed v0.3.0 tag pointing at the released commit.', [
        sc('Install from crates.io', 'a fresh project runs \`cargo add prometheus_parking_lot@0.3.0 --features postgres,embedded\` and builds', 'it resolves only 0.3.0 workspace crates from crates.io and compiles'),
        sc('Tag and release', 'the release workflow completes', '\`git tag -v v0.3.0\` verifies and the GitHub release lists the changelog for 0.3.0'),
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
];

// Phase-level boundary integration gate (single run, after all production changes).
export const BOUNDARY = {
  commands: [
    'cargo fmt --all -- --check',
    'cargo clippy --workspace --all-targets --all-features --locked -- -D warnings',
    'cargo nextest run --workspace --all-features --locked --no-tests=fail',
    'cargo +1.95 check --workspace --all-features --locked',
    'RUSTFLAGS="--cfg parking_lot_loom" LOOM_MAX_PREEMPTIONS=3 cargo test -p prometheus-parking-lot-core --test loom_capacity --test loom_wakeup --test loom_vector --locked',
    'POSTGRES_IMAGE=parking-lot-pg:17 cargo nextest run -p prometheus-parking-lot-pg --locked',
    'cargo test --doc --workspace --locked',
    'RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps --all-features --locked',
    'cargo deny check',
    'cargo package --workspace --locked',
    'gitleaks detect --redact --log-opts=--all',
    'rm -rf "$TMPDIR/pl-mirror" && git clone --mirror https://github.com/Prometheus-AGS/prometheus-parking-lot-rs.git "$TMPDIR/pl-mirror" && test -z "$(git -C "$TMPDIR/pl-mirror" log --all --oneline -- .codex/auth.json .codex/history.jsonl .codex/sessions .codex/config.toml .codex/version.json)"',
    '! cargo tree -p prometheus-parking-lot-core -e normal | grep -E " (tokio|sqlx|sea-orm|iggy|redb|reqwest) v"',
    '! cargo tree -p prometheus-parking-lot-core --target wasm32-unknown-unknown -e normal | grep -E " tokio v"',
    'wasm-pack test --headless --chrome crates/parking-lot-wasm',
    'cargo ndk -t arm64-v8a check -p prometheus-parking-lot-ffi --locked',
    'cargo check -p prometheus-parking-lot-ffi --target aarch64-apple-ios-sim --locked',
    'npm --prefix website ci && npm --prefix website run build',
    'npm --prefix website run verify',
  ],
};
