# Agent skill routing — Rust and systems programming

Reference for `AGENTS.md` § "Rust and systems skill routing". Loaded on demand,
not resident. Every skill listed here conforms to the
[agentskills.io specification](https://agentskills.io/specification)
(`npx skills-ref validate <dir>`), so the same directory works in Claude Code,
Codex, OpenCode, Zed, Kimi Code and MiniMax Code.

## Routing table

Load the **fewest** skills that cover the task (at most three). If a skill is
missing, run `scripts/install-agent-skills.sh` or say it is unavailable — never
improvise what it would have said.

| Task in this repo | Skill(s) |
|---|---|
| Any `.rs` / `Cargo.toml` edit — entry point and Cargo serialization | `prometheus-rust-workspace` |
| Writing or refactoring Rust (design) | `rust-guidelines`, then `rust-skills` for a specific rule |
| Tokio tasks, `spawn_blocking`, cancellation, shutdown, timeouts | `rust-async-patterns`; review with `tokio-async-code-review` |
| How futures/wakers/executors actually behave (hangs, lost wakeups) | `rust-async-internals` |
| `parking_lot_core`, condvars, atomics, `Ordering`, lock-free code | `memory-model`; verify with `rust-sanitizers-miri` |
| Deadlock, hang, or race being debugged | `concurrency-debugging`, `rust-debugging` |
| Tests: async, testcontainers, property, loom/miri | `rust-testing`; review with `rust-testing-code-review` |
| Serde config models (`src/config`) | `serde-code-review` |
| sqlx / SeaORM / pgmq backends (`src/infra`) | `sqlx-code-review` |
| Code review of a Rust diff | `rust-code-review` (+ the domain reviewer above) |
| Security / panic-DoS / concurrency-hazard audit | `rust-review` |
| Benchmark regression or scheduler throughput | `rust-profiling`, `flamegraphs` |
| Whole-crate quality remediation pipeline | `prometheus-rust-auditor` |
| Fuzzing parsers / config input | `cargo-fuzz` |


Not applicable here (do not load): `unsafe-checker` beyond confirming `unsafe`
is absent, FFI/no_std/embedded/cross-compilation skills, `axum`/web skills.

## Install locations

| Harness | Reads user skills from |
|---|---|
| Codex, Zed, OpenCode | `~/.agents/skills/` (canonical copy lives here) |
| Claude Code | `~/.claude/skills/` (symlinks to `~/.agents/skills/`) |
| Kimi Code | `~/.kimi/skills/` (symlinks) |
| MiniMax Code | `~/.minimax/skills/` (symlinks) |

`scripts/install-agent-skills.sh` installs the third-party skills below at pinned
commits, validates each with `skills-ref`, writes a `.provenance` file, and creates
the symlinks. It is idempotent and never overwrites an existing skill.

## Skill inventory

### Already installed (pre-existing on this machine)

| Skill | Source | Use it for |
|---|---|---|
| `prometheus-rust-workspace` | Prometheus skill pack | Entry router for all Rust/Cargo work; serialized, phase-gated Cargo verification. |
| `prometheus-rust-auditor` | Prometheus skill pack | Staged clippy/fmt/deny/architecture remediation of the whole crate. Not for single edits. |
| `rust-guidelines` | Microsoft Pragmatic Rust Guidelines + rust-unofficial patterns | Design decisions while writing code (API shape, error types, builders). Apply while writing, not only in review. |
| `rust-skills` | leonardomso/rust-skills (265 rules) | Look up one specific rule (e.g. `async-cancel-safety`, `test-loom-concurrency`) instead of loading all. |
| `rust-best-practices` | Apollo GraphQL handbook | Idiom questions when `rust-guidelines` is silent. |
| `rust-async-patterns` | Prometheus skill pack (derived from wshobson/agents) | Tokio task structure, channels, `select!`, graceful shutdown. |
| `rust-testing` | ECC | Test layout, async tests, property tests, coverage (`cargo llvm-cov`). |
| `rust-error-handling` | — | `thiserror` taxonomy work (decision CR-05: single `#[non_exhaustive]` enum). |
| `rust-review` | Trail of Bits | Security review: panic-induced DoS, concurrency hazards, async runtime mistakes. |
| `cargo-fuzz` | Trail of Bits testing handbook | Fuzz config/serde parsing. Nightly toolchain. |
| `unsafe-checker` | actionbook/rust-skills | Only to confirm a change introduces no `unsafe` (crate denies it). |

### Installed by `scripts/install-agent-skills.sh`

Pinned: `existential-birds/beagle@d1a74899fbfec74974d1818e4cac7c3d54d44b65` (Apache-2.0),
`mohitmishra786/low-level-dev-skills@bdc58472fa9f309ed1b3f7d985a0d8e9bd8f4608` (MIT).

| Skill | Source | When | How |
|---|---|---|---|
| `rust-code-review` | beagle | Reviewing any Rust diff before merge. | Run over `git diff main...HEAD`; it emits `[FILE:LINE] CODE` findings. Pair with one domain reviewer below. Its references `../../rust-best-practices` and `macros/axum/ffi-code-review` resolve to other skills or are absent — ignore those links. |
| `tokio-async-code-review` | beagle | Diffs touching `spawn`, `spawn_blocking`, `select!`, `CancellationToken`, `JoinHandle`, runtime shutdown. | Check cancel safety, blocking-in-async, and un-joined tasks — the exact class of the CR-01/CR-02 hang. |
| `rust-testing-code-review` | beagle | Reviewing tests, especially concurrency tests. | Its `concurrency-testing` reference covers loom and miri harnesses. |
| `serde-code-review` | beagle | Changes to `src/config` models or JSON task payloads. | Enum tagging, `deny_unknown_fields`, defaults, validate-on-deserialize. |
| `sqlx-code-review` | beagle | Changes to `src/infra/queue` or `src/infra/mailbox` Postgres code, or `migrations/`. | Query safety, transactions, `FOR UPDATE SKIP LOCKED` dequeue, pool usage. |
| `rust-async-internals` | low-level-dev-skills | A hang, lost wakeup, or executor starvation needs a mechanism-level explanation. | Read before changing waker/condvar interaction in `worker_pool/`. |
| `memory-model` | low-level-dev-skills | Any change to atomics, `Ordering`, `parking_lot_core` park/unpark, or condvar use. | Name the happens-before edge before choosing an ordering; do not default to `SeqCst`. |
| `rust-sanitizers-miri` | low-level-dev-skills | Verifying concurrency changes. | `cargo +nightly miri test <filter>` on single-threaded paths; ThreadSanitizer for real threads. Nightly only — never part of the stable CI gate. |
| `concurrency-debugging` | low-level-dev-skills | Reproducing a deadlock or race. | Capture thread dumps / lock order before hypothesizing. |
| `rust-debugging` | low-level-dev-skills | Stepping a hang in lldb/gdb, `RUST_BACKTRACE`, tokio-console. | macOS uses lldb. |
| `rust-profiling` | low-level-dev-skills | Throughput or latency regression in `benches/`. | `cargo flamegraph` / criterion. Its Linux `sudo sysctl` steps change host settings — ask the operator first; on macOS use the dtrace path. |
| `flamegraphs` | low-level-dev-skills | Reading a flamegraph produced by `rust-profiling`. | Interpretation only. |

### Considered and not installed

| Candidate | Reason |
|---|---|
| beagle `review-rust`, `review-verification-protocol` | Use Claude-only frontmatter (`disable-model-invocation`, `user-invocable`); fail `skills-ref validate`. |
| beagle `axum-code-review`, `ffi-code-review`, `macros-code-review` | No axum, FFI or proc-macros in this crate. |
| beagle `rust-best-practices` | Name collides with the installed Apollo-based skill. |
| low-level `rust-unsafe`, `rust-ffi`, `rust-no-std`, `rust-cross`, `embedded-rust` | Crate denies `unsafe`; no FFI, no_std or cross targets. |
| wshobson `memory-safety-patterns` | Mostly C/C++; covered by `memory-model` + `rust-sanitizers-miri`. |
| Jeffallan `rust-engineer`, sickn33 `rust-pro` | Generic persona prompts; overlap `rust-guidelines` + `rust-skills`. |
| microsoft/skills `*-rust` | Azure SDK skills; not applicable. |

## Usage rules

1. Start Rust work with `prometheus-rust-workspace`; it decides which others to load.
2. Load at most three skills for one task. The routing table is ordered by
   specificity — the domain reviewer beats the generic one.
3. Review skills run at the review boundary, not on every edit (see
   "Verification boundaries" in `AGENTS.md`).
4. A skill's advice never overrides `Cargo.toml` lints, `.kbd-orchestrator/constraints.md`,
   or a recorded decision. If they conflict, surface the conflict.
5. Skills that suggest host changes (`sudo`, sysctl, installing toolchains) require
   operator approval first.
