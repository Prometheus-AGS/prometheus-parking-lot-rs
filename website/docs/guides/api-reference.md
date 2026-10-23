---
title: API reference
description: Where the rustdoc API reference for each crate lives (docs.rs, from the 0.3.0 release), how to build it locally before then, and a map of the main types by task.
---

# API reference

The rustdoc of each crate is the API reference. This site does not host a copy:
it links to [docs.rs](https://docs.rs), which builds every published version.

:::warning[Available from the 0.3.0 release]

0.3.0 is the first version published to crates.io. Until it is published, the
docs.rs links below return "not found". Build the same reference locally:

```bash
cargo doc --workspace --no-deps --all-features --open
```

:::

| Crate | Reference | What it holds |
| --- | --- | --- |
| `prometheus_parking_lot` | [docs.rs/prometheus_parking_lot](https://docs.rs/prometheus_parking_lot) | The facade most applications depend on: re-exports, configuration, builders |
| `prometheus-parking-lot-core` | [docs.rs/prometheus-parking-lot-core](https://docs.rs/prometheus-parking-lot-core) | Runtime-free contracts, dispatcher, semaphore, hooks, in-memory backends, conformance suite |
| `prometheus-parking-lot-runtime-tokio` | [docs.rs/prometheus-parking-lot-runtime-tokio](https://docs.rs/prometheus-parking-lot-runtime-tokio) | `ResourcePool`, `DurableResourcePool`, native `WorkerPool`, shutdown, cancellation |
| `prometheus-parking-lot-pg` | [docs.rs/prometheus-parking-lot-pg](https://docs.rs/prometheus-parking-lot-pg) | `PgLeaseQueue`, `PgmqLeaseQueue`, `PgResultMailbox`, audit sink, migrations |
| `prometheus-parking-lot-embedded` | [docs.rs/prometheus-parking-lot-embedded](https://docs.rs/prometheus-parking-lot-embedded) | `EmbeddedStore`, `RedbLeaseQueue`, `RedbResultMailbox` |
| `prometheus-parking-lot-iggy` | [docs.rs/prometheus-parking-lot-iggy](https://docs.rs/prometheus-parking-lot-iggy) | `IggyHook`, `IggyNotifier`, `IggyStreamSink`, `IggyStreamReader` |
| `prometheus-parking-lot-wasm` | [docs.rs/prometheus-parking-lot-wasm](https://docs.rs/prometheus-parking-lot-wasm) | `WorkerPool` for `wasm32-unknown-unknown` (documented for that target) |

The UniFFI crate `prometheus-parking-lot-ffi` is not published; its API is
described in [`bindings/README.md`](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/bindings/README.md).

## Why docs.rs and not a copy on this site

- **It matches what you installed.** docs.rs documents each published version
  with all features enabled (each crate's `package.metadata.docs.rs` asks for
  it, and the wasm crate for its target). A copy built with this site would
  document `main`, which is ahead of the release users depend on.
- **It keeps the site build small.** The site builds with Node alone. Hosting
  rustdoc would add a Rust toolchain and an all-features build of sqlx, redb
  and the Iggy client to every documentation deploy.
- **CI already checks it.** The `Docs` job runs
  `cargo doc --workspace --no-deps --all-features` with warnings as errors, so
  a broken intra-doc link fails the pull request.

The cost is the gap above: between merge and release the hosted reference does
not exist. Revisit this if the project ever needs to document `main` publicly.

## Where to start, by task

| You want to | Start at |
| --- | --- |
| Run work on dedicated threads and wait for it | `WorkerPool`, `WorkerExecutor`, `WorkerPoolConfig` |
| Run async work and deliver it to a mailbox | `core::ResourcePool`, `TaskExecutor`, `ScheduledTask`, `TaskMetadata` |
| Survive a crash | `DurableResourcePool`, `DurableOptions`, `core::backend::{LeaseQueue, ResultMailbox}` |
| Build pools from a file | `config::SchedulerConfig`, `builders::build_pools_from_scheduler_config` |
| Describe cost and priority | `util::{ResourceCost, ResourceKind, Priority, MailboxKey}` |
| Shut down cleanly | `ShutdownPolicy`, `ShutdownReport`, `CancellationSink` |
| Observe tasks | `Hook`, `HookBus`, `LifecycleEvent`, `infra::events::IggyHook` |
| Handle errors | `SchedulerError` (admission, backend), `TaskError` (one task's failure) |

The repository's [API overview](https://github.com/Prometheus-AGS/prometheus-parking-lot-rs/blob/main/docs/API.md)
maps every public item to its path and feature.

## Compiled examples

Every Rust block in these guides is an excerpt of a file under `examples/`, and
CI checks that it still is. The examples are built in CI and run there: the
server-free ones directly, the Postgres and Iggy ones against those servers in
containers.

| Example | Guide | Needs |
| --- | --- | --- |
| `gpu_inference_pool` | [GPU inference pool](./gpu-inference-pool.md) | nothing |
| `disconnected_client` | [Disconnected clients](./disconnected-clients.md) | nothing (redb) |
| `shutdown_requeue` | [Shutdown and requeue](./shutdown-and-requeue.md), [Tauri local LLM](./tauri-local-llm.md) | nothing (redb) |
| `embedded_pool` | [Tauri local LLM](./tauri-local-llm.md) (direct queue use) | nothing (redb) |
| `postgres_tenants` | [Cloud Postgres](./cloud-postgres.md) | PostgreSQL |
| `postgres_pool` | [Cloud Postgres](./cloud-postgres.md) (direct queue use) | PostgreSQL |
| `iggy_events` | [Iggy events](./iggy-events.md) | Iggy 0.9 |
| `iggy_stream_replay` | [Streaming replay](./streaming-replay.md) | Iggy 0.9 |

The README's examples run as doctests of the facade crate.
