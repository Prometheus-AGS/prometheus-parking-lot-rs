# prometheus-parking-lot-core

The runtime-free core of [`prometheus_parking_lot`](https://crates.io/crates/prometheus_parking_lot),
a resource-aware scheduler for expensive work such as LLM inference. Most
applications depend on the `prometheus_parking_lot` facade, which re-exports
this crate; depend on `core` directly only to implement a backend or to build
for a target without an async runtime.

It performs no IO and depends on no async runtime, database or network crate.

## What it holds

- Scheduling contracts: `LeaseQueue`, `ResultMailbox`, `TaskQueue`, `Mailbox`,
  `TaskExecutor`, `WorkerExecutor`, `Spawn`, `Clock`, `Notifier`.
- Task and capacity types: `TaskMetadata`, `Priority`, `ResourceCost`,
  `ResourceKind`, `PoolLimits`, `PoolStats`, `SchedulerError`, `TaskError`.
- The admission core shared by every pool: the `dispatch` module and the
  unit-weighted `semaphore`.
- Lifecycle hooks and audit types: `Hook`, `HookBus`, `LifecycleEvent`,
  `AuditSink`.
- In-memory reference backends: `InMemoryQueue`, `InMemoryLeaseQueue`,
  `InMemoryResultMailbox`, `InMemoryMailbox`.

## Features

| Feature | Enables |
| --- | --- |
| `conformance` | The backend conformance suite (`conformance`, `lease_queue_conformance!`, `result_mailbox_conformance!`) that every queue and mailbox backend must pass. Test-only API; no extra dependencies. |

## Where it fits

`prometheus-parking-lot-runtime-tokio`, `-pg`, `-embedded`, `-iggy` and `-wasm`
build on this crate; the `prometheus_parking_lot` facade ties them together.
The dependency rules are in the repository's `docs/adr/0001-workspace-split.md`.

## License

MIT. See `LICENSE-MIT`.
