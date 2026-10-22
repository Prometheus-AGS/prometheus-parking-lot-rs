# Goals

- Make the crate's three promises true: weighted capacity is never exceeded, accepted work is never silently lost, results reach disconnected clients (spec: docs/update-plan-10-22-2026.md section 11)
- M0 Containment: CR-15 credential revocation/purge + include allowlist + secret scan, CR-16 doc archive and CLAUDE/README from real API, CR-17 KBD progress correction
- M1 Core rework: CR-18..CR-24 — Arc<Inner>, unit-weighted semaphore + priority dispatcher, skip-ahead, panic containment, oneshot result slots, multi-resource cost, FIFO sequencing; loom/property proofs
- M2 Durable backends: CR-25..CR-30 — async lease/ack TaskQueue, Postgres SKIP LOCKED lease queue and mailbox, redb embedded queue, shutdown/cancellation wiring, pgmq decision
- M3 Events: CR-31..CR-34 — HookBus emission, Iggy hook/notifier, stream replay (stretch), composable mailbox
- M4 Platform and CI: CR-35..CR-40 — real test targets, PG 17+18 matrix, config schema, wasm decision, dependency prune, reproducible benchmarks
- Delivered by agent team parking-lot-core (docs/agent-team.md): contracts first, one worktree per role, independent review at merge gates
